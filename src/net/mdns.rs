extern crate alloc;
use alloc::string::String;
use alloc::vec::Vec;
use crate::net::ip;
use crate::net::udp;
use crate::kinfo;

pub const MDNS_PORT: u16 = 5353;
pub const MDNS_MCAST_IP: [u8; 4] = [224, 0, 0, 251];
pub const MDNS_MCAST_MAC: [u8; 6] = [0x01, 0x00, 0x5E, 0x00, 0x00, 0xFB];

pub const DNS_TYPE_A: u16 = 1;
pub const DNS_TYPE_PTR: u16 = 12;
pub const DNS_TYPE_TXT: u16 = 16;
pub const DNS_TYPE_SRV: u16 = 33;
pub const DNS_TYPE_ANY: u16 = 255;

pub const DNS_CLASS_IN: u16 = 1;
pub const DNS_CACHE_FLUSH: u16 = 0x8000;

#[derive(Debug, Clone)]
pub struct MdnsQuestion {
    pub name: String,
    pub qtype: u16,
    pub unicast_response: bool,
}

#[derive(Debug, Clone)]
pub struct MdnsQueryPacket {
    pub id: u16,
    pub questions: Vec<MdnsQuestion>,
}

/// Parse incoming Multicast DNS query payload.
pub fn parse_mdns_query(data: &[u8]) -> Option<MdnsQueryPacket> {
    if data.len() < 12 {
        return None;
    }

    let id = u16::from_be_bytes([data[0], data[1]]);
    let flags = u16::from_be_bytes([data[2], data[3]]);
    let qdcount = u16::from_be_bytes([data[4], data[5]]) as usize;

    // Check if it's a query (QR bit 15 is 0)
    if (flags & 0x8000) != 0 || qdcount == 0 {
        return None;
    }

    let mut idx = 12;
    let mut questions = Vec::with_capacity(qdcount);

    for _ in 0..qdcount {
        if idx >= data.len() {
            break;
        }

        // Parse domain name labels
        let mut name_parts = Vec::new();
        while idx < data.len() {
            let len = data[idx] as usize;
            if len == 0 {
                idx += 1;
                break;
            }
            if (data[idx] & 0xC0) == 0xC0 {
                // Compression pointer
                idx += 2;
                break;
            }
            idx += 1;
            if idx + len <= data.len() {
                if let Ok(s) = core::str::from_utf8(&data[idx..idx + len]) {
                    name_parts.push(s);
                }
                idx += len;
            } else {
                break;
            }
        }

        if idx + 4 > data.len() {
            break;
        }

        let qtype = u16::from_be_bytes([data[idx], data[idx + 1]]);
        let qclass_raw = u16::from_be_bytes([data[idx + 2], data[idx + 3]]);
        idx += 4;

        let unicast_response = (qclass_raw & 0x8000) != 0;
        let mut full_name = String::new();
        for (i, part) in name_parts.iter().enumerate() {
            if i > 0 {
                full_name.push('.');
            }
            full_name.push_str(part);
        }

        questions.push(MdnsQuestion {
            name: full_name,
            qtype,
            unicast_response,
        });
    }

    if questions.is_empty() {
        None
    } else {
        Some(MdnsQueryPacket { id, questions })
    }
}

/// Encode a domain name into DNS wire format labels.
fn encode_dns_name(name: &str, buf: &mut Vec<u8>) {
    for part in name.split('.') {
        if !part.is_empty() {
            buf.push(part.len() as u8);
            buf.extend_from_slice(part.as_bytes());
        }
    }
    buf.push(0x00);
}

