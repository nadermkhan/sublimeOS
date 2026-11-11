extern crate alloc;
use alloc::vec::Vec;

pub const IP_PROTO_ICMP: u8 = 1;
pub const IP_PROTO_TCP: u8 = 6;
pub const IP_PROTO_UDP: u8 = 17;

/// Calculate RFC 1071 ones' complement checksum.
pub fn calculate_checksum(data: &[u8]) -> u16 {
    let mut sum: u32 = 0;
    let mut i = 0;
    while i + 1 < data.len() {
        let word = u16::from_be_bytes([data[i], data[i + 1]]) as u32;
        sum = sum.wrapping_add(word);
        i += 2;
    }
    if i < data.len() {
        let word = ((data[i] as u16) << 8) as u32;
        sum = sum.wrapping_add(word);
    }
    while (sum >> 16) != 0 {
        sum = (sum & 0xFFFF) + (sum >> 16);
    }
    !(sum as u16)
}

/// Build an IPv4 packet wrapping a payload.
pub fn build_ipv4_packet(src_ip: [u8; 4], dst_ip: [u8; 4], protocol: u8, payload: &[u8]) -> Vec<u8> {
    let total_len = (20 + payload.len()) as u16;
    let mut packet = Vec::with_capacity(total_len as usize);

    // 1. Version (4) & IHL (5 words = 20 bytes) -> 0x45
    packet.push(0x45);
    // 2. DSCP & ECN -> 0x00
    packet.push(0x00);
    // 3. Total Length (2 bytes, big-endian)
    packet.extend_from_slice(&total_len.to_be_bytes());
    // 4. Identification -> 0x1234
    packet.extend_from_slice(&0x1234u16.to_be_bytes());
    // 5. Flags & Fragment Offset -> Don't Fragment (0x4000)
    packet.extend_from_slice(&0x4000u16.to_be_bytes());
    // 6. Time to Live (TTL) -> 64
    packet.push(64);
    // 7. Protocol (ICMP = 1, TCP = 6, UDP = 17)
    packet.push(protocol);
    // 8. Checksum placeholder (0x0000)
    packet.extend_from_slice(&[0x00, 0x00]);
    // 9. Source IP
    packet.extend_from_slice(&src_ip);
    // 10. Destination IP
    packet.extend_from_slice(&dst_ip);

    // 11. Compute Header Checksum
    let checksum = calculate_checksum(&packet[0..20]);
    packet[10] = (checksum >> 8) as u8;
    packet[11] = (checksum & 0xFF) as u8;

    // 12. Append Payload
    packet.extend_from_slice(payload);
    packet
}

/// Parse an incoming IPv4 packet header.
pub fn parse_ipv4_header(data: &[u8]) -> Option<(u8, [u8; 4], [u8; 4], &[u8])> {
    if data.len() < 20 {
        return None;
    }

    let version = data[0] >> 4;
    let ihl = (data[0] & 0x0F) as usize * 4;
    if version != 4 || ihl < 20 || data.len() < ihl {
        return None;
    }

    let protocol = data[9];
    let mut src_ip = [0u8; 4];
    let mut dst_ip = [0u8; 4];
    src_ip.copy_from_slice(&data[12..16]);
    dst_ip.copy_from_slice(&data[16..20]);

    let total_len = u16::from_be_bytes([data[2], data[3]]) as usize;
    let payload_end = total_len.min(data.len());
    let payload = &data[ihl..payload_end];

    Some((protocol, src_ip, dst_ip, payload))
}
