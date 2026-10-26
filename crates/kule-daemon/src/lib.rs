//! Kule's daemon: one process per home that owns tasks, agent runs and the
//! questions they wait on, so they outlive any window. The app and the CLI
//! talk to it over a Unix socket that only the same user can open.

mod alerts;
mod client;
pub mod editors;
pub mod extensions;
pub mod files;
pub mod layout;
pub mod outside;
mod protocol;
mod server;
mod service;
pub mod settings_profile;
pub mod shell;
pub mod shortcuts;
pub mod themes;
mod transport;

use std::path::{Path, PathBuf};

pub use alerts::{Alert, Alerts};
pub use client::{Client, spawn_daemon};
pub use kule_acp::Profile as AgentProfile;
pub use kule_engine::osfs;
pub use kule_engine::{
    BudgetLeft, BudgetScope, Group, GroupPolicy, GroupSpent, Limits, Money, PolicyChoice, Question,
    Spent, TaskColor, TaskLimits, public_ip,
};
pub use kule_view::refs::Reference;
pub use protocol::{Call, Hello, PROTOCOL, Redacted, Request, Response};
pub use server::{MAX_REQUEST, bind, dispatch, new_token, run, serve, socket_path, token_path};
pub use service::{
    Behind, Commit, DailyBudget, GroupImpact, GroupPolicyText, IsolationView, Notice, Outside,
    Overlap, Result, Review, RunEvent, Sequenced, Service, SharedBudget, TaskChange, TaskView,
    Timeline,
};
#[cfg(unix)]
pub use transport::peer_uid;

/// Kule's `home` as it must be: a folder of this user's, open to nobody
/// else, never a link; it holds the daemon's token, every task's copy and
/// the secrets. Made so when missing. One of this user's that is open to
/// others is closed, and the returned note says so; anything else is
/// refused.
pub fn check_home(home: &Path) -> Result<Option<String>> {
    let shown = home.display();
    let meta = match std::fs::symlink_metadata(home) {
        Ok(meta) => meta,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            kule_engine::osfs::create_private_dir_all(home)
                .map_err(|e| format!("cannot make Kule's home {shown}: {e}"))?;
            return Ok(None);
        }
        Err(e) => return Err(format!("cannot read Kule's home {shown}: {e}")),
    };
    if meta.file_type().is_symlink() {
        return Err(format!(
            "Kule's home {shown} is a link; Kule keeps its home only in a folder of its own (pass --home or KULE_HOME with the folder itself)"
        ));
    }
    if !meta.is_dir() {
        return Err(format!("Kule's home {shown} is not a folder"));
    }
    let mine = kule_engine::osfs::owned_by_me(home)
        .map_err(|e| format!("cannot read who owns Kule's home {shown}: {e}"))?;
    if !mine {
        return Err(format!(
            "Kule's home {shown} belongs to another user; Kule uses only a home of yours"
        ));
    }
    let open = kule_engine::osfs::open_to_others(home)
        .map_err(|e| format!("cannot read who may open Kule's home {shown}: {e}"))?;
    let Some(found) = open else {
        return Ok(None);
    };
    // Through the folder itself: no link put in its place meanwhile is
    // followed.
    kule_engine::osfs::make_private(home)
        .map_err(|e| format!("cannot close Kule's home {shown} to others: {e}"))?;
    Ok(Some(format!(
        "Kule's home {shown} was open to others ({found}); it is private now"
    )))
}

/// Kule's home: `KULE_HOME`, else `~/.kule` (Unix) or `%LOCALAPPDATA%\\Kule`
/// (Windows).
pub fn default_home() -> Result<PathBuf> {
    if let Some(home) = std::env::var_os("KULE_HOME") {
        return Ok(PathBuf::from(home));
    }
    #[cfg(unix)]
    {
        std::env::var_os("HOME")
            .map(|h| PathBuf::from(h).join(".kule"))
            .ok_or_else(|| "HOME is not set; set KULE_HOME".to_owned())
    }
    #[cfg(windows)]
    {
        std::env::var_os("LOCALAPPDATA")
            .map(|h| PathBuf::from(h).join("Kule"))
            .ok_or_else(|| "LOCALAPPDATA is not set; set KULE_HOME".to_owned())
    }
}

#[cfg(all(test, unix))]
mod tests {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    use super::*;

    fn mode(path: &Path) -> u32 {
        fs::metadata(path).unwrap().permissions().mode() & 0o7777
    }

    /// Kule's home is a folder of the person's own, open to nobody else:
    /// made so, closed when it was open, refused when it is a link.
    #[test]
    fn kule_s_home_is_a_folder_of_one_s_own_and_closed_to_others() {
        let base = tempfile::TempDir::new().unwrap();
        let home = base.path().join("made/.kule");
        assert_eq!(check_home(&home).unwrap(), None);
        assert_eq!(mode(&home), 0o700);

        fs::set_permissions(&home, fs::Permissions::from_mode(0o755)).unwrap();
        let note = check_home(&home).unwrap().unwrap();
        assert!(note.contains("open to others (755)"), "{note}");
        assert_eq!(mode(&home), 0o700);
        assert_eq!(check_home(&home).unwrap(), None);

        let link = base.path().join("link");
        std::os::unix::fs::symlink(&home, &link).unwrap();
        let e = check_home(&link).unwrap_err();
        assert!(e.contains("is a link"), "{e}");
        let file = base.path().join("file");
        fs::write(&file, "").unwrap();
        assert!(check_home(&file).unwrap_err().contains("not a folder"));
    }

    /// The client sends its token only to a daemon of the same user: what
    /// listens at the socket's place as anyone else hears nothing.
    #[test]
    fn the_client_talks_only_to_a_daemon_of_its_own_user() {
        let theirs = ["/var/run/mDNSResponder", "/run/dbus/system_bus_socket"]
            .into_iter()
            .map(Path::new)
            .find(|p| {
                use std::os::unix::fs::MetadataExt;
                // SAFETY: geteuid has no preconditions.
                let me = unsafe { libc::geteuid() };
                fs::metadata(p).is_ok_and(|m| m.uid() != me)
            });
        let Some(theirs) = theirs else {
            eprintln!("no socket of another user to try here");
            return;
        };
        let home = tempfile::TempDir::new().unwrap();
        std::os::unix::fs::symlink(theirs, socket_path(home.path())).unwrap();
        fs::write(token_path(home.path()), "secret-token").unwrap();
        // What listens there never answers: a client that talked to it
        // would wait forever.
        let (tx, rx) = std::sync::mpsc::channel();
        let client = Client::new(home.path());
        std::thread::spawn(move || {
            let _ = tx.send(client.call::<Hello>(Call::Hello));
        });
        let e = rx
            .recv_timeout(std::time::Duration::from_secs(10))
            .expect("the client talked to what listens there")
            .unwrap_err();
        assert!(e.contains("not your own Kule daemon"), "{e}");
    }
}
