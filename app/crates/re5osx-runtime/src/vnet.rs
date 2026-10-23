//! The engine's sockets on a virtual LAN. The browser build's `netSocket`
//! binds nothing and sends nowhere; here every engine socket gets the
//! player's address on 10.77.0.0/16 and its datagrams go to the lobby's
//! players (p2p.rs). The game finds, hosts and joins sessions on it as on
//! a LAN.

use std::collections::{HashMap, VecDeque};
use std::net::Ipv4Addr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Condvar, LazyLock, Mutex};
use std::time::{Duration, Instant};

use re5osx_game::{Env, orig};

use crate::Locked;
use crate::memory::Mem;

/// `netSocket` fields (this build).
const SOCKET_HANDLE: u64 = 8; // i64, negative when unbound
const SOCKET_ADDRESS: u64 = 136; // netSocketAddress, the bound address
const SOCKET_PORT: u64 = 188; // u16, the port asked for
const SOCKET_FLAGS: u64 = 356; // u8
/// `netSocket::CanBroadcast()`: LAN session searches are answered by
/// broadcast.
const FLAG_BROADCAST: u8 = 2;
/// `netSocketAddress` / `netIpAddress` fields.
const ADDRESS_PORT: u64 = 20;
const ADDRESS_V6: u64 = 22;
/// Datagrams waiting for a socket, at most.
const QUEUE: usize = 1024;

struct Packet {
    from_ip: u32,
    from_port: u16,
    data: Vec<u8>,
}

#[derive(Default)]
struct Sockets {
    /// Datagrams received, by the engine port they are for (bound ports only).
    queues: HashMap<u16, VecDeque<Packet>>,
    /// The bound sockets' ports, by handle (the descriptor `select` gets).
    ports: HashMap<u32, u16>,
}

impl Sockets {
    fn has_data(&self, handle: u32) -> bool {
        self.ports.get(&handle).and_then(|port| self.queues.get(port)).is_some_and(|q| !q.is_empty())
    }
}

static SOCKETS: LazyLock<Mutex<Sockets>> = LazyLock::new(Mutex::default);
/// A datagram queued (`socket_select` waits for one).
static ARRIVED: Condvar = Condvar::new();

fn local_ip() -> u32 {
    crate::p2p::get().map_or(crate::p2p::NET | 1, |p| p.local_ip())
}

/// A datagram from the lobby (p2p.rs) for the engine.
pub(crate) fn deliver(from_ip: u32, from_port: u16, to_port: u16, data: &[u8]) {
    if to_port == crate::jobs::PORT {
        crate::jobs::receive(from_ip, data);
        return;
    }
    let mut sockets = SOCKETS.locked();
    if let Some(queue) = sockets.queues.get_mut(&to_port)
        && queue.len() < QUEUE
    {
        queue.push_back(Packet { from_ip, from_port, data: data.to_vec() });
        ARRIVED.notify_all();
    }
}

/// Hook for `rage::netHardware::CreateSocket(netSocket*, unsigned short,
/// netProtocol, netSocketBlockingType, netAddressFamily, ...)`: sockets are
/// bound as soon as they are created, also during `netUpdate()` (see
/// `network_up`).
pub(crate) unsafe extern "C" fn create_socket(
    env: *mut Env,
    socket: u64,
    port: u32,
    protocol: u32,
    blocking: u32,
    family: u32,
) -> u32 {
    let m = unsafe { Mem::of(env) };
    let state = m.u32(WINSOCK_STATE);
    m.set_u32(WINSOCK_STATE, 2);
    let created = unsafe { orig::create_socket(env, socket, port, protocol, blocking, family) };
    m.set_u32(WINSOCK_STATE, state);
    created
}

