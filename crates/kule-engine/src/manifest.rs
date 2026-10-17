//! Stamps: what `lstat` says about an entry, enough to notice any later change.

use std::collections::BTreeMap;
use std::fs::Metadata;
use std::io::ErrorKind;
use std::os::unix::fs::MetadataExt;
use std::path::Path;

use serde::{Deserialize, Serialize};
use walkdir::WalkDir;

use crate::error::{Error, IoContext, Result};
use crate::git::GitState;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    File,
    Dir,
    Symlink,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stamp {
    pub kind: Kind,
    pub size: u64,
    /// Permission bits only.
    pub mode: u32,
    pub mtime: (i64, i64),
    /// Any write or chmod moves the change time, and nothing can set it back,
    /// except that writes within one tick of the filesystem's clock share it
    /// (see `clock`).
    pub ctime: (i64, i64),
    pub ino: u64,
    pub link: Option<String>,
}

impl Stamp {
    /// The stamp of `path`, or `None` when nothing is there, including when
    /// a parent is no longer a directory. Sockets, FIFOs and devices count as
    /// nothing: Kule does not track them.
    pub fn of(path: &Path) -> Result<Option<Stamp>> {
        match std::fs::symlink_metadata(path) {
            Ok(meta) => Self::from_meta(path, &meta),
            Err(e) if matches!(e.kind(), ErrorKind::NotFound | ErrorKind::NotADirectory) => {
                Ok(None)
            }
            Err(e) => Err(e).at(path),
        }
    }

    fn from_meta(path: &Path, meta: &Metadata) -> Result<Option<Stamp>> {
        let ft = meta.file_type();
        let kind = if ft.is_symlink() {
            Kind::Symlink
        } else if ft.is_dir() {
            Kind::Dir
        } else if ft.is_file() {
            Kind::File
        } else {
            return Ok(None);
        };
        let link = match kind {
            Kind::Symlink => match std::fs::read_link(path) {
                Ok(target) => Some(utf8(&target)?),
                Err(e) if matches!(e.kind(), ErrorKind::NotFound | ErrorKind::NotADirectory) => {
                    return Ok(None);
                }
                Err(e) => return Err(e).at(path),
            },
            _ => None,
        };
        Ok(Some(Stamp {
            kind,
            size: if kind == Kind::Dir { 0 } else { meta.size() },
            mode: meta.mode() & 0o7777,
            mtime: (meta.mtime(), meta.mtime_nsec()),
            ctime: (meta.ctime(), meta.ctime_nsec()),
            ino: meta.ino(),
            link,
        }))
    }

    /// True when the entry cannot have changed between the two stamps. A
    /// directory changes only through its type or mode; what it contains is
    /// tracked entry by entry.
    pub fn unchanged(&self, now: &Stamp) -> bool {
        match self.kind {
            Kind::Dir => now.kind == Kind::Dir && now.mode == self.mode,
            _ => self == now,
        }
    }

    /// This stamp with a change time no entry has, for an entry whose
    /// content Kule could not pin down: it counts as changed from now on.
    pub fn unknown(mut self) -> Stamp {
        self.ctime = (i64::MIN, -1);
        self
    }

    /// Whether this is such a stamp: nothing is known of what it held.
    pub fn is_unknown(&self) -> bool {
        self.ctime == (i64::MIN, -1)
    }
}

/// Every tracked entry under `root` (not `root` itself), by relative path.
pub type Tree = BTreeMap<String, Stamp>;

/// One path as it stood at the last sync, in the workspace and in the copy.
/// Both held the same content then; the stamps differ (inode, ctime) because
/// they are different files.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub work: Stamp,
    pub copy: Stamp,
}

/// Where the workspace and the copy last agreed.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Baseline {
    pub entries: BTreeMap<String, Entry>,
    /// The workspace repository's state at that point, when it has one.
    pub git: Option<GitState>,
}

/// Walks the tree under `root`. Other processes keep creating and deleting
/// files while Kule looks (git maintenance, IDE indexers), so an entry that
/// disappears between being listed and being read is left out, not an error.
pub fn scan(root: &Path) -> Result<Tree> {
    let mut tree = Tree::new();
    for entry in WalkDir::new(root).min_depth(1).follow_links(false) {
        let entry = match entry {
            Ok(entry) => entry,
            Err(e) if vanished(&e) => continue,
            Err(e) => {
                return Err(Error::Io {
                    path: e.path().unwrap_or(root).to_path_buf(),
                    source: e.into(),
                });
            }
        };
        let meta = match entry.metadata() {
            Ok(meta) => meta,
            Err(e) if vanished(&e) => continue,
            Err(e) => {
                return Err(Error::Io {
                    path: entry.path().to_path_buf(),
                    source: e.into(),
                });
            }
        };
        if let Some(stamp) = Stamp::from_meta(entry.path(), &meta)? {
            tree.insert(relative(root, entry.path())?, stamp);
        }
    }
    Ok(tree)
}

pub fn vanished(e: &walkdir::Error) -> bool {
    e.io_error()
        .is_some_and(|io| matches!(io.kind(), ErrorKind::NotFound | ErrorKind::NotADirectory))
}

pub fn relative(root: &Path, path: &Path) -> Result<String> {
    let rel = path.strip_prefix(root).map_err(|_| {
        Error::Invalid(format!(
            "{} is not under {}",
            path.display(),
            root.display()
        ))
    })?;
    utf8(rel)
}

pub fn utf8(path: &Path) -> Result<String> {
    path.to_str()
        .map(str::to_owned)
        .ok_or_else(|| Error::NonUtf8Path(path.to_path_buf()))
}
