extern crate alloc;
use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use crate::net::ethernet::{build_frame, BROADCAST_MAC, ETHERTYPE_ARP};
use crate::sync::IrqSafeSpinlock;

pub const ARP_HTYPE_ETHERNET: u16 = 1;
pub const ARP_PTYPE_IPV4: u16 = 0x0800;
pub const ARP_OPER_REQUEST: u16 = 1;
pub const ARP_OPER_REPLY: u16 = 2;

#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct ArpHeader {
    pub htype: u16,
    pub ptype: u16,
    pub hlen: u8,
    pub plen: u8,
    pub oper: u16,
    pub sha: [u8; 6],
    pub spa: [u8; 4],
    pub tha: [u8; 6],
    pub tpa: [u8; 4],
}

pub struct ArpCache {
    pub table: BTreeMap<[u8; 4], [u8; 6]>,
}

impl ArpCache {
    pub const fn new() -> Self {
        Self {
            table: BTreeMap::new(),
        }
    }

    pub fn insert(&mut self, ip: [u8; 4], mac: [u8; 6]) {
        self.table.insert(ip, mac);
    }

    pub fn lookup(&self, ip: &[u8; 4]) -> Option<[u8; 6]> {
        self.table.get(ip).copied()
    }
}

pub static ARP_CACHE: IrqSafeSpinlock<ArpCache> = IrqSafeSpinlock::new(ArpCache::new());

/// Build an ARP Request frame asking for target IP.
pub fn build_arp_request(src_mac: [u8; 6], src_ip: [u8; 4], target_ip: [u8; 4]) -> Vec<u8> {
    let mut payload = Vec::with_capacity(28);
    payload.extend_from_slice(&ARP_HTYPE_ETHERNET.to_be_bytes());
    payload.extend_from_slice(&ARP_PTYPE_IPV4.to_be_bytes());
    payload.push(6); // MAC length
    payload.push(4); // IPv4 length
    payload.extend_from_slice(&ARP_OPER_REQUEST.to_be_bytes());
    payload.extend_from_slice(&src_mac);
    payload.extend_from_slice(&src_ip);
    payload.extend_from_slice(&[0u8; 6]); // Target MAC unknown
    payload.extend_from_slice(&target_ip);

    build_frame(BROADCAST_MAC, src_mac, ETHERTYPE_ARP, &payload)
}

/// Build an ARP Reply packet responding to an ARP Request.
pub fn build_arp_reply(src_mac: [u8; 6], src_ip: [u8; 4], target_mac: [u8; 6], target_ip: [u8; 4]) -> Vec<u8> {
    let mut payload = Vec::with_capacity(28);
    payload.extend_from_slice(&ARP_HTYPE_ETHERNET.to_be_bytes());
    payload.extend_from_slice(&ARP_PTYPE_IPV4.to_be_bytes());
    payload.push(6);
    payload.push(4);
    payload.extend_from_slice(&ARP_OPER_REPLY.to_be_bytes());
    payload.extend_from_slice(&src_mac);
    payload.extend_from_slice(&src_ip);
    payload.extend_from_slice(&target_mac);
    payload.extend_from_slice(&target_ip);

    build_frame(target_mac, src_mac, ETHERTYPE_ARP, &payload)
}

/// Process an incoming ARP packet and update the cache.
pub fn handle_arp_packet(data: &[u8], our_mac: [u8; 6], our_ip: [u8; 4]) -> Option<Vec<u8>> {
    let arp_data = if data.len() >= 42 && data[12] == 0x08 && data[13] == 0x06 {
        &data[14..]
    } else {
        data
    };

    if arp_data.len() < 28 {
        return None;
    }

    let oper = u16::from_be_bytes([arp_data[6], arp_data[7]]);
    let mut sha = [0u8; 6];
    let mut spa = [0u8; 4];
    let mut tpa = [0u8; 4];

    sha.copy_from_slice(&arp_data[8..14]);
    spa.copy_from_slice(&arp_data[14..18]);
    tpa.copy_from_slice(&arp_data[24..28]);

    // Update ARP cache with sender's info
    ARP_CACHE.lock().insert(spa, sha);

    // If it's a request for our IP, reply!
    if oper == ARP_OPER_REQUEST && tpa == our_ip && our_ip != [0, 0, 0, 0] {
        return Some(build_arp_reply(our_mac, our_ip, sha, spa));
    }

    None
}
