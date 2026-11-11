extern crate alloc;
use alloc::collections::VecDeque;
use alloc::vec::Vec;
use crate::net::ip::{build_ipv4_packet, IP_PROTO_TCP};
use crate::sync::IrqSafeSpinlock;

// TCP Flags
pub const TCP_FLAG_FIN: u8 = 0x01;
pub const TCP_FLAG_SYN: u8 = 0x02;
pub const TCP_FLAG_RST: u8 = 0x04;
pub const TCP_FLAG_PSH: u8 = 0x08;
pub const TCP_FLAG_ACK: u8 = 0x10;
pub const TCP_FLAG_URG: u8 = 0x20;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TcpState {
    Closed,
    Listen,
    SynSent,
    SynReceived,
    Established,
    FinWait1,
    FinWait2,
    CloseWait,
    Closing,
    LastAck,
    TimeWait,
}

#[derive(Debug, Clone)]
pub struct TcpHeader {
    pub src_port: u16,
    pub dst_port: u16,
    pub seq_num: u32,
    pub ack_num: u32,
    pub data_offset: usize,
    pub flags: u8,
    pub window_size: u16,
    pub checksum: u16,
    pub urgent_ptr: u16,
}

/// Calculate the standard 16-bit ones' complement TCP checksum including IPv4 pseudo-header.
pub fn calculate_tcp_checksum(src_ip: [u8; 4], dst_ip: [u8; 4], tcp_segment: &[u8]) -> u16 {
    let mut sum: u32 = 0;

    // IPv4 Pseudo-Header (12 bytes)
    // 1. Source IP
    sum = sum.wrapping_add(u16::from_be_bytes([src_ip[0], src_ip[1]]) as u32);
    sum = sum.wrapping_add(u16::from_be_bytes([src_ip[2], src_ip[3]]) as u32);
    // 2. Destination IP
    sum = sum.wrapping_add(u16::from_be_bytes([dst_ip[0], dst_ip[1]]) as u32);
    sum = sum.wrapping_add(u16::from_be_bytes([dst_ip[2], dst_ip[3]]) as u32);
    // 3. Zero byte & Protocol (6)
    sum = sum.wrapping_add(IP_PROTO_TCP as u32);
    // 4. TCP Length
    sum = sum.wrapping_add(tcp_segment.len() as u32);

    // TCP Header & Payload
    let mut i = 0;
    while i + 1 < tcp_segment.len() {
        let word = u16::from_be_bytes([tcp_segment[i], tcp_segment[i + 1]]) as u32;
        sum = sum.wrapping_add(word);
        i += 2;
    }
    if i < tcp_segment.len() {
        let word = ((tcp_segment[i] as u16) << 8) as u32;
        sum = sum.wrapping_add(word);
    }

    while (sum >> 16) != 0 {
        sum = (sum & 0xFFFF) + (sum >> 16);
    }

    !(sum as u16)
}

/// Construct a raw TCP segment with pseudo-header checksum.
pub fn build_tcp_packet(
    src_ip: [u8; 4],
    dst_ip: [u8; 4],
    src_port: u16,
    dst_port: u16,
    seq: u32,
    ack: u32,
    flags: u8,
    window: u16,
    payload: &[u8],
) -> Vec<u8> {
    let header_len = 20usize;
    let mut segment = Vec::with_capacity(header_len + payload.len());

    // 0..2: Source Port
    segment.extend_from_slice(&src_port.to_be_bytes());
    // 2..4: Destination Port
    segment.extend_from_slice(&dst_port.to_be_bytes());
    // 4..8: Sequence Number
    segment.extend_from_slice(&seq.to_be_bytes());
    // 8..12: Acknowledgment Number
    segment.extend_from_slice(&ack.to_be_bytes());
    // 12: Data Offset (5 * 32-bit words = 20 bytes -> 0x50)
    segment.push(0x50);
    // 13: Flags
    segment.push(flags);
    // 14..16: Window Size
    segment.extend_from_slice(&window.to_be_bytes());
    // 16..18: Checksum placeholder
    segment.extend_from_slice(&[0x00, 0x00]);
    // 18..20: Urgent Pointer
    segment.extend_from_slice(&[0x00, 0x00]);

    // Append Payload
    segment.extend_from_slice(payload);

    // Calculate & insert checksum
    let checksum = calculate_tcp_checksum(src_ip, dst_ip, &segment);
    segment[16] = (checksum >> 8) as u8;
    segment[17] = (checksum & 0xFF) as u8;

    segment
}

