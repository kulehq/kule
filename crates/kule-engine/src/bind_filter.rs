//! Linux, where the command has no network of its own (no unprivileged
//! user namespaces, as on Ubuntu 24.04, or a policy that allows the
//! network): servers listen on this computer only, whatever starts them.
//!
//! A seccomp filter hands every `bind` and `listen` of the command and all
//! it starts to Kule (a user notification). For a TCP socket, or a UDP one
//! on a fixed port, Kule takes a copy of the socket from the process
//! (`pidfd_getfd`) and does the call itself: a socket asking for every
//! address is bound to loopback instead, a socket asking for an address
//! other than loopback is refused, and an unbound socket is bound to
//! loopback before it listens (`listen` would bind it to every address).
//! Kule never lets the kernel read the address again after it looked, so
//! the process cannot change it in between. Other sockets (Unix, netlink,
//! a UDP client's) go on as asked.
//!
//! `io_uring`, which can bind without a system call, is refused, and so are
//! system calls of another architecture (32-bit), which have their own
//! numbers. A process that swaps a Unix socket's descriptor for a TCP one
//! between Kule's look and the kernel's could still bind the latter as it
//! asked; only a program racing its own threads on purpose can.

use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::sync::OnceLock;

use crate::namespace::{receive_fd, send_fd, socket_pair};

#[cfg(target_arch = "x86_64")]
const ARCH: u32 = 0xC000_003E; // AUDIT_ARCH_X86_64
#[cfg(target_arch = "aarch64")]
const ARCH: u32 = 0xC000_00B7; // AUDIT_ARCH_AARCH64
/// x32's system calls are x86_64's with this bit set.
const X32_BIT: u32 = 0x4000_0000;

/// The filter for one command, and the end Kule hears it on.
pub(crate) struct BindFilter {
    to_kule: OwnedFd,
    program: Vec<libc::sock_filter>,
}

// SAFETY: the program is plain data, only read.
unsafe impl Send for BindFilter {}
unsafe impl Sync for BindFilter {}

/// Whether this system lets Kule do a process's binds for it: seccomp user
/// notifications and `pidfd_getfd` on a child; asked once, by trying.
pub(crate) fn available() -> bool {
    static AVAILABLE: OnceLock<bool> = OnceLock::new();
    *AVAILABLE.get_or_init(probe)
}

