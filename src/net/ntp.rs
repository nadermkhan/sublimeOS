extern crate alloc;
use alloc::format;
use alloc::string::String;
use crate::sync::IrqSafeSpinlock;
use crate::kinfo;

pub const NTP_PORT: u16 = 123;

/// Difference in seconds between NTP epoch (1900-01-01) and UNIX epoch (1970-01-01):
/// 70 years including 17 leap years: (70 * 365 + 17) * 86400 = 2,208,988,800.
pub const NTP_UNIX_OFFSET: u64 = 2_208_988_800;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NtpTime {
    pub unix_seconds: u64,
    pub fraction: u32,
    pub milliseconds: u32,
    pub stratum: u8,
    pub precision: i8,
    pub root_delay_ms: u32,
    pub root_dispersion_ms: u32,
    pub ref_id: [u8; 4],
}

pub struct NtpClientState {
    pub last_synced_time: Option<NtpTime>,
    pub server_ip: [u8; 4],
    pub sync_count: u32,
    pub last_sync_tick: u64,
    pub roundtrip_delay_ms: u32,
}

impl NtpClientState {
    pub const fn new() -> Self {
        Self {
            last_synced_time: None,
            server_ip: [10, 0, 2, 2], // Standard QEMU host / default gateway
            sync_count: 0,
            last_sync_tick: 0,
            roundtrip_delay_ms: 0,
        }
    }
}

pub static NTP_CLIENT: IrqSafeSpinlock<NtpClientState> = IrqSafeSpinlock::new(NtpClientState::new());

/// Build an RFC 5905 compliant 48-byte NTP client request.
pub fn build_ntp_request(client_unix_secs: u64) -> [u8; 48] {
    let mut pkt = [0u8; 48];

    // Byte 0: LI = 0 (no warning), VN = 4 (IPv4 NTPv4), Mode = 3 (Client)
    // (0 << 6) | (4 << 3) | 3 = 0x23 (35)
    pkt[0] = 0x23;
    pkt[1] = 0; // Stratum: 0 (unspecified)
    pkt[2] = 4; // Poll interval: 16s (2^4)
    pkt[3] = 0xEC; // Precision: -20 (signed ~1 microsecond)

    // Root Delay (0), Root Dispersion (0), Ref ID (0) -> bytes 4..16 are 0
    // Reference Timestamp (bytes 16..24) -> 0
    // Origin Timestamp (bytes 24..32) -> 0
    // Receive Timestamp (bytes 32..40) -> 0

    // Transmit Timestamp (bytes 40..48):
    let ntp_secs = (client_unix_secs + NTP_UNIX_OFFSET) as u32;
    pkt[40..44].copy_from_slice(&ntp_secs.to_be_bytes());
    // Microsecond fraction from PIT timer ticks
    let ticks = crate::arch::x86_64::pit::get_ticks();
    let frac = (ticks as u32).wrapping_mul(4294967);
    pkt[44..48].copy_from_slice(&frac.to_be_bytes());

    pkt
}

/// Parse an RFC 5905 NTP server reply packet (48 bytes minimum).
pub fn parse_ntp_response(data: &[u8]) -> Option<NtpTime> {
    if data.len() < 48 {
        return None;
    }

    let mode = data[0] & 0x07;
    // Mode 4 is Server, Mode 5 is Broadcast
    if mode != 4 && mode != 5 {
        return None;
    }

    let stratum = data[1];
    // Stratum 0 represents Kiss-o'-Death or unsynchronized
    if stratum == 0 || stratum > 16 {
        return None;
    }

    let precision = data[3] as i8;

    let root_delay_raw = u32::from_be_bytes([data[4], data[5], data[6], data[7]]);
    let root_disp_raw = u32::from_be_bytes([data[8], data[9], data[10], data[11]]);
    let root_delay_ms = ((root_delay_raw as u64 * 1000) >> 16) as u32;
    let root_dispersion_ms = ((root_disp_raw as u64 * 1000) >> 16) as u32;

    let mut ref_id = [0u8; 4];
    ref_id.copy_from_slice(&data[12..16]);

    let tx_secs = u32::from_be_bytes([data[40], data[41], data[42], data[43]]) as u64;
    let tx_frac = u32::from_be_bytes([data[44], data[45], data[46], data[47]]);

    if tx_secs < NTP_UNIX_OFFSET {
        return None; // Pre-1970 timestamp is invalid
    }

    let unix_seconds = tx_secs - NTP_UNIX_OFFSET;
    let milliseconds = ((tx_frac as u64 * 1000) >> 32) as u32;

    Some(NtpTime {
        unix_seconds,
        fraction: tx_frac,
        milliseconds,
        stratum,
        precision,
        root_delay_ms,
        root_dispersion_ms,
        ref_id,
    })
}

