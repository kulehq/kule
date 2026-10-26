//! What changed inside the files of an effect set: the entity map of
//! [`kule_entities`], with each file's two sides read from where Kule keeps
//! them. The copy's side is what the agent wrote; the workspace holds the
//! other side for as long as nobody changed it since the last sync.
//!
//! The copy is the agent's to shape while a person reviews, so every read
//! goes down from the root one component at a time and follows no link,
//! opens nothing but regular files, and stops at the size Kule outlines.

use std::io::Read;
use std::path::Path;
use std::time::{Duration, Instant};

use kule_entities::{MAX_BYTES, Outline, Skip};
use serde::{Deserialize, Serialize};

use crate::effects::EffectSet;
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
pub(crate) enum Side {
    Bytes(Vec<u8>),
    Absent,
    Skipped(Skip),
}

/// The two sides of the file at `path`, each read up to `limit` bytes:
/// what the workspace held at the last sync (known while the workspace's
/// file is unchanged since), and what the copy holds now.
pub(crate) fn sides(
    copy: &Path,
    work: &Path,
    baseline: &Baseline,
    path: &str,
    limit: usize,
) -> Result<(Side, Side)> {
    let before = match baseline.entries.get(path) {
        Some(entry) if entry.work.kind == Kind::File => match Stamp::of(&work.join(path))? {
            Some(now) if entry.work.unchanged(&now) => read_beneath(work, path, limit)?,
            _ => Side::Skipped(Skip::BeforeUnknown),
        },
        _ => Side::Absent,
    };
    Ok((before, read_beneath(copy, path, limit)?))
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
        // A moved file holds what it held: nothing changed inside it.
        let moved = set
            .moves
            .iter()
            .any(|m| m.from == effect.path || m.to == effect.path);
        if moved || !kule_entities::supported(&effect.path) {
            continue;
        }
        let (before, after) = sides(copy, work, baseline, &effect.path, MAX_BYTES)?;
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
/// when it holds more than `limit` bytes.
fn read_beneath(root: &Path, rel: &str, limit: usize) -> Result<Side> {
    let path = root.join(rel);
    match open_beneath(root, rel) {
        Ok(Some(file)) => {
            let meta = file.metadata().at(&path)?;
            if !meta.is_file() {
                return Ok(Side::Absent);
            }
            let mut bytes = Vec::new();
            let most = u64::try_from(limit).unwrap_or(u64::MAX).saturating_add(1);
            file.take(most).read_to_end(&mut bytes).at(&path)?;
            if bytes.len() > limit {
                return Ok(Side::Skipped(Skip::TooLarge));
            }
            Ok(Side::Bytes(bytes))
        }
        Ok(None) => Ok(Side::Absent),
        Err(e) => Err(e).at(&path),
    }
}

/// The regular file at `rel` under `root`, read as [`read_beneath`] reads
/// (no link followed) up to the size Kule merges; `None` when it is not
/// there, not a regular file, or larger.
pub(crate) fn read_file(root: &Path, rel: &str) -> Result<Option<Vec<u8>>> {
    Ok(match read_beneath(root, rel, MAX_BYTES)? {
        Side::Bytes(bytes) => Some(bytes),
        _ => None,
    })
}

pub(crate) use crate::fsops::open_beneath;

#[cfg(all(test, unix))]
mod tests {
    use std::ffi::CString;
    use std::fs;
    use std::os::unix::ffi::OsStrExt;
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
            matches!(read_beneath(&root, "src/a.rs", MAX_BYTES).unwrap(), Side::Bytes(b) if b == b"fn a() {}\n")
        );
        assert!(matches!(
            read_beneath(&root, "src/link.rs", MAX_BYTES).unwrap(),
            Side::Absent
        ));
        assert!(matches!(
            read_beneath(&root, "up/secret.rs", MAX_BYTES).unwrap(),
            Side::Absent
        ));
        assert!(matches!(
            read_beneath(&root, "../secret.rs", MAX_BYTES).unwrap(),
            Side::Absent
        ));
        assert!(matches!(
            read_beneath(&root, "src", MAX_BYTES).unwrap(),
            Side::Absent
        ));
        assert!(matches!(
            read_beneath(&root, "src/none.rs", MAX_BYTES).unwrap(),
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
            read_beneath(dir.path(), "a.rs", MAX_BYTES).unwrap(),
            Side::Absent
        ));
    }

    #[test]
    fn large_files_are_not_read_whole() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("a.rs"), vec![b' '; MAX_BYTES + 10]).unwrap();
        assert!(matches!(
            read_beneath(dir.path(), "a.rs", MAX_BYTES).unwrap(),
            Side::Skipped(Skip::TooLarge)
        ));
    }
}
