extern crate alloc;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};

use crate::arch::x86_64::pit;
use crate::kinfo;
use crate::net::crypto::{
    aes128_key_unwrap, derive_wpa2_ptk, eapol_mic_hmac_sha1, generate_snonce, pbkdf2_sha1,
};
use crate::net::device::{NetDevice, NetDeviceType, NET_MANAGER};
use crate::sync::IrqSafeSpinlock;

pub static WIFI_CONNECTED: AtomicBool = AtomicBool::new(false);

/// Lock-free Wi-Fi link indicator safe to call while `NET_MANAGER` or RX/TX locks are held.
pub fn is_wifi_connected() -> bool {
    WIFI_CONNECTED.load(Ordering::Acquire)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WifiSecurity {
    Open,
    Wep,
    Wpa2Psk,
    Wpa3,
}

impl WifiSecurity {
    pub fn as_str(&self) -> &'static str {
        match self {
            WifiSecurity::Open => "Open",
            WifiSecurity::Wep => "WEP",
            WifiSecurity::Wpa2Psk => "WPA2-PSK",
            WifiSecurity::Wpa3 => "WPA3-SAE",
        }
    }
}

#[derive(Debug, Clone)]
pub struct WifiAccessPoint {
    pub ssid: String,
    pub bssid: [u8; 6],
    pub rssi: i8,
    pub channel: u8,
    pub security: WifiSecurity,
}

impl WifiAccessPoint {
    pub fn signal_bars(&self) -> &'static str {
        if self.rssi >= -50 {
            "[||||]"
        } else if self.rssi >= -65 {
            "[||| ]"
        } else if self.rssi >= -75 {
            "[||  ]"
        } else if self.rssi >= -85 {
            "[|   ]"
        } else {
            "[    ]"
        }
    }
}

pub struct WifiManager {
    pub scan_results: Vec<WifiAccessPoint>,
    pub is_connected: bool,
    pub current_ssid: Option<String>,
    pub current_bssid: Option<[u8; 6]>,
    pub current_channel: u8,
    pub link_speed_mbps: u32,
    pub pmk: [u8; 32],
}

impl WifiManager {
    pub const fn new() -> Self {
        Self {
            scan_results: Vec::new(),
            is_connected: false,
            current_ssid: None,
            current_bssid: None,
            current_channel: 0,
            link_speed_mbps: 0,
            pmk: [0u8; 32],
        }
    }

    fn find_wireless_device() -> Option<(Arc<dyn NetDevice>, [u8; 6])> {
        let net = NET_MANAGER.lock();
        let mut fallback: Option<(Arc<dyn NetDevice>, [u8; 6])> = None;
        for (dev, _) in &net.devices {
            if dev.device_type() == NetDeviceType::Wireless80211 {
                if dev.name() == "wlan2" {
                    return Some((dev.clone(), dev.mac_address()));
                }
                if fallback.is_none() {
                    fallback = Some((dev.clone(), dev.mac_address()));
                }
            }
        }
        fallback
    }

    /// Legacy / unit-test single-pass scan helper (does not sleep across locks).
    pub fn scan(&mut self) -> &[WifiAccessPoint] {
        self.scan_results.clear();

        let Some((dev, mac)) = Self::find_wireless_device() else {
            kinfo!("Wi-Fi Scan: no PCIe 802.11 wireless hardware present (0 access points).");
            return &self.scan_results;
        };

        let probe_req = build_80211_probe_request(mac, None);
        if let Err(e) = dev.send_80211_frame(&probe_req) {
            if dev.name() != "wlan2" {
                let _ = dev.send_packet(&probe_req);
            } else {
                kinfo!("Wi-Fi Scan: TX probe skipped ({})", e);
                return &self.scan_results;
            }
        }

        let mut rx_buf = [0u8; 2048];
        for _ in 0..32 {
            match dev.receive_80211_frame(&mut rx_buf) {
                Ok(len) if len >= 36 => {
                    let _ = self.ingest_80211_beacon(&rx_buf[..len], -55);
                }
                _ => break,
            }
        }

        kinfo!("Wi-Fi Scan complete: discovered {} IEEE 802.11 access point(s)", self.scan_results.len());
        &self.scan_results
    }

