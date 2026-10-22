//! macOS: what a task runs is kept off the network even where it listens on
//! every address, for programs no hook reaches (Go, Rust, Java and the like;
//! see `loopback`). Only where the person allows servers for a task do its
//! programs listen at all; this keeps them off the network from outside the
//! sandbox, which is weaker than holding them inside it, and the person
//! accepts it as such (see `isolation_gaps`).
//!
//! Seatbelt sees a listening port but not its address, so the address is
//! dealt with from outside the sandbox. Where a process of a run listens on
//! every address, Kule binds the same port on each of this computer's
//! network addresses (Wi-Fi, Ethernet, VPN, IPv6 included). The kernel
//! hands a connection to the socket with the most specific address, so
//! what comes from the network reaches Kule, which drops it at once, and
//! what comes through loopback still reaches the process: the person's
//! browser keeps working. A process that listens on a network address of
//! its own (which Kule cannot take ahead of it) is stopped.
//!
//! Kule looks every 50 ms while a run goes on, reading the processes'
//! sockets from the kernel (a look takes under a millisecond); a connection
//! from the network in that first moment can still get through.

use std::collections::{BTreeMap, BTreeSet};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV6, TcpListener};
use std::os::fd::AsRawFd;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::ports::macos;

/// A process Kule stopped for listening on a network address of its own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stopped {
    pub pid: u32,
    pub process: String,
    pub address: IpAddr,
    pub port: u16,
}

/// What a look found and did.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Guarded {
    /// Ports listened on every address, now kept off the network.
    pub shielded: BTreeSet<u16>,
    pub stopped: Vec<Stopped>,
}

/// The sockets Kule holds, each on a network address, by port.
type Shadows = Arc<Mutex<BTreeMap<u16, Vec<(SocketAddr, TcpListener)>>>>;

/// The sockets Kule holds on the network addresses, by port.
#[derive(Default)]
pub struct Shield {
    shadows: Shadows,
    done: Arc<AtomicBool>,
    drainer: bool,
}

impl Drop for Shield {
    fn drop(&mut self) {
        self.done.store(true, Ordering::SeqCst);
    }
}

/// Whether `ip` is this computer's own: loopback, IPv4 or IPv6, mapped too.
fn local_only(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => v4.is_loopback(),
        IpAddr::V6(v6) => {
            v6.is_loopback() || v6.to_ipv4_mapped().is_some_and(|v4| v4.is_loopback())
        }
    }
}

fn every(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => v4.is_unspecified(),
        IpAddr::V6(v6) => {
            v6.is_unspecified() || v6.to_ipv4_mapped().is_some_and(|v4| v4.is_unspecified())
        }
    }
}

/// This computer's addresses on its networks: every interface that is up,
/// loopback left out.
pub fn network_addresses() -> Vec<SocketAddr> {
    let mut out = Vec::new();
    let mut list: *mut libc::ifaddrs = std::ptr::null_mut();
    // SAFETY: getifaddrs fills list, freed below; each entry is read while
    // the list lives.
    unsafe {
        if libc::getifaddrs(&mut list) != 0 {
            return out;
        }
        let mut at = list;
        while !at.is_null() {
            let entry = &*at;
            at = entry.ifa_next;
            if entry.ifa_addr.is_null()
                || entry.ifa_flags & libc::IFF_UP as u32 == 0
                || entry.ifa_flags & libc::IFF_LOOPBACK as u32 != 0
            {
                continue;
            }
            match (*entry.ifa_addr).sa_family as i32 {
                libc::AF_INET => {
                    let a = &*(entry.ifa_addr as *const libc::sockaddr_in);
                    let ip = Ipv4Addr::from(u32::from_be(a.sin_addr.s_addr));
                    out.push(SocketAddr::new(IpAddr::V4(ip), 0));
                }
                libc::AF_INET6 => {
                    let a = &*(entry.ifa_addr as *const libc::sockaddr_in6);
                    let mut octets = a.sin6_addr.s6_addr;
                    let mut scope = a.sin6_scope_id;
                    // The kernel's own form of a link-local address carries
                    // its interface in the second 16 bits.
                    if octets[0] == 0xfe && octets[1] & 0xc0 == 0x80 {
                        let embedded = u16::from_be_bytes([octets[2], octets[3]]);
                        if scope == 0 {
                            scope = embedded as u32;
                        }
                        octets[2] = 0;
                        octets[3] = 0;
                    }
                    let ip = Ipv6Addr::from(octets);
                    out.push(SocketAddr::V6(SocketAddrV6::new(ip, 0, 0, scope)));
                }
                _ => {}
            }
        }
        libc::freeifaddrs(list);
    }
    out
}

