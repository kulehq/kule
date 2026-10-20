//! A file of a task's copy opened with its app, or shown in the file
//! manager, as the person asks from the window or `kule`. What may be
//! opened is decided before (see [`crate::files::openable`]); here only the
//! computer's own launcher runs, each path an argument of its own, never
//! through a shell.

use std::ffi::OsStr;
use std::path::Path;
use std::process::{Command, Stdio};

use crate::Result;

/// Runs `program` with `args` and waits for it, as the launchers return at
/// once; what it printed on failure is the error.
pub fn run(program: &str, args: &[&OsStr]) -> Result<()> {
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
pub fn start(program: &Path, args: &[&OsStr]) -> Result<()> {
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
}
