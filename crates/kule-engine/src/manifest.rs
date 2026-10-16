//! Stamps: what `lstat` says about an entry, enough to notice any later change.

use std::collections::BTreeMap;
use std::fs::Metadata;
use std::io::ErrorKind;
use std::os::unix::fs::MetadataExt;
use std::path::Path;

use serde::{Deserialize, Serialize};
use walkdir::WalkDir;

use crate::error::{Error, IoContext, Result};

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
    /// Any write or chmod moves the change time, and nothing can set it back.
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
            Kind::Symlink => Some(utf8(&std::fs::read_link(path).at(path)?)?),
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

pub type Baseline = BTreeMap<String, Entry>;

pub fn scan(root: &Path) -> Result<Tree> {
    let mut tree = Tree::new();
    for entry in WalkDir::new(root).min_depth(1).follow_links(false) {
        let entry = entry.map_err(|e| Error::Io {
            path: e.path().unwrap_or(root).to_path_buf(),
            source: e.into(),
        })?;
        let meta = entry.metadata().map_err(|e| Error::Io {
            path: entry.path().to_path_buf(),
            source: e.into(),
        })?;
        if let Some(stamp) = Stamp::from_meta(entry.path(), &meta)? {
            tree.insert(relative(root, entry.path())?, stamp);
        }
    }
    Ok(tree)
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
