extern crate alloc;
use alloc::vec::Vec;

pub const PORT_DNS: u16 = 53;
pub const PORT_DHCP_SERVER: u16 = 67;
pub const PORT_DHCP_CLIENT: u16 = 68;

/// Build a UDP packet wrapping payload data.
pub fn build_udp_packet(src_port: u16, dst_port: u16, payload: &[u8]) -> Vec<u8> {
    let len = (8 + payload.len()) as u16;
    let mut pkt = Vec::with_capacity(len as usize);

    pkt.extend_from_slice(&src_port.to_be_bytes());
    pkt.extend_from_slice(&dst_port.to_be_bytes());
    pkt.extend_from_slice(&len.to_be_bytes());
    pkt.extend_from_slice(&[0x00, 0x00]); // Checksum optional in IPv4
    pkt.extend_from_slice(payload);

    pkt
}

/// Parse incoming UDP datagram. Returns (src_port, dst_port, payload).
pub fn parse_udp_packet(data: &[u8]) -> Option<(u16, u16, &[u8])> {
    if data.len() < 8 {
        return None;
    }

    let src_port = u16::from_be_bytes([data[0], data[1]]);
    let dst_port = u16::from_be_bytes([data[2], data[3]]);
    let len = u16::from_be_bytes([data[4], data[5]]) as usize;

    if data.len() < len || len < 8 {
        return None;
    }

    Some((src_port, dst_port, &data[8..len]))
}
