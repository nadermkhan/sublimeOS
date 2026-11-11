extern crate alloc;
use alloc::collections::VecDeque;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use crate::fs::vfs::{FileType, InodeOperations, Stat};
use crate::sync::IrqSafeSpinlock;

pub const AF_UNIX: i32 = 1;
pub const AF_LOCAL: i32 = 1;

pub const SOCK_STREAM: i32 = 1;
pub const SOCK_DGRAM: i32 = 2;

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct SockAddrUn {
    pub sun_family: u16,
    pub sun_path: [u8; 108],
}

impl SockAddrUn {
    pub fn new(path: &str) -> Self {
        let mut sun = Self {
            sun_family: AF_UNIX as u16,
            sun_path: [0u8; 108],
        };
        let bytes = path.as_bytes();
        let len = bytes.len().min(107);
        sun.sun_path[..len].copy_from_slice(&bytes[..len]);
        sun
    }

    pub fn path_str(&self) -> &str {
        let len = self.sun_path.iter().position(|&c| c == 0).unwrap_or(108);
        core::str::from_utf8(&self.sun_path[..len]).unwrap_or("")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnixSocketState {
    Unbound,
    Bound,
    Listening,
    Connected,
    Closed,
}

pub struct UnixSocket {
    pub id: usize,
    pub sock_type: i32,
    pub path: Option<String>,
    pub state: UnixSocketState,
    pub peer_id: Option<usize>,
    pub rx_buffer: VecDeque<u8>,
    pub dgram_buffer: VecDeque<Vec<u8>>,
    pub pending_accept: VecDeque<usize>,
}

impl UnixSocket {
    pub fn new(id: usize, sock_type: i32) -> Self {
        Self {
            id,
            sock_type,
            path: None,
            state: UnixSocketState::Unbound,
            peer_id: None,
            rx_buffer: VecDeque::with_capacity(4096),
            dgram_buffer: VecDeque::new(),
            pending_accept: VecDeque::new(),
        }
    }
}

pub struct SocketEntry {
    pub id: usize,
    pub path: Option<String>,
    pub sock: Arc<IrqSafeSpinlock<UnixSocket>>,
}

pub struct UnixSocketRegistry {
    pub sockets: Vec<SocketEntry>,
    pub next_id: usize,
}

impl UnixSocketRegistry {
    pub const fn new() -> Self {
        Self {
            sockets: Vec::new(),
            next_id: 1,
        }
    }

    pub fn allocate(&mut self, sock_type: i32) -> Arc<IrqSafeSpinlock<UnixSocket>> {
        let id = self.next_id;
        self.next_id += 1;
        let sock = Arc::new(IrqSafeSpinlock::new(UnixSocket::new(id, sock_type)));
        self.sockets.push(SocketEntry {
            id,
            path: None,
            sock: sock.clone(),
        });
        sock
    }

    pub fn get(&self, id: usize) -> Option<Arc<IrqSafeSpinlock<UnixSocket>>> {
        self.sockets.iter().find(|e| e.id == id).map(|e| e.sock.clone())
    }

    pub fn find_by_path(&self, path: &str) -> Option<Arc<IrqSafeSpinlock<UnixSocket>>> {
        self.sockets.iter().find(|e| e.path.as_deref() == Some(path)).map(|e| e.sock.clone())
    }

    pub fn set_path(&mut self, id: usize, path: &str) {
        if let Some(e) = self.sockets.iter_mut().find(|e| e.id == id) {
            e.path = Some(String::from(path));
        }
    }

    pub fn remove(&mut self, id: usize) {
        self.sockets.retain(|e| e.id != id);
    }
}

pub static UNIX_SOCKETS: IrqSafeSpinlock<UnixSocketRegistry> =
    IrqSafeSpinlock::new(UnixSocketRegistry::new());

/// Create a new UNIX domain socket.
pub fn unix_socket_open(sock_type: i32) -> usize {
    let mut reg = UNIX_SOCKETS.lock();
    let sock = reg.allocate(sock_type);
    let id = sock.lock().id;
    id
}

/// Bind a UNIX domain socket to a filesystem pathname.
pub fn unix_socket_bind(id: usize, path: &str) -> Result<(), &'static str> {
    let mut reg = UNIX_SOCKETS.lock();
    if reg.find_by_path(path).is_some() {
        return Err("Address already in use (path bound)");
    }
    let sock_arc = reg.get(id).ok_or("Socket not found")?;
    let mut sock = sock_arc.lock();
    sock.path = Some(String::from(path));
    sock.state = UnixSocketState::Bound;
    reg.set_path(id, path);
    Ok(())
}

/// Mark a stream UNIX socket as listening for client connections.
pub fn unix_socket_listen(id: usize, _backlog: usize) -> Result<(), &'static str> {
    let reg = UNIX_SOCKETS.lock();
    let sock_arc = reg.get(id).ok_or("Socket not found")?;
    let mut sock = sock_arc.lock();
    if sock.sock_type != SOCK_STREAM {
        return Err("Cannot listen on non-stream socket");
    }
    sock.state = UnixSocketState::Listening;
    Ok(())
}

