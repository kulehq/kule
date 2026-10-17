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

/// The entries of directory `rel` in the copy at `root`.
pub fn list(root: &Path, rel: &str) -> Result<Listing> {
    let dir = inside(root, rel)?;
    let read = std::fs::read_dir(&dir).map_err(|e| format!("{rel}: {e}"))?;
    let mut entries = Vec::new();
    let mut cut = false;
    for entry in read {
        let entry = entry.map_err(|e| format!("{rel}: {e}"))?;
        if entries.len() == MAX_ENTRIES {
            cut = true;
            break;
        }
        let Ok(meta) = std::fs::symlink_metadata(entry.path()) else {
            continue;
        };
        let kind = if meta.file_type().is_symlink() {
            EntryKind::Link
        } else if meta.is_dir() {
            EntryKind::Dir
        } else {
            EntryKind::File
        };
        entries.push(DirEntry {
            name: entry.file_name().to_string_lossy().into_owned(),
            kind,
            size: if kind == EntryKind::File {
                meta.len()
            } else {
                0
            },
        });
    }
    entries.sort_by(|a, b| {
        (a.kind != EntryKind::Dir, a.name.to_lowercase())
            .cmp(&(b.kind != EntryKind::Dir, b.name.to_lowercase()))
    });
    Ok(Listing { entries, cut })
}

/// File `rel` in the copy at `root`, as text when it is text: its first
/// [`MAX_READ`] bytes.
pub fn read(root: &Path, rel: &str) -> Result<FileText> {
    use std::io::Read;
    let path = inside(root, rel)?;
    let meta = std::fs::symlink_metadata(&path).map_err(|e| format!("{rel}: {e}"))?;
    if !meta.is_file() {
        return Err(format!("{rel}: not a file"));
    }
    let size = meta.len();
    let mut bytes = Vec::new();
    std::fs::File::open(&path)
        .map_err(|e| format!("{rel}: {e}"))?
        .take(MAX_READ)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("{rel}: {e}"))?;
    let cut = size > MAX_READ;
    if bytes.contains(&0) {
        return Ok(FileText::Binary { size });
    }
    // A cut can fall inside a character; what is whole of it is shown.
    let text = match String::from_utf8(bytes) {
        Ok(text) => text,
        Err(e) if cut && e.utf8_error().error_len().is_none() => {
            let valid = e.utf8_error().valid_up_to();
            String::from_utf8_lossy(&e.into_bytes()[..valid]).into_owned()
        }
        Err(_) => return Ok(FileText::Binary { size }),
    };
    Ok(FileText::Text { text, cut, size })
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
