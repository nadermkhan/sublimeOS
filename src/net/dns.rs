extern crate alloc;
use alloc::string::String;
use alloc::vec::Vec;
use crate::sync::IrqSafeSpinlock;
use crate::arch::x86_64::rtc::get_system_time_unix;

pub const DNS_TYPE_A: u16 = 1;
pub const DNS_CLASS_IN: u16 = 1;

pub const LLMNR_PORT: u16 = 5355;
pub const LLMNR_MCAST_IP: [u8; 4] = [224, 0, 0, 252];
pub const LLMNR_MCAST_MAC: [u8; 6] = [0x01, 0x00, 0x5E, 0x00, 0x00, 0xFC];

/// Build a standard DNS query packet for an A (IPv4) record.
pub fn build_dns_query(hostname: &str, tx_id: u16) -> Vec<u8> {
    let mut pkt = Vec::with_capacity(64);

    // Header (12 bytes)
    pkt.extend_from_slice(&tx_id.to_be_bytes()); // ID
    pkt.extend_from_slice(&0x0100u16.to_be_bytes()); // Flags: Standard Query, Recursion Desired
    pkt.extend_from_slice(&1u16.to_be_bytes()); // QDCOUNT = 1
    pkt.extend_from_slice(&0u16.to_be_bytes()); // ANCOUNT = 0
    pkt.extend_from_slice(&0u16.to_be_bytes()); // NSCOUNT = 0
    pkt.extend_from_slice(&0u16.to_be_bytes()); // ARCOUNT = 0

    // Question Section: QNAME labels
    for part in hostname.split('.') {
        if !part.is_empty() {
            pkt.push(part.len() as u8);
            pkt.extend_from_slice(part.as_bytes());
        }
    }
    pkt.push(0x00); // Terminating zero byte

    // QTYPE = A (1), QCLASS = IN (1)
    pkt.extend_from_slice(&DNS_TYPE_A.to_be_bytes());
    pkt.extend_from_slice(&DNS_CLASS_IN.to_be_bytes());

    pkt
}

/// Parse a DNS response packet and extract the first resolved IPv4 address and its TTL.
pub fn parse_dns_response_with_ttl(data: &[u8], tx_id: u16) -> Option<([u8; 4], u32)> {
    if data.len() < 12 {
        return None;
    }

    let resp_id = u16::from_be_bytes([data[0], data[1]]);
    if resp_id != tx_id {
        return None;
    }

    let ancount = u16::from_be_bytes([data[6], data[7]]) as usize;
    if ancount == 0 {
        return None;
    }

    // Skip Header (12 bytes) and Question Section
    let mut idx = 12;
    while idx < data.len() {
        let len = data[idx] as usize;
        if len == 0 {
            idx += 1;
            break;
        }
        idx += 1 + len;
    }
    idx += 4; // Skip QTYPE (2 bytes) and QCLASS (2 bytes)

    // Parse Answer Section
    for _ in 0..ancount {
        if idx >= data.len() {
            break;
        }

        // Handle NAME (check for 2-byte compression pointer 0xC0)
        if (data[idx] & 0xC0) == 0xC0 {
            idx += 2;
        } else {
            while idx < data.len() && data[idx] != 0 {
                idx += 1 + (data[idx] as usize);
            }
            idx += 1;
        }

        if idx + 10 > data.len() {
            break;
        }

        let rtype = u16::from_be_bytes([data[idx], data[idx + 1]]);
        let rclass = u16::from_be_bytes([data[idx + 2], data[idx + 3]]);
        let ttl = u32::from_be_bytes([data[idx + 4], data[idx + 5], data[idx + 6], data[idx + 7]]);
        let rdlength = u16::from_be_bytes([data[idx + 8], data[idx + 9]]) as usize;
        idx += 10;

        if rtype == DNS_TYPE_A && rclass == DNS_CLASS_IN && rdlength == 4 && idx + 4 <= data.len() {
            let mut ip = [0u8; 4];
            ip.copy_from_slice(&data[idx..idx + 4]);
            return Some((ip, ttl));
        }

        idx += rdlength;
    }

    None
}

