//! Stamps: what `lstat` says about an entry, enough to notice any later change.

use std::collections::BTreeMap;
use std::fs::Metadata;
use std::io::ErrorKind;
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

#[derive(Debug, Clone, Eq, Serialize, Deserialize)]
pub struct Stamp {
    pub kind: Kind,
    pub size: u64,
    /// Permission bits only.
    pub mode: u32,
    pub mtime: (i64, i64),
    /// When the entry was made, where nobody can set it (Linux's statx):
    /// ext4 gives a deleted file's inode number to the next one made, and
    /// this tells the two apart. `None` elsewhere, and in stamps from
    /// before Kule kept it, which compare equal to any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub born: Option<(i64, i64)>,
    /// Any write or chmod moves the change time, and nothing can set it back,
    /// except that writes within one tick of the filesystem's clock share it
    /// (see `clock`).
    pub ctime: (i64, i64),
    pub ino: u64,
    /// The upper half of a 128-bit file identity (ReFS on Windows); zero
    /// elsewhere, and in stamps from before Kule kept it.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub ino_hi: u64,
    pub link: Option<String>,
}

fn is_zero(n: &u64) -> bool {
    *n == 0
}

impl PartialEq for Stamp {
    fn eq(&self, other: &Stamp) -> bool {
        let born = match (self.born, other.born) {
            (Some(a), Some(b)) => a == b,
            _ => true,
        };
        born && self.kind == other.kind
            && self.size == other.size
            && self.mode == other.mode
            && self.mtime == other.mtime
            && self.ctime == other.ctime
            && self.ino == other.ino
            && self.ino_hi == other.ino_hi
            && self.link == other.link
    }
}

