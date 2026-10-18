//! Servers working in a task's copy: processes whose working directory is
//! inside it and that listen on a TCP port, whoever started them (an agent,
//! or the person in a terminal). The window offers to open them in the
//! browser.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::{IoContext, Result};

/// A process listening on a TCP port.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Listener {
    pub port: u16,
    /// What it listens on: `127.0.0.1`, `::1`, or `*` for every address.
    pub address: String,
    pub pid: u32,
    /// The process's name.
    pub process: String,
    /// Whether this computer reaches it: a server inside a sandbox with a
    /// network of its own (Linux) is not.
    pub reachable: bool,
    /// Listening on every address, but kept off the network by Kule (see
    /// `shield`, macOS).
    #[serde(default)]
    pub shielded: bool,
}

/// The servers whose processes work inside `dir`, one per port, by port.
pub fn listening_in(dir: &Path) -> Result<Vec<Listener>> {
    let dir = std::fs::canonicalize(dir).at(dir)?;
    let mut by_port: BTreeMap<u16, Listener> = BTreeMap::new();
    for listener in listeners()? {
        let inside = cwd_of(listener.pid).is_some_and(|cwd| cwd.starts_with(&dir));
        if inside {
            by_port.entry(listener.port).or_insert(listener);
        }
    }
    Ok(by_port.into_values().collect())
}

/// What `address` is, said the way the window says it.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn address_of(ip: &str) -> String {
    match ip {
        "*" | "0.0.0.0" | "::" | "[::]" => "*".to_owned(),
        other => other
            .trim_start_matches('[')
            .trim_end_matches(']')
            .to_owned(),
    }
}

#[cfg(target_os = "macos")]
fn listeners() -> Result<Vec<Listener>> {
    let mut found = Vec::new();
    for pid in macos::all_pids() {
        let sockets = macos::listening(pid);
        if sockets.is_empty() {
            continue;
        }
        let process = macos::name_of(pid);
        for (ip, port) in sockets {
            found.push(Listener {
                port,
                address: address_of(&ip.to_string()),
                pid: pid as u32,
                process: process.clone(),
                reachable: true,
                shielded: false,
            });
        }
    }
    Ok(found)
}

/// macOS: processes and their listening sockets, read from the kernel
/// (libproc), for the window's port chips and for the shield.
#[cfg(target_os = "macos")]
pub(crate) mod macos {
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

    #[repr(C)]
    struct RawListener {
        family: libc::c_int,
        port: libc::c_ushort,
        addr: [u8; 16],
    }

    unsafe extern "C" {
        fn kule_listeners(pid: libc::c_int, out: *mut RawListener, max: libc::c_int)
        -> libc::c_int;
    }

    /// The person's processes (those this one may read).
    pub(crate) fn all_pids() -> Vec<i32> {
        // SAFETY: a null buffer asks for the count; the second call fills
        // at most the room given.
        unsafe {
            let n = libc::proc_listallpids(std::ptr::null_mut(), 0);
            if n <= 0 {
                return Vec::new();
            }
            let mut pids = vec![0i32; n as usize + 64];
            let bytes = (pids.len() * std::mem::size_of::<i32>()) as libc::c_int;
            let got = libc::proc_listallpids(pids.as_mut_ptr().cast(), bytes);
            pids.truncate(got.max(0) as usize);
            pids.retain(|&p| p > 0);
            pids
        }
    }

    /// The TCP addresses and ports `pid` listens on.
    pub(crate) fn listening(pid: i32) -> Vec<(IpAddr, u16)> {
        const MAX: usize = 256;
        let mut raw: Vec<RawListener> = (0..MAX)
            .map(|_| RawListener {
                family: 0,
                port: 0,
                addr: [0; 16],
            })
            .collect();
        // SAFETY: raw has room for MAX entries.
        let n = unsafe { kule_listeners(pid, raw.as_mut_ptr(), MAX as libc::c_int) };
        raw.truncate(n.max(0) as usize);
        raw.into_iter()
            .map(|l| {
                let ip = if l.family == 6 {
                    IpAddr::V6(Ipv6Addr::from(l.addr))
                } else {
                    IpAddr::V4(Ipv4Addr::new(l.addr[0], l.addr[1], l.addr[2], l.addr[3]))
                };
                (ip, l.port)
            })
            .collect()
    }

    /// The parent of `pid`, if it can be read.
    pub(crate) fn parent_of(pid: i32) -> Option<i32> {
        // SAFETY: an all-zero proc_bsdinfo is valid; proc_pidinfo fills at
        // most its size.
        let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
        let size = std::mem::size_of::<libc::proc_bsdinfo>() as libc::c_int;
        let got = unsafe {
            libc::proc_pidinfo(
                pid,
                libc::PROC_PIDTBSDINFO,
                0,
                (&mut info as *mut libc::proc_bsdinfo).cast(),
                size,
            )
        };
        (got == size).then_some(info.pbi_ppid as i32)
    }

    pub(crate) fn name_of(pid: i32) -> String {
        let mut buf = [0u8; 256];
        // SAFETY: buf has room for the name the call writes.
        let n = unsafe { libc::proc_name(pid, buf.as_mut_ptr().cast(), buf.len() as u32) };
        String::from_utf8_lossy(&buf[..n.max(0) as usize]).into_owned()
    }
}

