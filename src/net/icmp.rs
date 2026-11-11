extern crate alloc;
use alloc::collections::VecDeque;
use alloc::vec::Vec;
use crate::net::ip::{calculate_checksum, IP_PROTO_ICMP};
use crate::sync::IrqSafeSpinlock;

pub const ICMP_TYPE_ECHO_REPLY: u8 = 0;
pub const ICMP_TYPE_ECHO_REQUEST: u8 = 8;

#[derive(Debug, Clone, Copy)]
pub struct PingReply {
    pub src_ip: [u8; 4],
    pub seq: u16,
    pub ttl: u8,
    pub data_len: usize,
}

pub static PING_REPLIES: IrqSafeSpinlock<VecDeque<PingReply>> = IrqSafeSpinlock::new(VecDeque::new());

/// Build an ICMP Echo Request packet.
pub fn build_echo_request(id: u16, seq: u16, payload: &[u8]) -> Vec<u8> {
    let mut pkt = Vec::with_capacity(8 + payload.len());
    pkt.push(ICMP_TYPE_ECHO_REQUEST);
    pkt.push(0x00); // Code
    pkt.extend_from_slice(&[0x00, 0x00]); // Checksum placeholder
    pkt.extend_from_slice(&id.to_be_bytes());
    pkt.extend_from_slice(&seq.to_be_bytes());
    pkt.extend_from_slice(payload);

    let checksum = calculate_checksum(&pkt);
    pkt[2] = (checksum >> 8) as u8;
    pkt[3] = (checksum & 0xFF) as u8;
    pkt
}

/// Handle an incoming ICMP packet. Returns an Echo Reply packet if it was an Echo Request.
pub fn handle_icmp_packet(data: &[u8], src_ip: [u8; 4], _dst_ip: [u8; 4]) -> Option<(u8, Vec<u8>)> {
    if data.len() < 8 {
        return None;
    }

    let icmp_type = data[0];
    let seq = u16::from_be_bytes([data[6], data[7]]);

    if icmp_type == ICMP_TYPE_ECHO_REQUEST {
        // Build Echo Reply
        let mut reply = Vec::from(data);
        reply[0] = ICMP_TYPE_ECHO_REPLY;
        reply[2] = 0;
        reply[3] = 0;
        let checksum = calculate_checksum(&reply);
        reply[2] = (checksum >> 8) as u8;
        reply[3] = (checksum & 0xFF) as u8;

        return Some((IP_PROTO_ICMP, reply));
    } else if icmp_type == ICMP_TYPE_ECHO_REPLY {
        PING_REPLIES.lock().push_back(PingReply {
            src_ip,
            seq,
            ttl: 64,
            data_len: data.len(),
        });
    }

    None
}