impl Stamp {
    /// Whether this stamp and `other`, which share an inode number, are
    /// of the same entry: on Linux, by when each was made; elsewhere the
    /// number is never given to another while a stamp may name it (APFS).
    pub fn same_entry(&self, other: &Stamp) -> bool {
        if cfg!(target_os = "linux") {
            self.born.is_some() && self.born == other.born
        } else {
            true
        }
    }
    /// The stamp of `path`, or `None` when nothing is there, including when
    /// a parent is no longer a directory. Sockets, FIFOs and devices count as
    /// nothing: Kule does not track them.
    pub fn of(path: &Path) -> Result<Option<Stamp>> {
        match lstat(path) {
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
        let Some(facts) = Facts::of(path, meta).at(path)? else {
            return Ok(None);
        };
        Ok(Some(Stamp {
            kind,
            size: if kind == Kind::Dir { 0 } else { facts.size },
            mode: facts.mode,
            mtime: facts.mtime,
            ctime: facts.ctime,
            ino: facts.ino,
            ino_hi: facts.ino_hi,
            link,
            born: facts.born,
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

/// What the filesystem keeps about an entry, read as each system gives it.
struct Facts {
    size: u64,
    mode: u32,
    mtime: (i64, i64),
    ctime: (i64, i64),
    ino: u64,
    ino_hi: u64,
    born: Option<(i64, i64)>,
}

impl Facts {
    /// From `lstat` on Unix. `None` when the entry is gone.
    #[cfg(unix)]
    fn of(_path: &Path, meta: &Metadata) -> std::io::Result<Option<Facts>> {
        use std::os::unix::fs::MetadataExt;
        Ok(Some(Facts {
            size: meta.size(),
            mode: meta.mode() & 0o7777,
            mtime: (meta.mtime(), meta.mtime_nsec()),
            ctime: (meta.ctime(), meta.ctime_nsec()),
            ino: meta.ino(),
            ino_hi: 0,
            born: born(meta),
        }))
    }

    /// From the entry's own handle on Windows, opened without following a
    /// link: the change time and the file identity are not in what a
    /// directory listing gives. `None` when the entry is gone.
    #[cfg(windows)]
    fn of(path: &Path, meta: &Metadata) -> std::io::Result<Option<Facts>> {
        use crate::osfs::win;
        let file = match win::open_attributes(path, false) {
            Ok(file) => file,
            Err(e) if matches!(e.kind(), ErrorKind::NotFound | ErrorKind::NotADirectory) => {
                return Ok(None);
            }
            // A file another process is removing just then, or one Kule may
            // not open: what the listing said, with an identity no entry
            // has, so it counts as changed once it can be read.
            Err(e) if e.kind() == ErrorKind::PermissionDenied => {
                use std::os::windows::fs::MetadataExt;
                let mtime = win::unix_time(meta.last_write_time() as i64);
                return Ok(Some(Facts {
                    size: meta.file_size(),
                    mode: crate::osfs::synthesized_mode(meta.file_attributes()),
                    mtime,
                    ctime: mtime,
                    ino: 0,
                    ino_hi: 0,
                    born: None,
                }));
            }
            Err(e) => return Err(e),
        };
        let basic = win::basic_info(&file)?;
        let standard = win::standard_info(&file)?;
        let (_, id) = win::identity(&file)?;
        Ok(Some(Facts {
            size: standard.EndOfFile as u64,
            mode: crate::osfs::synthesized_mode(basic.FileAttributes),
            mtime: win::unix_time(basic.LastWriteTime),
            ctime: win::unix_time(basic.ChangeTime),
            ino: u64::from_le_bytes(id[..8].try_into().expect("8 bytes")),
            ino_hi: u64::from_le_bytes(id[8..].try_into().expect("8 bytes")),
            born: None,
        }))
    }
}

/// When the entry `meta` describes was made, on Linux (statx, where the
/// filesystem keeps it); `None` elsewhere.
#[cfg(unix)]
fn born(meta: &Metadata) -> Option<(i64, i64)> {
    if !cfg!(target_os = "linux") {
        return None;
    }
    let made = meta.created().ok()?;
    let since = made.duration_since(std::time::UNIX_EPOCH).ok()?;
    Some((since.as_secs() as i64, i64::from(since.subsec_nanos())))
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
    scan_at_most(root, SCAN_ENTRIES)
}

/// The most entries one scan reads: a folder past it is refused, saying
/// so, rather than read without end.
pub const SCAN_ENTRIES: usize = 1_000_000;

fn scan_at_most(root: &Path, most: usize) -> Result<Tree> {
    let mut tree = Tree::new();
    for (seen, entry) in WalkDir::new(root)
        .min_depth(1)
        .follow_links(false)
        .into_iter()
        .enumerate()
    {
        if seen >= most {
            return Err(Error::Invalid(format!(
                "{} holds more than {most} files and folders, more than Kule reads; a task works on a smaller folder",
                root.display()
            )));
        }
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
        let meta = match entry.metadata().map_err(std::io::Error::from) {
            Ok(meta) => meta,
            // Windows refuses to look at a file another process is removing
            // just then: asked again, it is gone or readable.
            Err(e) if e.kind() == ErrorKind::PermissionDenied => match lstat(entry.path()) {
                Ok(meta) => meta,
                Err(e) if matches!(e.kind(), ErrorKind::NotFound | ErrorKind::NotADirectory) => {
                    continue;
                }
                Err(e) => {
                    return Err(Error::Io {
                        path: entry.path().to_path_buf(),
                        source: e,
                    });
                }
            },
            Err(e) if matches!(e.kind(), ErrorKind::NotFound | ErrorKind::NotADirectory) => {
                continue;
            }
            Err(e) => {
                return Err(Error::Io {
                    path: entry.path().to_path_buf(),
                    source: e,
                });
            }
        };
        if let Some(stamp) = Stamp::from_meta(entry.path(), &meta)? {
            tree.insert(relative(root, entry.path())?, stamp);
        }
    }
    Ok(tree)
}

/// `symlink_metadata`, asked again a few times on Windows when it is
/// refused: a file another process is removing just then is refused until
/// it is gone.
fn lstat(path: &Path) -> std::io::Result<Metadata> {
    let mut tries = 0;
    loop {
        match std::fs::symlink_metadata(path) {
            Err(e) if cfg!(windows) && e.kind() == ErrorKind::PermissionDenied && tries < 5 => {
                tries += 1;
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            other => return other,
        }
    }
}

pub fn vanished(e: &walkdir::Error) -> bool {
    e.io_error()
        .is_some_and(|io| matches!(io.kind(), ErrorKind::NotFound | ErrorKind::NotADirectory))
}

/// `path` under `root` as Kule names it everywhere: its components joined
/// with `/`, whatever separator the system writes.
pub fn relative(root: &Path, path: &Path) -> Result<String> {
    let rel = path.strip_prefix(root).map_err(|_| {
        Error::Invalid(format!(
            "{} is not under {}",
            path.display(),
            root.display()
        ))
    })?;
    let mut text = String::new();
    for component in rel.components() {
        let part = component.as_os_str();
        let part = part
            .to_str()
            .ok_or_else(|| Error::NonUtf8Path(path.to_path_buf()))?;
        if !text.is_empty() {
            text.push('/');
        }
        text.push_str(part);
    }
    Ok(text)
}

pub fn utf8(path: &Path) -> Result<String> {
    path.to_str()
        .map(str::to_owned)
        .ok_or_else(|| Error::NonUtf8Path(path.to_path_buf()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A folder past the most a scan reads is refused, saying so.
    #[test]
    fn a_scan_stops_at_its_most_entries() {
        let dir = tempfile::TempDir::new().unwrap();
        for i in 0..5 {
            std::fs::write(dir.path().join(format!("f{i}")), "").unwrap();
        }
        assert_eq!(scan_at_most(dir.path(), 5).unwrap().len(), 5);
        let e = scan_at_most(dir.path(), 4).unwrap_err().to_string();
        assert!(e.contains("more than 4 files and folders"), "{e}");
    }
}
