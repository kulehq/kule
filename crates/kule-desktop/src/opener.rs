//! What is in a task's copy, opened outside Kule as the person asks: a file
//! with its app, a file or folder shown in the file manager, the copy in an
//! editor. The daemon decides first what may be opened (see
//! `kule_daemon::files::openable`); here only the computer's own launcher
//! runs, each path an argument of its own, never through a shell.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use kule_daemon::Result;
use kule_daemon::editors::EDITORS;
use serde::Serialize;

/// An editor this computer has.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Found {
    pub id: &'static str,
    pub name: &'static str,
}

/// Runs `program` with `args` and waits for it, as the launchers return at
/// once; what it printed on failure is the error.
fn run(program: &str, args: &[&OsStr]) -> Result<()> {
    let out = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| format!("{program}: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        let said = String::from_utf8_lossy(&out.stderr);
        Err(format!("{program}: {}", said.trim()))
    }
}

/// Starts `program` with `args` and leaves it running: an editor, or a
/// launcher that may wait for the app it opened. Reaped when it ends.
fn start(program: &Path, args: &[&OsStr]) -> Result<()> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("{}: {e}", program.display()))?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

/// Opens file `path` with the app the computer opens its kind with.
pub fn open_with_app(path: &Path) -> Result<()> {
    if cfg!(target_os = "macos") {
        run("open", &[path.as_os_str()])
    } else {
        start(Path::new("xdg-open"), &[path.as_os_str()])
    }
}

/// Shows `path` in the file manager, picked out in its folder; never opens
/// it, so a folder named like an app (`Tool.app`) is not started.
pub fn show(path: &Path) -> Result<()> {
    if cfg!(target_os = "macos") {
        return run("open", &[OsStr::new("-R"), path.as_os_str()]);
    }
    let uri = file_uri(path);
    let shown = run(
        "dbus-send",
        &[
            OsStr::new("--session"),
            OsStr::new("--print-reply"),
            OsStr::new("--dest=org.freedesktop.FileManager1"),
            OsStr::new("/org/freedesktop/FileManager1"),
            OsStr::new("org.freedesktop.FileManager1.ShowItems"),
            OsStr::new(&format!("array:string:{uri}")),
            OsStr::new("string:"),
        ],
    );
    // No file manager answers on the bus: its folder, opened.
    shown.or_else(|_| {
        start(
            Path::new("xdg-open"),
            &[path.parent().unwrap_or(path).as_os_str()],
        )
    })
}

/// `file://` and the path with everything but letters, digits, `-._~` and
/// `/` escaped: dbus-send splits an array at commas.
fn file_uri(path: &Path) -> String {
    use std::os::unix::ffi::OsStrExt;
    let mut uri = String::from("file://");
    for &b in path.as_os_str().as_bytes() {
        if b.is_ascii_alphanumeric() || b"-._~/".contains(&b) {
            uri.push(b as char);
        } else {
            uri.push_str(&format!("%{b:02X}"));
        }
    }
    uri
}

/// Where each editor is looked for: its app on macOS, its command on
/// Linux.
fn names(id: &str) -> &'static [&'static str] {
    match (id, cfg!(target_os = "macos")) {
        ("vscode", true) => &["Visual Studio Code.app"],
        ("cursor", true) => &["Cursor.app"],
        ("zed", true) => &["Zed.app"],
        ("vscode", false) => &["code"],
        ("cursor", false) => &["cursor"],
        ("zed", false) => &["zed", "zeditor"],
        _ => &[],
    }
}

/// The places looked in: the Applications folders on macOS, `PATH` on
/// Linux.
fn places() -> Vec<PathBuf> {
    if cfg!(target_os = "macos") {
        let mut places = vec![PathBuf::from("/Applications")];
        if let Some(home) = std::env::var_os("HOME") {
            places.push(Path::new(&home).join("Applications"));
        }
        places
    } else {
        std::env::var_os("PATH")
            .map(|p| std::env::split_paths(&p).collect())
            .unwrap_or_default()
    }
}

/// Where editor `id` is in `places`: an app folder on macOS, an executable
/// file on Linux.
fn find_in(id: &str, places: &[PathBuf]) -> Option<PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    names(id).iter().find_map(|name| {
        places.iter().map(|p| p.join(name)).find(|at| {
            let Ok(meta) = std::fs::metadata(at) else {
                return false;
            };
            if cfg!(target_os = "macos") {
                meta.is_dir()
            } else {
                meta.is_file() && meta.permissions().mode() & 0o111 != 0
            }
        })
    })
}

fn found_in(places: &[PathBuf]) -> Vec<Found> {
    EDITORS
        .iter()
        .filter(|(id, _)| find_in(id, places).is_some())
        .map(|&(id, name)| Found { id, name })
        .collect()
}

/// The editors this computer has, in Kule's order.
pub fn found() -> Vec<Found> {
    found_in(&places())
}

/// Opens folder `dir` in editor `id`, one of [`EDITORS`] this computer has.
pub fn open_in(id: &str, dir: &Path) -> Result<()> {
    let at = find_in(id, &places())
        .ok_or_else(|| format!("{id:?} is not an editor this computer has"))?;
    if cfg!(target_os = "macos") {
        run("open", &[OsStr::new("-a"), at.as_os_str(), dir.as_os_str()])
    } else {
        start(&at, &[dir.as_os_str()])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_path_becomes_a_uri_the_bus_does_not_split() {
        assert_eq!(
            file_uri(Path::new("/home/a b/x,y/ä.pdf")),
            "file:///home/a%20b/x%2Cy/%C3%A4.pdf"
        );
    }

    #[test]
    fn only_editors_that_are_there_are_found() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::TempDir::new().unwrap();
        let places = vec![dir.path().join("none"), dir.path().to_path_buf()];
        assert_eq!(found_in(&places), []);
        if cfg!(target_os = "macos") {
            std::fs::create_dir_all(dir.path().join("Cursor.app/Contents")).unwrap();
            // A file named like an app is not one.
            std::fs::write(dir.path().join("Zed.app"), "").unwrap();
        } else {
            let cursor = dir.path().join("cursor");
            std::fs::write(&cursor, "#!/bin/sh\n").unwrap();
            std::fs::set_permissions(&cursor, std::fs::Permissions::from_mode(0o755)).unwrap();
            // A file that does not run is not an editor.
            std::fs::write(dir.path().join("zed"), "").unwrap();
        }
        assert_eq!(
            found_in(&places),
            [Found {
                id: "cursor",
                name: "Cursor"
            }]
        );
        assert_eq!(find_in("bash", &places), None);
    }
}
