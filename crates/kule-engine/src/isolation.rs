//! What this computer cannot give a sandboxed command, and whether the
//! person accepted running with less. Kule never runs a command less
//! isolated than it means to without their say: a gap refuses the command
//! until they accept weaker isolation, which is kept in
//! `<home>/isolation.json` and shown while it holds.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::{IoContext, Result};
use crate::fsops::{read_json, write_json};
use crate::sandbox::Network;

const ISOLATION_FILE: &str = "isolation.json";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Isolation {
    /// Run commands with what this computer gives, gaps and all.
    pub accept_weaker: bool,
}

/// The person's choice; not accepted when never made.
pub fn isolation(home: &Path) -> Result<Isolation> {
    let path = home.join(ISOLATION_FILE);
    if !path.is_file() {
        return Ok(Isolation::default());
    }
    read_json(&path)
}

pub fn set_isolation(home: &Path, isolation: &Isolation) -> Result<()> {
    std::fs::create_dir_all(home).at(home)?;
    write_json(&home.join(ISOLATION_FILE), isolation)
}

/// What a command with `network` would go without on this computer, said
/// for a person; empty where nothing is missing (Linux with unprivileged
/// user namespaces and a recent Landlock, and macOS while the run's limits
/// keep its servers off). `servers` is the run's limit that lets its
/// programs listen on macOS (see [`crate::Limits::servers`]).
pub fn isolation_gaps(network: Network, servers: bool) -> Vec<String> {
    #[cfg(target_os = "linux")]
    {
        let _ = servers;
        gaps(
            network,
            crate::namespace::available(),
            crate::namespace::mounts_available(),
            crate::namespace::pids_available(),
            crate::namespace::ipc_available(),
            crate::sandbox::landlock_abi(),
            crate::bind_filter::available(),
        )
    }
    #[cfg(not(target_os = "linux"))]
    {
        macos_gaps(network, servers)
    }
}

/// macOS: Seatbelt sees a listening port but not its address, so where a
/// run's servers may listen while its network goes through the proxy, only
/// the hooks (see `loopback`) and, while an agent runs in the app, the
/// shield from outside (see `shield`) keep them off the network.
#[cfg_attr(target_os = "linux", allow(dead_code))]
pub(crate) fn macos_gaps(network: Network, servers: bool) -> Vec<String> {
    if servers && matches!(network, Network::Proxy(_)) {
        vec![
            "its servers held to this computer: Kule points Node, Python and Ruby servers at it, and keeps the others off your network from outside the sandbox while an agent runs in the app, looking every 50 ms; any other server could be reached from your network"
                .to_owned(),
        ]
    } else {
        Vec::new()
    }
}

/// [`isolation_gaps`] from what the system offers: a network namespace,
/// a mount namespace, a PID namespace, an IPC namespace, Landlock's ABI,
/// and Kule doing a command's binds (`bind_filter`).
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) fn gaps(
    network: Network,
    own_network: bool,
    own_mounts: bool,
    own_pids: bool,
    own_ipc: bool,
    abi: i64,
    bind_filter: bool,
) -> Vec<String> {
    let mut gaps = Vec::new();
    if !own_ipc {
        gaps.push(
            "shared memory of its own: it could open the System V segments and queues of your other programs"
                .to_owned(),
        );
    }
    // On the person's network (asked for, or for want of one of its own),
    // its servers are held to loopback by Kule doing their binds.
    let shared = network == Network::Allow || !own_network;
    if shared && !bind_filter {
        gaps.push(
            "its servers held to this computer: one it starts could listen on your network"
                .to_owned(),
        );
    }
    if network != Network::Allow && !own_network {
        gaps.push(
            "a network of its own: only the proxy's port is checked, on any address, and datagrams pass"
                .to_owned(),
        );
    }
    if abi < 9 && !own_mounts {
        gaps.push(
            "Unix sockets kept out: it could reach daemons running as you (the session bus, Docker, ssh-agent)"
                .to_owned(),
        );
    }
    if !own_mounts {
        gaps.push(
            "agents' settings kept read-only while it runs: they are put back after it instead"
                .to_owned(),
        );
        gaps.push(
            "a /tmp of its own: it shares /tmp with your other programs and tasks".to_owned(),
        );
    }
    if !own_pids {
        gaps.push(
            "processes of its own: what it leaves running is found by a guess, which one could slip"
                .to_owned(),
        );
        // Landlock keeps its signals in from ABI 6.
        if abi < 6 {
            gaps.push("signals kept in: it could signal your other programs".to_owned());
        }
    }
    // Abstract sockets belong to a network; Landlock keeps them in from
    // ABI 6 where the command shares the person's.
    if shared && abi < 6 {
        gaps.push(
            "abstract sockets kept out: on your network it could reach programs listening on one (the X server)"
                .to_owned(),
        );
    }
    gaps
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_missing_piece_is_a_gap_and_nothing_else_is() {
        let proxy = Network::Proxy(1);
        assert!(gaps(proxy, true, true, true, true, 9, true).is_empty());
        assert!(gaps(proxy, true, true, true, true, 6, true).is_empty());
        assert!(gaps(Network::Allow, false, true, true, true, 9, true).is_empty());
        assert_eq!(gaps(proxy, false, true, true, true, 9, true).len(), 1);
        assert_eq!(gaps(proxy, false, false, true, true, 6, true).len(), 4);
        assert_eq!(gaps(proxy, false, false, true, true, 9, true).len(), 3);
        // Servers on the person's network, with nobody doing their binds.
        assert!(gaps(proxy, true, true, true, true, 9, false).is_empty());
        assert_eq!(
            gaps(Network::Allow, true, true, true, true, 9, false).len(),
            1
        );
        assert_eq!(gaps(proxy, false, true, true, true, 9, false).len(), 2);
        // Processes of its own; before ABI 6 they also keep its signals in.
        assert_eq!(gaps(proxy, true, true, false, true, 9, true).len(), 1);
        assert_eq!(gaps(proxy, true, true, false, true, 5, true).len(), 2);
        assert!(gaps(proxy, true, true, true, true, 5, true).is_empty());
        // Abstract sockets, kept in by a network of its own or ABI 6.
        assert_eq!(
            gaps(Network::Allow, true, true, true, true, 5, true).len(),
            1
        );
        assert!(gaps(Network::Allow, true, true, true, true, 6, true).is_empty());
        // System V IPC of its own, which nothing else stands in for.
        assert_eq!(gaps(proxy, true, true, true, false, 9, true).len(), 1);
    }

    #[test]
    fn servers_on_macos_are_a_gap_only_where_the_proxy_holds_the_network() {
        assert!(macos_gaps(Network::Proxy(1), false).is_empty());
        assert_eq!(macos_gaps(Network::Proxy(1), true).len(), 1);
        // No network: nothing listens whatever the limit says.
        assert!(macos_gaps(Network::Deny, true).is_empty());
        // The person's own network, opened as such.
        assert!(macos_gaps(Network::Allow, true).is_empty());
    }

    #[test]
    fn the_choice_is_kept_and_none_means_not_accepted() {
        let home = tempfile::TempDir::new().unwrap();
        assert!(!isolation(home.path()).unwrap().accept_weaker);
        set_isolation(
            home.path(),
            &Isolation {
                accept_weaker: true,
            },
        )
        .unwrap();
        assert!(isolation(home.path()).unwrap().accept_weaker);
    }
}
