//! What differs between the platforms: socket options, disk read-ahead,
//! free space, sparse files, the open-file limit.

use std::fs::File;
use std::io;
use std::net::TcpStream;
use std::path::Path;

/// Unsent bytes the kernel may hold for a connection before the sender is
/// told to wait: the server queues a chunk only when the wire is about to
/// need it, so an urgent request is never behind more than this (plus one
/// chunk). Linux, Android, macOS and iOS have the option; elsewhere the
/// kernel's send buffer bounds it.
const NOTSENT_LOWAT: i32 = 128 * 1024;

pub fn tune_server_socket(s: &TcpStream) {
    let _ = s.set_nodelay(true);
    // A client gone without a word (a phone out of range, asleep) is let go
    // of after a minute: probed once the connection is idle for 30 s, every
    // 10 s, three times. (No read timeout: a client may ask nothing for
    // long, and still be there.)
    let keepalive = socket2::TcpKeepalive::new()
        .with_time(std::time::Duration::from_secs(30))
        .with_interval(std::time::Duration::from_secs(10))
        .with_retries(3);
    let _ = socket2::SockRef::from(s).set_tcp_keepalive(&keepalive);
    #[cfg(any(target_os = "linux", target_os = "android", target_vendor = "apple"))]
    unsafe {
        #[cfg(target_vendor = "apple")]
        const TCP_NOTSENT_LOWAT: libc::c_int = 0x201;
        #[cfg(not(target_vendor = "apple"))]
        const TCP_NOTSENT_LOWAT: libc::c_int = 25;
        use std::os::fd::AsRawFd;
        let v: libc::c_int = NOTSENT_LOWAT;
        libc::setsockopt(
            s.as_raw_fd(),
            libc::IPPROTO_TCP,
            TCP_NOTSENT_LOWAT,
            &v as *const _ as *const libc::c_void,
            size_of::<libc::c_int>() as libc::socklen_t,
        );
    }
}

/// Waits until the socket can take more data (with [`NOTSENT_LOWAT`]: until
/// little is left unsent). Errors and timeouts just let the write go ahead.
pub fn wait_writable(s: &TcpStream) {
    #[cfg(unix)]
    unsafe {
        use std::os::fd::AsRawFd;
        let mut p = libc::pollfd { fd: s.as_raw_fd(), events: libc::POLLOUT, revents: 0 };
        libc::poll(&mut p, 1, 1000);
    }
    #[cfg(not(unix))]
    let _ = s;
}

/// Tells the disk the server will read this range soon (requests wait in a
/// queue; a cold disk then reads ahead of the sender).
pub fn advise(f: &File, off: u64, len: u64) {
    #[cfg(any(target_os = "linux", target_os = "android"))]
    unsafe {
        use std::os::fd::AsRawFd;
        libc::posix_fadvise(f.as_raw_fd(), off as libc::off_t, len as libc::off_t, libc::POSIX_FADV_WILLNEED);
    }
    #[cfg(target_vendor = "apple")]
    unsafe {
        use std::os::fd::AsRawFd;
        let ra = libc::radvisory { ra_offset: off as libc::off_t, ra_count: len.min(i32::MAX as u64) as libc::c_int };
        libc::fcntl(f.as_raw_fd(), libc::F_RDADVISE, &ra);
    }
    #[cfg(not(any(target_os = "linux", target_os = "android", target_vendor = "apple")))]
    let _ = (f, off, len);
}

