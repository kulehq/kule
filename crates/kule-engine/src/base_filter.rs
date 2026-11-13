//! Linux: what every command is refused, whatever namespaces it has.
//!
//! A user namespace keeps the person's session keyring, and the kernel
//! checks keys by uid: so a command joins an anonymous session keyring of
//! its own before it starts, and a seccomp filter refuses the key calls
//! (`keyctl`, `add_key`, `request_key`, whose `dns_resolver` keys run a
//! helper outside the task). Sockets are made only in the families a
//! command needs (Unix, IPv4, IPv6, netlink): no network namespace holds a
//! virtual machine's `vsock`, and raw packets are not for a task. `io_uring`,
//! which makes sockets without a system call, is refused, and so are system
//! calls of another architecture, which have their own numbers.
//!
//! Before the filter, the command gives up every capability, for good:
//! none in its sets, none it could get back by `exec` (the bounding set
//! emptied, and the securebits that keep root from regaining them locked).
//! Run as root, Kule's commands are root without root's powers.
//!
//! The filter only refuses: it hands nothing to Kule (see `bind_filter` for
//! the one that does, on this computer's network). Installed after
//! Landlock, which sets no_new_privs, as seccomp needs.

use std::io;

#[cfg(target_arch = "x86_64")]
const ARCH: u32 = 0xC000_003E; // AUDIT_ARCH_X86_64
#[cfg(target_arch = "aarch64")]
const ARCH: u32 = 0xC000_00B7; // AUDIT_ARCH_AARCH64
/// x32's system calls are x86_64's with this bit set.
const X32_BIT: u32 = 0x4000_0000;

/// The socket families a command may make a socket in.
pub(crate) const FAMILIES: [i32; 4] = [
    libc::AF_UNIX,
    libc::AF_INET,
    libc::AF_INET6,
    libc::AF_NETLINK,
];

/// The calls refused with `EPERM`: the kernel's key store.
pub(crate) const REFUSED: [libc::c_long; 3] =
    [libc::SYS_keyctl, libc::SYS_add_key, libc::SYS_request_key];

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

/// The program: another architecture's calls and `io_uring` refused as not
/// there, [`REFUSED`] with `EPERM`, a socket outside [`FAMILIES`] with
/// `EAFNOSUPPORT`, everything else allowed.
pub(crate) fn program() -> Vec<libc::sock_filter> {
    const LD_W_ABS: u16 = (libc::BPF_LD | libc::BPF_W | libc::BPF_ABS) as u16;
    const JEQ: u16 = (libc::BPF_JMP | libc::BPF_JEQ | libc::BPF_K) as u16;
    const JGE: u16 = (libc::BPF_JMP | libc::BPF_JGE | libc::BPF_K) as u16;
    const RET: u16 = (libc::BPF_RET | libc::BPF_K) as u16;
    // offsets in struct seccomp_data; the low half of the first argument
    // (little-endian on both architectures)
    const NR: u32 = 0;
    const ARCH_AT: u32 = 4;
    const ARG0: u32 = 16;
    let errno = |e: i32| libc::SECCOMP_RET_ERRNO | (e as u32 & libc::SECCOMP_RET_DATA);
    let mut program = vec![
        statement(LD_W_ABS, ARCH_AT),
        jump(JEQ, ARCH, 1, 0),
        statement(RET, errno(libc::ENOSYS)),
        statement(LD_W_ABS, NR),
        jump(JGE, X32_BIT, 0, 1),
        statement(RET, errno(libc::ENOSYS)),
        jump(JEQ, libc::SYS_io_uring_setup as u32, 0, 1),
        statement(RET, errno(libc::ENOSYS)),
    ];
    for nr in REFUSED {
        program.push(jump(JEQ, nr as u32, 0, 1));
        program.push(statement(RET, errno(libc::EPERM)));
    }
    // socket and socketpair go on to the family check, the rest is allowed.
    program.push(jump(JEQ, libc::SYS_socket as u32, 2, 0));
    program.push(jump(JEQ, libc::SYS_socketpair as u32, 1, 0));
    program.push(statement(RET, libc::SECCOMP_RET_ALLOW));
    program.push(statement(LD_W_ABS, ARG0));
    let n = FAMILIES.len();
    for (i, family) in FAMILIES.iter().enumerate() {
        // To the allow after the refusal, past the checks still to come.
        let to_allow = (n - i) as u8;
        program.push(jump(JEQ, *family as u32, to_allow, 0));
    }
    program.push(statement(RET, errno(libc::EAFNOSUPPORT)));
    program.push(statement(RET, libc::SECCOMP_RET_ALLOW));
    program
}

