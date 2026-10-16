//! Linux: a network namespace of the command's own.
//!
//! Landlock matches TCP ports, not addresses, and has no rules for UDP: on
//! its own it lets a command reach any host on the proxy's port number, and
//! send datagrams anywhere. In a network namespace of its own the command
//! has nothing but a loopback interface, so nothing leaves the machine but
//! what goes through the proxy. The proxy itself stays in Kule's namespace:
//! before exec the command listens on `127.0.0.1:<port>` in its namespace
//! and hands the listening socket to Kule, which relays each connection to
//! the proxy on the same port in its own.
//!
//! The namespace needs an unprivileged user namespace, which some systems
//! refuse (Ubuntu 24.04 by default, through AppArmor). There the command
//! runs without one, under Landlock's port rules alone.

use std::io;
use std::net::{Ipv4Addr, Shutdown, TcpListener, TcpStream};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::sync::OnceLock;

/// Whether this system lets an unprivileged process make the namespaces;
/// asked once, by trying.
pub(crate) fn available() -> bool {
    static AVAILABLE: OnceLock<bool> = OnceLock::new();
    *AVAILABLE.get_or_init(|| {
        use std::os::unix::process::CommandExt;
        let mut probe = std::process::Command::new("/bin/sh");
        probe
            .args(["-c", "exit 0"])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        // SAFETY: unshare is a plain system call.
        unsafe {
            probe.pre_exec(|| {
                check(libc::unshare(libc::CLONE_NEWUSER | libc::CLONE_NEWNET))?;
                Ok(())
            });
        }
        probe.status().is_ok_and(|s| s.success())
    })
}

/// What the command needs, between fork and exec, to move into its own
/// namespace; made before the fork, since nothing may be allocated after.
pub(crate) struct Isolation {
    uid_map: Vec<u8>,
    gid_map: Vec<u8>,
    /// The proxy's port, to listen on inside; `None`: no network at all.
    port: Option<u16>,
    /// Where the command sends its listening socket.
    to_kule: Option<OwnedFd>,
    /// Held open by the command and everything it starts; when the last
    /// one is gone, the relay stops.
    alive: OwnedFd,
}

impl Isolation {
    /// Prepares the namespace for one command and starts the relay that
    /// waits for its listening socket. When the command is never started,
    /// dropping the isolation stops the relay.
    pub(crate) fn new(port: Option<u16>) -> io::Result<Isolation> {
        // SAFETY: getuid and getgid cannot fail.
        let (uid, gid) = unsafe { (libc::getuid(), libc::getgid()) };
        let (alive_read, alive) = pipe()?;
        let to_kule = match port {
            Some(port) => {
                let (from_command, to_kule) = socket_pair()?;
                std::thread::spawn(move || relay(from_command, alive_read, port));
                Some(to_kule)
            }
            None => None,
        };
        Ok(Isolation {
            uid_map: format!("{uid} {uid} 1\n").into_bytes(),
            gid_map: format!("{gid} {gid} 1\n").into_bytes(),
            port,
            to_kule,
            alive,
        })
    }

    /// Runs in the child, between fork and exec: only system calls.
    pub(crate) fn enter(&self) -> io::Result<()> {
        // SAFETY: plain system calls on buffers made before the fork.
        unsafe {
            check(libc::unshare(libc::CLONE_NEWUSER | libc::CLONE_NEWNET))?;
            // Our own ids, mapped to themselves: files keep their owner, and
            // tools that compare it with their own id (git) see no change.
            write_file(c"/proc/self/setgroups", b"deny")?;
            write_file(c"/proc/self/uid_map", &self.uid_map)?;
            write_file(c"/proc/self/gid_map", &self.gid_map)?;
            loopback_up()?;
            if let (Some(port), Some(to_kule)) = (self.port, &self.to_kule) {
                let listener = listen(port)?;
                let sent = send_fd(to_kule.as_raw_fd(), listener);
                libc::close(listener);
                sent?;
            }
            // Inherited by what the command runs: the relay lives as long.
            let fd = self.alive.as_raw_fd();
            let flags = check(libc::fcntl(fd, libc::F_GETFD))?;
            check(libc::fcntl(fd, libc::F_SETFD, flags & !libc::FD_CLOEXEC))?;
        }
        Ok(())
    }
}

