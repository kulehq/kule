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

/// Where, in Kule's home, the copies of files opened with their app are.
pub const OPENED: &str = "opened";

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
        // Named as the copy's top, not where the copy is on disk.
        return std::fs::File::open(root).map_err(|e| format!("the task's top folder: {e}"));
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

/// File `rel` in the copy at `root`, opened (see [`open`]), and its size:
/// a regular file with one name.
fn open_file(root: &Path, rel: &str) -> Result<(std::fs::File, u64)> {
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
    Ok((file, meta.len()))
}

/// The first `max` bytes of file `rel` in the copy at `root`, and the
/// file's size.
fn read_start(root: &Path, rel: &str, max: u64) -> Result<(Vec<u8>, u64)> {
    use std::io::Read;
    let (file, size) = open_file(root, rel)?;
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

/// How a kind of file starts.
#[derive(Clone, Copy)]
enum Starts {
    With(&'static [&'static [u8]]),
    /// `ftyp` and the like, four bytes in (MP4, QuickTime, HEIC).
    BoxAt4(&'static [&'static [u8]]),
    /// A RIFF file of this form.
    Riff(&'static [u8]),
    /// Text in any 8-bit encoding (a CSV Excel saved), not a script.
    Text,
}

const ZIP: Starts = Starts::With(&[b"PK\x03\x04"]);
const ISO_BOX: Starts = Starts::BoxAt4(&[b"ftyp", b"moov", b"wide", b"mdat", b"free"]);

/// What the person's own app may open from the copy, by name, and how such
/// a file starts: documents, pictures, sound and video. Never a program, a
/// script, a page a browser would run, or a kind whose documents can run
/// macros or fetch from elsewhere unless Kule looks inside it first (see
/// [`kule_view::opens`]): those would run on the computer, outside the
/// sandbox and the proxy, and the agent wrote them.
const OPENABLE: &[(&str, Starts)] = &[
    ("pdf", Starts::With(&[b"%PDF-"])),
    ("png", Starts::With(&[b"\x89PNG\r\n\x1a\n"])),
    ("jpg", Starts::With(&[b"\xff\xd8\xff"])),
    ("jpeg", Starts::With(&[b"\xff\xd8\xff"])),
    ("gif", Starts::With(&[b"GIF87a", b"GIF89a"])),
    ("webp", Starts::Riff(b"WEBP")),
    ("heic", ISO_BOX),
    ("heif", ISO_BOX),
    ("tif", Starts::With(&[b"II*\0", b"MM\0*"])),
    ("tiff", Starts::With(&[b"II*\0", b"MM\0*"])),
    ("bmp", Starts::With(&[b"BM"])),
    (
        "mp3",
        Starts::With(&[b"ID3", b"\xff\xfb", b"\xff\xf3", b"\xff\xf2"]),
    ),
    ("m4a", ISO_BOX),
    ("wav", Starts::Riff(b"WAVE")),
    ("flac", Starts::With(&[b"fLaC"])),
    ("ogg", Starts::With(&[b"OggS"])),
    ("opus", Starts::With(&[b"OggS"])),
    ("mp4", ISO_BOX),
    ("m4v", ISO_BOX),
    ("mov", ISO_BOX),
    ("avi", Starts::Riff(b"AVI ")),
    ("webm", Starts::With(&[b"\x1a\x45\xdf\xa3"])),
    ("mkv", Starts::With(&[b"\x1a\x45\xdf\xa3"])),
    ("docx", ZIP),
    ("xlsx", ZIP),
    ("pptx", ZIP),
    ("odt", ZIP),
    ("ods", ZIP),
    ("odp", ZIP),
    ("csv", Starts::Text),
    ("txt", Starts::Text),
];

/// The names, without the dot, of the files [`openable`] lets through.
pub fn openable_kinds() -> Vec<&'static str> {
    OPENABLE.iter().map(|(ext, _)| *ext).collect()
}

