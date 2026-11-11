extern crate alloc;
use crate::fs::vfs::{FileType, InodeOperations, Stat};
use crate::net::device::NET_MANAGER;
use crate::net::tcp::{
    send_tcp_packet, TcpConnection, TcpState, TCP_FLAG_ACK, TCP_FLAG_FIN, TCP_FLAG_PSH,
    TCP_FLAG_SYN, TCP_TABLE,
};

/// Open a new TCP stream socket descriptor.
pub fn socket_open() -> Result<usize, &'static str> {
    let mut table = TCP_TABLE.lock();
    let id = table.allocate_id();
    let port = table.allocate_port();

    let net = NET_MANAGER.lock();
    let (_, cfg) = net.get_default_device().ok_or("No active network interface")?;
    drop(net);

    let conn = TcpConnection::new(id, cfg.ipv4, port, [0, 0, 0, 0], 0);
    table.connections.push(conn);

    Ok(id)
}

/// Initiate TCP 3-way handshake to remote host (SYN -> SYN-ACK -> ACK).
pub fn socket_connect(sock_id: usize, remote_ip: [u8; 4], remote_port: u16) -> Result<(), &'static str> {
    let mut table = TCP_TABLE.lock();
    let is_loopback = remote_ip == [127, 0, 0, 1];
    let has_listener = table.connections.iter().any(|c| c.local_port == remote_port && c.state == TcpState::Listen);

    let conn = table.find(sock_id).ok_or("Invalid socket ID")?;

    // Determine correct local IP for this destination route
    let net = NET_MANAGER.lock();
    if let Some((_, cfg)) = net.get_device_for_ip(remote_ip) {
        conn.local_ip = cfg.ipv4;
    }
    drop(net);

    conn.remote_ip = remote_ip;
    conn.remote_port = remote_port;
    conn.state = TcpState::SynSent;

    if is_loopback && !has_listener {
        conn.state = TcpState::Closed;
        return Err("Connection refused: no listener on loopback port");
    }

    let src_ip = conn.local_ip;
    let local_port = conn.local_port;
    let seq = conn.local_seq;
    drop(table);

    // 1. Send SYN
    send_tcp_packet(src_ip, remote_ip, local_port, remote_port, seq, 0, TCP_FLAG_SYN, &[])?;

    // 2. Poll for SYN-ACK and transition to Established
    let start_ticks = crate::arch::x86_64::pit::get_ticks();
    let timeout_ticks = start_ticks + 3000; // 3 second timeout

    while crate::arch::x86_64::pit::get_ticks() < timeout_ticks {
        crate::net::poll();

        let table = TCP_TABLE.lock();
        if let Some(c) = table.connections.iter().find(|c| c.id == sock_id) {
            if c.state == TcpState::Established {
                return Ok(());
            } else if c.state == TcpState::Closed {
                return Err("TCP Connection reset by peer");
            }
        }
        drop(table);

        core::hint::spin_loop();
    }

    // Timed out: mark closed
    let mut table = TCP_TABLE.lock();
    if let Some(c) = table.find(sock_id) {
        c.state = TcpState::Closed;
    }

    Err("Connection timed out (no SYN-ACK received)")
}

/// Send data payload through an established TCP connection.
pub fn socket_send(sock_id: usize, data: &[u8]) -> Result<usize, &'static str> {
    let mut table = TCP_TABLE.lock();
    let conn = table.find(sock_id).ok_or("Invalid socket ID")?;

    if conn.state != TcpState::Established {
        return Err("Socket is not in ESTABLISHED state");
    }

    let src_ip = conn.local_ip;
    let dst_ip = conn.remote_ip;
    let src_port = conn.local_port;
    let dst_port = conn.remote_port;
    let seq = conn.local_seq;
    let ack = conn.local_ack;

    conn.local_seq = conn.local_seq.wrapping_add(data.len() as u32);
    drop(table);

    send_tcp_packet(src_ip, dst_ip, src_port, dst_port, seq, ack, TCP_FLAG_ACK | TCP_FLAG_PSH, data)?;
    Ok(data.len())
}