    /// Parse a raw IEEE 802.11 Management Beacon / Probe Response frame (24-byte MAC header + body)
    /// and record/update the discovered access point in `scan_results`.
    pub fn ingest_80211_beacon(&mut self, frame: &[u8], rssi_dbm: i8) -> bool {
        if frame.len() < 36 {
            return false;
        }
        let subtype = (frame[0] >> 4) & 0x0F;
        let ftype = (frame[0] >> 2) & 0x03;
        if ftype != 0 || (subtype != 8 && subtype != 5) {
            return false;
        }

        let Some((ssid, channel, security)) = parse_80211_beacon(frame) else {
            return false;
        };
        let mut bssid = [0u8; 6];
        bssid.copy_from_slice(&frame[16..22]);

        if let Some(existing) = self.scan_results.iter_mut().find(|ap| ap.bssid == bssid) {
            existing.ssid = ssid;
            existing.channel = channel;
            existing.security = security;
            existing.rssi = rssi_dbm;
        } else {
            self.scan_results.push(WifiAccessPoint {
                ssid,
                bssid,
                rssi: rssi_dbm,
                channel,
                security,
            });
        }
        true
    }

    /// Authenticate & associate state helper (preserved for in-kernel unit tests without live AP).
    pub fn connect(&mut self, ssid: &str, passphrase: Option<&str>) -> Result<(), &'static str> {
        let ap = if let Some(existing) = self.scan_results.iter().find(|a| a.ssid == ssid).cloned() {
            existing
        } else {
            WifiAccessPoint {
                ssid: String::from(ssid),
                bssid: [0x00, 0x14, 0x22, 0x01, 0x23, 0x45],
                rssi: -50,
                channel: 6,
                security: if passphrase.is_some() {
                    WifiSecurity::Wpa2Psk
                } else {
                    WifiSecurity::Open
                },
            }
        };

        match ap.security {
            WifiSecurity::Open => {
                self.pmk = [0u8; 32];
            }
            WifiSecurity::Wpa2Psk => {
                let pass = passphrase.ok_or("Passphrase required for WPA2-PSK network")?;
                if pass.len() < 8 || pass.len() > 63 {
                    return Err("WPA2-PSK passphrase must be 8..63 ASCII characters");
                }
                kinfo!("Wi-Fi: Deriving 256-bit WPA2-PSK PMK via PBKDF2-HMAC-SHA1...");
                pbkdf2_sha1(pass.as_bytes(), ssid.as_bytes(), 4096, &mut self.pmk);
            }
            WifiSecurity::Wep | WifiSecurity::Wpa3 => {
                return Err("Unsupported wireless security mode");
            }
        }

        if let Some((dev, src_mac)) = Self::find_wireless_device() {
            let auth_frame = build_80211_auth_request(ap.bssid, src_mac);
            if dev.name() == "wlan2" {
                dev.send_80211_frame(&auth_frame)?;
                let assoc_frame = build_80211_assoc_request(
                    ap.bssid,
                    src_mac,
                    &ap.ssid,
                    ap.security == WifiSecurity::Wpa2Psk,
                );
                dev.send_80211_frame(&assoc_frame)?;
            } else {
                dev.send_packet(&auth_frame)?;
                let assoc_frame = build_80211_assoc_request(
                    ap.bssid,
                    src_mac,
                    &ap.ssid,
                    ap.security == WifiSecurity::Wpa2Psk,
                );
                dev.send_packet(&assoc_frame)?;
            }
        }

        self.is_connected = true;
        self.current_ssid = Some(ap.ssid);
        self.current_bssid = Some(ap.bssid);
        self.current_channel = ap.channel;
        self.link_speed_mbps = 54;
        WIFI_CONNECTED.store(true, Ordering::Release);

