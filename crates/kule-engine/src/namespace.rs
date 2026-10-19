//! Linux: namespaces of the command's own, for its network and its mounts.
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
//! Landlock cannot keep one file read-only inside a directory it lets the
//! command write to, either. In a mount namespace of its own, such a file
//! (an agent's settings, inside its writable state) is mounted read-only
//! over itself, for the command alone: Kule and the person keep writing to
//! it as before. Landlock then forbids the command any change to mounts.
//!
//! Its processes are its own, too: in a PID namespace the command can
//! neither see nor signal any other process, and when it ends, the kernel
//! ends everything it left running, however it hid. The process Kule starts
//! stays outside and waits; inside, a small first process (the namespace's
//! init) passes signals on to the command and reaps what is orphaned. Each
//! exits as the command did.
//!
//! The namespaces need an unprivileged user namespace, which some systems
//! refuse (Ubuntu 24.04 by default, through AppArmor). There the command
//! runs without them, under Landlock's port rules alone, and protected
//! files are put back after the run instead (see `protect`).

use std::ffi::{CStr, CString};
use std::io;
use std::net::{Ipv4Addr, Shutdown, TcpListener, TcpStream};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::os::unix::ffi::OsStrExt;
use std::path::PathBuf;
use std::sync::OnceLock;

/// `ST_RELATIME`, which glibc names but not every libc.
const ST_RELATIME: libc::c_ulong = 0x1000;

/// Whether this system lets an unprivileged process have a network of its
/// own; asked once, by trying.
pub(crate) fn available() -> bool {
    static AVAILABLE: OnceLock<bool> = OnceLock::new();
    *AVAILABLE.get_or_init(|| probe(Net::Own(None), &[]))
}

/// Whether this system lets an unprivileged process mount a path read-only
/// for itself; asked once, by trying on the temporary directory.
pub(crate) fn mounts_available() -> bool {
    static AVAILABLE: OnceLock<bool> = OnceLock::new();
    *AVAILABLE.get_or_init(|| {
        std::fs::canonicalize(std::env::temp_dir())
            .is_ok_and(|tmp| probe(Net::Shared, std::slice::from_ref(&tmp)))
    })
}

/// Whether this system lets an unprivileged process have processes of its
/// own, with a `/proc` that shows only them; asked once, by trying.
pub(crate) fn pids_available() -> bool {
    static AVAILABLE: OnceLock<bool> = OnceLock::new();
    *AVAILABLE.get_or_init(|| {
        Isolation::new(Net::Shared, &[]).is_ok_and(|isolation| {
            // The command is the second process there, and /proc says so.
            run_probe(
                isolation.own_pids(),
                "test $$ = 2 && read p rest < /proc/self/stat && test \"$p\" = 2",
            )
        })
    })
}

/// Whether this system lets an unprivileged process have a devpts instance
/// of its own; asked once, by trying.
pub(crate) fn ptys_available() -> bool {
    static AVAILABLE: OnceLock<bool> = OnceLock::new();
    *AVAILABLE.get_or_init(|| {
        Isolation::new(Net::Shared, &[]).is_ok_and(|isolation| {
            // A new terminal there is the instance's first, and its only.
            run_probe(
                isolation.own_ptys(),
                "exec 3<>/dev/ptmx && set -- /dev/pts/* && test \"$*\" = '/dev/pts/0 /dev/pts/ptmx'",
            )
        })
    })
}

/// Runs `true` the way a command would run with this isolation.
fn probe(net: Net, read_only: &[PathBuf]) -> bool {
    Isolation::new(net, read_only).is_ok_and(|isolation| run_probe(isolation, "exit 0"))
}

fn run_probe(isolation: Isolation, script: &str) -> bool {
    use std::os::unix::process::CommandExt;
    let mut probe = std::process::Command::new("/bin/sh");
    probe
        .args(["-c", script])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    // SAFETY: enter only makes system calls on what was prepared before.
    unsafe {
        probe.pre_exec(move || isolation.enter());
    }
    probe.status().is_ok_and(|s| s.success())
}

