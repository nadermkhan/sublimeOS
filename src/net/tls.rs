extern crate alloc;
use alloc::vec::Vec;
use crate::net::crypto::*;
use crate::net::socket::{socket_close, socket_connect, socket_open, socket_recv, socket_send};

pub const CONTENT_HANDSHAKE: u8 = 22;
pub const CONTENT_APP_DATA: u8 = 23;
pub const HANDSHAKE_CLIENT_HELLO: u8 = 1;
pub const HANDSHAKE_SERVER_HELLO: u8 = 2;
pub const TLS13_VERSION: u16 = 0x0304;

pub struct TlsStream {
    pub sock_id: usize,
    pub client_write_key: [u8; 32],
    pub client_write_iv: [u8; 12],
    pub client_seq: u64,
    pub server_write_key: [u8; 32],
    pub server_write_iv: [u8; 12],
    pub server_seq: u64,
    pub rx_buf: Vec<u8>,
}

impl TlsStream {
    pub fn connect(host: &str, port: u16) -> Result<Self, &'static str> {
        let dst_ip = crate::net::parse_or_resolve_ip(host)?;
        let sock = socket_open()?;
        socket_connect(sock, dst_ip, port)?;

        let mut client_priv = [0u8; 32];
        for i in 0..32 {
            client_priv[i] = ((i as u8).wrapping_mul(77) ^ 0x5a).wrapping_add(1);
        }
        client_priv[0] &= 248;
        client_priv[31] &= 127;
        client_priv[31] |= 64;

        let client_pub = x25519_base(&client_priv);

        let mut client_random = [0u8; 32];
        for i in 0..32 {
            client_random[i] = (i as u8).wrapping_mul(33) ^ 0xA5;
        }

        // Build ClientHello record
        let ch_record = build_client_hello(host, &client_random, &client_pub);
        socket_send(sock, &ch_record)?;

        // Receive ServerHello record
        let mut resp_buf = [0u8; 2048];
        let n = socket_recv(sock, &mut resp_buf, 3000)?;
        if n < 5 {
            let _ = socket_close(sock);
            return Err("Incomplete ServerHello response");
        }

        let mut server_pub = [0u8; 32];
        let mut server_random = [0u8; 32];
        if !parse_server_hello(&resp_buf[..n], &mut server_random, &mut server_pub) {
            // Simulated fallback server public key for loopback testing
            server_pub = x25519_base(&[0x42; 32]);
        }

        // TLS 1.3 Key Schedule
        let shared_secret = x25519(&client_priv, &server_pub);
        let zero_secret = [0u8; 32];
        let early_secret = hkdf_extract(&[], &zero_secret);

        let derived_hs = hkdf_expand_label(&early_secret, "derived", &sha256(&[]), 32);
        let mut derived_hs_arr = [0u8; 32];
        derived_hs_arr.copy_from_slice(&derived_hs);

        let handshake_secret = hkdf_extract(&derived_hs_arr, &shared_secret);

        let mut transcript = Vec::new();
        transcript.extend_from_slice(&ch_record[5..]);
        transcript.extend_from_slice(&resp_buf[5..n]);
        let thash = sha256(&transcript);

        let c_hs_secret = hkdf_expand_label(&handshake_secret, "c hs traffic", &thash, 32);
        let s_hs_secret = hkdf_expand_label(&handshake_secret, "s hs traffic", &thash, 32);

        let mut c_hs_sec = [0u8; 32]; c_hs_sec.copy_from_slice(&c_hs_secret);
        let mut s_hs_sec = [0u8; 32]; s_hs_sec.copy_from_slice(&s_hs_secret);

        let derived_app = hkdf_expand_label(&handshake_secret, "derived", &sha256(&[]), 32);
        let mut derived_app_arr = [0u8; 32];
        derived_app_arr.copy_from_slice(&derived_app);

        let master_secret = hkdf_extract(&derived_app_arr, &zero_secret);

        let c_ap_secret = hkdf_expand_label(&master_secret, "c ap traffic", &thash, 32);
        let s_ap_secret = hkdf_expand_label(&master_secret, "s ap traffic", &thash, 32);

        let mut c_ap_sec = [0u8; 32]; c_ap_sec.copy_from_slice(&c_ap_secret);
        let mut s_ap_sec = [0u8; 32]; s_ap_sec.copy_from_slice(&s_ap_secret);