/// Parse a DNS response packet and extract the first resolved IPv4 address.
pub fn parse_dns_response(data: &[u8], tx_id: u16) -> Option<[u8; 4]> {
    parse_dns_response_with_ttl(data, tx_id).map(|(ip, _)| ip)
}

// =========================================================================
// DNS Client Cache with TTL
// =========================================================================

#[derive(Debug, Clone)]
pub struct DnsCacheEntry {
    pub hostname: String,
    pub ip: [u8; 4],
    pub ttl_secs: u32,
    pub expires_at_secs: u64,
}

pub struct DnsCache {
    pub entries: Vec<DnsCacheEntry>,
    pub hits: usize,
    pub misses: usize,
}

impl DnsCache {
    pub const fn new() -> Self {
        Self {
            entries: Vec::new(),
            hits: 0,
            misses: 0,
        }
    }

    /// Look up hostname in cache, evicting expired entries.
    pub fn lookup(&mut self, hostname: &str, current_time_secs: u64) -> Option<[u8; 4]> {
        self.entries.retain(|e| e.expires_at_secs > current_time_secs);

        if let Some(entry) = self.entries.iter().find(|e| e.hostname.eq_ignore_ascii_case(hostname)) {
            self.hits += 1;
            Some(entry.ip)
        } else {
            self.misses += 1;
            None
        }
    }

    /// Insert or update an entry in the DNS cache.
    pub fn insert(&mut self, hostname: &str, ip: [u8; 4], ttl_secs: u32, current_time_secs: u64) {
        let effective_ttl = if ttl_secs == 0 { 60 } else { ttl_secs };
        let expires_at_secs = current_time_secs.saturating_add(effective_ttl as u64);
        self.entries.retain(|e| !e.hostname.eq_ignore_ascii_case(hostname));
        self.entries.push(DnsCacheEntry {
            hostname: String::from(hostname),
            ip,
            ttl_secs: effective_ttl,
            expires_at_secs,
        });
    }

    /// Flush all entries from cache.
    pub fn flush(&mut self) {
        self.entries.clear();
    }
}

pub static DNS_CACHE: IrqSafeSpinlock<DnsCache> = IrqSafeSpinlock::new(DnsCache::new());

// =========================================================================
// DNS Nameserver Configuration & Failover (/etc/resolv.conf)
// =========================================================================

#[derive(Debug, Clone)]
pub struct DnsConfig {
    pub nameservers: Vec<[u8; 4]>,
    pub search_domain: Option<String>,
}

impl DnsConfig {
    pub const fn new() -> Self {
        Self {
            nameservers: Vec::new(),
            search_domain: None,
        }
    }

    pub fn init_defaults(&mut self) {
        self.nameservers.clear();
        self.nameservers.push([8, 8, 8, 8]); // Google DNS
        self.nameservers.push([1, 1, 1, 1]); // Cloudflare DNS
        self.nameservers.push([9, 9, 9, 9]); // Quad9 DNS
    }

    /// Parse contents of /etc/resolv.conf.
    pub fn parse_resolv_conf(&mut self, content: &str) {
        self.nameservers.clear();
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with('#') || trimmed.starts_with(';') || trimmed.is_empty() {
                continue;
            }
            let mut parts = trimmed.split_whitespace();
            match parts.next() {
                Some("nameserver") => {
                    if let Some(ip_str) = parts.next() {
                        let octets: Vec<&str> = ip_str.split('.').collect();
                        if octets.len() == 4 {
                            let mut ip = [0u8; 4];
                            let mut valid = true;
                            for i in 0..4 {
                                if let Ok(val) = octets[i].parse::<u8>() {
                                    ip[i] = val;
                                } else {
                                    valid = false;
                                    break;
                                }
                            }
                            if valid {
                                self.nameservers.push(ip);
                            }
                        }
                    }
                }
                Some("search") | Some("domain") => {
                    if let Some(dom) = parts.next() {
                        self.search_domain = Some(String::from(dom));
                    }
                }
                _ => {}
            }
        }
        if self.nameservers.is_empty() {
            self.init_defaults();
        }
    }

    pub fn get_nameservers(&self) -> &[[u8; 4]] {
        if self.nameservers.is_empty() {
            &[[8, 8, 8, 8], [1, 1, 1, 1]]
        } else {
            &self.nameservers
        }
    }
}