fn starts_as(bytes: &[u8], starts: Starts) -> bool {
    match starts {
        Starts::With(heads) => heads.iter().any(|h| bytes.starts_with(h)),
        Starts::BoxAt4(boxes) => bytes.len() >= 8 && boxes.contains(&&bytes[4..8]),
        Starts::Riff(form) => {
            bytes.starts_with(b"RIFF") && bytes.len() >= 12 && &bytes[8..12] == form
        }
        Starts::Text => !bytes.contains(&0) && !bytes.starts_with(b"#!"),
    }
}

/// How long a copy of a file opened with its app is kept: the app reads it
/// at once, and may keep it open through a day's work.
const OPENED_FOR: std::time::Duration = std::time::Duration::from_secs(24 * 3600);

/// File `rel` of the copy at `root`, for the person's own app to open:
/// reached without a link, a single-named regular file, of a kind in
/// [`OPENABLE`] by its name, starting as that kind does (so a script named
/// `notes.pdf` is not handed to whatever opens it), and, for the kinds
/// Kule looks inside, reaching nothing outside itself (see
/// [`kule_view::opens`]). What opens is Kule's own copy of it under
/// `opened`, made before it is checked: nothing the agent does to its file
/// afterwards is what the app reads, and the copy carries none of the
/// file's extended attributes (one names the app a file opens with), only
/// macOS's mark of a file come from elsewhere. That copy's path.
pub fn openable(root: &Path, rel: &str, opened: &Path) -> Result<PathBuf> {
    let ext = Path::new(rel)
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    let Some(&(_, starts)) = OPENABLE.iter().find(|(kind, _)| *kind == ext) else {
        return Err(format!(
            "{rel}: Kule opens documents, pictures, sound and video with their app, not this kind of file; it would run outside the sandbox"
        ));
    };
    let looked_inside = kule_view::opens::LOOKED_INSIDE.contains(&ext.as_str());
    let (file, size) = open_file(root, rel)?;
    if looked_inside && size > kule_view::opens::max_size(&ext) {
        return Err(format!(
            "{rel}: too large for Kule to look inside, so it does not open it with its app"
        ));
    }
    let name = Path::new(rel)
        .file_name()
        .ok_or_else(|| format!("{rel}: not a file"))?;
    let dir = fresh_dir(opened)?;
    let at = dir.join(name);
    let checked = (|| {
        copy_out(&file, &at).map_err(|e| format!("{rel}: {e}"))?;
        let max = if looked_inside { size } else { 8192 };
        let mut bytes = Vec::new();
        {
            use std::io::Read;
            std::fs::File::open(&at)
                .and_then(|f| f.take(max).read_to_end(&mut bytes))
                .map_err(|e| format!("{rel}: {e}"))?;
        }
        if !starts_as(&bytes, starts) {
            return Err(format!(
                "{rel}: it is not the .{ext} file its name says, so Kule does not open it"
            ));
        }
        kule_view::opens::opens_nothing_else(&ext, &bytes)
            .map_err(|why| format!("{rel}: Kule does not open it with its app: {why}"))
    })();
    match checked {
        Ok(()) => Ok(at),
        Err(e) => {
            let _ = std::fs::remove_dir_all(&dir);
            Err(e)
        }
    }
}