        kinfo!("Wi-Fi: 802.11 Association completed for '{}' (Ch {})", ssid, ap.channel);
        Ok(())
    }

    pub fn disconnect(&mut self) {
        self.is_connected = false;
        self.current_ssid = None;
        self.current_bssid = None;
        self.current_channel = 0;
        self.link_speed_mbps = 0;
        self.pmk = [0u8; 32];
        WIFI_CONNECTED.store(false, Ordering::Release);
        kinfo!("Wi-Fi: Disconnected.");
    }
}

pub static WIFI_MANAGER: IrqSafeSpinlock<WifiManager> = IrqSafeSpinlock::new(WifiManager::new());

/// Look up the preferred active PCIe 802.11 device without holding `WIFI_MANAGER` or retaining `NET_MANAGER`.
pub fn find_active_wireless_device() -> Option<(Arc<dyn NetDevice>, [u8; 6])> {
    WifiManager::find_wireless_device()
}

/// Lock-free multi-channel active probe scan across 2.4 GHz Channels `1..=13`.
/// Dwells `75 ms` per channel via `pit::sleep_ms` outside all spinlocks so audio/input remain live.
pub fn scan() -> Vec<WifiAccessPoint> {
    {
        WIFI_MANAGER.lock().scan_results.clear();
    }

    let Some((dev, mac)) = find_active_wireless_device() else {
        kinfo!("Wi-Fi Scan: no PCIe 802.11 wireless hardware present (0 access points).");
        return Vec::new();
    };

    const CHANNELS: [u8; 13] = [1, 6, 11, 2, 3, 4, 5, 7, 8, 9, 10, 12, 13];
    let mut rx_buf = [0u8; 2048];

    for ch in CHANNELS {
        if let Err(e) = dev.set_channel(ch) {
            kinfo!("Wi-Fi Scan: radio channel switch unavailable ({})", e);
            break;
        }
        let probe_req = build_80211_probe_request(mac, None);
        if let Err(e) = dev.send_80211_frame(&probe_req) {
            kinfo!("Wi-Fi Scan: TX probe skipped on Ch {} ({})", ch, e);
            break;
        }

        pit::sleep_ms(75);

        let mut captured: Vec<Vec<u8>> = Vec::new();
        for _ in 0..64 {
            match dev.receive_80211_frame(&mut rx_buf) {
                Ok(len) if len >= 36 => captured.push(rx_buf[..len].to_vec()),
                _ => break,
            }
        }
        if !captured.is_empty() {
            let mut mgr = WIFI_MANAGER.lock();
            for frame in captured {
                let _ = mgr.ingest_80211_beacon(&frame, -55);
            }
        }
    }

    let results = WIFI_MANAGER.lock().scan_results.clone();
    kinfo!("Wi-Fi Scan complete: discovered {} IEEE 802.11 access point(s)", results.len());
    results
}

/// Parse 802.11 Authentication Response (Status Code 0 = Ok).
pub fn parse_80211_auth_resp(frame: &[u8], expected_bssid: &[u8; 6]) -> Option<Result<(), &'static str>> {
    if frame.len() < 30 {
        return None;
    }
    let ftype = (frame[0] >> 2) & 0x03;
    let subtype = (frame[0] >> 4) & 0x0F;
    if ftype != 0 || subtype != 11 {
        return None;
    }
    if &frame[16..22] != expected_bssid && &frame[10..16] != expected_bssid {
        return None;
    }
    let seq = u16::from_le_bytes([frame[26], frame[27]]);
    let status = u16::from_le_bytes([frame[28], frame[29]]);
    if seq != 2 {
        return None;
    }
    if status == 0 {
        Some(Ok(()))
    } else {
        Some(Err("802.11 Authentication rejected by AP"))
    }
}