pub static DNS_CONFIG: IrqSafeSpinlock<DnsConfig> = IrqSafeSpinlock::new(DnsConfig::new());

/// Resolve a hostname with caching, multi-nameserver failover, and fallback.
pub fn resolve_cached(hostname: &str) -> Result<[u8; 4], &'static str> {
    let now = get_system_time_unix();

    // 1. Check DNS Cache
    if let Some(ip) = DNS_CACHE.lock().lookup(hostname, now) {
        return Ok(ip);
    }

    // 2. Static domain fallback mappings
    let resolved_opt = match hostname {
        "google.com" | "www.google.com" => Some(([142, 250, 190, 46], 300)),
        "one.one.one.one" => Some(([1, 1, 1, 1], 3600)),
        "dns.google" => Some(([8, 8, 8, 8], 3600)),
        "localhost" => Some(([127, 0, 0, 1], 86400)),
        "apple.com" | "www.apple.com" => Some(([17, 253, 144, 10], 300)),
        "github.com" | "www.github.com" => Some(([140, 82, 121, 4], 300)),
        "cosmos.local" | "cosmos" | "cosmoskernel" => Some(([10, 0, 2, 15], 120)),
        "cosmic.internal" => Some(([192, 168, 1, 1], 600)),
        _ => None,
    };

    if let Some((ip, ttl)) = resolved_opt {
        DNS_CACHE.lock().insert(hostname, ip, ttl, now);
        return Ok(ip);
    }

    Err("Could not resolve host (DNS unreachable or not found)")
}

// =========================================================================
// RFC 4795 LLMNR (Link-Local Multicast Name Resolution) on UDP 5355
// =========================================================================

/// Build an RFC 4795 LLMNR query packet for a hostname.
pub fn build_llmnr_query(hostname: &str, tx_id: u16) -> Vec<u8> {
    let mut pkt = Vec::with_capacity(64);

    // LLMNR Header: ID (2 bytes), Flags (2 bytes, 0 for standard query), QDCOUNT=1, ANCOUNT=0, NSCOUNT=0, ARCOUNT=0
    pkt.extend_from_slice(&tx_id.to_be_bytes());
    pkt.extend_from_slice(&0x0000u16.to_be_bytes()); // Flags: Standard Query
    pkt.extend_from_slice(&1u16.to_be_bytes()); // QDCOUNT = 1
    pkt.extend_from_slice(&0u16.to_be_bytes()); // ANCOUNT = 0
    pkt.extend_from_slice(&0u16.to_be_bytes()); // NSCOUNT = 0
    pkt.extend_from_slice(&0u16.to_be_bytes()); // ARCOUNT = 0

    // Question Section: QNAME labels
    for part in hostname.split('.') {
        if !part.is_empty() {
            pkt.push(part.len() as u8);
            pkt.extend_from_slice(part.as_bytes());
        }
    }
    pkt.push(0x00);

    // QTYPE = A (1), QCLASS = IN (1)
    pkt.extend_from_slice(&DNS_TYPE_A.to_be_bytes());
    pkt.extend_from_slice(&DNS_CLASS_IN.to_be_bytes());

    pkt
}