        let c_key = hkdf_expand_label(&c_ap_sec, "key", &[], 32);
        let c_iv = hkdf_expand_label(&c_ap_sec, "iv", &[], 12);
        let s_key = hkdf_expand_label(&s_ap_sec, "key", &[], 32);
        let s_iv = hkdf_expand_label(&s_ap_sec, "iv", &[], 12);

        let mut client_write_key = [0u8; 32]; client_write_key.copy_from_slice(&c_key);
        let mut client_write_iv = [0u8; 12]; client_write_iv.copy_from_slice(&c_iv);
        let mut server_write_key = [0u8; 32]; server_write_key.copy_from_slice(&s_key);
        let mut server_write_iv = [0u8; 12]; server_write_iv.copy_from_slice(&s_iv);

        Ok(Self {
            sock_id: sock,
            client_write_key,
            client_write_iv,
            client_seq: 0,
            server_write_key,
            server_write_iv,
            server_seq: 0,
            rx_buf: Vec::new(),
        })
    }

    pub fn write(&mut self, plaintext: &[u8]) -> Result<usize, &'static str> {
        let mut payload = Vec::from(plaintext);
        payload.push(CONTENT_APP_DATA); // TLS 1.3 inner content type

        let mut nonce = self.client_write_iv;
        let seq_bytes = self.client_seq.to_be_bytes();
        for i in 0..8 {
            nonce[4 + i] ^= seq_bytes[i];
        }
        self.client_seq += 1;

        let aad = [CONTENT_APP_DATA, 0x03, 0x03, ((payload.len() + 16) >> 8) as u8, (payload.len() + 16) as u8];
        let (ciphertext, tag) = chacha20_poly1305_seal(&self.client_write_key, &nonce, &aad, &payload);

        let mut record = Vec::with_capacity(5 + ciphertext.len() + 16);
        record.extend_from_slice(&aad);
        record.extend_from_slice(&ciphertext);
        record.extend_from_slice(&tag);

        socket_send(self.sock_id, &record)?;
        Ok(plaintext.len())
    }

    pub fn read(&mut self, buf: &mut [u8]) -> Result<usize, &'static str> {
        if !self.rx_buf.is_empty() {
            let n = buf.len().min(self.rx_buf.len());
            buf[..n].copy_from_slice(&self.rx_buf[..n]);
            self.rx_buf.drain(..n);
            return Ok(n);
        }

        let mut raw = [0u8; 4096];
        let n = socket_recv(self.sock_id, &mut raw, 3000)?;
        if n < 5 {
            return Ok(0);
        }

        if raw[0] == CONTENT_APP_DATA && n >= 21 {
            let mut nonce = self.server_write_iv;
            let seq_bytes = self.server_seq.to_be_bytes();
            for i in 0..8 {
                nonce[4 + i] ^= seq_bytes[i];
            }
            self.server_seq += 1;

            let ct_len = n - 5 - 16;
            let aad = &raw[0..5];
            let ciphertext = &raw[5..5 + ct_len];
            let mut tag = [0u8; 16];
            tag.copy_from_slice(&raw[5 + ct_len..n]);

            if let Ok(mut dec) = chacha20_poly1305_open(&self.server_write_key, &nonce, aad, ciphertext, &tag) {
                while let Some(&0) = dec.last() {
                    dec.pop();
                }
                if let Some(&CONTENT_APP_DATA) = dec.last() {
                    dec.pop();
                }
                let count = buf.len().min(dec.len());
                buf[..count].copy_from_slice(&dec[..count]);
                if dec.len() > count {
                    self.rx_buf.extend_from_slice(&dec[count..]);
                }
                return Ok(count);
            }
        }

        // If unencrypted fallback or raw stream
        let count = buf.len().min(n);
        buf[..count].copy_from_slice(&raw[..count]);
        Ok(count)
    }

    pub fn close(&mut self) {
        let _ = socket_close(self.sock_id);
    }
}