/// Parse an incoming TCP segment header and payload.
pub fn parse_tcp_packet(data: &[u8]) -> Option<(TcpHeader, &[u8])> {
    if data.len() < 20 {
        return None;
    }

    let src_port = u16::from_be_bytes([data[0], data[1]]);
    let dst_port = u16::from_be_bytes([data[2], data[3]]);
    let seq_num = u32::from_be_bytes([data[4], data[5], data[6], data[7]]);
    let ack_num = u32::from_be_bytes([data[8], data[9], data[10], data[11]]);
    let data_offset = ((data[12] >> 4) as usize) * 4;
    let flags = data[13];
    let window_size = u16::from_be_bytes([data[14], data[15]]);
    let checksum = u16::from_be_bytes([data[16], data[17]]);
    let urgent_ptr = u16::from_be_bytes([data[18], data[19]]);

    if data_offset < 20 || data.len() < data_offset {
        return None;
    }

    let header = TcpHeader {
        src_port,
        dst_port,
        seq_num,
        ack_num,
        data_offset,
        flags,
        window_size,
        checksum,
        urgent_ptr,
    };

    let payload = &data[data_offset..];
    Some((header, payload))
}

#[derive(Debug)]
pub struct TcpConnection {
    pub id: usize,
    pub state: TcpState,
    pub local_ip: [u8; 4],
    pub local_port: u16,
    pub remote_ip: [u8; 4],
    pub remote_port: u16,
    pub local_seq: u32,
    pub local_ack: u32,
    pub remote_win: u16,
    pub rx_buffer: VecDeque<u8>,
}

impl TcpConnection {
    pub fn new(id: usize, local_ip: [u8; 4], local_port: u16, remote_ip: [u8; 4], remote_port: u16) -> Self {
        Self {
            id,
            state: TcpState::Closed,
            local_ip,
            local_port,
            remote_ip,
            remote_port,
            local_seq: 0x1000 + (id as u32 * 0x100),
            local_ack: 0,
            remote_win: 65535,
            rx_buffer: VecDeque::with_capacity(8192),
        }
    }
}

pub struct TcpTable {
    pub connections: Vec<TcpConnection>,
    next_id: usize,
    next_port: u16,
}

impl TcpTable {
    pub const fn new() -> Self {
        Self {
            connections: Vec::new(),
            next_id: 1,
            next_port: 49152,
        }
    }

    pub fn allocate_id(&mut self) -> usize {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    pub fn allocate_port(&mut self) -> u16 {
        let p = self.next_port;
        self.next_port = if self.next_port >= 65000 { 49152 } else { self.next_port + 1 };
        p
    }

    pub fn find(&mut self, id: usize) -> Option<&mut TcpConnection> {
        self.connections.iter_mut().find(|c| c.id == id)
    }

    pub fn find_by_endpoint(&mut self, local_port: u16, remote_ip: [u8; 4], remote_port: u16) -> Option<&mut TcpConnection> {
        self.connections.iter_mut().find(|c| {
            c.local_port == local_port && (c.remote_port == 0 || c.remote_port == remote_port) && (c.remote_ip == [0, 0, 0, 0] || c.remote_ip == remote_ip)
        })
    }
}

pub static TCP_TABLE: IrqSafeSpinlock<TcpTable> = IrqSafeSpinlock::new(TcpTable::new());

/// Dispatch an outgoing TCP frame via the routing and network device subsystem.
pub fn send_tcp_packet(
    src_ip: [u8; 4],
    dst_ip: [u8; 4],
    src_port: u16,
    dst_port: u16,
    seq: u32,
    ack: u32,
    flags: u8,
    payload: &[u8],
) -> Result<(), &'static str> {
    let tcp_seg = build_tcp_packet(src_ip, dst_ip, src_port, dst_port, seq, ack, flags, 65535, payload);
    let ip_pkt = build_ipv4_packet(src_ip, dst_ip, IP_PROTO_TCP, &tcp_seg);

    let net = crate::net::device::NET_MANAGER.lock();
    let (dev, cfg) = net.get_device_for_ip(dst_ip).ok_or("No route to host")?;
    drop(net);

    let next_hop = if dev.device_type() == crate::net::device::NetDeviceType::Loopback {
        dst_ip
    } else {
        let is_local = (0..4).all(|i| (dst_ip[i] & cfg.netmask[i]) == (cfg.ipv4[i] & cfg.netmask[i]));
        if is_local || cfg.gateway == [0, 0, 0, 0] {
            dst_ip
        } else {
            cfg.gateway
        }
    };

    let dst_mac = crate::net::arp::ARP_CACHE.lock().lookup(&next_hop).unwrap_or(crate::net::ethernet::BROADCAST_MAC);
    let frame = crate::net::ethernet::build_frame(dst_mac, dev.mac_address(), crate::net::ethernet::ETHERTYPE_IPV4, &ip_pkt);

    dev.send_packet(&frame)
}