/// The network a command gets.
pub(crate) enum Net {
    /// Kule's own, as it is.
    Shared,
    /// Nothing but loopback; with a port, the proxy's, relayed in on it.
    Own(Option<u16>),
}

/// A path to mount read-only over itself, and the flags its mount has and
/// must keep: an unprivileged namespace may add restrictions, never drop
/// them.
struct ReadOnly {
    path: CString,
    flags: libc::c_ulong,
}

/// What the command needs, between fork and exec, to move into its own
/// namespaces; made before the fork, since nothing may be allocated after.
pub(crate) struct Isolation {
    /// The namespaces to make: `CLONE_NEWUSER` and those it allows.
    namespaces: libc::c_int,
    uid_map: Vec<u8>,
    gid_map: Vec<u8>,
    /// The proxy's port, to listen on inside.
    port: Option<u16>,
    /// Where the command sends its listening socket.
    to_kule: Option<OwnedFd>,
    /// Held open by the command and everything it starts; when the last
    /// one is gone, the relay stops.
    alive: OwnedFd,
    read_only: Vec<ReadOnly>,
    /// Unix sockets to cover with an empty file for the command.
    masked: Vec<CString>,
    /// Directories to cover with an empty one of the command's own, so no
    /// socket made there, before or after it started, is in its reach.
    covered: Vec<CString>,
    /// The task's own directory, mounted over `/tmp` for the command.
    tmp: Option<CString>,
    /// Processes of its own (see the module's notes).
    pids: bool,
    /// Terminals of its own: a devpts instance over `/dev/pts`.
    ptys: bool,
}