/// Build an mDNS response packet for questions matching CosmicOS services.
pub fn build_mdns_response(query: &MdnsQueryPacket, local_ip: [u8; 4]) -> Option<Vec<u8>> {
    let hostname = "cosmicos.local";
    let mut answers: Vec<Vec<u8>> = Vec::new();

    for q in &query.questions {
        let name_lower = q.name.to_ascii_lowercase();

        if name_lower == "cosmicos.local" || name_lower == "cosmicos" {
            // Type A (IPv4 Address)
            if q.qtype == DNS_TYPE_A || q.qtype == DNS_TYPE_ANY {
                let mut record = Vec::with_capacity(32);
                encode_dns_name(hostname, &mut record);
                record.extend_from_slice(&DNS_TYPE_A.to_be_bytes());
                record.extend_from_slice(&(DNS_CLASS_IN | DNS_CACHE_FLUSH).to_be_bytes());
                record.extend_from_slice(&120u32.to_be_bytes()); // TTL 120s
                record.extend_from_slice(&4u16.to_be_bytes()); // RDLENGTH 4
                record.extend_from_slice(&local_ip); // IPv4
                answers.push(record);
            }
        } else if name_lower == "_services._dns-sd._udp.local" {
            // DNS-SD Service Enumeration
            for service in &["_http._tcp.local", "_ssh._tcp.local", "_telnet._tcp.local"] {
                let mut record = Vec::with_capacity(64);
                encode_dns_name("_services._dns-sd._udp.local", &mut record);
                record.extend_from_slice(&DNS_TYPE_PTR.to_be_bytes());
                record.extend_from_slice(&DNS_CLASS_IN.to_be_bytes());
                record.extend_from_slice(&4500u32.to_be_bytes()); // TTL 4500s

                let mut ptr_data = Vec::new();
                encode_dns_name(service, &mut ptr_data);
                record.extend_from_slice(&(ptr_data.len() as u16).to_be_bytes());
                record.extend_from_slice(&ptr_data);
                answers.push(record);
            }
        } else if name_lower == "_http._tcp.local" {
            // Web Dashboard HTTP Service
            let srv_name = "CosmicOS Web Dashboard._http._tcp.local";

            // PTR Record
            let mut ptr_rec = Vec::new();
            encode_dns_name("_http._tcp.local", &mut ptr_rec);
            ptr_rec.extend_from_slice(&DNS_TYPE_PTR.to_be_bytes());
            ptr_rec.extend_from_slice(&DNS_CLASS_IN.to_be_bytes());
            ptr_rec.extend_from_slice(&4500u32.to_be_bytes());
            let mut srv_encoded = Vec::new();
            encode_dns_name(srv_name, &mut srv_encoded);
            ptr_rec.extend_from_slice(&(srv_encoded.len() as u16).to_be_bytes());
            ptr_rec.extend_from_slice(&srv_encoded);
            answers.push(ptr_rec);

            // SRV Record (Priority=0, Weight=0, Port=80, Target=cosmicos.local)
            let mut srv_rec = Vec::new();
            encode_dns_name(srv_name, &mut srv_rec);
            srv_rec.extend_from_slice(&DNS_TYPE_SRV.to_be_bytes());
            srv_rec.extend_from_slice(&(DNS_CLASS_IN | DNS_CACHE_FLUSH).to_be_bytes());
            srv_rec.extend_from_slice(&120u32.to_be_bytes());
            let mut target_enc = Vec::new();
            encode_dns_name(hostname, &mut target_enc);
            let srv_len = (6 + target_enc.len()) as u16;
            srv_rec.extend_from_slice(&srv_len.to_be_bytes());
            srv_rec.extend_from_slice(&0u16.to_be_bytes()); // Priority 0
            srv_rec.extend_from_slice(&0u16.to_be_bytes()); // Weight 0
            srv_rec.extend_from_slice(&80u16.to_be_bytes()); // Port 80
            srv_rec.extend_from_slice(&target_enc);
            answers.push(srv_rec);

            // A Record
            let mut a_rec = Vec::new();
            encode_dns_name(hostname, &mut a_rec);
            a_rec.extend_from_slice(&DNS_TYPE_A.to_be_bytes());
            a_rec.extend_from_slice(&(DNS_CLASS_IN | DNS_CACHE_FLUSH).to_be_bytes());
            a_rec.extend_from_slice(&120u32.to_be_bytes());
            a_rec.extend_from_slice(&4u16.to_be_bytes());
            a_rec.extend_from_slice(&local_ip);
            answers.push(a_rec);
        } else if name_lower == "_ssh._tcp.local" {
            // SSH Service
            let srv_name = "CosmicOS Secure Shell._ssh._tcp.local";

            let mut ptr_rec = Vec::new();
            encode_dns_name("_ssh._tcp.local", &mut ptr_rec);
            ptr_rec.extend_from_slice(&DNS_TYPE_PTR.to_be_bytes());
            ptr_rec.extend_from_slice(&DNS_CLASS_IN.to_be_bytes());
            ptr_rec.extend_from_slice(&4500u32.to_be_bytes());
            let mut srv_encoded = Vec::new();
            encode_dns_name(srv_name, &mut srv_encoded);
            ptr_rec.extend_from_slice(&(srv_encoded.len() as u16).to_be_bytes());
            ptr_rec.extend_from_slice(&srv_encoded);
            answers.push(ptr_rec);

            let mut srv_rec = Vec::new();
            encode_dns_name(srv_name, &mut srv_rec);
            srv_rec.extend_from_slice(&DNS_TYPE_SRV.to_be_bytes());
            srv_rec.extend_from_slice(&(DNS_CLASS_IN | DNS_CACHE_FLUSH).to_be_bytes());
            srv_rec.extend_from_slice(&120u32.to_be_bytes());
            let mut target_enc = Vec::new();
            encode_dns_name(hostname, &mut target_enc);
            let srv_len = (6 + target_enc.len()) as u16;
            srv_rec.extend_from_slice(&srv_len.to_be_bytes());
            srv_rec.extend_from_slice(&0u16.to_be_bytes());
            srv_rec.extend_from_slice(&0u16.to_be_bytes());
            srv_rec.extend_from_slice(&22u16.to_be_bytes()); // Port 22
            srv_rec.extend_from_slice(&target_enc);
            answers.push(srv_rec);
        } else if name_lower == "_telnet._tcp.local" {
            // Telnet Service
            let srv_name = "CosmicOS Telnet Service._telnet._tcp.local";

            let mut ptr_rec = Vec::new();
            encode_dns_name("_telnet._tcp.local", &mut ptr_rec);
            ptr_rec.extend_from_slice(&DNS_TYPE_PTR.to_be_bytes());
            ptr_rec.extend_from_slice(&DNS_CLASS_IN.to_be_bytes());
            ptr_rec.extend_from_slice(&4500u32.to_be_bytes());
            let mut srv_encoded = Vec::new();
            encode_dns_name(srv_name, &mut srv_encoded);
            ptr_rec.extend_from_slice(&(srv_encoded.len() as u16).to_be_bytes());
            ptr_rec.extend_from_slice(&srv_encoded);
            answers.push(ptr_rec);

            let mut srv_rec = Vec::new();
            encode_dns_name(srv_name, &mut srv_rec);
            srv_rec.extend_from_slice(&DNS_TYPE_SRV.to_be_bytes());
            srv_rec.extend_from_slice(&(DNS_CLASS_IN | DNS_CACHE_FLUSH).to_be_bytes());
            srv_rec.extend_from_slice(&120u32.to_be_bytes());
            let mut target_enc = Vec::new();
            encode_dns_name(hostname, &mut target_enc);
            let srv_len = (6 + target_enc.len()) as u16;
            srv_rec.extend_from_slice(&srv_len.to_be_bytes());
            srv_rec.extend_from_slice(&0u16.to_be_bytes());
            srv_rec.extend_from_slice(&0u16.to_be_bytes());
            srv_rec.extend_from_slice(&23u16.to_be_bytes()); // Port 23
            srv_rec.extend_from_slice(&target_enc);
            answers.push(srv_rec);
        }
    }

    if answers.is_empty() {
        return None;
    }

    let mut response = Vec::with_capacity(256);
    // Header (12 bytes)
    response.extend_from_slice(&query.id.to_be_bytes()); // ID
    response.extend_from_slice(&0x8400u16.to_be_bytes()); // Flags: Response, Authoritative
    response.extend_from_slice(&0u16.to_be_bytes()); // QDCOUNT = 0
    response.extend_from_slice(&(answers.len() as u16).to_be_bytes()); // ANCOUNT
    response.extend_from_slice(&0u16.to_be_bytes()); // NSCOUNT
    response.extend_from_slice(&0u16.to_be_bytes()); // ARCOUNT

    for ans in answers {
        response.extend_from_slice(&ans);
    }

    Some(response)
}