/// Bytes free for this app on the volume holding `path`.
pub fn free_space(path: &Path) -> Option<u64> {
    #[cfg(unix)]
    unsafe {
        use std::os::unix::ffi::OsStrExt;
        let c = std::ffi::CString::new(path.as_os_str().as_bytes()).ok()?;
        let mut s: libc::statvfs = std::mem::zeroed();
        if libc::statvfs(c.as_ptr(), &mut s) != 0 {
            return None;
        }
        Some(s.f_bavail as u64 * s.f_frsize as u64)
    }
    #[cfg(windows)]
    unsafe {
        use std::os::windows::ffi::OsStrExt;
        let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        let mut free = 0u64;
        let ok = windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW(
            wide.as_ptr(),
            &mut free,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
        (ok != 0).then_some(free)
    }
}

/// Makes `f` sparse where files are not by default (Windows: otherwise
/// setting its length would take the whole size on disk).
pub fn make_sparse(f: &File) {
    #[cfg(windows)]
    unsafe {
        use std::os::windows::io::AsRawHandle;
        let mut returned = 0u32;
        windows_sys::Win32::System::IO::DeviceIoControl(
            f.as_raw_handle(),
            windows_sys::Win32::System::Ioctl::FSCTL_SET_SPARSE,
            std::ptr::null(),
            0,
            std::ptr::null_mut(),
            0,
            &mut returned,
            std::ptr::null_mut(),
        );
    }
    #[cfg(not(windows))]
    let _ = f;
}

/// A file's data to the drive. On Apple systems, only that: `fsync` (the
/// drive may still cache it), not `F_FULLFSYNC`, a flush of the drive's
/// whole cache for every file; [`sync_full`] on a later file covers it.
pub fn sync_data(f: &File) -> io::Result<()> {
    #[cfg(target_vendor = "apple")]
    unsafe {
        use std::os::fd::AsRawFd;
        if libc::fsync(f.as_raw_fd()) != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
    #[cfg(not(target_vendor = "apple"))]
    f.sync_data()
}

/// A file's data on permanent storage, and on Apple systems everything
/// [`sync_data`] sent before it too (`F_FULLFSYNC` flushes the drive's
/// cache).
pub fn sync_full(f: &File) -> io::Result<()> {
    #[cfg(target_vendor = "apple")]
    unsafe {
        use std::os::fd::AsRawFd;
        if libc::fcntl(f.as_raw_fd(), libc::F_FULLFSYNC) != 0 && libc::fsync(f.as_raw_fd()) != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
    #[cfg(not(target_vendor = "apple"))]
    f.sync_all()
}

/// The ranges of `f` that take space on disk (`SEEK_DATA`/`SEEK_HOLE`).
#[cfg(target_vendor = "apple")]
pub fn data_extents(f: &File) -> io::Result<Vec<(u64, u64)>> {
    use std::os::fd::AsRawFd;
    const SEEK_HOLE: libc::c_int = 3;
    const SEEK_DATA: libc::c_int = 4;
    let size = f.metadata()?.len();
    let (fd, mut out, mut off) = (f.as_raw_fd(), Vec::new(), 0u64);
    while off < size {
        let start = unsafe { libc::lseek(fd, off as libc::off_t, SEEK_DATA) };
        if start < 0 {
            let e = io::Error::last_os_error();
            // ENXIO: no data after `off`.
            return if e.raw_os_error() == Some(libc::ENXIO) { Ok(out) } else { Err(e) };
        }
        let end = unsafe { libc::lseek(fd, start, SEEK_HOLE) };
        if end < 0 {
            return Err(io::Error::last_os_error());
        }
        out.push((start as u64, end as u64));
        off = end as u64;
    }
    Ok(out)
}

/// Frees the space of `len` bytes of `f` at `off` (block-aligned), which then
/// read as zeros.
#[cfg(target_vendor = "apple")]
pub fn punch_hole(f: &File, off: u64, len: u64) -> io::Result<()> {
    use std::os::fd::AsRawFd;
    #[repr(C)]
    struct Punch {
        flags: u32,
        reserved: u32,
        offset: libc::off_t,
        length: libc::off_t,
    }
    const F_PUNCHHOLE: libc::c_int = 99;
    let p = Punch { flags: 0, reserved: 0, offset: off as libc::off_t, length: len as libc::off_t };
    if unsafe { libc::fcntl(f.as_raw_fd(), F_PUNCHHOLE, &p) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// Positioned reads and writes, which leave the handle's offset alone.
pub fn read_at(f: &File, buf: &mut [u8], off: u64) -> io::Result<usize> {
    #[cfg(unix)]
    return std::os::unix::fs::FileExt::read_at(f, buf, off);
    #[cfg(windows)]
    return std::os::windows::fs::FileExt::seek_read(f, buf, off);
}

pub fn write_at(f: &File, buf: &[u8], off: u64) -> io::Result<usize> {
    #[cfg(unix)]
    return std::os::unix::fs::FileExt::write_at(f, buf, off);
    #[cfg(windows)]
    return std::os::windows::fs::FileExt::seek_write(f, buf, off);
}

/// Reads until `buf` is full or the file ends; returns the bytes read.
pub fn read_full_at(f: &File, buf: &mut [u8], off: u64) -> io::Result<usize> {
    let mut done = 0;
    while done < buf.len() {
        match read_at(f, &mut buf[done..], off + done as u64) {
            Ok(0) => break,
            Ok(n) => done += n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(done)
}

pub fn write_all_at(f: &File, mut buf: &[u8], mut off: u64) -> io::Result<()> {
    while !buf.is_empty() {
        match write_at(f, buf, off) {
            Ok(0) => return Err(io::ErrorKind::WriteZero.into()),
            Ok(n) => {
                buf = &buf[n..];
                off += n as u64;
            }
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

/// A server or a cache keeps thousands of the game's files open; a process
/// started from the Finder or the home screen may have 256 at most.
pub fn raise_open_file_limit() {
    #[cfg(unix)]
    unsafe {
        let mut limit: libc::rlimit = std::mem::zeroed();
        if libc::getrlimit(libc::RLIMIT_NOFILE, &mut limit) != 0 {
            return;
        }
        // macOS takes no more than OPEN_MAX (10240) under an unlimited hard limit.
        let want = limit.rlim_max.min(10240);
        if limit.rlim_cur < want {
            limit.rlim_cur = want;
            libc::setrlimit(libc::RLIMIT_NOFILE, &limit);
        }
    }
}

/// How many files a cache may keep open (after [`raise_open_file_limit`]):
/// half the limit, at most 4096.
pub fn open_file_budget() -> usize {
    #[cfg(unix)]
    unsafe {
        let mut limit: libc::rlimit = std::mem::zeroed();
        if libc::getrlimit(libc::RLIMIT_NOFILE, &mut limit) == 0 {
            return (limit.rlim_cur as usize / 2).clamp(64, 4096);
        }
        256
    }
    #[cfg(not(unix))]
    4096
}

/// The addresses other devices reach this machine at: the default route's
/// first, then the other networks (Tailscale's 100.64/10 included; virtual
/// machines' and containers' bridges left out).
pub fn lan_addresses() -> Vec<std::net::Ipv4Addr> {
    use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, UdpSocket};
    let reachable = |ip: Ipv4Addr| ip.is_private() || (ip.octets()[0] == 100 && ip.octets()[1] & 0xc0 == 64);
    let mut out = Vec::new();
    // The address the default route leaves from (connecting a UDP socket sends nothing).
    if let Ok(s) = UdpSocket::bind(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0))
        && s.connect(SocketAddrV4::new(Ipv4Addr::new(192, 0, 2, 1), 9)).is_ok()
        && let Ok(SocketAddr::V4(a)) = s.local_addr()
        && reachable(*a.ip())
    {
        out.push(*a.ip());
    }
    #[cfg(unix)]
    unsafe {
        const VIRTUAL: [&str; 9] = ["bridge", "vmnet", "docker", "br-", "veth", "vbox", "virbr", "awdl", "llw"];
        let mut list: *mut libc::ifaddrs = std::ptr::null_mut();
        if libc::getifaddrs(&mut list) == 0 {
            let mut at = list;
            while !at.is_null() {
                let a = (*at).ifa_addr;
                let name = std::ffi::CStr::from_ptr((*at).ifa_name).to_string_lossy();
                if !a.is_null() && (*a).sa_family as i32 == libc::AF_INET && !VIRTUAL.iter().any(|v| name.starts_with(v)) {
                    let sin = &*(a as *const libc::sockaddr_in);
                    let ip = Ipv4Addr::from(u32::from_be(sin.sin_addr.s_addr));
                    if reachable(ip) && !out.contains(&ip) {
                        out.push(ip);
                    }
                }
                at = (*at).ifa_next;
            }
            libc::freeifaddrs(list);
        }
    }
    out
}