/// Send an NTP request packet to the specified server IP.
pub fn send_ntp_request(server_ip: [u8; 4]) -> Result<(), &'static str> {
    let net = crate::net::device::NET_MANAGER.lock();
    let (dev, cfg) = net.get_device_for_ip(server_ip).ok_or("No route to NTP server")?;
    let src_ip = cfg.ipv4;
    let dev_mac = dev.mac_address();
    let dev_type = dev.device_type();
    drop(net);

    let cur_unix = crate::arch::x86_64::rtc::get_system_time_unix();
    let payload = build_ntp_request(cur_unix);

    let udp_pkt = crate::net::udp::build_udp_packet(NTP_PORT, NTP_PORT, &payload);
    let ip_pkt = crate::net::ip::build_ipv4_packet(src_ip, server_ip, crate::net::ip::IP_PROTO_UDP, &udp_pkt);

    let next_hop = if dev_type == crate::net::device::NetDeviceType::Loopback {
        server_ip
    } else {
        let is_local = (0..4).all(|i| (server_ip[i] & cfg.netmask[i]) == (cfg.ipv4[i] & cfg.netmask[i]));
        if is_local || cfg.gateway == [0, 0, 0, 0] {
            server_ip
        } else {
            cfg.gateway
        }
    };

    let dst_mac = crate::net::arp::ARP_CACHE
        .lock()
        .lookup(&next_hop)
        .unwrap_or(crate::net::ethernet::BROADCAST_MAC);

    let frame = crate::net::ethernet::build_frame(
        dst_mac,
        dev_mac,
        crate::net::ethernet::ETHERTYPE_IPV4,
        &ip_pkt,
    );

    dev.send_packet(&frame)?;

    {
        let mut client = NTP_CLIENT.lock();
        client.server_ip = server_ip;
    }

    kinfo!(
        "NTP: Transmitted NTPv4 request to {}.{}.{}.{}:123",
        server_ip[0],
        server_ip[1],
        server_ip[2],
        server_ip[3]
    );

    Ok(())
}

/// Handle an incoming NTP response datagram.
pub fn handle_ntp_packet(src_ip: [u8; 4], payload: &[u8]) -> bool {
    if let Some(ntp_time) = parse_ntp_response(payload) {
        crate::arch::x86_64::rtc::set_system_time(ntp_time.unix_seconds);

        let dt = crate::arch::x86_64::rtc::unix_to_datetime(ntp_time.unix_seconds);
        let current_tick = crate::arch::x86_64::pit::get_ticks();

        {
            let mut client = NTP_CLIENT.lock();
            client.last_synced_time = Some(ntp_time);
            client.server_ip = src_ip;
            client.sync_count = client.sync_count.saturating_add(1);
            if client.last_sync_tick > 0 {
                client.roundtrip_delay_ms = (current_tick.saturating_sub(client.last_sync_tick)) as u32;
            }
            client.last_sync_tick = current_tick;
        }

        kinfo!(
            "NTP: Synchronized clock with {}.{}.{}.{} (Stratum {}): {:04}-{:02}-{:02} {:02}:{:02}:{:02} UTC (Unix: {})",
            src_ip[0],
            src_ip[1],
            src_ip[2],
            src_ip[3],
            ntp_time.stratum,
            dt.year,
            dt.month,
            dt.day,
            dt.hour,
            dt.minute,
            dt.second,
            ntp_time.unix_seconds
        );

        true
    } else {
        false
    }
}

/// Format the current NTP client status for display in CLI or GUI.
pub fn format_ntp_status() -> String {
    let client = NTP_CLIENT.lock();
    let is_synced = crate::arch::x86_64::rtc::is_time_synchronized();
    let dt = crate::arch::x86_64::rtc::get_system_datetime();
    let unix = crate::arch::x86_64::rtc::get_system_time_unix();

    let mut out = String::new();
    out.push_str("=== Network Time Protocol (NTP) Status ===\n");
    out.push_str(&format!(
        "System Clock:      {:04}-{:02}-{:02} {:02}:{:02}:{:02} UTC (Unix: {})\n",
        dt.year, dt.month, dt.day, dt.hour, dt.minute, dt.second, unix
    ));
    out.push_str(&format!(
        "Synchronized:      {}\n",
        if is_synced { "YES (Active NTP Anchor)" } else { "NO (CMOS RTC Fallback)" }
    ));
    out.push_str(&format!(
        "NTP Server:        {}.{}.{}.{}\n",
        client.server_ip[0], client.server_ip[1], client.server_ip[2], client.server_ip[3]
    ));
    out.push_str(&format!("Sync Count:        {}\n", client.sync_count));

    if let Some(t) = client.last_synced_time {
        out.push_str(&format!("Stratum:           {}\n", t.stratum));
        out.push_str(&format!("Precision:         2^{} s\n", t.precision));
        out.push_str(&format!("Root Delay:        {} ms\n", t.root_delay_ms));
        out.push_str(&format!("Root Dispersion:   {} ms\n", t.root_dispersion_ms));
        if t.stratum == 1 {
            let ref_str = core::str::from_utf8(&t.ref_id).unwrap_or("????");
            out.push_str(&format!("Reference ID:      {}\n", ref_str));
        } else {
            out.push_str(&format!(
                "Reference ID:      {}.{}.{}.{}\n",
                t.ref_id[0], t.ref_id[1], t.ref_id[2], t.ref_id[3]
            ));
        }
    }

    out
}
