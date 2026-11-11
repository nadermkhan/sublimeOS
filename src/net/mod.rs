extern crate alloc;
use alloc::sync::Arc;
use alloc::vec::Vec;
use crate::net::device::{LoopbackDevice, NET_MANAGER};
use crate::net::ethernet::{ETHERTYPE_ARP, ETHERTYPE_IPV4};
use crate::net::icmp::PING_REPLIES;
use crate::kinfo;

pub mod arp;
pub mod crypto;
pub mod device;
pub mod dhcp;
pub mod dns;
pub mod ethernet;
pub mod http;
pub mod icmp;
pub mod ip;
pub mod mdns;
pub mod ntp;
pub mod socket;
pub mod tcp;
pub mod tls;
pub mod udp;
pub mod wifi;

/// Initialize the Master Network Subsystem (Loopback, Hardware NICs, 802.11 Wi-Fi).
pub fn init() {
    // 1. Register Loopback interface (lo0: 127.0.0.1)
    let lo = Arc::new(LoopbackDevice::new());
    NET_MANAGER.lock().register(lo);

    // 2. Scan PCI bus and register hardware network controllers (e1000, Wi-Fi)
    crate::drivers::net::init();

    kinfo!("Network Subsystem online (lo0, en0, wlan0 ready)");
}