/// Every process descended from this one: what its runs started.
pub fn descendants() -> Vec<i32> {
    let me = std::process::id() as i32;
    let parents: BTreeMap<i32, i32> = macos::all_pids()
        .into_iter()
        .filter_map(|pid| Some((pid, macos::parent_of(pid)?)))
        .collect();
    let mut found = Vec::new();
    let mut frontier = vec![me];
    while let Some(parent) = frontier.pop() {
        for (&pid, &ppid) in &parents {
            if ppid == parent && pid != me && !found.contains(&pid) {
                found.push(pid);
                frontier.push(pid);
            }
        }
    }
    found
}

impl Shield {
    pub fn new() -> Shield {
        Shield::default()
    }

    /// Looks at what `pids` listen on and keeps it off the network.
    pub fn guard(&mut self, pids: &[i32]) -> Guarded {
        let mut guarded = Guarded::default();
        for &pid in pids {
            for (ip, port) in macos::listening(pid) {
                if local_only(&ip) {
                    continue;
                }
                if every(&ip) {
                    guarded.shielded.insert(port);
                    continue;
                }
                // SAFETY: a signal to a process of this one's own run.
                unsafe { libc::kill(pid, libc::SIGKILL) };
                guarded.stopped.push(Stopped {
                    pid: pid as u32,
                    process: macos::name_of(pid),
                    address: ip,
                    port,
                });
            }
        }
        self.cover(&guarded.shielded);
        guarded
    }

    /// Holds `ports` on every network address, and lets go of the rest.
    fn cover(&mut self, ports: &BTreeSet<u16>) {
        let addresses = network_addresses();
        let mut shadows = self
            .shadows
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        shadows.retain(|port, _| ports.contains(port));
        for &port in ports {
            let held = shadows.entry(port).or_default();
            for address in &addresses {
                let mut at = *address;
                at.set_port(port);
                if held.iter().any(|(a, _)| *a == at) {
                    continue;
                }
                // Taken by another program of the person's: theirs to keep.
                if let Ok(listener) = TcpListener::bind(at)
                    && listener.set_nonblocking(true).is_ok()
                {
                    held.push((at, listener));
                }
            }
        }
        let any = !shadows.is_empty();
        drop(shadows);
        if any && !self.drainer {
            self.drainer = true;
            let (shadows, done) = (Arc::clone(&self.shadows), Arc::clone(&self.done));
            std::thread::spawn(move || drain(shadows, done));
        }
    }

    /// Lets go of every port: no run is going on.
    pub fn release(&mut self) {
        self.shadows
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clear();
    }

    /// The ports held now.
    pub fn held(&self) -> BTreeSet<u16> {
        self.shadows
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .keys()
            .copied()
            .collect()
    }
}

/// Drops every connection that reaches Kule's sockets, with a reset.
fn drain(shadows: Shadows, done: Arc<AtomicBool>) {
    while !done.load(Ordering::SeqCst) {
        let fds: Vec<libc::pollfd> = shadows
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .values()
            .flatten()
            .map(|(_, l)| libc::pollfd {
                fd: l.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            })
            .collect();
        if fds.is_empty() {
            std::thread::sleep(Duration::from_millis(100));
            continue;
        }
        let mut fds = fds;
        // SAFETY: fds is a live array of pollfd.
        let ready = unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, 100) };
        if ready <= 0 {
            continue;
        }
        let shadows = shadows
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for (_, listener) in shadows.values().flatten() {
            while let Ok((stream, _)) = listener.accept() {
                // A reset, not a goodbye: nothing reads as an answer.
                let linger = libc::linger {
                    l_onoff: 1,
                    l_linger: 0,
                };
                // SAFETY: linger is live and sized for the option.
                unsafe {
                    libc::setsockopt(
                        stream.as_raw_fd(),
                        libc::SOL_SOCKET,
                        libc::SO_LINGER,
                        (&linger as *const libc::linger).cast(),
                        std::mem::size_of::<libc::linger>() as libc::socklen_t,
                    );
                }
                drop(stream);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses_are_told_apart() {
        let v4 = |s: &str| s.parse::<IpAddr>().unwrap();
        assert!(
            local_only(&v4("127.0.0.1"))
                && local_only(&v4("::1"))
                && local_only(&v4("::ffff:127.0.0.1"))
        );
        assert!(every(&v4("0.0.0.0")) && every(&v4("::")) && every(&v4("::ffff:0.0.0.0")));
        assert!(!local_only(&v4("192.168.1.5")) && !every(&v4("192.168.1.5")));
        assert!(network_addresses().iter().all(|a| !a.ip().is_loopback()));
    }
}