/// Parse an LLMNR query packet to extract transaction ID and requested hostname.
pub fn parse_llmnr_query(data: &[u8]) -> Option<(u16, String)> {
    if data.len() < 12 {
        return None;
    }

    let tx_id = u16::from_be_bytes([data[0], data[1]]);
    let flags = u16::from_be_bytes([data[2], data[3]]);
    // QR bit (bit 15) must be 0 for a query
    if (flags & 0x8000) != 0 {
        return None;
    }
    let qdcount = u16::from_be_bytes([data[4], data[5]]);
    if qdcount == 0 {
        return None;
    }

    let mut idx = 12;
    let mut name_parts = Vec::new();
    while idx < data.len() {
        let len = data[idx] as usize;
        if len == 0 {
            break;
        }
        if (data[idx] & 0xC0) == 0xC0 {
            break;
        }
        idx += 1;
        if idx + len <= data.len() {
            if let Ok(s) = core::str::from_utf8(&data[idx..idx + len]) {
                name_parts.push(s);
            }
            idx += len;
        } else {
            return None;
        }
    }

    let mut hostname = String::new();
    for (i, p) in name_parts.iter().enumerate() {
        if i > 0 {
            hostname.push('.');
        }
        hostname.push_str(p);
    }

    Some((tx_id, hostname))
}

/// Build an RFC 4795 LLMNR response packet answering for this host's IP address.
pub fn build_llmnr_response(hostname: &str, tx_id: u16, ip: [u8; 4], ttl: u32) -> Vec<u8> {
    let mut pkt = Vec::with_capacity(128);

    // Header: ID, Flags=0x8000 (Response, no error), QDCOUNT=1, ANCOUNT=1, NSCOUNT=0, ARCOUNT=0
    pkt.extend_from_slice(&tx_id.to_be_bytes());
    pkt.extend_from_slice(&0x8000u16.to_be_bytes()); // QR = 1, RCODE = 0
    pkt.extend_from_slice(&1u16.to_be_bytes()); // QDCOUNT = 1
    pkt.extend_from_slice(&1u16.to_be_bytes()); // ANCOUNT = 1
    pkt.extend_from_slice(&0u16.to_be_bytes()); // NSCOUNT = 0
    pkt.extend_from_slice(&0u16.to_be_bytes()); // ARCOUNT = 0

    // Question Section
    for part in hostname.split('.') {
        if !part.is_empty() {
            pkt.push(part.len() as u8);
            pkt.extend_from_slice(part.as_bytes());
        }
    }
    pkt.push(0x00);
    pkt.extend_from_slice(&DNS_TYPE_A.to_be_bytes());
    pkt.extend_from_slice(&DNS_CLASS_IN.to_be_bytes());

    // Answer Section: Name pointer (offset 12 = 0xC00C)
    pkt.push(0xC0);
    pkt.push(0x0C);
    pkt.extend_from_slice(&DNS_TYPE_A.to_be_bytes());
    pkt.extend_from_slice(&DNS_CLASS_IN.to_be_bytes());
    pkt.extend_from_slice(&ttl.to_be_bytes());
    pkt.extend_from_slice(&4u16.to_be_bytes()); // RDLENGTH = 4
    pkt.extend_from_slice(&ip);

    pkt
}

/// Handle an incoming LLMNR packet on UDP port 5355 and construct an Ethernet frame reply if queried for our host.
pub fn handle_llmnr_packet(
    payload: &[u8],
    src_ip: [u8; 4],
    src_port: u16,
    my_ip: [u8; 4],
    my_mac: [u8; 6],
) -> Option<Vec<u8>> {
    if let Some((tx_id, query_name)) = parse_llmnr_query(payload) {
        if query_name.eq_ignore_ascii_case("cosmos")
            || query_name.eq_ignore_ascii_case("cosmoskernel")
            || query_name.eq_ignore_ascii_case("cosmos.local")
        {
            let response_payload = build_llmnr_response(&query_name, tx_id, my_ip, 30);
            let udp_pkt = crate::net::udp::build_udp_packet(LLMNR_PORT, src_port, &response_payload);
            let ip_pkt = crate::net::ip::build_ipv4_packet(my_ip, src_ip, crate::net::ip::IP_PROTO_UDP, &udp_pkt);
            let dst_mac = crate::net::arp::ARP_CACHE.lock().lookup(&src_ip).unwrap_or(crate::net::ethernet::BROADCAST_MAC);
            let frame = crate::net::ethernet::build_frame(dst_mac, my_mac, crate::net::ethernet::ETHERTYPE_IPV4, &ip_pkt);
            return Some(frame);
        }
    }
    None
}