#[cfg(target_os = "macos")]
fn cwd_of(pid: u32) -> Option<std::path::PathBuf> {
    use std::os::unix::ffi::OsStrExt;
    // SAFETY: an all-zero proc_vnodepathinfo is valid; proc_pidinfo fills
    // at most its size.
    let mut info: libc::proc_vnodepathinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of::<libc::proc_vnodepathinfo>() as i32;
    let got = unsafe {
        libc::proc_pidinfo(
            pid as i32,
            libc::PROC_PIDVNODEPATHINFO,
            0,
            (&mut info as *mut libc::proc_vnodepathinfo).cast(),
            size,
        )
    };
    if got != size {
        return None;
    }
    // SAFETY: the path is NUL-terminated within its buffer.
    let bytes: &[u8] = unsafe {
        std::slice::from_raw_parts(
            info.pvi_cdir.vip_path.as_ptr().cast::<u8>(),
            std::mem::size_of_val(&info.pvi_cdir.vip_path),
        )
    };
    let end = bytes.iter().position(|&b| b == 0)?;
    Some(std::path::PathBuf::from(std::ffi::OsStr::from_bytes(
        &bytes[..end],
    )))
}

#[cfg(target_os = "linux")]
fn listeners() -> Result<Vec<Listener>> {
    use std::collections::HashMap;
    let ours = std::fs::read_link("/proc/self/ns/net").ok();
    let mut found = Vec::new();
    let Ok(procs) = std::fs::read_dir("/proc") else {
        return Ok(found);
    };
    for entry in procs.flatten() {
        let Ok(pid) = entry.file_name().to_string_lossy().parse::<u32>() else {
            continue;
        };
        let base = entry.path();
        // The process's sockets, by inode.
        let Ok(fds) = std::fs::read_dir(base.join("fd")) else {
            continue;
        };
        let inodes: Vec<String> = fds
            .flatten()
            .filter_map(|fd| std::fs::read_link(fd.path()).ok())
            .filter_map(|l| {
                let l = l.to_string_lossy().into_owned();
                l.strip_prefix("socket:[")?
                    .strip_suffix(']')
                    .map(str::to_owned)
            })
            .collect();
        if inodes.is_empty() {
            continue;
        }
        // Listening sockets of its network, by inode: tcp and tcp6 tables,
        // state 0A, the inode in the tenth column.
        let mut listening: HashMap<String, (String, u16)> = HashMap::new();
        for (table, v6) in [("net/tcp", false), ("net/tcp6", true)] {
            let Ok(text) = std::fs::read_to_string(base.join(table)) else {
                continue;
            };
            for line in text.lines().skip(1) {
                let cols: Vec<&str> = line.split_whitespace().collect();
                if cols.len() < 10 || cols[3] != "0A" {
                    continue;
                }
                let Some((ip, port)) = cols[1].split_once(':') else {
                    continue;
                };
                let Ok(port) = u16::from_str_radix(port, 16) else {
                    continue;
                };
                listening.insert(cols[9].to_owned(), (linux_address(ip, v6), port));
            }
        }
        let name = std::fs::read_to_string(base.join("comm"))
            .map(|n| n.trim().to_owned())
            .unwrap_or_default();
        let reachable = std::fs::read_link(base.join("ns/net")).ok() == ours;
        for inode in inodes {
            if let Some((address, port)) = listening.get(&inode) {
                found.push(Listener {
                    port: *port,
                    address: address.clone(),
                    pid,
                    process: name.clone(),
                    reachable,
                    shielded: false,
                });
            }
        }
    }
    Ok(found)
}

/// A /proc/net/tcp address (hex, in the kernel's byte order) as text.
#[cfg(target_os = "linux")]
fn linux_address(hex: &str, v6: bool) -> String {
    if hex.chars().all(|c| c == '0') {
        return "*".to_owned();
    }
    if !v6 {
        if let Ok(n) = u32::from_str_radix(hex, 16) {
            return std::net::Ipv4Addr::from(n.swap_bytes()).to_string();
        }
    } else if hex == "00000000000000000000000001000000" {
        return "::1".to_owned();
    }
    hex.to_owned()
}

#[cfg(target_os = "linux")]
fn cwd_of(pid: u32) -> Option<std::path::PathBuf> {
    std::fs::read_link(format!("/proc/{pid}/cwd")).ok()
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn listeners() -> Result<Vec<Listener>> {
    Ok(Vec::new())
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn cwd_of(_pid: u32) -> Option<std::path::PathBuf> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses_are_said_plainly() {
        for (ip, said) in [
            ("*", "*"),
            ("0.0.0.0", "*"),
            ("[::]", "*"),
            ("127.0.0.1", "127.0.0.1"),
            ("[::1]", "::1"),
        ] {
            assert_eq!(address_of(ip), said);
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_addresses_are_read_from_the_kernel_s_hex() {
        assert_eq!(linux_address("0100007F", false), "127.0.0.1");
        assert_eq!(linux_address("00000000", false), "*");
        assert_eq!(
            linux_address("00000000000000000000000001000000", true),
            "::1"
        );
    }
}