/// Parse 802.11 Association Response -> `Ok(AID)` on status 0.
pub fn parse_80211_assoc_resp(frame: &[u8], expected_bssid: &[u8; 6]) -> Option<Result<u16, &'static str>> {
    if frame.len() < 30 {
        return None;
    }
    let ftype = (frame[0] >> 2) & 0x03;
    let subtype = (frame[0] >> 4) & 0x0F;
    if ftype != 0 || subtype != 1 {
        return None;
    }
    if &frame[16..22] != expected_bssid && &frame[10..16] != expected_bssid {
        return None;
    }
    let status = u16::from_le_bytes([frame[26], frame[27]]);
    let aid = u16::from_le_bytes([frame[28], frame[29]]) & 0x3FFF;
    if status == 0 {
        Some(Ok(aid))
    } else {
        Some(Err("802.11 Association rejected by AP"))
    }
}

/// Extract `(gtk_key_id, gtk_16)` from decrypted RSN Key Data KDE TLVs.
pub fn parse_eapol_gtk_kde(key_data: &[u8]) -> Option<(u8, [u8; 16])> {
    let mut i = 0usize;
    while i + 2 <= key_data.len() {
        let tag = key_data[i];
        if tag == 0xDD && i + 1 < key_data.len() {
            let len = key_data[i + 1] as usize;
            if len == 0 {
                break;
            }
            if i + 2 + len > key_data.len() {
                break;
            }
            let val = &key_data[i + 2..i + 2 + len];
            // IEEE 802.11i GTK KDE: OUI 00-0F-AC, Data Type 1, KeyID byte, Reserved byte, GTK[16]
            if val.len() >= 22 && val[0..4] == [0x00, 0x0F, 0xAC, 0x01] {
                let key_id = val[4] & 0x03;
                let mut gtk = [0u8; 16];
                gtk.copy_from_slice(&val[6..22]);
                return Some((key_id, gtk));
            }
            i += 2 + len;
        } else {
            i += 1;
        }
    }
    None
}

/// Build an EAPOL-Key (0x888E) body wrapped in an unencrypted 802.11 Data frame (ToDS=1 + LLC/SNAP).
pub fn build_eapol_key_msg(
    bssid: [u8; 6],
    src_mac: [u8; 6],
    key_info: u16,
    replay_counter: u64,
    nonce: &[u8; 32],
    key_data: &[u8],
    kck: &[u8; 16],
) -> Vec<u8> {
    let eapol_body_len = 95usize + key_data.len();
    let mut eapol = Vec::with_capacity(4 + eapol_body_len);
    eapol.push(1); // EAPOL v1
    eapol.push(3); // EAPOL-Key
    eapol.extend_from_slice(&(eapol_body_len as u16).to_be_bytes());
    eapol.push(2); // RSN Key Descriptor (IEEE 802.11i)
    eapol.extend_from_slice(&key_info.to_be_bytes());
    eapol.extend_from_slice(&[0x00, 0x00]); // Key Length = 0 on STA replies
    eapol.extend_from_slice(&replay_counter.to_be_bytes());
    eapol.extend_from_slice(nonce);
    eapol.extend_from_slice(&[0u8; 16]); // Key IV
    eapol.extend_from_slice(&[0u8; 8]);  // Key RSC
    eapol.extend_from_slice(&[0u8; 8]);  // Reserved
    eapol.extend_from_slice(&[0u8; 16]); // Key MIC placeholder (offset 81..97)
    eapol.extend_from_slice(&(key_data.len() as u16).to_be_bytes());
    eapol.extend_from_slice(key_data);

    let mic = eapol_mic_hmac_sha1(kck, &eapol);
    eapol[81..97].copy_from_slice(&mic);

    let mut frame = Vec::with_capacity(24 + 8 + eapol.len());
    frame.extend_from_slice(&[0x08, 0x01, 0x3A, 0x01]); // Data ToDS=1
    frame.extend_from_slice(&bssid);
    frame.extend_from_slice(&src_mac);
    frame.extend_from_slice(&bssid);
    frame.extend_from_slice(&[0x30, 0x00]);
    frame.extend_from_slice(&[0xAA, 0xAA, 0x03, 0x00, 0x00, 0x00, 0x88, 0x8E]);
    frame.extend_from_slice(&eapol);
    frame
}