/// Hook for `rage::netSocket::NativeBind()`.
pub(crate) unsafe extern "C" fn native_bind(env: *mut Env, socket: u64) -> u32 {
    static EPHEMERAL: AtomicU64 = AtomicU64::new(50000);
    let m = unsafe { Mem::of(env) };
    let mut port = m.u16(socket + SOCKET_PORT);
    if port == 0 {
        port = EPHEMERAL.fetch_add(1, Ordering::Relaxed) as u16;
    }
    // The lowest free, as a system's descriptors: the engine puts them in
    // fd_sets of 1024 (a number past that would write past its set).
    let mut sockets = SOCKETS.locked();
    let handle = (1u32..).find(|h| !sockets.ports.contains_key(h)).unwrap();
    sockets.queues.entry(port).or_default();
    sockets.ports.insert(handle, port);
    drop(sockets);
    m.set_i64(socket + SOCKET_HANDLE, handle as i64);
    m.set_u8(socket + SOCKET_FLAGS, m.u8(socket + SOCKET_FLAGS) | FLAG_BROADCAST);
    let address = socket + SOCKET_ADDRESS;
    m.set_u32(address, local_ip());
    m.set_u8(address + 4, 0);
    m.set_u16(address + ADDRESS_PORT, port);
    m.set_u8(address + ADDRESS_V6, 0);
    log::debug!("vnet: socket {socket:#x} bound to {}:{port}", Ipv4Addr::from(local_ip()));
    1
}

/// Hook for `rage::netSocket::NativeShutdown()`: its port goes with it
/// (each session search makes a socket of its own: its queue, and the
/// late answers queued there, would stay for good).
pub(crate) unsafe extern "C" fn native_shutdown(env: *mut Env, socket: u64) {
    let m = unsafe { Mem::of(env) };
    if let Ok(handle) = u32::try_from(m.u64(socket + SOCKET_HANDLE) as i64) {
        let mut sockets = SOCKETS.locked();
        if let Some(port) = sockets.ports.remove(&handle)
            && !sockets.ports.values().any(|&p| p == port)
        {
            sockets.queues.remove(&port);
        }
    }
    unsafe { orig::socket_shutdown(env, socket) }
}

/// Hook for `rage::netSocket::Send(netSocketAddress const&, void const*,
/// unsigned int, netSocketError*)`.
pub(crate) unsafe extern "C" fn socket_send(
    env: *mut Env,
    socket: u64,
    to: u64,
    data: u64,
    len: u32,
    error: u64,
) -> u32 {
    let m = unsafe { Mem::of(env) };
    if error != 0 {
        m.set_u32(error, 0);
    }
    let from_port = m.u16(socket + SOCKET_ADDRESS + ADDRESS_PORT);
    if let Some(p2p) = crate::p2p::get() {
        p2p.send(from_port, m.u32(to), m.u16(to + ADDRESS_PORT), m.bytes(data, len as usize));
    }
    1
}

/// Hook for `rage::netSocket::Receive(netSocketAddress*, void*, int,
/// netSocketError*)`: a datagram for this socket, or 0.
pub(crate) unsafe extern "C" fn socket_receive(
    env: *mut Env,
    socket: u64,
    from: u64,
    buf: u64,
    max: u32,
    error: u64,
) -> u32 {
    let m = unsafe { Mem::of(env) };
    if error != 0 {
        m.set_u32(error, 0);
    }
    let port = m.u16(socket + SOCKET_ADDRESS + ADDRESS_PORT);
    let packet = SOCKETS.locked().queues.get_mut(&port).and_then(VecDeque::pop_front);
    let Some(packet) = packet else { return 0 };
    let len = packet.data.len().min(max as usize);
    m.bytes_mut(buf, len).copy_from_slice(&packet.data[..len]);
    if from != 0 {
        m.set_u32(from, packet.from_ip);
        m.set_u8(from + 4, 0);
        m.set_u16(from + ADDRESS_PORT, packet.from_port);
        m.set_u8(from + ADDRESS_V6, 0);
    }
    len as u32
}