/// Connect a UNIX socket to a listening or destination server at the given pathname.
pub fn unix_socket_connect(id: usize, target_path: &str) -> Result<(), &'static str> {
    let mut reg = UNIX_SOCKETS.lock();
    let client_arc = reg.get(id).ok_or("Client socket not found")?;
    let server_arc = reg.find_by_path(target_path).ok_or("No such file or server socket")?;

    let is_dgram = client_arc.lock().sock_type == SOCK_DGRAM;
    if is_dgram {
        let mut client = client_arc.lock();
        client.peer_id = Some(server_arc.lock().id);
        client.state = UnixSocketState::Connected;
        return Ok(());
    }

    // Stream socket connection:
    let mut server = server_arc.lock();
    if server.state != UnixSocketState::Listening {
        return Err("Connection refused: server is not listening");
    }

    // Allocate an accepted server endpoint pair for this client
    let child_id = reg.next_id;
    reg.next_id += 1;
    let child_arc = Arc::new(IrqSafeSpinlock::new(UnixSocket::new(child_id, SOCK_STREAM)));
    {
        let mut child = child_arc.lock();
        child.state = UnixSocketState::Connected;
        child.peer_id = Some(id);
    }
    reg.sockets.push(SocketEntry {
        id: child_id,
        path: None,
        sock: child_arc,
    });

    let mut client = client_arc.lock();
    client.state = UnixSocketState::Connected;
    client.peer_id = Some(child_id);

    server.pending_accept.push_back(child_id);
    Ok(())
}

/// Accept an incoming connection on a listening UNIX stream socket.
pub fn unix_socket_accept(id: usize) -> Result<usize, &'static str> {
    let reg = UNIX_SOCKETS.lock();
    let server_arc = reg.get(id).ok_or("Server socket not found")?;
    let mut server = server_arc.lock();
    if server.state != UnixSocketState::Listening {
        return Err("Socket is not in listening state");
    }

    if let Some(child_id) = server.pending_accept.pop_front() {
        Ok(child_id)
    } else {
        Err("No pending connections (EAGAIN)")
    }
}

/// Send data across a UNIX domain socket.
pub fn unix_socket_send(id: usize, data: &[u8]) -> Result<usize, &'static str> {
    let reg = UNIX_SOCKETS.lock();
    let sock_arc = reg.get(id).ok_or("Socket not found")?;
    let peer_id = {
        let sock = sock_arc.lock();
        sock.peer_id.ok_or("Socket is not connected")?
    };

    let peer_arc = reg.get(peer_id).ok_or("Peer disconnected (EPIPE)")?;
    drop(reg);

    let mut peer = peer_arc.lock();
    if peer.state == UnixSocketState::Closed {
        return Err("Peer disconnected (EPIPE)");
    }

    if peer.sock_type == SOCK_DGRAM {
        peer.dgram_buffer.push_back(data.to_vec());
    } else {
        peer.rx_buffer.extend(data.iter().copied());
    }

    Ok(data.len())
}

/// Receive data from a UNIX domain socket.
pub fn unix_socket_recv(id: usize, buf: &mut [u8]) -> Result<usize, &'static str> {
    let sock_arc = {
        let reg = UNIX_SOCKETS.lock();
        reg.get(id).ok_or("Socket not found")?
    };
    let mut sock = sock_arc.lock();

    if sock.sock_type == SOCK_DGRAM {
        if let Some(pkt) = sock.dgram_buffer.pop_front() {
            let n = buf.len().min(pkt.len());
            buf[..n].copy_from_slice(&pkt[..n]);
            return Ok(n);
        }
    } else {
        if !sock.rx_buffer.is_empty() {
            let n = buf.len().min(sock.rx_buffer.len());
            for i in 0..n {
                buf[i] = sock.rx_buffer.pop_front().unwrap();
            }
            return Ok(n);
        }
    }

    if sock.state == UnixSocketState::Closed {
        return Ok(0); // EOF
    }

    Err("No data available (EAGAIN)")
}

/// Close a UNIX domain socket and unlink its VFS node if bound.
pub fn unix_socket_close(id: usize) -> Result<(), &'static str> {
    let mut reg = UNIX_SOCKETS.lock();
    if let Some(sock_arc) = reg.get(id) {
        let mut sock = sock_arc.lock();
        sock.state = UnixSocketState::Closed;
        if let Some(peer_id) = sock.peer_id {
            if let Some(peer_arc) = reg.get(peer_id) {
                let mut peer = peer_arc.lock();
                if peer.peer_id == Some(id) {
                    peer.peer_id = None;
                }
            }
        }
        sock.path = None;
        reg.remove(id);
    }
    Ok(())
}

pub struct UnixSocketInode {
    pub sock_id: usize,
}

impl InodeOperations for UnixSocketInode {
    fn read(&self, _offset: usize, buf: &mut [u8]) -> Result<usize, &'static str> {
        unix_socket_recv(self.sock_id, buf)
    }

    fn write(&mut self, _offset: usize, buf: &[u8]) -> Result<usize, &'static str> {
        unix_socket_send(self.sock_id, buf)
    }

    fn stat(&self) -> Stat {
        Stat {
            inode: 5000 + (self.sock_id as u64),
            size: 0,
            file_type: FileType::Socket,
            mode: 0o777,
        }
    }
}