impl Isolation {
    /// Prepares the namespaces for one command: its network, and the paths
    /// in `read_only` (which must exist and not be links) mounted read-only.
    /// With a proxy port, starts the relay that waits for the command's
    /// listening socket. When the command is never started, dropping the
    /// isolation stops the relay.
    pub(crate) fn new(net: Net, read_only: &[PathBuf]) -> io::Result<Isolation> {
        // SAFETY: getuid and getgid cannot fail.
        let (uid, gid) = unsafe { (libc::getuid(), libc::getgid()) };
        let mut namespaces = libc::CLONE_NEWUSER;
        let port = match net {
            Net::Shared => None,
            Net::Own(port) => {
                namespaces |= libc::CLONE_NEWNET;
                port
            }
        };
        if !read_only.is_empty() {
            namespaces |= libc::CLONE_NEWNS;
        }
        let read_only = read_only
            .iter()
            .map(|path| {
                let path = CString::new(path.as_os_str().as_bytes())?;
                let flags = mount_flags(&path)?;
                Ok(ReadOnly { path, flags })
            })
            .collect::<io::Result<_>>()?;
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
            namespaces,
            uid_map: format!("{uid} {uid} 1\n").into_bytes(),
            gid_map: format!("{gid} {gid} 1\n").into_bytes(),
            port,
            to_kule,
            alive,
            read_only,
            masked: Vec::new(),
            covered: Vec::new(),
            tmp: None,
            pids: false,
            ptys: false,
        })
    }

    /// Gives the command terminals of its own: a devpts instance of its
    /// own over `/dev/pts`, whose `ptmx` stands at `/dev/ptmx` too. It can
    /// make a terminal and open its other end, and none of the person's is
    /// there to open.
    pub(crate) fn own_ptys(mut self) -> Isolation {
        self.namespaces |= libc::CLONE_NEWNS;
        self.ptys = true;
        self
    }

    /// Gives the command a PID namespace of its own, and a `/proc` that
    /// shows only it: what it leaves running ends with it.
    pub(crate) fn own_pids(mut self) -> Isolation {
        self.namespaces |= libc::CLONE_NEWPID | libc::CLONE_NEWNS;
        self.pids = true;
        self
    }

    /// Gives the command `dir` as its `/tmp`, in a mount namespace of its
    /// own: what tools write there by hard-coded path stays the task's.
    pub(crate) fn own_tmp(mut self, dir: &std::path::Path) -> io::Result<Isolation> {
        self.namespaces |= libc::CLONE_NEWNS;
        self.tmp = Some(CString::new(dir.as_os_str().as_bytes())?);
        Ok(self)
    }

    /// Covers each of `sockets` with an empty file in the command's mount
    /// namespace, so connecting to it reaches nothing; for kernels whose
    /// Landlock has no rule for Unix sockets.
    pub(crate) fn mask_sockets(mut self, sockets: &[PathBuf]) -> io::Result<Isolation> {
        if sockets.is_empty() {
            return Ok(self);
        }
        self.namespaces |= libc::CLONE_NEWNS;
        self.masked = sockets
            .iter()
            .map(|p| CString::new(p.as_os_str().as_bytes()))
            .collect::<Result<_, _>>()?;
        Ok(self)
    }

    /// Covers each of `dirs` with an empty directory in the command's mount
    /// namespace (a small tmpfs of its own): for where the person's daemons
    /// make their sockets, on kernels whose Landlock has no rule for them,
    /// since a socket made after the command started is in no list.
    pub(crate) fn cover(mut self, dirs: &[PathBuf]) -> io::Result<Isolation> {
        if dirs.is_empty() {
            return Ok(self);
        }
        self.namespaces |= libc::CLONE_NEWNS;
        self.covered = dirs
            .iter()
            .map(|p| CString::new(p.as_os_str().as_bytes()))
            .collect::<Result<_, _>>()?;
        Ok(self)
    }

    /// Runs in the child, between fork and exec: only system calls.
    pub(crate) fn enter(&self) -> io::Result<()> {
        // SAFETY: plain system calls on buffers made before the fork.
        unsafe {
            check(libc::unshare(self.namespaces))?;
            // Our own ids, mapped to themselves: files keep their owner, and
            // tools that compare it with their own id (git) see no change.
            write_file(c"/proc/self/setgroups", b"deny")?;
            write_file(c"/proc/self/uid_map", &self.uid_map)?;
            write_file(c"/proc/self/gid_map", &self.gid_map)?;
            if self.pids {
                // Returns only in the process that goes on to the command.
                split()?;
            }
            if self.namespaces & libc::CLONE_NEWNET != 0 {
                loopback_up()?;
            }
            if let (Some(port), Some(to_kule)) = (self.port, &self.to_kule) {
                let listener = listen(port)?;
                let sent = send_fd(to_kule.as_raw_fd(), listener);
                libc::close(listener);
                sent?;
            }
            if self.namespaces & libc::CLONE_NEWNS != 0 {
                // Nothing mounted here reaches Kule's namespace, or any other.
                check(libc::mount(
                    std::ptr::null(),
                    c"/".as_ptr(),
                    std::ptr::null(),
                    libc::MS_REC | libc::MS_PRIVATE,
                    std::ptr::null(),
                ))?;
            }
            if self.pids {
                // Only its own processes to see under /proc.
                check(libc::mount(
                    c"proc".as_ptr(),
                    c"/proc".as_ptr(),
                    c"proc".as_ptr(),
                    libc::MS_NOSUID | libc::MS_NODEV | libc::MS_NOEXEC,
                    std::ptr::null(),
                ))?;
            }
            if self.ptys {
                check(libc::mount(
                    c"devpts".as_ptr(),
                    c"/dev/pts".as_ptr(),
                    c"devpts".as_ptr(),
                    libc::MS_NOSUID | libc::MS_NOEXEC,
                    c"newinstance,ptmxmode=0666,mode=0620".as_ptr().cast(),
                ))?;
                check(libc::mount(
                    c"/dev/pts/ptmx".as_ptr(),
                    c"/dev/ptmx".as_ptr(),
                    std::ptr::null(),
                    libc::MS_BIND,
                    std::ptr::null(),
                ))?;
            }
            for dir in &self.covered {
                check(libc::mount(
                    c"tmpfs".as_ptr(),
                    dir.as_ptr(),
                    c"tmpfs".as_ptr(),
                    libc::MS_NOSUID | libc::MS_NODEV | libc::MS_NOEXEC,
                    c"mode=0700,size=1m".as_ptr().cast(),
                ))?;
            }
            if let Some(dir) = &self.tmp {
                check(libc::mount(
                    dir.as_ptr(),
                    c"/tmp".as_ptr(),
                    std::ptr::null(),
                    libc::MS_BIND | libc::MS_REC,
                    std::ptr::null(),
                ))?;
            }
            for socket in &self.masked {
                let covered = libc::mount(
                    c"/dev/null".as_ptr(),
                    socket.as_ptr(),
                    std::ptr::null(),
                    libc::MS_BIND,
                    std::ptr::null(),
                );
                // A socket gone since it was listed needs no cover; any
                // other failure leaves it reachable, so the command fails.
                if covered == -1 {
                    let e = io::Error::last_os_error();
                    if e.raw_os_error() != Some(libc::ENOENT) {
                        return Err(e);
                    }
                }
            }
            for read_only in &self.read_only {
                let path = read_only.path.as_ptr();
                check(libc::mount(
                    path,
                    path,
                    std::ptr::null(),
                    libc::MS_BIND | libc::MS_REC,
                    std::ptr::null(),
                ))?;
                check(libc::mount(
                    std::ptr::null(),
                    path,
                    std::ptr::null(),
                    libc::MS_REMOUNT | libc::MS_BIND | libc::MS_RDONLY | read_only.flags,
                    std::ptr::null(),
                ))?;
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

/// Starts the command's PID namespace, after `unshare` asked for one: this
/// process (the one Kule started and waits on) forks the namespace's first
/// process and stays outside as its keeper; that one forks the command's.
/// Returns only in the command's process, with the signal mask it came
/// with. Keeper and init take every signal through `sigwaitinfo`, so none
/// ends them by surprise, and pass it on; a SIGKILL to the keeper (or its
/// process group, which all three share) ends the init with it, and the
/// kernel ends the rest.
///
/// # Safety
/// As [`write_file`]: between fork and exec, nothing allocated.
unsafe fn split() -> io::Result<()> {
    // SAFETY: plain system calls on stack buffers.
    unsafe {
        let mut all: libc::sigset_t = std::mem::zeroed();
        let mut before: libc::sigset_t = std::mem::zeroed();
        libc::sigfillset(&mut all);
        check(libc::sigprocmask(libc::SIG_BLOCK, &all, &mut before))?;
        // The init learns of the keeper's end by this pipe closing, and
        // tells it how the command ended over the other.
        let (keeper_end, keeper_alive) = raw_pipe()?;
        let (told, tell) = raw_pipe()?;
        let init = check(libc::fork())?;
        if init > 0 {
            libc::close(keeper_end);
            libc::close(tell);
            keep(init, &all, keeper_alive, told);
        }
        libc::close(keeper_alive);
        libc::close(told);
        // Ends with the keeper; a keeper already gone ends it here.
        check(libc::prctl(
            libc::PR_SET_PDEATHSIG,
            libc::SIGKILL as libc::c_ulong,
        ))?;
        let mut gone = libc::pollfd {
            fd: keeper_end,
            events: libc::POLLIN,
            revents: 0,
        };
        if libc::poll(&mut gone, 1, 0) != 0 {
            libc::_exit(1);
        }
        libc::close(keeper_end);
        let command = check(libc::fork())?;
        if command > 0 {
            init_loop(command, &all, tell);
        }
        libc::close(tell);
        check(libc::sigprocmask(
            libc::SIG_SETMASK,
            &before,
            std::ptr::null_mut(),
        ))?;
    }
    Ok(())
}

/// The keeper: waits for the init, passes signals on to it, and ends as
/// the command ended, by the same signal or with the same code.
///
/// # Safety
/// As [`split`]; `all` is the blocked set.
unsafe fn keep(init: libc::pid_t, all: &libc::sigset_t, alive: RawFd, told: RawFd) -> ! {
    // SAFETY: plain system calls on stack buffers.
    unsafe {
        close_except(&[alive, told]);
        loop {
            let signal = libc::sigwaitinfo(all, std::ptr::null_mut());
            if signal == libc::SIGCHLD {
                let mut status = 0;
                if libc::waitpid(init, &mut status, libc::WNOHANG) == init {
                    // The command's own end, unless the init was ended first.
                    let mut command = [0u8; 4];
                    if libc::read(told, command.as_mut_ptr().cast(), 4) == 4 {
                        status = libc::c_int::from_ne_bytes(command);
                    }
                    end_as(status, all);
                }
            } else if signal > 0 {
                libc::kill(init, signal);
            }
        }
    }
}

/// The namespace's init: passes signals on to the command, reaps every
/// process orphaned to it, and once the command is gone, tells the keeper
/// how it ended and exits, which ends whatever it left running.
///
/// # Safety
/// As [`split`]; `all` is the blocked set.
unsafe fn init_loop(command: libc::pid_t, all: &libc::sigset_t, tell: RawFd) -> ! {
    // SAFETY: plain system calls on stack buffers.
    unsafe {
        close_except(&[tell]);
        loop {
            let signal = libc::sigwaitinfo(all, std::ptr::null_mut());
            if signal == libc::SIGCHLD {
                loop {
                    let mut status = 0;
                    let reaped = libc::waitpid(-1, &mut status, libc::WNOHANG);
                    if reaped <= 0 {
                        break;
                    }
                    if reaped == command {
                        let bytes = status.to_ne_bytes();
                        libc::write(tell, bytes.as_ptr().cast(), 4);
                        libc::_exit(0);
                    }
                }
            } else if signal > 0 {
                libc::kill(command, signal);
            }
        }
    }
}

/// Exits the way `status` says a process ended: by its signal, without a
/// core file, or with its code.
///
/// # Safety
/// As [`split`]; `all` is the blocked set.
unsafe fn end_as(status: libc::c_int, all: &libc::sigset_t) -> ! {
    // SAFETY: plain system calls on stack buffers.
    unsafe {
        if libc::WIFSIGNALED(status) {
            let signal = libc::WTERMSIG(status);
            let none = libc::rlimit {
                rlim_cur: 0,
                rlim_max: 0,
            };
            libc::setrlimit(libc::RLIMIT_CORE, &none);
            libc::signal(signal, libc::SIG_DFL);
            libc::sigprocmask(libc::SIG_UNBLOCK, all, std::ptr::null_mut());
            libc::kill(libc::getpid(), signal);
            libc::_exit(128 + signal);
        }
        libc::_exit(if libc::WIFEXITED(status) {
            libc::WEXITSTATUS(status)
        } else {
            1
        });
    }
}

/// Closes every descriptor but `keep`: keeper and init hold nothing of the
/// command's, Kule's pipe that waits for its exec included.
///
/// # Safety
/// As [`split`].
unsafe fn close_except(keep: &[RawFd]) {
    let mut keep_sorted = [0; 2];
    let keep_sorted = &mut keep_sorted[..keep.len()];
    keep_sorted.copy_from_slice(keep);
    keep_sorted.sort_unstable();
    let mut first: libc::c_uint = 0;
    // SAFETY: as the caller's.
    unsafe {
        for &fd in keep_sorted.iter() {
            let fd = fd as libc::c_uint;
            if fd > first {
                close_range(first, fd - 1);
            }
            first = fd + 1;
        }
        close_range(first, libc::c_uint::MAX);
    }
}

/// # Safety
/// As [`split`].
unsafe fn close_range(first: libc::c_uint, last: libc::c_uint) {
    // SAFETY: closes descriptors only; Landlock's ABI 3, which every
    // sandbox needs, came after close_range (Linux 5.9).
    unsafe {
        libc::syscall(libc::SYS_close_range, first, last, 0);
    }
}

/// A pipe for between fork and exec: (read end, write end), closed on exec.
fn raw_pipe() -> io::Result<(RawFd, RawFd)> {
    let mut fds = [0; 2];
    // SAFETY: fds has room for the two descriptors.
    check(unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) })?;
    Ok((fds[0], fds[1]))
}

/// The flags of the mount `path` is on that a remount has to repeat.
fn mount_flags(path: &CStr) -> io::Result<libc::c_ulong> {
    // SAFETY: statvfs fills the struct it is given and reads only the path.
    let stat = unsafe {
        let mut stat: libc::statvfs = std::mem::zeroed();
        check(libc::statvfs(path.as_ptr(), &mut stat))?;
        stat
    };
    let kept = [
        (libc::ST_NOSUID, libc::MS_NOSUID),
        (libc::ST_NODEV, libc::MS_NODEV),
        (libc::ST_NOEXEC, libc::MS_NOEXEC),
        (libc::ST_NOATIME, libc::MS_NOATIME),
        (libc::ST_NODIRATIME, libc::MS_NODIRATIME),
        (ST_RELATIME, libc::MS_RELATIME),
    ];
    Ok(kept
        .into_iter()
        .filter(|(on, _)| stat.f_flag & on != 0)
        .fold(0, |flags, (_, flag)| flags | flag))
}

fn pipe() -> io::Result<(OwnedFd, OwnedFd)> {
    let mut fds = [0; 2];
    // SAFETY: fds has room for the two descriptors.
    check(unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) })?;
    // SAFETY: both were just opened and are owned here only.
    Ok(unsafe { (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])) })
}