/// Gives up every capability, for this process and what it runs: the
/// securebits locked that would let root regain them on `exec`, the
/// bounding and ambient sets emptied, the process's own sets cleared. A
/// process that holds none (an ordinary user without namespaces) cannot
/// change its securebits or bounding set and has nothing to give up; one
/// running as root that cannot is refused.
fn drop_capabilities() -> io::Result<()> {
    // SAFETY: plain system calls on values.
    let root = unsafe { libc::geteuid() } == 0;
    let refused = |e: io::Error| -> io::Result<()> {
        if root || e.raw_os_error() != Some(libc::EPERM) {
            Err(e)
        } else {
            Ok(())
        }
    };
    // SECBIT_NOROOT, SECBIT_NO_SETUID_FIXUP, SECBIT_NO_CAP_AMBIENT_RAISE and
    // SECBIT_KEEP_CAPS left off, each with its lock.
    const SECUREBITS: libc::c_ulong = 0b1110_1111;
    // SAFETY: as above.
    if unsafe { libc::prctl(libc::PR_SET_SECUREBITS, SECUREBITS, 0, 0, 0) } == -1 {
        refused(io::Error::last_os_error())?;
    }
    for cap in 0..64 {
        // SAFETY: as above.
        if unsafe { libc::prctl(libc::PR_CAPBSET_DROP, cap, 0, 0, 0) } == -1 {
            let e = io::Error::last_os_error();
            // Past the last capability this kernel knows.
            if e.raw_os_error() == Some(libc::EINVAL) {
                break;
            }
            refused(e)?;
            break;
        }
    }
    // SAFETY: as above; a kernel without ambient capabilities has none.
    if unsafe {
        libc::prctl(
            libc::PR_CAP_AMBIENT,
            libc::PR_CAP_AMBIENT_CLEAR_ALL,
            0,
            0,
            0,
        )
    } == -1
    {
        let e = io::Error::last_os_error();
        if e.raw_os_error() != Some(libc::EINVAL) {
            return Err(e);
        }
    }
    #[repr(C)]
    struct Header {
        version: u32,
        pid: i32,
    }
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct Data {
        effective: u32,
        permitted: u32,
        inheritable: u32,
    }
    const LINUX_CAPABILITY_VERSION_3: u32 = 0x2008_0522;
    let mut header = Header {
        version: LINUX_CAPABILITY_VERSION_3,
        pid: 0,
    };
    let data = [Data {
        effective: 0,
        permitted: 0,
        inheritable: 0,
    }; 2];
    // SAFETY: a version 3 header and its two data words, alive for the call.
    if unsafe { libc::syscall(libc::SYS_capset, &mut header as *mut Header, data.as_ptr()) } == -1 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// Runs in the child, between fork and exec, after no_new_privs: only
/// system calls. Joins a session keyring of the command's own, gives up
/// every capability, then puts `program` in place. A system without
/// keyrings has none to join.
pub(crate) fn enter(program: &[libc::sock_filter]) -> io::Result<()> {
    const KEYCTL_JOIN_SESSION_KEYRING: libc::c_long = 1;
    // SAFETY: joins a new anonymous session keyring; a null name is allowed.
    let joined = unsafe {
        libc::syscall(
            libc::SYS_keyctl,
            KEYCTL_JOIN_SESSION_KEYRING,
            std::ptr::null::<libc::c_char>(),
        )
    };
    if joined < 0 {
        let e = io::Error::last_os_error();
        // No keyrings in this kernel, or already refused around Kule (a
        // container's own filter): nothing of the person's to reach.
        if !matches!(e.raw_os_error(), Some(libc::ENOSYS | libc::EPERM)) {
            return Err(e);
        }
    }
    drop_capabilities()?;
    let prog = libc::sock_fprog {
        len: program.len() as u16,
        filter: program.as_ptr().cast_mut(),
    };
    // SAFETY: prog points at the program, alive for the call.
    let r = unsafe {
        libc::syscall(
            libc::SYS_seccomp,
            libc::SECCOMP_SET_MODE_FILTER,
            0,
            &prog as *const libc::sock_fprog,
        )
    };
    if r < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}