/// Handle incoming mDNS packet arriving on port 5353.
/// Returns complete Ethernet frame if a reply should be sent.
pub fn handle_mdns_packet(
    udp_payload: &[u8],
    src_ip: [u8; 4],
    src_port: u16,
    local_ip: [u8; 4],
    local_mac: [u8; 6],
) -> Option<Vec<u8>> {
    let query = parse_mdns_query(udp_payload)?;
    let dns_resp = build_mdns_response(&query, local_ip)?;

    let (dst_ip, dst_port, dst_mac) = if query.questions.iter().any(|q| q.unicast_response) {
        (src_ip, src_port, crate::net::ethernet::BROADCAST_MAC)
    } else {
        (MDNS_MCAST_IP, MDNS_PORT, MDNS_MCAST_MAC)
    };

    let udp_packet = udp::build_udp_packet(MDNS_PORT, dst_port, &dns_resp);
    let ip_packet = ip::build_ipv4_packet(local_ip, dst_ip, ip::IP_PROTO_UDP, &udp_packet);
    let eth_frame = crate::net::ethernet::build_frame(
        dst_mac,
        local_mac,
        crate::net::ETHERTYPE_IPV4,
        &ip_packet,
    );

    kinfo!(
        "mDNS responder: Answered query for {} -> {}.{}.{}.{}",
        query.questions.first().map(|q| q.name.as_str()).unwrap_or("?"),
        local_ip[0], local_ip[1], local_ip[2], local_ip[3]
    );

    Some(eth_frame)
}