/// A new directory of Kule's own under `opened` (made, only Kule's to
/// read, when missing) for one copy, the ones there from more than
/// [`OPENED_FOR`] ago removed.
fn fresh_dir(opened: &Path) -> Result<PathBuf> {
    use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};
    let shown = |e: std::io::Error| format!("{}: {e}", opened.display());
    match std::fs::DirBuilder::new().mode(0o700).create(opened) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(shown(e)),
    }
    let meta = std::fs::symlink_metadata(opened).map_err(shown)?;
    // SAFETY: geteuid only reads the process's user.
    if !meta.is_dir() || meta.uid() != unsafe { libc::geteuid() } {
        return Err(format!(
            "{}: not a folder of Kule's own, so nothing is opened from it",
            opened.display()
        ));
    }
    std::fs::set_permissions(opened, std::fs::Permissions::from_mode(0o700)).map_err(shown)?;
    let now = std::time::SystemTime::now();
    for entry in std::fs::read_dir(opened).map_err(shown)?.flatten() {
        let old = entry
            .metadata()
            .and_then(|m| m.modified())
            .is_ok_and(|t| now.duration_since(t).is_ok_and(|age| age > OPENED_FOR));
        if old {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
    static MADE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    loop {
        let nanos = now
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        let n = MADE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = opened.join(format!("{nanos:x}-{}-{n}", std::process::id()));
        match std::fs::DirBuilder::new().mode(0o700).create(&dir) {
            Ok(()) => return Ok(dir),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(shown(e)),
        }
    }
}

/// The open `file` copied to `to`, which must not exist: a clone where the
/// filesystem makes one, only Kule's to read, without the extended
/// attributes it had, and on macOS marked as come from elsewhere, so the
/// app opens it as it would a download.
fn copy_out(file: &std::fs::File, to: &Path) -> std::io::Result<()> {
    use std::os::fd::AsRawFd;
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    let name = std::ffi::CString::new(to.as_os_str().as_bytes())?;
    #[cfg(target_os = "macos")]
    // SAFETY: the descriptor is open and the path NUL-terminated.
    let cloned =
        unsafe { libc::fclonefileat(file.as_raw_fd(), libc::AT_FDCWD, name.as_ptr(), 0) } == 0;
    #[cfg(not(target_os = "macos"))]
    let cloned = false;
    let out = if cloned {
        std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW)
            .open(to)?
    } else {
        let out = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(to)?;
        #[cfg(target_os = "linux")]
        // SAFETY: both descriptors are open; FICLONE only shares extents.
        let shared = unsafe { libc::ioctl(out.as_raw_fd(), libc::FICLONE, file.as_raw_fd()) } == 0;
        #[cfg(not(target_os = "linux"))]
        let shared = false;
        if !shared {
            let (mut from, mut into) = (file, &out);
            std::io::copy(&mut from, &mut into)?;
        }
        out
    };
    out.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    #[cfg(target_os = "macos")]
    {
        let fd = out.as_raw_fd();
        let mut names = vec![0u8; 64 * 1024];
        // SAFETY: the descriptor is open and names valid for its length.
        let n = unsafe { libc::flistxattr(fd, names.as_mut_ptr().cast(), names.len(), 0) };
        if n < 0 {
            return Err(std::io::Error::last_os_error());
        }
        names.truncate(n as usize);
        for attribute in names.split(|b| *b == 0).filter(|n| !n.is_empty()) {
            let attribute = std::ffi::CString::new(attribute)?;
            // SAFETY: the descriptor is open and the name NUL-terminated.
            if unsafe { libc::fremovexattr(fd, attribute.as_ptr(), 0) } != 0 {
                return Err(std::io::Error::last_os_error());
            }
        }
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        let value = format!("0081;{secs:08x};Kule;");
        // SAFETY: the descriptor is open, the name NUL-terminated and the
        // value valid for its length.
        let set = unsafe {
            libc::fsetxattr(
                fd,
                c"com.apple.quarantine".as_ptr(),
                value.as_ptr().cast(),
                value.len(),
                0,
                0,
            )
        };
        if set != 0 {
            return Err(std::io::Error::last_os_error());
        }
    }
    #[cfg(not(target_os = "macos"))]
    let _ = name;
    Ok(())
}