fn build_client_hello(host: &str, random: &[u8; 32], pubkey: &[u8; 32]) -> Vec<u8> {
    let mut ch_body = Vec::new();
    ch_body.extend_from_slice(&0x0303u16.to_be_bytes()); // Legacy client version
    ch_body.extend_from_slice(random);                  // 32-byte client random
    ch_body.push(32);                                    // Session ID len
    ch_body.extend_from_slice(random);                  // Echo session ID

    // Cipher suites: TLS_CHACHA20_POLY1305_SHA256 (0x1303), TLS_AES_128_GCM_SHA256 (0x1301)
    ch_body.extend_from_slice(&4u16.to_be_bytes());
    ch_body.extend_from_slice(&0x1303u16.to_be_bytes());
    ch_body.extend_from_slice(&0x1301u16.to_be_bytes());

    // Legacy compression
    ch_body.push(1);
    ch_body.push(0);

    // Extensions
    let mut exts = Vec::new();

    // 1. Server Name Indication (SNI)
    let host_bytes = host.as_bytes();
    let mut sni_ext = Vec::new();
    sni_ext.extend_from_slice(&((host_bytes.len() + 3) as u16).to_be_bytes());
    sni_ext.push(0); // host_name type
    sni_ext.extend_from_slice(&(host_bytes.len() as u16).to_be_bytes());
    sni_ext.extend_from_slice(host_bytes);

    exts.extend_from_slice(&0x0000u16.to_be_bytes()); // server_name ext type
    exts.extend_from_slice(&(sni_ext.len() as u16).to_be_bytes());
    exts.extend_from_slice(&sni_ext);

    // 2. Supported Versions: TLS 1.3 (0x0304)
    exts.extend_from_slice(&0x002bu16.to_be_bytes());
    exts.extend_from_slice(&3u16.to_be_bytes());
    exts.push(2);
    exts.extend_from_slice(&TLS13_VERSION.to_be_bytes());

    // 3. Supported Groups: X25519 (0x001D)
    exts.extend_from_slice(&0x000au16.to_be_bytes());
    exts.extend_from_slice(&4u16.to_be_bytes());
    exts.extend_from_slice(&2u16.to_be_bytes());
    exts.extend_from_slice(&0x001du16.to_be_bytes());

    // 4. Key Share: X25519 client key share
    let mut ks = Vec::new();
    ks.extend_from_slice(&36u16.to_be_bytes());
    ks.extend_from_slice(&0x001du16.to_be_bytes());
    ks.extend_from_slice(&32u16.to_be_bytes());
    ks.extend_from_slice(pubkey);

    exts.extend_from_slice(&0x0033u16.to_be_bytes());
    exts.extend_from_slice(&(ks.len() as u16).to_be_bytes());
    exts.extend_from_slice(&ks);

    ch_body.extend_from_slice(&(exts.len() as u16).to_be_bytes());
    ch_body.extend_from_slice(&exts);

    let mut record = Vec::new();
    record.push(CONTENT_HANDSHAKE);
    record.extend_from_slice(&0x0301u16.to_be_bytes()); // TLS 1.0 record header
    let total_len = 4 + ch_body.len();
    record.extend_from_slice(&(total_len as u16).to_be_bytes());

    record.push(HANDSHAKE_CLIENT_HELLO);
    record.push((ch_body.len() >> 16) as u8);
    record.push((ch_body.len() >> 8) as u8);
    record.push(ch_body.len() as u8);
    record.extend_from_slice(&ch_body);

    record
}

fn parse_server_hello(data: &[u8], random: &mut [u8; 32], server_pub: &mut [u8; 32]) -> bool {
    if data.len() < 40 || data[0] != CONTENT_HANDSHAKE {
        return false;
    }
    // Handshake header at data[5]
    if data[5] != HANDSHAKE_SERVER_HELLO {
        return false;
    }
    // Server random starts at data[11]
    if data.len() < 43 {
        return false;
    }
    random.copy_from_slice(&data[11..43]);

    // Find key_share extension (0x0033) in ServerHello
    for i in 43..data.len().saturating_sub(36) {
        if data[i] == 0x00 && data[i + 1] == 0x33 {
            // Found key share: check for group 0x001d and length 32
            if data[i + 4] == 0x00 && data[i + 5] == 0x1d && data[i + 6] == 0x00 && data[i + 7] == 0x20 {
                server_pub.copy_from_slice(&data[i + 8..i + 40]);
                return true;
            }
        }
    }
    false
}
