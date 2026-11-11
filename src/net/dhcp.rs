extern crate alloc;
use alloc::string::String;
use alloc::vec::Vec;
use crate::net::device::NET_MANAGER;
use crate::sync::IrqSafeSpinlock;
use crate::kinfo;

pub const DHCP_DISCOVER: u8 = 1;
pub const DHCP_OFFER: u8 = 2;
pub const DHCP_REQUEST: u8 = 3;
pub const DHCP_ACK: u8 = 5;
pub const DHCP_NAK: u8 = 6;
pub const DHCP_RELEASE: u8 = 7;

pub const DHCP_MAGIC_COOKIE: [u8; 4] = [0x63, 0x82, 0x53, 0x63];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DhcpState {
    Init,
    Selecting,
    Requesting,
    Bound,
}

#[derive(Debug, Clone, Copy)]
pub struct DhcpLease {
    pub assigned_ip: [u8; 4],
    pub server_id: [u8; 4],
    pub subnet_mask: [u8; 4],
    pub router: [u8; 4],
    pub dns_server: [u8; 4],
    pub lease_time: u32,
}

pub struct DhcpClientState {
    pub state: DhcpState,
    pub xid: u32,
    pub interface_name: String,
    pub active_lease: Option<DhcpLease>,
}

impl DhcpClientState {
    pub const fn new() -> Self {
        Self {
            state: DhcpState::Init,
            xid: 0x3903F326,
            interface_name: String::new(),
            active_lease: None,
        }
    }
}

pub static DHCP_CLIENT: IrqSafeSpinlock<DhcpClientState> = IrqSafeSpinlock::new(DhcpClientState::new());

/// Build a DHCP Discover packet.
pub fn build_dhcp_discover(client_mac: [u8; 6], xid: u32) -> Vec<u8> {
    let mut pkt = Vec::with_capacity(300);

    pkt.push(1); // op = BOOTREQUEST
    pkt.push(1); // htype = Ethernet
    pkt.push(6); // hlen = 6
    pkt.push(0); // hops = 0
    pkt.extend_from_slice(&xid.to_be_bytes()); // xid
    pkt.extend_from_slice(&[0, 0]); // secs
    pkt.extend_from_slice(&[0x80, 0x00]); // flags = Broadcast
    pkt.extend_from_slice(&[0; 16]); // ciaddr, yiaddr, siaddr, giaddr

    // chaddr (16 bytes)
    pkt.extend_from_slice(&client_mac);
    pkt.extend_from_slice(&[0; 10]);

    // sname (64 bytes) & file (128 bytes)
    pkt.extend_from_slice(&[0; 192]);

    // Magic Cookie (0x63, 0x82, 0x53, 0x63)
    pkt.extend_from_slice(&DHCP_MAGIC_COOKIE);

    // Option 53: DHCP Message Type = DHCP Discover (1)
    pkt.push(53);
    pkt.push(1);
    pkt.push(DHCP_DISCOVER);

    // Option 12: Host Name = "cosmicos"
    pkt.push(12);
    pkt.push(8);
    pkt.extend_from_slice(b"cosmicos");

    // Option 55: Parameter Request List (1 = Subnet, 3 = Router, 6 = DNS)
    pkt.push(55);
    pkt.push(3);
    pkt.push(1);
    pkt.push(3);
    pkt.push(6);

    // Option 255: End
    pkt.push(255);

    pkt
}

