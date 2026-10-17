//! What changed inside the files of an effect set: the entity map of
//! [`kule_entities`], with each file's two sides read from where Kule keeps
//! them. The copy's side is what the agent wrote; the workspace holds the
//! other side for as long as nobody changed it since the last sync.
//!
//! The copy is the agent's to shape while a person reviews, so every read
//! goes down from the root one component at a time and follows no link,
//! opens nothing but regular files, and stops at the size Kule outlines.

use std::ffi::CString;
use std::fs::File;
use std::io::{self, Read};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::time::{Duration, Instant};

use kule_entities::{MAX_BYTES, Outline, Skip};
use serde::{Deserialize, Serialize};

use crate::effects::{Change, EffectSet};
use crate::error::{IoContext, Result};
use crate::manifest::{Baseline, Kind, Stamp};

/// More changed files than this and the rest go without an outline.
const MAX_FILES: usize = 300;
/// What outlining one effect set may take; files after that go without.
const MAX_TIME: Duration = Duration::from_secs(5);

/// What changed inside one file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileOutline {
    pub path: String,
    #[serde(flatten)]
    pub outline: Outline,
}

/// One side of a file.
enum Side {
    Bytes(Vec<u8>),
    Absent,
    Skipped(Skip),
}

pub(crate) fn outline(
    copy: &Path,
    work: &Path,
    baseline: &Baseline,
    set: &EffectSet,
) -> Result<Vec<FileOutline>> {
    let started = Instant::now();
    let mut outlines = Vec::new();
    for effect in set.files() {
        if outlines.len() == MAX_FILES || started.elapsed() > MAX_TIME {
            break;
        }
        if !kule_entities::supported(&effect.path) {
            continue;
        }
        let entry = baseline.entries.get(&effect.path);
        let before = match (effect.change, entry) {
            (Change::Created, _) | (_, None) => Side::Absent,
            (_, Some(entry)) if entry.work.kind != Kind::File => Side::Absent,
            (_, Some(entry)) => {
                let path = work.join(&effect.path);
                match Stamp::of(&path)? {
                    Some(now) if entry.work.unchanged(&now) => read_beneath(work, &effect.path)?,
                    _ => Side::Skipped(Skip::BeforeUnknown),
                }
            }
        };
        let after = match effect.change {
            Change::Deleted => Side::Absent,
            _ if effect.kind != Kind::File => Side::Absent,
            _ => read_beneath(copy, &effect.path)?,
        };
        let outline = match (before, after) {
            (Side::Absent, Side::Absent) => continue,
            (Side::Skipped(reason), _) | (_, Side::Skipped(reason)) => Outline::Skipped { reason },
            (before, after) => {
                let bytes = |side: &Side| match side {
                    Side::Bytes(bytes) => Some(bytes.clone()),
                    _ => None,
                };
                kule_entities::diff(
                    &effect.path,
                    bytes(&before).as_deref(),
                    bytes(&after).as_deref(),
                )
            }
        };
        outlines.push(FileOutline {
            path: effect.path.clone(),
            outline,
        });
    }
    Ok(outlines)
}

/// Reads `rel` under `root` without following a link anywhere on the way:
/// absent when something else than a regular file is there now, skipped
/// when it is larger than Kule outlines.
fn read_beneath(root: &Path, rel: &str) -> Result<Side> {
    let path = root.join(rel);
    match open_beneath(root, rel) {
        Ok(Some(file)) => {
            let meta = file.metadata().at(&path)?;
            if !meta.is_file() {
                return Ok(Side::Absent);
            }
            let mut bytes = Vec::new();
            let limit = u64::try_from(MAX_BYTES).unwrap_or(u64::MAX) + 1;
            file.take(limit).read_to_end(&mut bytes).at(&path)?;
            if bytes.len() > MAX_BYTES {
                return Ok(Side::Skipped(Skip::TooLarge));
            }
            Ok(Side::Bytes(bytes))
        }
        Ok(None) => Ok(Side::Absent),
        Err(e) => Err(e).at(&path),
    }
}

/// Opens `rel` under `root`, each component with `O_NOFOLLOW`; `None` when
/// a component is missing, is a link, or is not what it must be.
fn open_beneath(root: &Path, rel: &str) -> io::Result<Option<File>> {
    let mut dir = File::open(root)?;
    let parts: Vec<&str> = rel.split('/').collect();
    for (i, part) in parts.iter().enumerate() {
        if matches!(*part, "" | "." | "..") {
            return Ok(None);
        }
        let last = i + 1 == parts.len();
        let name = CString::new(std::ffi::OsStr::new(part).as_bytes())?;
        let mut flags = libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC;
        // A FIFO put where a file was must not block the read.
        flags |= if last {
            libc::O_NONBLOCK
        } else {
            libc::O_DIRECTORY
        };
        // SAFETY: openat reads the name and returns a new descriptor or -1.
        let fd = unsafe { libc::openat(dir.as_raw_fd(), name.as_ptr(), flags) };
        if fd == -1 {
            let e = io::Error::last_os_error();
            return match e.raw_os_error() {
                Some(libc::ENOENT | libc::ELOOP | libc::ENOTDIR | libc::ENXIO) => Ok(None),
                _ => Err(e),
            };
        }
        // SAFETY: fd was just opened and is owned by nothing else.
        dir = unsafe { File::from_raw_fd(fd) };
    }
    Ok(Some(dir))
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::symlink;

    use tempfile::TempDir;

    use super::*;

    #[test]
    fn reads_go_down_without_following_links() {
        let dir = TempDir::new().unwrap();
        let root = dir.path().join("root");
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src/a.rs"), "fn a() {}\n").unwrap();
        fs::write(dir.path().join("secret.rs"), "fn secret() {}\n").unwrap();
        symlink(dir.path().join("secret.rs"), root.join("src/link.rs")).unwrap();
        symlink(dir.path(), root.join("up")).unwrap();

        assert!(
            matches!(read_beneath(&root, "src/a.rs").unwrap(), Side::Bytes(b) if b == b"fn a() {}\n")
        );
        assert!(matches!(
            read_beneath(&root, "src/link.rs").unwrap(),
            Side::Absent
        ));
        assert!(matches!(
            read_beneath(&root, "up/secret.rs").unwrap(),
            Side::Absent
        ));
        assert!(matches!(
            read_beneath(&root, "../secret.rs").unwrap(),
            Side::Absent
        ));
        assert!(matches!(read_beneath(&root, "src").unwrap(), Side::Absent));
        assert!(matches!(
            read_beneath(&root, "src/none.rs").unwrap(),
            Side::Absent
        ));
    }

    #[test]
    fn a_fifo_does_not_block_the_read() {
        let dir = TempDir::new().unwrap();
        let fifo = CString::new(dir.path().join("a.rs").as_os_str().as_bytes()).unwrap();
        // SAFETY: mkfifo reads the path.
        assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
        assert!(matches!(
            read_beneath(dir.path(), "a.rs").unwrap(),
            Side::Absent
        ));
    }

    #[test]
    fn large_files_are_not_read_whole() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("a.rs"), vec![b' '; MAX_BYTES + 10]).unwrap();
        assert!(matches!(
            read_beneath(dir.path(), "a.rs").unwrap(),
            Side::Skipped(Skip::TooLarge)
        ));
    }
}