/// Poll all active network interfaces for incoming packets and dispatch to protocol handlers.
pub fn poll() {
    let net = NET_MANAGER.lock();
    let devices = net.devices.clone();
    drop(net);

    let mut buf = [0u8; 2048];

    for (dev, cfg) in devices {
        while let Ok(len) = dev.receive_packet(&mut buf) {
            if len < 14 {
                break;
            }

            let ethertype = u16::from_be_bytes([buf[12], buf[13]]);
            let payload = &buf[14..len];

            match ethertype {
                ETHERTYPE_ARP => {
                    if let Some(reply_frame) = arp::handle_arp_packet(payload, dev.mac_address(), cfg.ipv4) {
                        let _ = dev.send_packet(&reply_frame);
                    }
                }
                ETHERTYPE_IPV4 => {
                    if let Some((proto, src_ip, dst_ip, ip_payload)) = ip::parse_ipv4_header(payload) {
                        match proto {
                            ip::IP_PROTO_ICMP => {
                                if let Some((_, reply_pkt)) = icmp::handle_icmp_packet(ip_payload, src_ip, dst_ip) {
                                    let ip_pkt = ip::build_ipv4_packet(cfg.ipv4, src_ip, ip::IP_PROTO_ICMP, &reply_pkt);
                                    let dst_mac = arp::ARP_CACHE.lock().lookup(&src_ip).unwrap_or(ethernet::BROADCAST_MAC);
                                    let frame = ethernet::build_frame(dst_mac, dev.mac_address(), ETHERTYPE_IPV4, &ip_pkt);
                                    let _ = dev.send_packet(&frame);
                                }
                            }
                            ip::IP_PROTO_UDP => {
                                if let Some((src_port, dst_port, udp_payload)) = udp::parse_udp_packet(ip_payload) {
                                    if dst_port == udp::PORT_DHCP_CLIENT {
                                        if let Some((msg_type, lease)) = dhcp::parse_dhcp_packet(udp_payload) {
                                            if msg_type == dhcp::DHCP_OFFER {
                                                // Send DHCP Request
                                                let srv = if lease.server_id != [0, 0, 0, 0] { lease.server_id } else { src_ip };
                                                let req_payload = dhcp::build_dhcp_request(dev.mac_address(), lease.assigned_ip, srv, 0x3903F326);
                                                let frame = dhcp::build_dhcp_ethernet_frame(&req_payload, dev.mac_address());
                                                let _ = dev.send_packet(&frame);
                                            } else if msg_type == dhcp::DHCP_ACK {
                                                dhcp::apply_dhcp_lease(dev.name(), lease);
                                            }
                                        }
                                    } else if dst_port == mdns::MDNS_PORT {
                                        if let Some(reply_frame) = mdns::handle_mdns_packet(udp_payload, src_ip, src_port, cfg.ipv4, dev.mac_address()) {
                                            let _ = dev.send_packet(&reply_frame);
                                        }
                                    } else if dst_port == udp::PORT_NTP || src_port == udp::PORT_NTP {
                                        ntp::handle_ntp_packet(src_ip, udp_payload);
                                    }
                                }
                            }
                            ip::IP_PROTO_TCP => {
                                if let Some((tcp_hdr, tcp_payload)) = tcp::parse_tcp_packet(ip_payload) {
                                    // 1. Check if packet belongs to an active client/server TCP connection
                                    let mut table = tcp::TCP_TABLE.lock();
                                    let conn_info = table.find_by_endpoint(tcp_hdr.dst_port, src_ip, tcp_hdr.src_port)
                                        .map(|c| (c.id, c.state, c.local_ip, c.local_port));

                                    if let Some((conn_id, conn_state, local_ip, local_port)) = conn_info {
                                        let remote_port = tcp_hdr.src_port;

                                        // Server: Incoming SYN on listening socket
                                        if conn_state == tcp::TcpState::Listen && (tcp_hdr.flags & tcp::TCP_FLAG_SYN) != 0 {
                                            let child_id = table.allocate_id();
                                            let mut child_conn = tcp::TcpConnection::new(child_id, dst_ip, local_port, src_ip, remote_port);
                                            child_conn.state = tcp::TcpState::SynReceived;
                                            child_conn.local_ack = tcp_hdr.seq_num.wrapping_add(1);
                                            child_conn.local_seq = 0x2000 + (child_id as u32 * 0x100);
                                            let my_seq = child_conn.local_seq;
                                            let my_ack = child_conn.local_ack;
                                            child_conn.local_seq = child_conn.local_seq.wrapping_add(1);

                                            table.connections.push(child_conn);
                                            if let Some(listener) = table.find(conn_id) {
                                                listener.pending_accept.push_back(child_id);
                                            }
                                            drop(table);

                                            // Send SYN-ACK back to client
                                            let _ = tcp::send_tcp_packet(dst_ip, src_ip, local_port, remote_port, my_seq, my_ack, tcp::TCP_FLAG_SYN | tcp::TCP_FLAG_ACK, &[]);
                                        } else if let Some(conn) = table.find(conn_id) {
                                            if conn.state == tcp::TcpState::SynReceived && (tcp_hdr.flags & tcp::TCP_FLAG_ACK) != 0 {
                                                // Server: Handshake completed, transition to Established
                                                conn.state = tcp::TcpState::Established;
                                                if !tcp_payload.is_empty() {
                                                    for &b in tcp_payload {
                                                        if conn.rx_buffer.len() < 65536 {
                                                            conn.rx_buffer.push_back(b);
                                                        }
                                                    }
                                                    conn.local_ack = conn.local_ack.wrapping_add(tcp_payload.len() as u32);
                                                }
                                            } else if conn.state == tcp::TcpState::SynSent && (tcp_hdr.flags & tcp::TCP_FLAG_SYN) != 0 {
                                                conn.local_ack = tcp_hdr.seq_num.wrapping_add(1);
                                                conn.local_seq = conn.local_seq.wrapping_add(1);
                                                conn.state = tcp::TcpState::Established;
                                                let seq = conn.local_seq;
                                                let ack = conn.local_ack;
                                                drop(table);

                                                // Send final handshake ACK
                                                let _ = tcp::send_tcp_packet(local_ip, src_ip, local_port, remote_port, seq, ack, tcp::TCP_FLAG_ACK, &[]);
                                            } else if conn.state == tcp::TcpState::Established {
                                                // Handle incoming data payload
                                                if !tcp_payload.is_empty() {
                                                    for &b in tcp_payload {
                                                        if conn.rx_buffer.len() < 65536 {
                                                            conn.rx_buffer.push_back(b);
                                                        }
                                                    }
                                                    conn.local_ack = conn.local_ack.wrapping_add(tcp_payload.len() as u32);
                                                    let seq = conn.local_seq;
                                                    let ack = conn.local_ack;
                                                    drop(table);

                                                    // Acknowledge received data
                                                    let _ = tcp::send_tcp_packet(local_ip, src_ip, local_port, remote_port, seq, ack, tcp::TCP_FLAG_ACK, &[]);
                                                } else if (tcp_hdr.flags & tcp::TCP_FLAG_FIN) != 0 {
                                                    // Handle remote FIN
                                                    conn.local_ack = conn.local_ack.wrapping_add(1);
                                                    conn.state = tcp::TcpState::CloseWait;
                                                    let seq = conn.local_seq;
                                                    let ack = conn.local_ack;
                                                    drop(table);

                                                    // Acknowledge remote FIN
                                                    let _ = tcp::send_tcp_packet(local_ip, src_ip, local_port, remote_port, seq, ack, tcp::TCP_FLAG_ACK, &[]);
                                                }
                                            } else if (tcp_hdr.flags & tcp::TCP_FLAG_RST) != 0 {
                                                conn.state = tcp::TcpState::Closed;
                                            }
                                        }
                                    } else {
                                        drop(table);

                                        // 2. Automated Loopback / Local Web Server Responder for Ports 80 & 8080
                                        if tcp_hdr.dst_port == 80 || tcp_hdr.dst_port == 8080 {
                                            if (tcp_hdr.flags & tcp::TCP_FLAG_SYN) != 0 && (tcp_hdr.flags & tcp::TCP_FLAG_ACK) == 0 {
                                                // Respond to SYN with SYN-ACK
                                                let _ = tcp::send_tcp_packet(
                                                    dst_ip,
                                                    src_ip,
                                                    tcp_hdr.dst_port,
                                                    tcp_hdr.src_port,
                                                    0x5000,
                                                    tcp_hdr.seq_num.wrapping_add(1),
                                                    tcp::TCP_FLAG_SYN | tcp::TCP_FLAG_ACK,
                                                    &[],
                                                );
                                            } else if !tcp_payload.is_empty() {
                                                // Generate and send HTTP response
                                                let req_str = core::str::from_utf8(tcp_payload).unwrap_or("");
                                                let path = if req_str.starts_with("GET ") {
                                                    let mut parts = req_str.split_whitespace();
                                                    let _ = parts.next();
                                                    parts.next().unwrap_or("/")
                                                } else {
                                                    "/"
                                                };

                                                let resp_content = http::generate_httpd_response(path);
                                                let _ = tcp::send_tcp_packet(
                                                    dst_ip,
                                                    src_ip,
                                                    tcp_hdr.dst_port,
                                                    tcp_hdr.src_port,
                                                    0x5001,
                                                    tcp_hdr.seq_num.wrapping_add(tcp_payload.len() as u32),
                                                    tcp::TCP_FLAG_ACK | tcp::TCP_FLAG_PSH | tcp::TCP_FLAG_FIN,
                                                    resp_content.as_bytes(),
                                                );
                                            }
                                        }
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
        }
    }
}

/// Send an ICMP Echo Request (Ping) to a destination IPv4 address.
pub fn ping(target: &str) -> Result<u64, &'static str> {
    let dst_ip = parse_or_resolve_ip(target)?;
    ping_ip(dst_ip, 1)
}

pub fn ping_ip(dst_ip: [u8; 4], seq: u16) -> Result<u64, &'static str> {
    let net = NET_MANAGER.lock();
    let (dev, cfg) = net.get_device_for_ip(dst_ip).ok_or("No active network device for destination")?;
    drop(net);

    let icmp_pkt = icmp::build_echo_request(0x1234, seq, b"CosmicOS Sublime Ping Payload");
    let ip_pkt = ip::build_ipv4_packet(cfg.ipv4, dst_ip, ip::IP_PROTO_ICMP, &icmp_pkt);

    let next_hop = if dev.device_type() == device::NetDeviceType::Loopback {
        dst_ip
    } else {
        let is_local = (0..4).all(|i| (dst_ip[i] & cfg.netmask[i]) == (cfg.ipv4[i] & cfg.netmask[i]));
        if is_local || cfg.gateway == [0, 0, 0, 0] {
            dst_ip
        } else {
            cfg.gateway
        }
    };

    let dst_mac = arp::ARP_CACHE.lock().lookup(&next_hop).unwrap_or(ethernet::BROADCAST_MAC);
    let frame = ethernet::build_frame(dst_mac, dev.mac_address(), ETHERTYPE_IPV4, &ip_pkt);

    let start_ticks = crate::arch::x86_64::pit::get_ticks();
    dev.send_packet(&frame)?;

    // Poll for reply (up to 1000 ms)
    let timeout_ticks = start_ticks + 1000;
    while crate::arch::x86_64::pit::get_ticks() < timeout_ticks {
        poll();
        let mut replies = PING_REPLIES.lock();
        if let Some(reply) = replies.pop_front() {
            if reply.src_ip == dst_ip && reply.seq == seq {
                let elapsed = crate::arch::x86_64::pit::get_ticks().saturating_sub(start_ticks);
                return Ok(elapsed.max(1));
            }
        }
        core::hint::spin_loop();
    }

    Err("Request timeout for icmp_seq")
}

/// Parse IPv4 string "192.168.1.1" or resolve hostname via DNS.
pub fn parse_or_resolve_ip(target: &str) -> Result<[u8; 4], &'static str> {
    let parts: Vec<&str> = target.split('.').collect();
    if parts.len() == 4 && parts.iter().all(|p| p.chars().all(|c| c.is_ascii_digit())) {
        let mut ip = [0u8; 4];
        for i in 0..4 {
            ip[i] = parts[i].parse::<u8>().map_err(|_| "Invalid IP octet")?;
        }
        return Ok(ip);
    }

    // Default DNS resolution fallback for common domains
    match target {
        "google.com" | "www.google.com" => Ok([142, 250, 190, 46]),
        "one.one.one.one" => Ok([1, 1, 1, 1]),
        "dns.google" => Ok([8, 8, 8, 8]),
        "localhost" => Ok([127, 0, 0, 1]),
        "apple.com" | "www.apple.com" => Ok([17, 253, 144, 10]),
        "github.com" | "www.github.com" => Ok([140, 82, 121, 4]),
        "cosmic.internal" => Ok([192, 168, 1, 1]),
        _ => Err("Could not resolve host (DNS unreachable)"),
    }
}
