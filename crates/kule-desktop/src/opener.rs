//! The task's copy opened in an editor this computer has, as the person
//! asks from the task's menu. Opening a file with its app and showing it
//! in the file manager are `kule_daemon::outside`, shared with `kule`.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use kule_daemon::Result;
use kule_daemon::editors::EDITORS;
use kule_daemon::outside::{run, start};
use serde::Serialize;

/// An editor this computer has.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Found {
    pub id: &'static str,
    pub name: &'static str,
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