/// Build a DHCP Request packet to confirm an offered IP.
pub fn build_dhcp_request(client_mac: [u8; 6], requested_ip: [u8; 4], server_ip: [u8; 4], xid: u32) -> Vec<u8> {
    let mut pkt = Vec::with_capacity(300);

    pkt.push(1); // op = BOOTREQUEST
    pkt.push(1);
    pkt.push(6);
    pkt.push(0);
    pkt.extend_from_slice(&xid.to_be_bytes());
    pkt.extend_from_slice(&[0, 0]);
    pkt.extend_from_slice(&[0x80, 0x00]); // Broadcast
    pkt.extend_from_slice(&[0; 16]);

    pkt.extend_from_slice(&client_mac);
    pkt.extend_from_slice(&[0; 10]);
    pkt.extend_from_slice(&[0; 192]);

    pkt.extend_from_slice(&DHCP_MAGIC_COOKIE);

    // Option 53: DHCP Message Type = DHCP Request (3)
    pkt.push(53);
    pkt.push(1);
    pkt.push(DHCP_REQUEST);

    // Option 50: Requested IP
    pkt.push(50);
    pkt.push(4);
    pkt.extend_from_slice(&requested_ip);

    // Option 54: Server Identifier
    pkt.push(54);
    pkt.push(4);
    pkt.extend_from_slice(&server_ip);

    // Option 55: Parameter Request List
    pkt.push(55);
    pkt.push(3);
    pkt.push(1);
    pkt.push(3);
    pkt.push(6);

    // Option 255: End
    pkt.push(255);

    pkt
}

/// Build a DHCP Release packet to gracefully release an assigned IP.
pub fn build_dhcp_release(client_mac: [u8; 6], client_ip: [u8; 4], server_ip: [u8; 4], xid: u32) -> Vec<u8> {
    let mut pkt = Vec::with_capacity(300);

    pkt.push(1); // op = BOOTREQUEST
    pkt.push(1);
    pkt.push(6);
    pkt.push(0);
    pkt.extend_from_slice(&xid.to_be_bytes());
    pkt.extend_from_slice(&[0, 0]);
    pkt.extend_from_slice(&[0, 0]); // Unicast
    pkt.extend_from_slice(&client_ip); // ciaddr
    pkt.extend_from_slice(&[0; 12]); // yiaddr, siaddr, giaddr

    pkt.extend_from_slice(&client_mac);
    pkt.extend_from_slice(&[0; 10]);
    pkt.extend_from_slice(&[0; 192]);

    pkt.extend_from_slice(&DHCP_MAGIC_COOKIE);

    // Option 53: DHCP Message Type = DHCP Release (7)
    pkt.push(53);
    pkt.push(1);
    pkt.push(DHCP_RELEASE);

    // Option 54: Server Identifier
    pkt.push(54);
    pkt.push(4);
    pkt.extend_from_slice(&server_ip);

    // Option 255: End
    pkt.push(255);

    pkt
}

/// Parse incoming DHCP Offer or Ack.
pub fn parse_dhcp_packet(data: &[u8]) -> Option<(u8, DhcpLease)> {
    if data.len() < 240 {
        return None;
    }

    // Verify magic cookie
    if &data[236..240] != DHCP_MAGIC_COOKIE {
        return None;
    }

    let mut assigned_ip = [0u8; 4];
    assigned_ip.copy_from_slice(&data[16..20]); // yiaddr

    let mut msg_type = 0u8;
    let mut server_id = [0u8; 4];
    let mut subnet_mask = [255, 255, 255, 0];
    let mut router = [0u8; 4];
    let mut dns_server = [8, 8, 8, 8];
    let mut lease_time = 86400u32;

    let mut idx = 240;
    while idx < data.len() {
        let opt = data[idx];
        if opt == 255 {
            break; // End option
        }
        if opt == 0 {
            idx += 1; // Pad option
            continue;
        }

        if idx + 1 >= data.len() {
            break;
        }
        let len = data[idx + 1] as usize;
        idx += 2;

        if idx + len > data.len() {
            break;
        }

        match opt {
            53 => {
                // Message Type
                if len >= 1 {
                    msg_type = data[idx];
                }
            }
            54 => {
                // Server Identifier
                if len >= 4 {
                    server_id.copy_from_slice(&data[idx..idx + 4]);
                }
            }
            1 => {
                // Subnet Mask
                if len >= 4 {
                    subnet_mask.copy_from_slice(&data[idx..idx + 4]);
                }
            }
            3 => {
                // Router / Default Gateway
                if len >= 4 {
                    router.copy_from_slice(&data[idx..idx + 4]);
                }
            }
            6 => {
                // DNS Server
                if len >= 4 {
                    dns_server.copy_from_slice(&data[idx..idx + 4]);
                }
            }
            51 => {
                // Lease Time
                if len >= 4 {
                    lease_time = u32::from_be_bytes([data[idx], data[idx + 1], data[idx + 2], data[idx + 3]]);
                }
            }
            _ => {}
        }

        idx += len;
    }

    if msg_type > 0 {
        Some((
            msg_type,
            DhcpLease {
                assigned_ip,
                server_id,
                subnet_mask,
                router,
                dns_server,
                lease_time,
            },
        ))
    } else {
        None
    }
}

