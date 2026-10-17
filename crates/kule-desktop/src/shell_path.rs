//! The `PATH` a person's shell sets up. An app opened from the Finder or a
//! desktop launcher starts with the system's bare `PATH`, where `npx`,
//! `gemini` and the rest are not, so the daemon and the agents it starts
//! would not find them. The app asks the person's login shell instead.

use std::ffi::{OsStr, OsString};
use std::io::Read;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Around `PATH` in what the shell prints, so whatever its startup files
/// print besides is left out.
const MARK: &str = "__KULE_PATH__";

/// The `PATH` the login shell `shell` sets up, or `None` when it fails or
/// takes longer than `wait`.
pub fn login_path(shell: &Path, wait: Duration) -> Option<OsString> {
    let script = format!("printf '%s%s%s' {MARK} \"$PATH\" {MARK}");
    let mut child = Command::new(shell)
        .args(["-l", "-i", "-c", &script])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if started.elapsed() < wait => std::thread::sleep(Duration::from_millis(20)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
    let mut out = Vec::new();
    child.stdout.take()?.read_to_end(&mut out).ok()?;
    between_marks(&out)
}

fn between_marks(out: &[u8]) -> Option<OsString> {
    let mark = MARK.as_bytes();
    let start = out.windows(mark.len()).position(|w| w == mark)? + mark.len();
    let len = out[start..].windows(mark.len()).position(|w| w == mark)?;
    let path = &out[start..start + len];
    (!path.is_empty()).then(|| OsString::from_vec(path.to_vec()))
}

/// The directories of `found`, then those of `current` it does not have.
pub fn merged(found: &OsStr, current: Option<&OsStr>) -> OsString {
    let mut dirs: Vec<PathBuf> = Vec::new();
    let all =
        std::env::split_paths(found).chain(current.into_iter().flat_map(std::env::split_paths));
    for dir in all {
        if !dir.as_os_str().is_empty() && !dirs.contains(&dir) {
            dirs.push(dir);
        }
    }
    std::env::join_paths(dirs).unwrap_or_else(|_| found.to_owned())
}

/// The `PATH` the app should run with: the login shell's ahead of its own,
/// unless it was started from a terminal, which already set it up.
pub fn for_app() -> Option<OsString> {
    use std::io::IsTerminal;
    if std::io::stdin().is_terminal() {
        return None;
    }
    let shell = std::env::var_os("SHELL")
        .filter(|s| Path::new(s).is_absolute())
        .unwrap_or_else(|| OsStr::from_bytes(b"/bin/sh").to_owned());
    let found = login_path(Path::new(&shell), Duration::from_secs(5))?;
    Some(merged(&found, std::env::var_os("PATH").as_deref()))
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    use tempfile::TempDir;

    use super::*;

    fn shell(dir: &TempDir, body: &str) -> PathBuf {
        let path = dir.path().join("shell");
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    #[test]
    fn the_login_shells_path_is_read_between_the_marks() {
        let dir = TempDir::new().unwrap();
        // What its startup files print around it is left out.
        let noisy = shell(
            &dir,
            "echo 'Welcome back'; PATH=/opt/tools/bin:/usr/bin; eval \"$4\"; echo bye",
        );
        assert_eq!(
            login_path(&noisy, Duration::from_secs(5)),
            Some(OsString::from("/opt/tools/bin:/usr/bin"))
        );
    }

    #[test]
    fn a_shell_that_hangs_or_fails_gives_nothing() {
        let dir = TempDir::new().unwrap();
        let slow = shell(&dir, "sleep 10");
        let started = Instant::now();
        assert_eq!(login_path(&slow, Duration::from_millis(300)), None);
        assert!(started.elapsed() < Duration::from_secs(3));

        let quiet = shell(&dir, "exit 0");
        assert_eq!(login_path(&quiet, Duration::from_secs(5)), None);
        assert_eq!(
            login_path(&dir.path().join("none"), Duration::from_secs(5)),
            None
        );
    }

    #[test]
    fn the_shells_directories_come_first_once_each() {
        assert_eq!(
            merged(
                OsStr::new("/opt/homebrew/bin:/usr/bin"),
                Some(OsStr::new("/usr/bin:/bin::/opt/homebrew/bin"))
            ),
            OsString::from("/opt/homebrew/bin:/usr/bin:/bin")
        );
        assert_eq!(merged(OsStr::new("/a"), None), OsString::from("/a"));
    }
}