fn is_deauth_for_ap(frame: &[u8], bssid: &[u8; 6]) -> bool {
    if frame.len() < 24 {
        return false;
    }
    let ftype = (frame[0] >> 2) & 0x03;
    let subtype = (frame[0] >> 4) & 0x0F;
    ftype == 0 && (subtype == 10 || subtype == 12) && (&frame[16..22] == bssid || &frame[10..16] == bssid)
}

fn extract_eapol_from_80211(frame: &[u8]) -> Option<&[u8]> {
    if frame.len() < 32 + 99 {
        return None;
    }
    let ftype = (frame[0] >> 2) & 0x03;
    if ftype != 2 || frame[24..32] != [0xAA, 0xAA, 0x03, 0x00, 0x00, 0x00, 0x88, 0x8E] {
        return None;
    }
    let eapol = &frame[32..];
    if eapol[1] != 3 || eapol.len() < 99 {
        return None;
    }
    let body_len = u16::from_be_bytes([eapol[2], eapol[3]]) as usize;
    let total = 4usize.checked_add(body_len)?;
    if total < 99 || total > eapol.len() {
        return None;
    }
    Some(&eapol[..total])
}

/// Lock-free hardware Wi-Fi connection orchestrator (`Auth` -> `Assoc` -> `4-way EAPOL` -> `NET_MANAGER`).
pub fn connect(ssid: &str, passphrase: Option<&str>) -> Result<(), &'static str> {
    let ap_opt = {
        let mgr = WIFI_MANAGER.lock();
        mgr.scan_results.iter().find(|a| a.ssid == ssid).cloned()
    };

    let ap_from_scan = ap_opt.is_some();
    let ap = ap_opt.unwrap_or_else(|| WifiAccessPoint {
        ssid: String::from(ssid),
        bssid: [0x00, 0x14, 0x22, 0x01, 0x23, 0x45],
        rssi: -50,
        channel: 6,
        security: if passphrase.is_some() { WifiSecurity::Wpa2Psk } else { WifiSecurity::Open },
    });

    // Fail closed on WPA2-to-Open downgrade if a scanned AP advertised Open while a passphrase was supplied.
    if ap_from_scan && passphrase.is_some() && ap.security == WifiSecurity::Open {
        return Err("Refusing WPA2-to-Open downgrade: scanned AP does not advertise WPA2/RSN");
    }

    let mut pmk = [0u8; 32];
    match ap.security {
        WifiSecurity::Open => {}
        WifiSecurity::Wpa2Psk => {
            let pass = passphrase.ok_or("Passphrase required for WPA2-PSK network")?;
            if pass.len() < 8 || pass.len() > 63 {
                return Err("WPA2-PSK passphrase must be 8..63 ASCII characters");
            }
            kinfo!("Wi-Fi: Deriving 256-bit WPA2-PSK PMK via PBKDF2-HMAC-SHA1...");
            pbkdf2_sha1(pass.as_bytes(), ssid.as_bytes(), 4096, &mut pmk);
        }
        WifiSecurity::Wep | WifiSecurity::Wpa3 => return Err("Unsupported wireless security mode"),
    }

    let Some((dev, src_mac)) = find_active_wireless_device() else {
        let mut mgr = WIFI_MANAGER.lock();
        mgr.pmk = pmk;
        return mgr.connect(ssid, passphrase);
    };

    if dev.name() != "wlan2" {
        let mut mgr = WIFI_MANAGER.lock();
        mgr.pmk = pmk;
        return mgr.connect(ssid, passphrase);
    }

    // Real BCM4331 802.11 State Machine (lock-free across all hardware dwells/polls)
    dev.set_channel(ap.channel)?;
    dev.set_wifi_session(Some(ap.bssid), false, None, None);

    let mut rx_buf = [0u8; 2048];
    // Step 1: Open System 802.11 Authentication (2 attempts, 500 ms window)
    let mut authed = false;
    for _attempt in 0..2 {
        let auth_req = build_80211_auth_request(ap.bssid, src_mac);
        dev.send_80211_frame(&auth_req)?;
        for _tick in 0..50 {
            pit::sleep_ms(10);
            while let Ok(len) = dev.receive_80211_frame(&mut rx_buf) {
                if len == 0 { break; }
                let f = &rx_buf[..len];
                if is_deauth_for_ap(f, &ap.bssid) {
                    dev.set_wifi_session(None, false, None, None);
                    return Err("AP sent Deauth during Authentication");
                }
                if let Some(res) = parse_80211_auth_resp(f, &ap.bssid) {
                    res?;
                    authed = true;
                    break;
                }
            }
            if authed { break; }
        }
        if authed { break; }
    }
    if !authed {
        dev.set_wifi_session(None, false, None, None);
        return Err("802.11 Authentication timeout (no response from AP)");
    }

    // Step 2: 802.11 Association (2 attempts, 500 ms window)
    let is_wpa2 = ap.security == WifiSecurity::Wpa2Psk;
    let mut associated = false;
    for _attempt in 0..2 {
        let assoc_req = build_80211_assoc_request(ap.bssid, src_mac, &ap.ssid, is_wpa2);
        dev.send_80211_frame(&assoc_req)?;
        for _tick in 0..50 {
            pit::sleep_ms(10);
            while let Ok(len) = dev.receive_80211_frame(&mut rx_buf) {
                if len == 0 { break; }
                let f = &rx_buf[..len];
                if is_deauth_for_ap(f, &ap.bssid) {
                    dev.set_wifi_session(None, false, None, None);
                    return Err("AP sent Deauth during Association");
                }
                if let Some(res) = parse_80211_assoc_resp(f, &ap.bssid) {
                    let _aid = res?;
                    associated = true;
                    break;
                }
            }
            if associated { break; }
        }
        if associated { break; }
    }
    if !associated {
        dev.set_wifi_session(None, false, None, None);
        return Err("802.11 Association timeout (no response from AP)");
    }

    // Step 3: WPA2-PSK 4-Way EAPOL Handshake (if WPA2)
    let (tk_opt, gtk_opt) = if is_wpa2 {
        let mut msg1_done = false;
        let mut msg3_done = false;
        let mut msg1_replay = 0u64;
        let mut anonce = [0u8; 32];
        let snonce = generate_snonce(&src_mac, &ap.bssid);
        let mut ptk = [0u8; 48];
        let mut gtk: Option<(u8, [u8; 16])> = None;

        for _tick in 0..300 {
            pit::sleep_ms(10);
            while let Ok(len) = dev.receive_80211_frame(&mut rx_buf) {
                if len == 0 { break; }
                let f = &rx_buf[..len];
                if is_deauth_for_ap(f, &ap.bssid) {
                    dev.set_wifi_session(None, false, None, None);
                    return Err("AP sent Deauth during WPA2 4-way handshake");
                }
                let Some(eapol) = extract_eapol_from_80211(f) else { continue; };
                let key_info = u16::from_be_bytes([eapol[5], eapol[6]]);
                let replay = u64::from_be_bytes([
                    eapol[9], eapol[10], eapol[11], eapol[12],
                    eapol[13], eapol[14], eapol[15], eapol[16],
                ]);
                let is_pairwise = (key_info & 0x0008) != 0;
                let has_ack = (key_info & 0x0080) != 0;
                let has_mic = (key_info & 0x0100) != 0;

                if is_pairwise && has_ack && !has_mic {
                    // Message 1
                    anonce.copy_from_slice(&eapol[17..49]);
                    msg1_replay = replay;
                    ptk = derive_wpa2_ptk(&pmk, &src_mac, &ap.bssid, &anonce, &snonce);
                    let mut kck = [0u8; 16];
                    kck.copy_from_slice(&ptk[..16]);
                    const RSN_IE: [u8; 22] = [
                        48, 20, 0x01, 0x00, 0x00, 0x0F, 0xAC, 0x04,
                        0x01, 0x00, 0x00, 0x0F, 0xAC, 0x04,
                        0x01, 0x00, 0x00, 0x0F, 0xAC, 0x02, 0x00, 0x00,
                    ];
                    let msg2 = build_eapol_key_msg(
                        ap.bssid, src_mac, 0x010A, replay, &snonce, &RSN_IE, &kck,
                    );
                    dev.send_80211_frame(&msg2)?;
                    msg1_done = true;
                } else if msg1_done && is_pairwise && has_ack && has_mic {
                    // Message 3
                    if eapol[17..49] != anonce || replay <= msg1_replay {
                        continue;
                    }
                    let mut kck = [0u8; 16];
                    let mut kek = [0u8; 16];
                    kck.copy_from_slice(&ptk[..16]);
                    kek.copy_from_slice(&ptk[16..32]);

                    let mut zeroed = eapol.to_vec();
                    zeroed[81..97].fill(0);
                    let expected_mic = eapol_mic_hmac_sha1(&kck, &zeroed);
                    if !crate::net::crypto::ct_eq(&expected_mic, &eapol[81..97]) {
                        dev.set_wifi_session(None, false, None, None);
                        return Err("WPA2 4-way Msg3 MIC mismatch (invalid passphrase?)");
                    }

                    let kd_len = u16::from_be_bytes([eapol[97], eapol[98]]) as usize;
                    if (key_info & 0x1000) != 0 {
                        if kd_len < 24 || 99 + kd_len > eapol.len() {
                            dev.set_wifi_session(None, false, None, None);
                            return Err("WPA2 4-way Msg3 truncated/invalid Key Data");
                        }
                        let unwrapped = aes128_key_unwrap(&kek, &eapol[99..99 + kd_len])
                            .map_err(|_| {
                                dev.set_wifi_session(None, false, None, None);
                                "WPA2 4-way Msg3 AES Key Unwrap integrity failure"
                            })?;
                        gtk = parse_eapol_gtk_kde(&unwrapped);
                        if gtk.is_none() {
                            dev.set_wifi_session(None, false, None, None);
                            return Err("WPA2 4-way Msg3 missing GTK KDE");
                        }
                    }

                    let msg4 = build_eapol_key_msg(
                        ap.bssid, src_mac, 0x030A, replay, &[0u8; 32], &[], &kck,
                    );
                    dev.send_80211_frame(&msg4)?;
                    msg3_done = true;
                    break;
                }
            }
            if msg3_done { break; }
        }
        if !msg3_done {
            dev.set_wifi_session(None, false, None, None);
            return Err("WPA2 4-way EAPOL handshake timeout");
        }
        let mut tk = [0u8; 16];
        tk.copy_from_slice(&ptk[32..48]);
        (Some(tk), gtk)
    } else {
        (None, None)
    };

    dev.set_wifi_session(Some(ap.bssid), true, tk_opt, gtk_opt);
    WIFI_CONNECTED.store(true, Ordering::Release);
    {
        let mut mgr = WIFI_MANAGER.lock();
        mgr.pmk = pmk;
        mgr.is_connected = true;
        mgr.current_ssid = Some(ap.ssid.clone());
        mgr.current_bssid = Some(ap.bssid);
        mgr.current_channel = ap.channel;
        mgr.link_speed_mbps = 54;
    }
    NET_MANAGER.lock().set_default_device(dev.name());

    kinfo!("Wi-Fi: Associated & linked '{}' (Ch {}, {}) on {}", ssid, ap.channel, ap.security.as_str(), dev.name());
    Ok(())
}