pub(crate) fn socket_pair() -> io::Result<(OwnedFd, OwnedFd)> {
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

/// # Safety
/// Only system calls, for between fork and exec: the caller must be the
/// child of a fork and allocate nothing around it.
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

/// # Safety
/// As [`write_file`]: between fork and exec, in a network namespace of the
/// process's own.
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

/// # Safety
/// As [`write_file`]: between fork and exec. The caller owns the descriptor
/// returned and closes it.
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

/// # Safety
/// `socket` and `fd` must be open descriptors for the call's length; it
/// only makes system calls, so it may run between fork and exec.
pub(crate) unsafe fn send_fd(socket: RawFd, fd: RawFd) -> io::Result<()> {
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
pub(crate) fn receive_fd(socket: &OwnedFd) -> Option<OwnedFd> {
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A covered directory is empty for the command: what was there, and a
    /// socket the person's daemon makes there after the command started.
    #[test]
    fn a_covered_directory_hides_sockets_made_after_the_start() {
        use std::os::unix::net::UnixListener;
        use std::os::unix::process::CommandExt;
        if !mounts_available() {
            eprintln!("skipped: no mount namespace here");
            return;
        }
        let (covered, elsewhere) = (
            tempfile::TempDir::new().unwrap(),
            tempfile::TempDir::new().unwrap(),
        );
        let dir = std::fs::canonicalize(covered.path()).unwrap();
        let started = elsewhere.path().join("started");
        let _early = UnixListener::bind(dir.join("early.sock")).unwrap();
        let isolation = Isolation::new(Net::Shared, &[])
            .and_then(|i| i.cover(std::slice::from_ref(&dir)))
            .unwrap();
        // It looks once the late socket is there.
        let script = format!(
            "while [ ! -e '{}' ]; do sleep 0.01; done; ls -A '{}'",
            started.display(),
            dir.display()
        );
        let mut command = std::process::Command::new("/bin/sh");
        command
            .args(["-c", &script])
            .stdout(std::process::Stdio::piped());
        // SAFETY: enter only makes system calls on what was prepared before.
        unsafe {
            command.pre_exec(move || isolation.enter());
        }
        let child = command.spawn().unwrap();
        // The daemon's socket comes after the command started.
        let _late = UnixListener::bind(dir.join("late.sock")).unwrap();
        std::fs::write(&started, "").unwrap();
        let out = child.wait_with_output().unwrap();
        assert!(out.status.success(), "{out:?}");
        assert_eq!(
            String::from_utf8_lossy(&out.stdout),
            "",
            "seen in the cover"
        );
        // Outside the command, both are there as they were.
        assert!(dir.join("early.sock").exists() && dir.join("late.sock").exists());
    }
}