/// File or directory `rel` of the copy at `root` (the copy itself when
/// empty), reached without a link: its path on disk, to show where it is.
pub fn located(root: &Path, rel: &str) -> Result<PathBuf> {
    if rel.is_empty() {
        return Ok(root.to_path_buf());
    }
    inside(root, rel)
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
    fn only_documents_pictures_sound_and_video_are_opened_with_their_app() {
        let (copy, outside) = (
            tempfile::TempDir::new().unwrap(),
            tempfile::TempDir::new().unwrap(),
        );
        let root = copy.path();
        let kule = tempfile::TempDir::new().unwrap();
        let opened = kule.path().join("opened");
        std::fs::create_dir_all(root.join("invoices")).unwrap();
        std::fs::write(root.join("invoices/summary.pdf"), b"%PDF-1.7\n...").unwrap();
        std::fs::write(root.join("Photo.JPG"), b"\xff\xd8\xff\xe0rest").unwrap();
        std::fs::write(root.join("march.xlsx"), sheet("SUM(1,2)")).unwrap();
        std::fs::write(root.join("clip.mov"), b"\0\0\0\x14ftypqt  ").unwrap();
        std::fs::write(root.join("song.wav"), b"RIFF\x24\0\0\0WAVEfmt ").unwrap();
        std::fs::write(root.join("prices.csv"), b"name;price\nk\xe4se;3\n").unwrap();
        for ok in [
            "invoices/summary.pdf",
            "Photo.JPG",
            "march.xlsx",
            "clip.mov",
            "song.wav",
            "prices.csv",
        ] {
            assert!(openable(root, ok, &opened).is_ok(), "{ok}");
        }

        // What would run: a script, a program, a page, a link file, macros;
        // and a script named as a document.
        std::fs::write(root.join("rename.command"), "#!/bin/sh\nrm -rf ~\n").unwrap();
        std::fs::write(root.join("run.sh"), "#!/bin/sh\n").unwrap();
        std::fs::write(root.join("page.html"), "<script>1</script>").unwrap();
        std::fs::write(root.join("logo.svg"), "<svg><script>1</script></svg>").unwrap();
        std::fs::write(root.join("site.webloc"), "<plist/>").unwrap();
        std::fs::write(root.join("macro.xlsm"), b"PK\x03\x04rest").unwrap();
        std::fs::write(root.join("notes.pdf"), "#!/bin/sh\nrm -rf ~\n").unwrap();
        std::fs::write(root.join("notes.txt"), "#!/bin/sh\nrm -rf ~\n").unwrap();
        std::fs::write(root.join("noname"), b"%PDF-1.7").unwrap();
        std::fs::create_dir_all(root.join("Tool.app/Contents")).unwrap();
        std::fs::create_dir_all(root.join("Slides.key")).unwrap();
        // Through a link, or a link itself, even to a real document.
        std::fs::write(outside.path().join("real.pdf"), b"%PDF-1.7").unwrap();
        std::os::unix::fs::symlink(outside.path(), root.join("away")).unwrap();
        std::os::unix::fs::symlink(outside.path().join("real.pdf"), root.join("linked.pdf"))
            .unwrap();
        std::os::unix::fs::symlink(root.join("run.sh"), root.join("inner.pdf")).unwrap();
        std::fs::hard_link(outside.path().join("real.pdf"), root.join("second.pdf")).unwrap();
        for refused in [
            "rename.command",
            "run.sh",
            "page.html",
            "logo.svg",
            "site.webloc",
            "macro.xlsm",
            "notes.pdf",
            "notes.txt",
            "noname",
            "Tool.app",
            "Slides.key",
            "away/real.pdf",
            "linked.pdf",
            "inner.pdf",
            "second.pdf",
            "../real.pdf",
            "",
        ] {
            assert!(
                openable(root, refused, &opened).is_err(),
                "{refused} was let through"
            );
        }
        let why = openable(root, "notes.pdf", &opened).unwrap_err();
        assert!(why.contains("not the .pdf file its name says"), "{why}");
        let why = openable(root, "rename.command", &opened).unwrap_err();
        assert!(why.contains("outside the sandbox"), "{why}");
    }

    /// A zip archive of `parts`, as office files are.
    fn archive(parts: &[(&str, &[u8])]) -> Vec<u8> {
        use std::io::Write;
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        for (name, bytes) in parts {
            zip.start_file(*name, options).unwrap();
            zip.write_all(bytes).unwrap();
        }
        zip.finish().unwrap().into_inner()
    }

    fn sheet(formula: &str) -> Vec<u8> {
        let xml = format!(
            "<worksheet><sheetData><row><c r=\"A1\"><f>{formula}</f></c></row></sheetData></worksheet>"
        );
        archive(&[("xl/worksheets/sheet1.xml", xml.as_bytes())])
    }

    fn docx(extra: &[(&str, &[u8])]) -> Vec<u8> {
        let mut parts: Vec<(&str, &[u8])> = vec![(
            "word/document.xml",
            b"<w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:body><w:p><w:r><w:t>Hi</w:t></w:r></w:p></w:body></w:document>",
        )];
        parts.extend_from_slice(extra);
        archive(&parts)
    }

    #[test]
    fn documents_that_would_reach_outside_themselves_are_not_opened() {
        let (copy, kule) = (
            tempfile::TempDir::new().unwrap(),
            tempfile::TempDir::new().unwrap(),
        );
        let (root, opened) = (copy.path(), kule.path().join("opened"));
        let template = br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/attachedTemplate" Target="https://example.com/t.dotm" TargetMode="External"/></Relationships>"#;
        std::fs::write(
            root.join("template.docx"),
            docx(&[("word/_rels/settings.xml.rels", template)]),
        )
        .unwrap();
        std::fs::write(
            root.join("macros.docx"),
            docx(&[("word/vbaProject.bin", b"\xd0\xcf\x11\xe0")]),
        )
        .unwrap();
        std::fs::write(
            root.join("linked.xlsx"),
            archive(&[("xl/externalLinks/externalLink1.xml", b"<externalLink/>")]),
        )
        .unwrap();
        std::fs::write(
            root.join("picture.xlsx"),
            sheet("_xlfn.IMAGE(\"https://example.com/p\")"),
        )
        .unwrap();
        std::fs::write(
            root.join("sums.csv"),
            "a,b\n1,=IMAGE(\"https://example.com/p\")\n",
        )
        .unwrap();
        std::fs::write(
            root.join("script.pdf"),
            b"%PDF-1.7\n1 0 obj\n<< /Type /Catalog /OpenAction << /S /JavaScript /JS (app.alert(1)) >> >>\nendobj\n",
        )
        .unwrap();
        // Kinds Kule does not look inside, whatever they hold.
        std::fs::write(root.join("letter.rtf"), b"{\\rtf1 hi}").unwrap();
        std::fs::write(
            root.join("book.epub"),
            archive(&[("mimetype", b"application/epub+zip")]),
        )
        .unwrap();
        std::fs::write(root.join("deck.key"), archive(&[("Index.zip", b"")])).unwrap();
        for refused in [
            "template.docx",
            "macros.docx",
            "linked.xlsx",
            "picture.xlsx",
            "sums.csv",
            "script.pdf",
            "letter.rtf",
            "book.epub",
            "deck.key",
        ] {
            assert!(
                openable(root, refused, &opened).is_err(),
                "{refused} was let through"
            );
        }
        let why = openable(root, "template.docx", &opened).unwrap_err();
        assert!(why.contains("outside the file"), "{why}");
        // Nothing refused is left behind for an app to find.
        let left = std::fs::read_dir(&opened)
            .map(|d| d.flatten().count())
            .unwrap_or(0);
        assert_eq!(left, 0);
    }

    #[test]
    fn what_opens_is_kules_own_copy_and_never_the_agents_file() {
        use std::os::unix::fs::PermissionsExt;
        let (copy, kule, outside) = (
            tempfile::TempDir::new().unwrap(),
            tempfile::TempDir::new().unwrap(),
            tempfile::TempDir::new().unwrap(),
        );
        let (root, opened) = (copy.path(), kule.path().join("opened"));
        let bytes = docx(&[]);
        std::fs::write(root.join("report.docx"), &bytes).unwrap();
        #[cfg(target_os = "macos")]
        set_xattr(
            &root.join("report.docx"),
            "com.apple.LaunchServices.OpenWith",
            b"bplist00",
        );

        let at = openable(root, "report.docx", &opened).unwrap();
        assert!(
            at.starts_with(&opened) && !at.starts_with(root),
            "{}",
            at.display()
        );
        assert_eq!(at.file_name().unwrap(), "report.docx");
        assert_eq!(std::fs::read(&at).unwrap(), bytes);
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&at), 0o600);
        assert_eq!(mode(at.parent().unwrap()), 0o700);
        assert_eq!(mode(&opened), 0o700);
        #[cfg(target_os = "macos")]
        {
            // Marked as come from elsewhere, and opened with no app the
            // agent chose.
            // The system's own provenance mark aside, which no program
            // sets or removes.
            let names: Vec<String> = xattr_names(&at)
                .into_iter()
                .filter(|n| n != "com.apple.provenance")
                .collect();
            assert_eq!(names, ["com.apple.quarantine"], "{names:?}");
        }

        // What the agent does to its file afterwards is not what opens.
        std::fs::write(outside.path().join("run.sh"), "#!/bin/sh\n").unwrap();
        std::fs::remove_file(root.join("report.docx")).unwrap();
        std::os::unix::fs::symlink(outside.path().join("run.sh"), root.join("report.docx"))
            .unwrap();
        assert_eq!(std::fs::read(&at).unwrap(), bytes);

        // A picture, which Kule does not look inside, is copied all the same.
        std::fs::write(root.join("logo.png"), b"\x89PNG\r\n\x1a\nrest").unwrap();
        let picture = openable(root, "logo.png", &opened).unwrap();
        assert!(picture.starts_with(&opened));
        assert_eq!(std::fs::read(&picture).unwrap(), b"\x89PNG\r\n\x1a\nrest");
        assert_ne!(picture.parent(), at.parent());
    }

    #[test]
    fn copies_opened_long_ago_are_cleared() {
        let (copy, kule) = (
            tempfile::TempDir::new().unwrap(),
            tempfile::TempDir::new().unwrap(),
        );
        let (root, opened) = (copy.path(), kule.path().join("opened"));
        std::fs::write(root.join("a.txt"), "a\n").unwrap();
        let first = openable(root, "a.txt", &opened).unwrap();
        let old = std::time::SystemTime::now() - std::time::Duration::from_secs(2 * 24 * 3600);
        std::fs::File::open(first.parent().unwrap())
            .unwrap()
            .set_modified(old)
            .unwrap();
        let second = openable(root, "a.txt", &opened).unwrap();
        assert!(!first.exists(), "{}", first.display());
        assert!(second.exists());
    }

    #[cfg(target_os = "macos")]
    fn set_xattr(path: &Path, name: &str, value: &[u8]) {
        use std::os::unix::ffi::OsStrExt;
        let p = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
        let n = std::ffi::CString::new(name).unwrap();
        // SAFETY: both strings are NUL-terminated; value is valid for its length.
        let r = unsafe {
            libc::setxattr(
                p.as_ptr(),
                n.as_ptr(),
                value.as_ptr().cast(),
                value.len(),
                0,
                0,
            )
        };
        assert_eq!(r, 0);
    }

    #[cfg(target_os = "macos")]
    fn xattr_names(path: &Path) -> Vec<String> {
        use std::os::unix::ffi::OsStrExt;
        let p = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
        let mut buf = vec![0u8; 4096];
        // SAFETY: the path is NUL-terminated and buf is valid for its length.
        let n = unsafe { libc::listxattr(p.as_ptr(), buf.as_mut_ptr().cast(), buf.len(), 0) };
        assert!(n >= 0);
        buf.truncate(n as usize);
        buf.split(|b| *b == 0)
            .filter(|n| !n.is_empty())
            .map(|n| String::from_utf8_lossy(n).into_owned())
            .collect()
    }

    #[test]
    fn what_is_shown_where_it_is_stays_in_the_copy() {
        let (copy, outside) = (
            tempfile::TempDir::new().unwrap(),
            tempfile::TempDir::new().unwrap(),
        );
        let root = copy.path();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/main.rs"), "fn main() {}\n").unwrap();
        std::os::unix::fs::symlink(outside.path(), root.join("away")).unwrap();
        assert_eq!(located(root, "").unwrap(), root);
        assert_eq!(located(root, "src").unwrap(), root.join("src"));
        assert_eq!(
            located(root, "src/main.rs").unwrap(),
            root.join("src/main.rs")
        );
        for refused in ["away", "away/x", "../x", "/etc/hosts", "missing.txt"] {
            assert!(located(root, refused).is_err(), "{refused} was located");
        }
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