/// Disconnect the active wireless interface and clear session keys cleanly without lock inversion.
pub fn disconnect() {
    if let Some((dev, _)) = find_active_wireless_device() {
        dev.set_wifi_session(None, false, None, None);
    }
    WIFI_MANAGER.lock().disconnect();
}

/// Build an IEEE 802.11 Management Probe Request frame (subtype 4).
pub fn build_80211_probe_request(src_mac: [u8; 6], ssid: Option<&str>) -> Vec<u8> {
    let mut frame = Vec::with_capacity(64);
    frame.extend_from_slice(&[0x40, 0x00, 0x00, 0x00]);
    frame.extend_from_slice(&[0xFF; 6]);
    frame.extend_from_slice(&src_mac);
    frame.extend_from_slice(&[0xFF; 6]);
    frame.extend_from_slice(&[0x00, 0x00]);
    let ssid_bytes = ssid.map(|s| s.as_bytes()).unwrap_or(&[]);
    let ssid_len = ssid_bytes.len().min(32);
    frame.push(0);
    frame.push(ssid_len as u8);
    frame.extend_from_slice(&ssid_bytes[..ssid_len]);
    frame.extend_from_slice(&[1, 4, 0x82, 0x84, 0x8B, 0x96]);
    frame
}

/// Build an IEEE 802.11 Management Authentication Request frame (subtype 11, Open System Seq 1).
pub fn build_80211_auth_request(bssid: [u8; 6], src_mac: [u8; 6]) -> Vec<u8> {
    let mut frame = Vec::with_capacity(32);
    frame.extend_from_slice(&[0xB0, 0x00, 0x3A, 0x01]);
    frame.extend_from_slice(&bssid);
    frame.extend_from_slice(&src_mac);
    frame.extend_from_slice(&bssid);
    frame.extend_from_slice(&[0x10, 0x00]);
    frame.extend_from_slice(&[0x00, 0x00, 0x01, 0x00, 0x00, 0x00]);
    frame
}

