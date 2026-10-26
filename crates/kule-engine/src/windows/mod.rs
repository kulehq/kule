//! What Kule asks of Windows beyond its filesystem (see [`crate::osfs`]):
//! the app container its proxy runs in, the certificates it trusts, and
//! what the sandbox around a task's commands needs of this Windows.

pub mod appcontainer;
pub mod preconditions;
pub(crate) mod roots;

use std::io;
use std::path::{Path, PathBuf};

/// Where Kule's program `name` is: beside this program, and nowhere else,
/// except that a test program cargo put in a `deps` folder looks in the
/// folder above it, where cargo puts the programs it built.
pub fn beside_me(name: &str) -> io::Result<PathBuf> {
    let me = std::env::current_exe()?;
    let dir = me.parent();
    let beside = dir.map(|dir| dir.join(name));
    let above = dir
        .filter(|dir| dir.file_name().is_some_and(|n| n == "deps"))
        .and_then(Path::parent)
        .map(|dir| dir.join(name));
    [beside, above]
        .into_iter()
        .flatten()
        .find(|p| p.is_file())
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!("{name} is not beside {}", me.display()),
            )
        })
}