fn check(result: libc::c_int) -> io::Result<libc::c_int> {
    if result == -1 {
        Err(io::Error::last_os_error())
    } else {
        Ok(result)
    }
}

fn pipe() -> io::Result<(OwnedFd, OwnedFd)> {
    let mut fds = [0; 2];
    // SAFETY: fds has room for the two descriptors.
    check(unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) })?;
    // SAFETY: both were just opened and are owned here only.
    Ok(unsafe { (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])) })
}

fn socket_pair() -> io::Result<(OwnedFd, OwnedFd)> {
    let mut fds = [0; 2];
    // SAFETY: fds has room for the two descriptors.
    check(unsafe {
        libc::socketpair(
            libc::AF_UNIX,
            libc::SOCK_STREAM | libc::SOCK_CLOEXEC,
            0,
            fds.as_mut_ptr(),
        )
    })?;
    // SAFETY: both were just opened and are owned here only.
    Ok(unsafe { (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])) })
}

unsafe fn write_file(path: &std::ffi::CStr, data: &[u8]) -> io::Result<()> {
    // SAFETY: path is NUL-terminated; data is a live buffer.
    unsafe {
        let fd = check(libc::open(path.as_ptr(), libc::O_WRONLY | libc::O_CLOEXEC))?;
        let written = libc::write(fd, data.as_ptr().cast(), data.len());
        libc::close(fd);
        if written != data.len() as isize {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}

unsafe fn loopback_up() -> io::Result<()> {
    // SAFETY: ifreq is plain data; the ioctls read and write only it.
    unsafe {
        let fd = check(libc::socket(
            libc::AF_INET,
            libc::SOCK_DGRAM | libc::SOCK_CLOEXEC,
            0,
        ))?;
        let mut request: libc::ifreq = std::mem::zeroed();
        request.ifr_name[0] = b'l' as libc::c_char;
        request.ifr_name[1] = b'o' as libc::c_char;
        let result = (|| {
            check(libc::ioctl(fd, libc::SIOCGIFFLAGS, &mut request))?;
            request.ifr_ifru.ifru_flags |= libc::IFF_UP as libc::c_short;
            check(libc::ioctl(fd, libc::SIOCSIFFLAGS, &request))
        })();
        libc::close(fd);
        result.map(|_| ())
    }
}

unsafe fn listen(port: u16) -> io::Result<RawFd> {
    // SAFETY: the address is plain data, passed with its size.
    unsafe {
        let fd = check(libc::socket(
            libc::AF_INET,
            libc::SOCK_STREAM | libc::SOCK_CLOEXEC,
            0,
        ))?;
        let address = libc::sockaddr_in {
            sin_family: libc::AF_INET as libc::sa_family_t,
            sin_port: port.to_be(),
            sin_addr: libc::in_addr {
                s_addr: u32::from(Ipv4Addr::LOCALHOST).to_be(),
            },
            sin_zero: [0; 8],
        };
        let bound = libc::bind(
            fd,
            (&raw const address).cast(),
            std::mem::size_of::<libc::sockaddr_in>() as libc::socklen_t,
        );
        if bound == -1 || libc::listen(fd, 128) == -1 {
            let e = io::Error::last_os_error();
            libc::close(fd);
            return Err(e);
        }
        Ok(fd)
    }
}

/// Room for one descriptor's control message, aligned for its header.
#[repr(C, align(8))]
struct Control([u8; 64]);

unsafe fn send_fd(socket: RawFd, fd: RawFd) -> io::Result<()> {
    // SAFETY: the message points at live buffers sized for one descriptor.
    unsafe {
        let mut byte = [0u8; 1];
        let mut iov = libc::iovec {
            iov_base: byte.as_mut_ptr().cast(),
            iov_len: 1,
        };
        let mut control = Control([0; 64]);
        let mut message: libc::msghdr = std::mem::zeroed();
        message.msg_iov = &mut iov;
        message.msg_iovlen = 1;
        message.msg_control = control.0.as_mut_ptr().cast();
        message.msg_controllen = libc::CMSG_SPACE(std::mem::size_of::<RawFd>() as u32) as _;
        let header = libc::CMSG_FIRSTHDR(&message);
        (*header).cmsg_level = libc::SOL_SOCKET;
        (*header).cmsg_type = libc::SCM_RIGHTS;
        (*header).cmsg_len = libc::CMSG_LEN(std::mem::size_of::<RawFd>() as u32) as _;
        std::ptr::write_unaligned(libc::CMSG_DATA(header).cast::<RawFd>(), fd);
        if libc::sendmsg(socket, &message, 0) != 1 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}

/// The descriptor sent over `socket`; `None` when the other end closed
/// without sending one.
fn receive_fd(socket: &OwnedFd) -> Option<OwnedFd> {
    // SAFETY: the message points at live buffers sized for one descriptor;
    // a descriptor received is owned here only.
    unsafe {
        let mut byte = [0u8; 1];
        let mut iov = libc::iovec {
            iov_base: byte.as_mut_ptr().cast(),
            iov_len: 1,
        };
        let mut control = Control([0; 64]);
        let mut message: libc::msghdr = std::mem::zeroed();
        message.msg_iov = &mut iov;
        message.msg_iovlen = 1;
        message.msg_control = control.0.as_mut_ptr().cast();
        message.msg_controllen = control.0.len() as _;
        if libc::recvmsg(socket.as_raw_fd(), &mut message, libc::MSG_CMSG_CLOEXEC) != 1 {
            return None;
        }
        let header = libc::CMSG_FIRSTHDR(&message);
        if header.is_null()
            || (*header).cmsg_level != libc::SOL_SOCKET
            || (*header).cmsg_type != libc::SCM_RIGHTS
        {
            return None;
        }
        let fd = std::ptr::read_unaligned(libc::CMSG_DATA(header).cast::<RawFd>());
        Some(OwnedFd::from_raw_fd(fd))
    }
}

/// Waits for the command's listening socket, then relays each connection
/// to the proxy until the command and all it started are gone.
fn relay(from_command: OwnedFd, alive: OwnedFd, port: u16) {
    let Some(listener) = receive_fd(&from_command) else {
        return;
    };
    drop(from_command);
    let listener = TcpListener::from(listener);
    loop {
        let mut fds = [
            libc::pollfd {
                fd: listener.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            },
            libc::pollfd {
                fd: alive.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            },
        ];
        // SAFETY: fds holds two live pollfd entries.
        if unsafe { libc::poll(fds.as_mut_ptr(), 2, -1) } == -1 {
            if io::Error::last_os_error().kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return;
        }
        if fds[1].revents != 0 {
            // Every holder of the write end is gone.
            return;
        }
        if fds[0].revents != 0
            && let Ok((inside, _)) = listener.accept()
        {
            std::thread::spawn(move || pass(inside, port));
        }
    }
}

/// Copies bytes both ways between a connection from inside and the proxy.
fn pass(inside: TcpStream, port: u16) {
    let Ok(proxy) = TcpStream::connect((Ipv4Addr::LOCALHOST, port)) else {
        return;
    };
    let (Ok(mut inside_read), Ok(mut proxy_write)) = (inside.try_clone(), proxy.try_clone()) else {
        return;
    };
    let up = std::thread::spawn(move || {
        let _ = io::copy(&mut inside_read, &mut proxy_write);
        let _ = proxy_write.shutdown(Shutdown::Write);
    });
    let (mut proxy_read, mut inside_write) = (proxy, inside);
    let _ = io::copy(&mut proxy_read, &mut inside_write);
    let _ = inside_write.shutdown(Shutdown::Write);
    let _ = up.join();
}