/// Receive available bytes from socket RX buffer. Blocks until data arrives, timeout expires, or connection closes.
pub fn socket_recv(sock_id: usize, buf: &mut [u8], timeout_ms: u64) -> Result<usize, &'static str> {
    let start_ticks = crate::arch::x86_64::pit::get_ticks();
    let timeout_ticks = start_ticks + timeout_ms;

    while crate::arch::x86_64::pit::get_ticks() < timeout_ticks {
        crate::net::poll();

        let mut table = TCP_TABLE.lock();
        if let Some(conn) = table.find(sock_id) {
            if !conn.rx_buffer.is_empty() {
                let to_read = buf.len().min(conn.rx_buffer.len());
                for i in 0..to_read {
                    buf[i] = conn.rx_buffer.pop_front().unwrap();
                }
                return Ok(to_read);
            }

            if conn.state == TcpState::Closed || conn.state == TcpState::CloseWait {
                return Ok(0); // EOF / Peer closed
            }
        } else {
            return Err("Socket not found");
        }
        drop(table);

        core::hint::spin_loop();
    }

    // Timeout: return 0 if no data
    Ok(0)
}

/// Close a TCP connection by initiating the FIN handshake.
pub fn socket_close(sock_id: usize) -> Result<(), &'static str> {
    let mut table = TCP_TABLE.lock();
    if let Some(conn) = table.find(sock_id) {
        if conn.state == TcpState::Established || conn.state == TcpState::SynReceived {
            let src_ip = conn.local_ip;
            let dst_ip = conn.remote_ip;
            let src_port = conn.local_port;
            let dst_port = conn.remote_port;
            let seq = conn.local_seq;
            let ack = conn.local_ack;
            conn.state = TcpState::Closed;
            drop(table);

            let _ = send_tcp_packet(src_ip, dst_ip, src_port, dst_port, seq, ack, TCP_FLAG_FIN | TCP_FLAG_ACK, &[]);
        } else {
            conn.state = TcpState::Closed;
        }
    }
    Ok(())
}

/// Bind a socket to a specific local port and optional IP.
pub fn socket_bind(sock_id: usize, local_ip: [u8; 4], local_port: u16) -> Result<(), &'static str> {
    let mut table = TCP_TABLE.lock();
    let conn = table.find(sock_id).ok_or("Invalid socket ID")?;
    conn.local_port = local_port;
    if local_ip != [0, 0, 0, 0] {
        conn.local_ip = local_ip;
    }
    Ok(())
}

/// Mark a socket as listening for incoming TCP client connections.
pub fn socket_listen(sock_id: usize, _backlog: usize) -> Result<(), &'static str> {
    let mut table = TCP_TABLE.lock();
    let conn = table.find(sock_id).ok_or("Invalid socket ID")?;
    conn.state = TcpState::Listen;
    Ok(())
}

/// Accept an established client connection from the listening socket's pending queue.
pub fn socket_accept(sock_id: usize, timeout_ms: u64) -> Result<(usize, [u8; 4], u16), &'static str> {
    let start_ticks = crate::arch::x86_64::pit::get_ticks();
    let timeout_ticks = start_ticks + timeout_ms;

    while crate::arch::x86_64::pit::get_ticks() < timeout_ticks {
        crate::net::poll();

        let mut table = TCP_TABLE.lock();
        if let Some(conn) = table.find(sock_id) {
            if let Some(client_id) = conn.pending_accept.pop_front() {
                if let Some(client) = table.find(client_id) {
                    let r_ip = client.remote_ip;
                    let r_port = client.remote_port;
                    return Ok((client_id, r_ip, r_port));
                }
            }
        } else {
            return Err("Listening socket not found");
        }
        drop(table);

        core::hint::spin_loop();
    }

    Err("Accept timed out")
}

pub struct SocketInode {
    pub sock_id: usize,
}

impl InodeOperations for SocketInode {
    fn read(&self, _offset: usize, buf: &mut [u8]) -> Result<usize, &'static str> {
        socket_recv(self.sock_id, buf, 1000)
    }

    fn write(&mut self, _offset: usize, buf: &[u8]) -> Result<usize, &'static str> {
        socket_send(self.sock_id, buf)
    }

    fn stat(&self) -> Stat {
        Stat {
            inode: 200 + (self.sock_id as u64),
            size: 0,
            file_type: FileType::CharDevice,
            mode: 0o666,
        }
    }
}