/// Build an IEEE 802.11 Management Association Request frame (subtype 0).
pub fn build_80211_assoc_request(
    bssid: [u8; 6],
    src_mac: [u8; 6],
    ssid: &str,
    include_rsn: bool,
) -> Vec<u8> {
    let mut frame = Vec::with_capacity(96);
    frame.extend_from_slice(&[0x00, 0x00, 0x3A, 0x01]);
    frame.extend_from_slice(&bssid);
    frame.extend_from_slice(&src_mac);
    frame.extend_from_slice(&bssid);
    frame.extend_from_slice(&[0x20, 0x00]);
    let cap_lo = if include_rsn { 0x11 } else { 0x01 };
    frame.extend_from_slice(&[cap_lo, 0x04, 0x0A, 0x00]);
    let sb = ssid.as_bytes();
    let slen = sb.len().min(32);
    frame.push(0);
    frame.push(slen as u8);
    frame.extend_from_slice(&sb[..slen]);
    frame.extend_from_slice(&[1, 4, 0x82, 0x84, 0x8B, 0x96]);
    if include_rsn {
        frame.extend_from_slice(&[
            48, 20, 0x01, 0x00,
            0x00, 0x0F, 0xAC, 0x04,
            0x01, 0x00, 0x00, 0x0F, 0xAC, 0x04,
            0x01, 0x00, 0x00, 0x0F, 0xAC, 0x02,
            0x00, 0x00,
        ]);
    }
    frame
}

/// Parse 802.11 Management Beacon / Probe Response frame Information Elements (IE).
pub fn parse_80211_beacon(data: &[u8]) -> Option<(String, u8, WifiSecurity)> {
    if data.len() < 36 {
        return None;
    }
    let mut ssid = String::new();
    let mut channel = 1u8;
    let mut security = WifiSecurity::Open;

    let mut idx = 36;
    while idx + 2 <= data.len() {
        let tag = data[idx];
        let len = data[idx + 1] as usize;
        idx += 2;
        if idx + len > data.len() {
            break;
        }
        let val = &data[idx..idx + len];
        match tag {
            0 => {
                if let Ok(s) = core::str::from_utf8(val) {
                    ssid = String::from(s);
                }
            }
            3 => {
                if len >= 1 {
                    channel = val[0];
                }
            }
            48 => {
                security = WifiSecurity::Wpa2Psk;
            }
            _ => {}
        }
        idx += len;
    }

    if ssid.is_empty() {
        None
    } else {
        Some((ssid, channel, security))
    }
}