/// Hook for `rage::netSocket::Select(int, fd_set*, fd_set*, fd_set*,
/// timeval*)`: waits until a datagram is queued for a socket of the read
/// set (the handles), up to the timeout, as `select` would; then the
/// browser build's `select`, which has every socket ready at once. Without
/// the wait the socket manager's receive thread (a second's timeout) spun,
/// a core busy and the network's locks taken from the game's update.
pub(crate) unsafe extern "C" fn socket_select(
    env: *mut Env,
    count: u32,
    read: u64,
    write: u64,
    except: u64,
    timeout: u64,
) -> u32 {
    let m = unsafe { Mem::of(env) };
    // fd_set: bits in 64-bit words.
    let handles = |set: u64| {
        (0..count.min(1024)).filter(move |&fd| set != 0 && m.u64(set + u64::from(fd / 64) * 8) >> (fd % 64) & 1 != 0)
    };
    // Sockets are always writable.
    if handles(write).next().is_none() {
        let deadline = (timeout != 0).then(|| {
            Instant::now() + Duration::from_secs(m.u64(timeout)) + Duration::from_micros(m.u32(timeout + 8).into())
        });
        let mut sockets = SOCKETS.locked();
        while !handles(read).any(|fd| sockets.has_data(fd)) {
            let Some(left) = deadline.map(|d| d.saturating_duration_since(Instant::now())) else {
                sockets = ARRIVED.wait(sockets).unwrap();
                continue;
            };
            if left.is_zero() {
                break;
            }
            sockets = ARRIVED.wait_timeout(sockets, left).unwrap().0;
        }
    }
    unsafe { orig::socket_select(env, count, read, write, except, timeout) }
}

/// Hook for `rage::netHardware::GetLocalIpAddress(netIpAddress*)`, and the
/// public one: the player's address on the virtual LAN.
pub(crate) unsafe extern "C" fn local_ip_address(env: *mut Env, ip: u64) -> u32 {
    let m = unsafe { Mem::of(env) };
    m.set_u32(ip, local_ip());
    m.set_u8(ip + 4, 0);
    1
}

pub(crate) unsafe extern "C" fn public_ip_address(env: *mut Env, ip: u64) -> u32 {
    unsafe { local_ip_address(env, ip) }
}

/// The device's network, up. In the browser build the page reported it;
/// `CLiveManager::Update` runs `netUpdate()` only while it is up.
/// `netUpdate()` begins with the hardware's check, which sees the cable
/// unplugged (the check is folded away in this build) whenever WinSock is
/// started, and then shuts the sockets down: WinSock counts as stopped
/// from `netUpdate()`'s start (`profiler_push`) to the end of the live
/// manager's update, started (state 2: sockets get bound) otherwise.
/// Sockets are bound when created all the same (`create_socket`).
const NETWORK_UP: u64 = 13346512;
const WINSOCK_STATE: u64 = 13347088;

pub(crate) fn network_up(m: Mem) {
    m.atomic_u32(NETWORK_UP).store(1, Ordering::Release);
    m.set_u32(WINSOCK_STATE, 2);
}

/// Hook for `rage::netFunctionProfiler::PushProfiler(char const*, bool)`,
/// which `CLiveManager::Update` calls just before `netUpdate()`'s hardware
/// check.
pub(crate) unsafe extern "C" fn profiler_push(env: *mut Env, profiler: u64, name: u64, flag: u32) -> u32 {
    /// "netUpdate()".
    const NET_UPDATE: u64 = 3746537;
    if name == NET_UPDATE {
        unsafe { Mem::of(env) }.set_u32(WINSOCK_STATE, 0);
    }
    unsafe { orig::profiler_push(env, profiler, name, flag) }
}

/// This module's hooks, online.
pub(crate) fn hooks(hooks: &mut re5osx_game::Hooks) {
    hooks.profiler_push = Some(profiler_push);
    hooks.create_socket = Some(create_socket);
    hooks.socket_bind = Some(native_bind);
    hooks.socket_shutdown = Some(native_shutdown);
    hooks.socket_send = Some(socket_send);
    hooks.socket_receive = Some(socket_receive);
    hooks.socket_select = Some(socket_select);
    hooks.local_ip_address = Some(local_ip_address);
    hooks.public_ip_address = Some(public_ip_address);
}