/// Wrap a DHCP payload in UDP, IPv4 broadcast, and Ethernet broadcast frame.
pub fn build_dhcp_ethernet_frame(payload: &[u8], local_mac: [u8; 6]) -> Vec<u8> {
    let udp_pkt = crate::net::udp::build_udp_packet(
        crate::net::udp::PORT_DHCP_CLIENT,
        crate::net::udp::PORT_DHCP_SERVER,
        payload,
    );
    let ip_pkt = crate::net::ip::build_ipv4_packet(
        [0, 0, 0, 0],
        [255, 255, 255, 255],
        crate::net::ip::IP_PROTO_UDP,
        &udp_pkt,
    );
    crate::net::ethernet::build_frame(
        crate::net::ethernet::BROADCAST_MAC,
        local_mac,
        crate::net::ethernet::ETHERTYPE_IPV4,
        &ip_pkt,
    )
}

/// Start DHCP discovery on the specified interface (e.g. "en0").
pub fn start_dhcp_discovery(interface_name: &str) -> Result<Vec<u8>, &'static str> {
    let net = NET_MANAGER.lock();
    let (dev, _) = net.get_device(interface_name).ok_or("Interface not found")?;
    let mac = dev.mac_address();
    drop(net);

    let xid = 0x3903F326;
    let discover_payload = build_dhcp_discover(mac, xid);
    let frame = build_dhcp_ethernet_frame(&discover_payload, mac);

    {
        let mut client = DHCP_CLIENT.lock();
        client.state = DhcpState::Selecting;
        client.xid = xid;
        client.interface_name = String::from(interface_name);
    }

    kinfo!("DHCP: Initiating DHCPDISCOVER on {} (xid=0x{:08X})", interface_name, xid);
    Ok(frame)
}

/// Apply a leased DHCP configuration to an interface.
pub fn apply_dhcp_lease(interface_name: &str, lease: DhcpLease) {
    let mut net = NET_MANAGER.lock();
    if let Some((_, mut cfg)) = net.get_device(interface_name) {
        cfg.ipv4 = lease.assigned_ip;
        cfg.netmask = lease.subnet_mask;
        cfg.gateway = lease.router;
        cfg.dns = lease.dns_server;
        net.set_interface_config(interface_name, cfg);

        {
            let mut client = DHCP_CLIENT.lock();
            client.state = DhcpState::Bound;
            client.active_lease = Some(lease);
        }

        kinfo!(
            "DHCP Lease Acquired for {}: IP {}.{}.{}.{}, Mask {}.{}.{}.{}, Gateway {}.{}.{}.{}, DNS {}.{}.{}.{}",
            interface_name,
            lease.assigned_ip[0], lease.assigned_ip[1], lease.assigned_ip[2], lease.assigned_ip[3],
            lease.subnet_mask[0], lease.subnet_mask[1], lease.subnet_mask[2], lease.subnet_mask[3],
            lease.router[0], lease.router[1], lease.router[2], lease.router[3],
            lease.dns_server[0], lease.dns_server[1], lease.dns_server[2], lease.dns_server[3],
        );
    }
}
