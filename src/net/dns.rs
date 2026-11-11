extern crate alloc;
use alloc::vec::Vec;

pub const DNS_TYPE_A: u16 = 1;
pub const DNS_CLASS_IN: u16 = 1;

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

/// Parse a DNS response packet and extract the first resolved IPv4 address.
pub fn parse_dns_response(data: &[u8], tx_id: u16) -> Option<[u8; 4]> {
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
        let rdlength = u16::from_be_bytes([data[idx + 8], data[idx + 9]]) as usize;
        idx += 10;

        if rtype == DNS_TYPE_A && rclass == DNS_CLASS_IN && rdlength == 4 && idx + 4 <= data.len() {
            let mut ip = [0u8; 4];
            ip.copy_from_slice(&data[idx..idx + 4]);
            return Some(ip);
        }

        idx += rdlength;
    }

    None
}
