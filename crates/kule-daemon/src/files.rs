//! A task's copy as the window's file explorer reads it: one directory's
//! entries at a time, and a file's text to look at. Nothing here leaves the
//! copy: a path is relative, has no `..`, and is never followed through a
//! symbolic link, so a link an agent left in the copy shows as a link and
//! is not read.

use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::Result;

/// The most of a file the explorer shows.
pub const MAX_READ: u64 = 256 * 1024;

/// The most entries of one directory listed.
const MAX_ENTRIES: usize = 5000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EntryKind {
    File,
    Dir,
    Link,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DirEntry {
    pub name: String,
    pub kind: EntryKind,
    pub size: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Listing {
    /// Directories first, then files, each by name.
    pub entries: Vec<DirEntry>,
    /// More entries than were listed.
    pub cut: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum FileText {
    Text { text: String, cut: bool, size: u64 },
    Binary { size: u64 },
}

/// `rel` inside `root`, every step of it a real directory, never a link.
fn inside(root: &Path, rel: &str) -> Result<PathBuf> {
    let rel = Path::new(rel);
    if rel.is_absolute() || rel.components().any(|c| !matches!(c, Component::Normal(_))) {
        return Err(format!(
            "{}: a path inside the copy, without ..",
            rel.display()
        ));
    }
    let mut at = root.to_path_buf();
    for part in rel.components() {
        at.push(part);
        let meta = std::fs::symlink_metadata(&at).map_err(|e| format!("{}: {e}", rel.display()))?;
        if meta.file_type().is_symlink() {
            return Err(format!(
                "{}: a link, which the explorer does not follow",
                rel.display()
            ));
        }
    }
    Ok(at)
}

/// Whether `rel` is a file or directory of the copy at `root`, reached
/// without a link.
pub fn existing(root: &Path, rel: &str) -> Result<()> {
    inside(root, rel).map(|_| ())
}

/// Opens `rel` (checked by [`inside`]) without following a link at any
/// step, so one put in place between the check and the open (an agent can
/// swap files in the copy at will) is not followed either. A FIFO does not
/// block it.
fn open(root: &Path, rel: &str) -> Result<std::fs::File> {
    inside(root, rel)?;
    if rel.is_empty() {
        return std::fs::File::open(root).map_err(|e| format!("{}: {e}", root.display()));
    }
    let rel_slash = rel.trim_end_matches('/');
    kule_engine::open_beneath(root, rel_slash)
        .map_err(|e| format!("{rel}: {e}"))?
        .ok_or_else(|| format!("{rel}: not there, or a link, which the explorer does not follow"))
}

/// The entries of the open directory `dir`: name, kind and size, each read
/// beside it without following a link (`fstatat`), never through a path.
fn entries_of(dir: std::fs::File, rel: &str) -> Result<(Vec<DirEntry>, bool)> {
    use std::ffi::CStr;
    use std::os::fd::IntoRawFd;
    let fd = dir.into_raw_fd();
    // SAFETY: fdopendir takes the descriptor, which closedir closes below.
    let dirp = unsafe { libc::fdopendir(fd) };
    if dirp.is_null() {
        let e = std::io::Error::last_os_error();
        // SAFETY: the descriptor was not taken; it is closed once here.
        unsafe { libc::close(fd) };
        return Err(format!("{rel}: {e}"));
    }
    let mut entries = Vec::new();
    let mut cut = false;
    loop {
        // SAFETY: dirp is open; readdir returns an entry or null at the end.
        let ent = unsafe { libc::readdir(dirp) };
        if ent.is_null() {
            break;
        }
        // SAFETY: d_name is a NUL-terminated name inside the entry.
        let name = unsafe { CStr::from_ptr((*ent).d_name.as_ptr()) };
        if name.to_bytes() == b"." || name.to_bytes() == b".." {
            continue;
        }
        if entries.len() == MAX_ENTRIES {
            cut = true;
            break;
        }
        // SAFETY: an all-zero stat is valid; fstatat fills it in.
        let mut st: libc::stat = unsafe { std::mem::zeroed() };
        // SAFETY: dirfd(dirp) is open and name is NUL-terminated.
        let ok = unsafe {
            libc::fstatat(
                libc::dirfd(dirp),
                name.as_ptr(),
                &mut st,
                libc::AT_SYMLINK_NOFOLLOW,
            )
        };
        if ok != 0 {
            continue;
        }
        let kind = match st.st_mode & libc::S_IFMT {
            libc::S_IFLNK => EntryKind::Link,
            libc::S_IFDIR => EntryKind::Dir,
            _ => EntryKind::File,
        };
        entries.push(DirEntry {
            name: name.to_string_lossy().into_owned(),
            kind,
            size: if kind == EntryKind::File {
                st.st_size as u64
            } else {
                0
            },
        });
    }
    // SAFETY: dirp is open and closed once, with its descriptor.
    unsafe { libc::closedir(dirp) };
    Ok((entries, cut))
}

/// The entries of directory `rel` in the copy at `root`.
pub fn list(root: &Path, rel: &str) -> Result<Listing> {
    let opened = open(root, rel)?;
    if !opened
        .metadata()
        .map_err(|e| format!("{rel}: {e}"))?
        .is_dir()
    {
        return Err(format!("{rel}: not a directory"));
    }
    // Listed through the descriptor just opened, not the path again.
    let (mut entries, cut) = entries_of(opened, rel)?;
    entries.sort_by(|a, b| {
        (a.kind != EntryKind::Dir, a.name.to_lowercase())
            .cmp(&(b.kind != EntryKind::Dir, b.name.to_lowercase()))
    });
    Ok(Listing { entries, cut })
}

/// The first `max` bytes of file `rel` in the copy at `root`, and the
/// file's size.
fn read_start(root: &Path, rel: &str, max: u64) -> Result<(Vec<u8>, u64)> {
    use std::io::Read;
    use std::os::unix::fs::MetadataExt;
    let file = open(root, rel)?;
    // What was opened, not what the path names now.
    let meta = file.metadata().map_err(|e| format!("{rel}: {e}"))?;
    if !meta.is_file() {
        return Err(format!("{rel}: not a file"));
    }
    // A second name for a file outside the copy (a hard link) would read
    // what the copy does not hold.
    if meta.nlink() > 1 {
        return Err(format!(
            "{rel}: a file with more than one name, which the explorer does not read"
        ));
    }
    let size = meta.len();
    let mut bytes = Vec::with_capacity(size.min(max) as usize);
    file.take(max)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("{rel}: {e}"))?;
    Ok((bytes, size))
}

/// File `rel` in the copy at `root`, as text when it is text: its first
/// [`MAX_READ`] bytes.
pub fn read(root: &Path, rel: &str) -> Result<FileText> {
    let (bytes, size) = read_start(root, rel, MAX_READ)?;
    let cut = size > bytes.len() as u64;
    Ok(match kule_view::text::decode(bytes, cut) {
        Some(text) => FileText::Text { text, cut, size },
        None => FileText::Binary { size },
    })
}

/// File `rel` in the copy at `root` as the Preview shows it.
pub fn view(root: &Path, rel: &str) -> Result<kule_view::View> {
    let (bytes, size) = read_start(root, rel, kule_view::wanted(rel))?;
    Ok(kule_view::view(rel, bytes, size))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_explorer_reads_the_copy_and_nothing_outside_it() {
        let (copy, outside) = (
            tempfile::TempDir::new().unwrap(),
            tempfile::TempDir::new().unwrap(),
        );
        let root = copy.path();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/main.rs"), "fn main() {}\n").unwrap();
        std::fs::write(root.join("README.md"), "# Hi\n").unwrap();
        std::fs::write(root.join("logo.png"), [0x89, b'P', b'N', b'G', 0, 1, 2]).unwrap();
        std::fs::write(outside.path().join("secret.txt"), "secret\n").unwrap();
        std::os::unix::fs::symlink(outside.path(), root.join("away")).unwrap();
        std::os::unix::fs::symlink(outside.path().join("secret.txt"), root.join("secret.txt"))
            .unwrap();

        let top = list(root, "").unwrap();
        let names: Vec<(&str, EntryKind)> = top
            .entries
            .iter()
            .map(|e| (e.name.as_str(), e.kind))
            .collect();
        assert_eq!(
            names,
            [
                ("src", EntryKind::Dir),
                ("away", EntryKind::Link),
                ("logo.png", EntryKind::File),
                ("README.md", EntryKind::File),
                ("secret.txt", EntryKind::Link)
            ]
        );
        assert_eq!(list(root, "src").unwrap().entries[0].name, "main.rs");
        assert_eq!(
            read(root, "src/main.rs").unwrap(),
            FileText::Text {
                text: "fn main() {}\n".into(),
                cut: false,
                size: 13
            }
        );
        assert_eq!(
            read(root, "logo.png").unwrap(),
            FileText::Binary { size: 7 }
        );

        // Nothing through a link, up a level or from an absolute path; the
        // file next to the copy and the absolute path are real and readable,
        // so only the rules keep them out.
        let beside = root.parent().unwrap().join(format!(
            "{}-beside.txt",
            root.file_name().unwrap().to_string_lossy()
        ));
        std::fs::write(&beside, "beside\n").unwrap();
        let real = std::fs::canonicalize(outside.path().join("secret.txt")).unwrap();
        let up = format!("../{}", beside.file_name().unwrap().to_string_lossy());
        for (what, call) in [
            ("link dir", list(root, "away").err()),
            ("link file", read(root, "secret.txt").err()),
            ("through link", read(root, "away/secret.txt").err()),
            ("parent", read(root, &up).err()),
            ("absolute", read(root, &real.to_string_lossy()).err()),
        ] {
            assert!(call.is_some(), "{what} was read");
        }
        std::fs::remove_file(&beside).unwrap();
    }

    /// A second name for a file outside the copy, and a FIFO left where a
    /// file was: neither is read, and the FIFO does not hang the daemon.
    #[test]
    fn a_hard_link_or_a_fifo_is_not_read() {
        let (copy, outside) = (
            tempfile::TempDir::new().unwrap(),
            tempfile::TempDir::new().unwrap(),
        );
        let root = copy.path();
        std::fs::write(outside.path().join("id_rsa"), "secret\n").unwrap();
        std::fs::hard_link(outside.path().join("id_rsa"), root.join("key.txt")).unwrap();
        let e = read(root, "key.txt").unwrap_err();
        assert!(e.contains("more than one name"), "{e}");

        let fifo = std::ffi::CString::new(root.join("pipe").to_str().unwrap()).unwrap();
        // SAFETY: mkfifo reads the path and makes a FIFO there.
        assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
        let started = std::time::Instant::now();
        let e = read(root, "pipe").unwrap_err();
        assert!(e.contains("not a file"), "{e}");
        assert!(started.elapsed() < std::time::Duration::from_secs(2));
        // Directories are listed through what was opened.
        std::fs::create_dir(root.join("d")).unwrap();
        std::fs::write(root.join("d/x.txt"), "x").unwrap();
        assert_eq!(list(root, "d").unwrap().entries[0].name, "x.txt");
        assert!(list(root, "key.txt").is_err());
    }

    /// The Preview reads what the explorer reads, by the same rules, and as
    /// much of a file as its view needs.
    #[test]
    fn the_preview_views_files_of_the_copy_only() {
        let (copy, outside) = (
            tempfile::TempDir::new().unwrap(),
            tempfile::TempDir::new().unwrap(),
        );
        let root = copy.path();
        let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
        png.extend(2u32.to_be_bytes());
        png.extend(2u32.to_be_bytes());
        png.extend([8, 6, 0, 0, 0, 0, 0, 0, 0]);
        std::fs::write(root.join("logo.png"), &png).unwrap();
        std::fs::write(root.join("notes.md"), "# Notes\n").unwrap();
        // Larger than the explorer reads, smaller than the Preview does.
        let long = "x".repeat(MAX_READ as usize * 2);
        std::fs::write(root.join("long.txt"), &long).unwrap();
        std::fs::write(outside.path().join("a.png"), &png).unwrap();
        std::os::unix::fs::symlink(outside.path().join("a.png"), root.join("away.png")).unwrap();
        std::fs::hard_link(outside.path().join("a.png"), root.join("twice.png")).unwrap();

        assert!(matches!(
            view(root, "logo.png").unwrap(),
            kule_view::View::Image {
                width: 2,
                height: 2,
                ..
            }
        ));
        assert!(matches!(
            view(root, "notes.md").unwrap(),
            kule_view::View::Markdown { .. }
        ));
        match view(root, "long.txt").unwrap() {
            kule_view::View::Text { text, cut, .. } => assert!(!cut && text == long),
            other => panic!("{other:?}"),
        }
        assert!(view(root, "away.png").is_err());
        assert!(view(root, "twice.png").is_err());
        assert!(view(root, "../x.png").is_err());
    }

    #[test]
    fn a_large_file_is_cut_on_a_whole_character() {
        let copy = tempfile::TempDir::new().unwrap();
        // Two-byte characters across the cut.
        let text = "ç".repeat((MAX_READ as usize) / 2 + 10);
        std::fs::write(copy.path().join("big.txt"), &text).unwrap();
        match read(copy.path(), "big.txt").unwrap() {
            FileText::Text {
                text: shown,
                cut,
                size,
            } => {
                assert!(cut);
                assert_eq!(size, text.len() as u64);
                assert!(shown.chars().all(|c| c == 'ç'));
                assert_eq!(shown.len() as u64, MAX_READ);
            }
            other => panic!("{other:?}"),
        }
    }
}