fn probe() -> bool {
    use std::os::unix::process::CommandExt;
    let Ok(filter) = BindFilter::new() else {
        return false;
    };
    let mut probe = std::process::Command::new("/bin/sh");
    probe
        .args(["-c", "exit 0"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    // SAFETY: enter only makes system calls on data prepared before the fork.
    unsafe {
        probe.pre_exec(move || {
            check(libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0))?;
            filter.enter()
        });
    }
    probe.status().is_ok_and(|s| s.success()) && getfd_works()
}

/// Whether Kule may take a descriptor from a child (Yama and the kernel
/// allow `pidfd_getfd`).
fn getfd_works() -> bool {
    let Ok(mut child) = std::process::Command::new("/bin/sh")
        .args(["-c", "read x"])
        .stdin(std::process::Stdio::piped())
        .spawn()
    else {
        return false;
    };
    let worked = take_fd(child.id() as i32, 0).is_ok();
    let _ = child.kill();
    let _ = child.wait();
    worked
}

impl BindFilter {
    /// The filter for one command, and Kule's side of it, which waits for
    /// the command's notifications and answers them until it is gone.
    pub(crate) fn new() -> io::Result<BindFilter> {
        let (from_command, to_kule) = socket_pair()?;
        std::thread::spawn(move || {
            if let Some(listener) = receive_fd(&from_command) {
                drop(from_command);
                supervise(listener);
            }
        });
        Ok(BindFilter {
            to_kule,
            program: program(),
        })
    }

    /// Runs in the child, between fork and exec, after no_new_privs: only
    /// system calls.
    pub(crate) fn enter(&self) -> io::Result<()> {
        let prog = libc::sock_fprog {
            len: self.program.len() as u16,
            filter: self.program.as_ptr().cast_mut(),
        };
        // SAFETY: prog points at the program, alive for the call.
        let fd = unsafe {
            libc::syscall(
                libc::SYS_seccomp,
                libc::SECCOMP_SET_MODE_FILTER,
                libc::SECCOMP_FILTER_FLAG_NEW_LISTENER,
                &prog as *const libc::sock_fprog,
            )
        };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        let fd = fd as RawFd;
        // SAFETY: fd was just made; the socket was made before the fork.
        let sent = unsafe { send_fd(self.to_kule.as_raw_fd(), fd) };
        // SAFETY: closed once, here.
        unsafe { libc::close(fd) };
        sent
    }
}

fn statement(code: u16, k: u32) -> libc::sock_filter {
    libc::sock_filter {
        code,
        jt: 0,
        jf: 0,
        k,
    }
}

fn jump(code: u16, k: u32, jt: u8, jf: u8) -> libc::sock_filter {
    libc::sock_filter { code, jt, jf, k }
}

/// The program: another architecture's calls and io_uring refused, bind
/// and listen to Kule, everything else allowed.
fn program() -> Vec<libc::sock_filter> {
    const LD_W_ABS: u16 = (libc::BPF_LD | libc::BPF_W | libc::BPF_ABS) as u16;
    const JEQ: u16 = (libc::BPF_JMP | libc::BPF_JEQ | libc::BPF_K) as u16;
    const JGE: u16 = (libc::BPF_JMP | libc::BPF_JGE | libc::BPF_K) as u16;
    const RET: u16 = (libc::BPF_RET | libc::BPF_K) as u16;
    // offsets in struct seccomp_data
    const NR: u32 = 0;
    const ARCH_AT: u32 = 4;
    let errno = |e: i32| libc::SECCOMP_RET_ERRNO | (e as u32 & libc::SECCOMP_RET_DATA);
    vec![
        statement(LD_W_ABS, ARCH_AT),
        jump(JEQ, ARCH, 1, 0),
        statement(RET, errno(libc::ENOSYS)),
        statement(LD_W_ABS, NR),
        jump(JGE, X32_BIT, 0, 1),
        statement(RET, errno(libc::ENOSYS)),
        jump(JEQ, libc::SYS_io_uring_setup as u32, 0, 1),
        statement(RET, errno(libc::ENOSYS)),
        jump(JEQ, libc::SYS_bind as u32, 2, 0),
        jump(JEQ, libc::SYS_listen as u32, 1, 0),
        statement(RET, libc::SECCOMP_RET_ALLOW),
        statement(RET, libc::SECCOMP_RET_USER_NOTIF),
    ]
}

fn check(r: libc::c_int) -> io::Result<libc::c_int> {
    if r == -1 {
        Err(io::Error::last_os_error())
    } else {
        Ok(r)
    }
}

/// Descriptor `fd` of process `pid`, as a descriptor of Kule's.
fn take_fd(pid: i32, fd: i32) -> io::Result<OwnedFd> {
    // SAFETY: plain system calls; descriptors made here are owned here.
    unsafe {
        let pidfd = libc::syscall(libc::SYS_pidfd_open, pid, 0);
        if pidfd < 0 {
            return Err(io::Error::last_os_error());
        }
        let pidfd = OwnedFd::from_raw_fd(pidfd as RawFd);
        let taken = libc::syscall(libc::SYS_pidfd_getfd, pidfd.as_raw_fd(), fd, 0);
        if taken < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(OwnedFd::from_raw_fd(taken as RawFd))
    }
}

/// Answers the command's binds and listens until it and all it started
/// are gone.
fn supervise(listener: OwnedFd) {
    let fd = listener.as_raw_fd();
    loop {
        let mut poll = libc::pollfd {
            fd,
            events: libc::POLLIN,
            revents: 0,
        };
        // SAFETY: one live pollfd.
        if unsafe { libc::poll(&mut poll, 1, -1) } == -1 {
            if io::Error::last_os_error().kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return;
        }
        if poll.revents & libc::POLLIN == 0 {
            // POLLHUP: no process holds the filter any more.
            return;
        }
        // SAFETY: an all-zero notification is valid; the kernel fills it.
        let mut request: libc::seccomp_notif = unsafe { std::mem::zeroed() };
        // SAFETY: request is sized for the kernel's notification.
        if unsafe { libc::ioctl(fd, libc::SECCOMP_IOCTL_NOTIF_RECV, &mut request) } == -1 {
            match io::Error::last_os_error().raw_os_error() {
                Some(libc::EINTR | libc::ENOENT) => continue,
                _ => return,
            }
        }
        let answer = answer(fd, &request);
        let mut response = libc::seccomp_notif_resp {
            id: request.id,
            val: 0,
            error: 0,
            flags: 0,
        };
        match answer {
            Answer::Continue => response.flags = libc::SECCOMP_USER_NOTIF_FLAG_CONTINUE as u32,
            Answer::Done => {}
            Answer::Refuse(errno) => response.error = -errno,
        }
        // SAFETY: response is a complete answer to request; a process gone
        // meanwhile (ENOENT) needs none.
        unsafe { libc::ioctl(fd, libc::SECCOMP_IOCTL_NOTIF_SEND, &mut response) };
    }
}

enum Answer {
    /// The kernel does the call as asked.
    Continue,
    /// Kule did it; it succeeded.
    Done,
    /// It failed, or is refused, with this errno.
    Refuse(i32),
}

/// What an address asks for.
#[derive(Debug, PartialEq, Eq)]
enum Asks {
    Every,
    Loopback,
    Other,
}

/// What the bytes of a `sockaddr_in` or `sockaddr_in6` ask for, and their
/// port; `None` for another family.
fn asks(address: &[u8]) -> Option<(Asks, u16)> {
    if address.len() < 4 {
        return None;
    }
    let family = u16::from_ne_bytes([address[0], address[1]]) as i32;
    let port = u16::from_be_bytes([address[2], address[3]]);
    match family {
        libc::AF_INET if address.len() >= 8 => {
            let ip = std::net::Ipv4Addr::new(address[4], address[5], address[6], address[7]);
            Some((v4(ip), port))
        }
        libc::AF_INET6 if address.len() >= 24 => {
            let bytes: [u8; 16] = address[8..24].try_into().ok()?;
            let ip = std::net::Ipv6Addr::from(bytes);
            let asks = match ip.to_ipv4_mapped() {
                Some(v4ip) => v4(v4ip),
                None if ip.is_unspecified() => Asks::Every,
                None if ip.is_loopback() => Asks::Loopback,
                None => Asks::Other,
            };
            Some((asks, port))
        }
        _ => None,
    }
}

fn v4(ip: std::net::Ipv4Addr) -> Asks {
    if ip.is_unspecified() {
        Asks::Every
    } else if ip.is_loopback() {
        Asks::Loopback
    } else {
        Asks::Other
    }
}

fn int_option(fd: RawFd, level: i32, name: i32) -> Option<i32> {
    let mut value: libc::c_int = 0;
    let mut len = std::mem::size_of::<libc::c_int>() as libc::socklen_t;
    // SAFETY: value and len are live and sized for an int.
    let r = unsafe {
        libc::getsockopt(
            fd,
            level,
            name,
            (&mut value as *mut libc::c_int).cast(),
            &mut len,
        )
    };
    (r == 0).then_some(value)
}

/// Binds `socket` (of `family`) to loopback on `port`: for IPv6, the IPv4
/// loopback on a dual-stack socket (so 127.0.0.1 still reaches it), `::1`
/// on one that takes IPv6 only.
fn bind_loopback(socket: RawFd, family: i32, port: u16) -> Answer {
    // SAFETY: the address structs are fully set up and sized for the call.
    let r = unsafe {
        if family == libc::AF_INET {
            let mut address: libc::sockaddr_in = std::mem::zeroed();
            address.sin_family = libc::AF_INET as libc::sa_family_t;
            address.sin_port = port.to_be();
            address.sin_addr.s_addr = u32::from(std::net::Ipv4Addr::LOCALHOST).to_be();
            libc::bind(
                socket,
                (&address as *const libc::sockaddr_in).cast(),
                std::mem::size_of::<libc::sockaddr_in>() as libc::socklen_t,
            )
        } else {
            let only6 = int_option(socket, libc::IPPROTO_IPV6, libc::IPV6_V6ONLY) == Some(1);
            let ip = if only6 {
                std::net::Ipv6Addr::LOCALHOST
            } else {
                std::net::Ipv4Addr::LOCALHOST.to_ipv6_mapped()
            };
            let mut address: libc::sockaddr_in6 = std::mem::zeroed();
            address.sin6_family = libc::AF_INET6 as libc::sa_family_t;
            address.sin6_port = port.to_be();
            address.sin6_addr.s6_addr = ip.octets();
            libc::bind(
                socket,
                (&address as *const libc::sockaddr_in6).cast(),
                std::mem::size_of::<libc::sockaddr_in6>() as libc::socklen_t,
            )
        }
    };
    if r == 0 {
        Answer::Done
    } else {
        Answer::Refuse(
            io::Error::last_os_error()
                .raw_os_error()
                .unwrap_or(libc::EACCES),
        )
    }
}

/// Whether notification `id` still stands for the process that made it, so
/// what was read of it is that process's.
fn still_valid(listener: RawFd, id: u64) -> bool {
    let mut id = id;
    // SAFETY: id is a live u64.
    unsafe { libc::ioctl(listener, libc::SECCOMP_IOCTL_NOTIF_ID_VALID, &mut id) == 0 }
}

fn answer(listener: RawFd, request: &libc::seccomp_notif) -> Answer {
    let pid = request.pid as i32;
    let args = request.data.args;
    // Every decision is made on a copy of the socket itself, not on its
    // number in the process, which may be reused meanwhile.
    let Ok(socket) = take_fd(pid, args[0] as i32) else {
        // Not a descriptor, or gone: the kernel says so itself.
        return Answer::Continue;
    };
    let socket = socket.as_raw_fd();
    let Some(family) = int_option(socket, libc::SOL_SOCKET, libc::SO_DOMAIN) else {
        // Not a socket: the kernel refuses it as it would.
        return Answer::Continue;
    };
    if family != libc::AF_INET && family != libc::AF_INET6 {
        return Answer::Continue;
    }
    let kind = int_option(socket, libc::SOL_SOCKET, libc::SO_TYPE).unwrap_or(libc::SOCK_STREAM);
    if request.data.nr == libc::SYS_listen as i32 {
        // Unbound, listen would bind it to every address: loopback first.
        // SAFETY: the sockaddr buffer is large enough for either family.
        let mut bound: libc::sockaddr_storage = unsafe { std::mem::zeroed() };
        let mut len = std::mem::size_of::<libc::sockaddr_storage>() as libc::socklen_t;
        let named = unsafe {
            libc::getsockname(
                socket,
                (&mut bound as *mut libc::sockaddr_storage).cast(),
                &mut len,
            )
        };
        // SAFETY: `bound` is a live sockaddr_storage; the length the kernel
        // reports is held to its size, so the slice stays within it.
        let bytes = unsafe {
            std::slice::from_raw_parts(
                (&bound as *const libc::sockaddr_storage).cast::<u8>(),
                (len as usize).min(std::mem::size_of::<libc::sockaddr_storage>()),
            )
        };
        if named == 0
            && let Some((Asks::Every, 0)) = asks(bytes)
            && let Answer::Refuse(e) = bind_loopback(socket, family, 0)
        {
            return Answer::Refuse(e);
        }
        // SAFETY: plain listen on Kule's copy of the socket.
        let r = unsafe { libc::listen(socket, args[1] as i32) };
        return if r == 0 {
            Answer::Done
        } else {
            Answer::Refuse(
                io::Error::last_os_error()
                    .raw_os_error()
                    .unwrap_or(libc::EACCES),
            )
        };
    }
    // bind: what address it asks for, read from the process's memory.
    let len = (args[2] as usize).min(std::mem::size_of::<libc::sockaddr_in6>());
    let mut buf = [0u8; std::mem::size_of::<libc::sockaddr_in6>()];
    let local = libc::iovec {
        iov_base: buf.as_mut_ptr().cast(),
        iov_len: len,
    };
    let remote = libc::iovec {
        iov_base: args[1] as *mut libc::c_void,
        iov_len: len,
    };
    // SAFETY: the local buffer has room for len bytes.
    let read = unsafe { libc::process_vm_readv(pid, &local, 1, &remote, 1, 0) };
    if read != len as isize || !still_valid(listener, request.id) {
        return Answer::Refuse(libc::EFAULT);
    }
    let Some((asked, port)) = asks(&buf[..len]) else {
        return Answer::Refuse(libc::EINVAL);
    };
    // A UDP socket on port 0 is a client's: it goes on as asked.
    if kind == libc::SOCK_DGRAM && port == 0 {
        return bind_as_asked(socket, &buf[..len]);
    }
    match asked {
        Asks::Every => bind_loopback(socket, family, port),
        Asks::Loopback => bind_as_asked(socket, &buf[..len]),
        Asks::Other => Answer::Refuse(libc::EACCES),
    }
}

/// Binds Kule's copy of the socket to the address the process asked for,
/// as Kule read it.
fn bind_as_asked(socket: RawFd, address: &[u8]) -> Answer {
    // SAFETY: address holds the bytes read, of their own length.
    let r = unsafe {
        libc::bind(
            socket,
            address.as_ptr().cast(),
            address.len() as libc::socklen_t,
        )
    };
    if r == 0 {
        Answer::Done
    } else {
        Answer::Refuse(
            io::Error::last_os_error()
                .raw_os_error()
                .unwrap_or(libc::EACCES),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v4_address(ip: [u8; 4], port: u16) -> Vec<u8> {
        let mut a = (libc::AF_INET as u16).to_ne_bytes().to_vec();
        a.extend(port.to_be_bytes());
        a.extend(ip);
        a.extend([0; 8]);
        a
    }

    fn v6_address(ip: std::net::Ipv6Addr, port: u16) -> Vec<u8> {
        let mut a = (libc::AF_INET6 as u16).to_ne_bytes().to_vec();
        a.extend(port.to_be_bytes());
        a.extend([0; 4]);
        a.extend(ip.octets());
        a.extend([0; 4]);
        a
    }

    #[test]
    fn addresses_are_read_for_what_they_ask() {
        assert_eq!(
            asks(&v4_address([0, 0, 0, 0], 3000)),
            Some((Asks::Every, 3000))
        );
        assert_eq!(
            asks(&v4_address([127, 0, 0, 1], 80)),
            Some((Asks::Loopback, 80))
        );
        assert_eq!(
            asks(&v4_address([127, 3, 2, 1], 80)),
            Some((Asks::Loopback, 80))
        );
        assert_eq!(
            asks(&v4_address([192, 168, 1, 5], 80)),
            Some((Asks::Other, 80))
        );
        assert_eq!(
            asks(&v6_address(std::net::Ipv6Addr::UNSPECIFIED, 1)),
            Some((Asks::Every, 1))
        );
        assert_eq!(
            asks(&v6_address(std::net::Ipv6Addr::LOCALHOST, 1)),
            Some((Asks::Loopback, 1))
        );
        let mapped_any = std::net::Ipv4Addr::UNSPECIFIED.to_ipv6_mapped();
        assert_eq!(asks(&v6_address(mapped_any, 1)), Some((Asks::Every, 1)));
        let lan = "fe80::1".parse().unwrap();
        assert_eq!(asks(&v6_address(lan, 1)), Some((Asks::Other, 1)));
        assert_eq!(asks(&[1, 0]), None);
    }
}
