//! Filesystem moves shared by task creation, commit, rollback and undo.

use std::collections::BTreeSet;
use std::fs::{self, File};
use std::io::{self, Read};
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use filetime::FileTime;
use serde::Serialize;
use serde::de::DeserializeOwned;
use walkdir::WalkDir;

use crate::clock::Clock;
use crate::error::{Error, IoContext, Result};
use crate::manifest::{Kind, Stamp, Tree, relative, scan, vanished};

/// Clones the tree under `src` into `dst`, which must not exist yet, and
/// returns the stamp each entry of `src` had when it was cloned. Files are
/// reflinked where the filesystem allows it and copied otherwise; modes and
/// modification times are kept so incremental builds see the same tree.
///
/// Other processes may write to `src` meanwhile (an IDE, git maintenance).
/// An entry is cloned again until its stamp is the same before and after,
/// and one that disappears is left out. The copy is then not one moment of
/// `src`, but every entry holds what its returned stamp describes, and that
/// is what conflict checks compare against later.
///
/// An entry that never holds still gets its last clone and the stamp from
/// before that clone. The workspace has moved past that stamp, so if the
/// task changes the entry, commit reports a conflict instead of
/// overwriting; if it does not, nothing happens to it.
///
/// A file written during the clone may be written again within the same
/// tick of `clock`, unseen by its stamp. Such files are compared with their
/// clone once the clock has moved on and cloned again when they differ; one
/// that is never pinned down gets a stamp that matches nothing.
pub(crate) fn clone_tree(src: &Path, dst: &Path, clock: &Clock) -> Result<Tree> {
    const ROUNDS: usize = 5;
    let start = clock.now()?;
    let Cloned {
        mut taken,
        mut dirs,
        mut racy,
    } = match clone_at_once(src, dst, start)? {
        Some(cloned) => cloned,
        None => clone_one_by_one(src, dst, start)?,
    };

    for round in 1..=ROUNDS {
        let Some(latest) = racy.iter().map(|rel| taken[rel].ctime).max() else {
            break;
        };
        let past = clock.wait_past(latest)?;
        let mut again = Vec::new();
        for rel in racy {
            let (from, to) = (src.join(&rel), dst.join(&rel));
            let stamp = &taken[&rel];
            let same = |path: &Path| Ok::<_, Error>(Stamp::of(path)?.as_ref() == Some(stamp));
            if past && same(&from)? && same_bytes(&from, &to)? && same(&from)? {
                continue;
            }
            if round == ROUNDS {
                // The workspace has moved past a stamp that changed; one that
                // did not may hide a write the clone missed.
                if same(&from)? {
                    let unknown = stamp.clone().unknown();
                    taken.insert(rel, unknown);
                }
                continue;
            }
            remove(&to)?;
            match clone_entry(&from, &to)? {
                Some(stamp) => {
                    match stamp.kind {
                        Kind::Dir => dirs.push((to, stamp.clone())),
                        Kind::File => again.push(rel.clone()),
                        _ => {}
                    }
                    taken.insert(rel, stamp);
                }
                None => {
                    taken.remove(&rel);
                }
            }
        }
        racy = again;
    }

    for (dir, stamp) in dirs.iter().rev() {
        finish(dir, stamp)?;
    }
    if let Some(root) = Stamp::of(src)? {
        finish(dst, &root)?;
    }
    Ok(taken)
}

/// What a first pass of [`clone_tree`] made: the stamp of each entry it
/// took, the directories whose times are set last, and the files written
/// so recently that a write in the same tick could hide behind their stamp.
#[derive(Default)]
struct Cloned {
    taken: Tree,
    dirs: Vec<(PathBuf, Stamp)>,
    racy: Vec<String>,
}

impl Cloned {
    fn take(&mut self, rel: String, to: PathBuf, stamp: Stamp, start: (i64, i64)) {
        match stamp.kind {
            Kind::Dir => self.dirs.push((to, stamp.clone())),
            Kind::File if stamp.ctime >= start => self.racy.push(rel.clone()),
            _ => {}
        }
        self.taken.insert(rel, stamp);
    }
}

/// Clones each entry on its own, parents first.
fn clone_one_by_one(src: &Path, dst: &Path, start: (i64, i64)) -> Result<Cloned> {
    fs::create_dir(dst).at(dst)?;
    let mut cloned = Cloned::default();
    for entry in WalkDir::new(src)
        .min_depth(1)
        .follow_links(false)
        .sort_by_file_name()
    {
        let entry = match entry {
            Ok(entry) => entry,
            Err(e) if vanished(&e) => continue,
            Err(e) => {
                return Err(Error::Io {
                    path: e.path().unwrap_or(src).to_path_buf(),
                    source: e.into(),
                });
            }
        };
        let rel = relative(src, entry.path())?;
        let to = dst.join(&rel);
        if let Some(stamp) = clone_entry(entry.path(), &to)? {
            cloned.take(rel, to, stamp, start);
        }
    }
    Ok(cloned)
}

/// Clones the whole tree in one call where the system can (a directory
/// `clonefile` on APFS: a fraction of a second where one call per entry
/// takes most of a minute), then makes it what [`clone_one_by_one`] would
/// have: an entry whose stamp was the same before and after the call was
/// cloned as that stamp says; any other is cloned again on its own, or
/// removed when it is gone. `None` when the call is not possible here.
fn clone_at_once(src: &Path, dst: &Path, start: (i64, i64)) -> Result<Option<Cloned>> {
    let before = scan(src)?;
    if !clone_dir_at_once(src, dst) {
        // Nothing may be left of a call that failed half way.
        remove(dst)?;
        return Ok(None);
    }
    reconcile(src, dst, &before, start).map(Some)
}

/// Makes `dst`, cloned from `src` while `src` was `before`, what cloning
/// each entry on its own would have made.
fn reconcile(src: &Path, dst: &Path, before: &Tree, start: (i64, i64)) -> Result<Cloned> {
    let after = scan(src)?;
    let copied = scan(dst)?;
    let mut paths: Vec<&String> = before
        .keys()
        .chain(after.keys())
        .chain(copied.keys())
        .collect();
    // Sorted, a parent comes before what is in it.
    paths.sort();
    paths.dedup();
    let mut cloned = Cloned::default();
    for rel in paths {
        let (from, to) = (src.join(rel), dst.join(rel));
        let held = match (before.get(rel), after.get(rel), copied.get(rel)) {
            (Some(b), Some(a), Some(c)) if b.unchanged(a) && b.kind == c.kind => Some(b.clone()),
            _ => None,
        };
        let stamp = match held {
            Some(stamp) => stamp,
            None => {
                // Changed, come or gone while the call ran: on its own.
                match after.get(rel) {
                    None => {
                        remove(&to)?;
                        continue;
                    }
                    Some(a)
                        if a.kind == Kind::Dir
                            && copied.get(rel).is_some_and(|c| c.kind == Kind::Dir) =>
                    {
                        // What is in it is reconciled entry by entry.
                        match Stamp::of(&from)? {
                            Some(stamp) => stamp,
                            None => continue,
                        }
                    }
                    Some(_) => {
                        remove(&to)?;
                        match clone_entry(&from, &to)? {
                            Some(stamp) => stamp,
                            None => continue,
                        }
                    }
                }
            }
        };
        cloned.take(rel.clone(), to, stamp, start);
    }
    Ok(cloned)
}

/// Files kept as a task's base no larger than this: what Kule merges.
pub(crate) const BASE_MAX: u64 = 1 << 20;

/// Directories never kept as a base: what tools rebuild, nobody merges.
const NOT_BASE: &[&str] = &[
    ".git",
    "node_modules",
    "target",
    ".venv",
    "venv",
    "__pycache__",
    ".next",
    ".gradle",
    ".cache",
];

/// Keeps `copy`, as it is right after it was made, as the task's base at
/// `base`, which must not exist: the text the task and the folder both
/// start from, for a later merge. In one clone where the system can (it
/// takes no space until files change); otherwise the files up to
/// [`BASE_MAX`], outside directories tools rebuild, one by one.
pub(crate) fn keep_base(copy: &Path, base: &Path) -> Result<()> {
    if clone_dir_at_once(copy, base) {
        return Ok(());
    }
    remove(base)?;
    fs::create_dir(base).at(base)?;
    let walk = WalkDir::new(copy)
        .min_depth(1)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| {
            !(e.file_type().is_dir()
                && e.file_name()
                    .to_str()
                    .is_some_and(|n| NOT_BASE.contains(&n)))
        });
    for entry in walk.flatten() {
        let rel = relative(copy, entry.path())?;
        base_one(copy, base, &rel)?;
    }
    Ok(())
}

/// Makes `rel` under `base` what it is under `copy`: a directory, a file up
/// to [`BASE_MAX`], or nothing.
pub(crate) fn base_one(copy: &Path, base: &Path, rel: &str) -> Result<()> {
    let (from, to) = (copy.join(rel), base.join(rel));
    let rebuilt = rel.split('/').any(|part| NOT_BASE.contains(&part));
    let meta = fs::symlink_metadata(&from).ok().filter(|_| !rebuilt);
    match meta {
        Some(m) if m.is_dir() => {
            if !fs::symlink_metadata(&to).is_ok_and(|t| t.is_dir()) {
                remove(&to)?;
                fs::create_dir_all(&to).at(&to)?;
            }
        }
        Some(m) if m.is_file() && m.len() <= BASE_MAX => {
            remove(&to)?;
            if let Some(parent) = to.parent() {
                fs::create_dir_all(parent).at(parent)?;
            }
            reflink_copy::reflink_or_copy(&from, &to).at(&from)?;
        }
        _ => remove(&to)?,
    }
    Ok(())
}

/// Clones directory `src` to `dst`, which must not exist, in one call.
#[cfg(target_os = "macos")]
fn clone_dir_at_once(src: &Path, dst: &Path) -> bool {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    let c = |p: &Path| CString::new(p.as_os_str().as_bytes()).ok();
    let (Some(from), Some(to)) = (c(src), c(dst)) else {
        return false;
    };
    // From <sys/clonefile.h>: links inside are cloned as links, not followed.
    const CLONE_NOFOLLOW: u32 = 0x0001;
    // SAFETY: both are valid NUL-terminated paths for the call's duration.
    unsafe { libc::clonefile(from.as_ptr(), to.as_ptr(), CLONE_NOFOLLOW) == 0 }
}

/// Linux has no call that clones a directory.
#[cfg(not(target_os = "macos"))]
fn clone_dir_at_once(_src: &Path, _dst: &Path) -> bool {
    false
}

/// Clones one entry to `to`, which must not exist, until its stamp is the
/// same before and after, and returns that stamp; for an entry that never
/// holds still, the stamp from before its last clone. `None` when the entry
/// disappeared.
pub(crate) fn clone_entry(from: &Path, to: &Path) -> Result<Option<Stamp>> {
    const ATTEMPTS: usize = 5;
    for attempt in 1..=ATTEMPTS {
        let Some(before) = Stamp::of(from)? else {
            return Ok(None);
        };
        let cloned = match before.kind {
            Kind::Dir => fs::create_dir(to).at(to),
            _ => place(from, to, &before),
        };
        // Copying reports a vanished source differently per platform
        // (`fs::copy` on Linux says the source is not a regular file),
        // so ask the filesystem rather than the error.
        if let Err(e) = cloned {
            if is_vanished(&e) || Stamp::of(from)?.is_none() {
                remove(to)?;
                return Ok(None);
            }
            return Err(e);
        }
        match Stamp::of(from)? {
            Some(after) if before.unchanged(&after) => return Ok(Some(before)),
            Some(_) if attempt == ATTEMPTS => return Ok(Some(before)),
            _ => remove(to)?,
        }
    }
    Ok(None)
}

fn is_vanished(e: &Error) -> bool {
    matches!(e, Error::Io { source, .. }
        if matches!(source.kind(), io::ErrorKind::NotFound | io::ErrorKind::NotADirectory))
}

/// Makes each of `paths` under `dst` what it is under `src`; a path missing
/// under `src` is removed from `dst`. `paths` must be sorted so that parents
/// come before their children, and their parents must exist under `dst`.
/// With `atomic`, files and symlinks arrive through a rename, so nobody
/// reading `dst` sees half a file.
pub fn mirror(src: &Path, dst: &Path, paths: &[String], atomic: bool) -> Result<()> {
    // Under `dst`, every directory on the way to a path is one: a link there
    // (put in place meanwhile, or a name that a case-insensitive disk reads
    // as another) would take the write, or the removal, somewhere else.
    let on_the_way = |p: &str| -> Result<()> {
        let mut at = dst.to_path_buf();
        for part in Path::new(p).parent().into_iter().flat_map(Path::components) {
            at.push(part);
            match fs::symlink_metadata(&at) {
                Ok(m) if m.file_type().is_symlink() => {
                    return Err(Error::Invalid(format!(
                        "{} is a link, which a commit does not write through",
                        at.display()
                    )));
                }
                _ => {}
            }
        }
        Ok(())
    };
    let stamps = paths
        .iter()
        .map(|p| Ok((Stamp::of(&src.join(p))?, Stamp::of(&dst.join(p))?)))
        .collect::<Result<Vec<_>>>()?;

    // 1. Clear what has to go, children before parents.
    for (p, (s, d)) in paths.iter().zip(&stamps).rev() {
        let Some(d) = d else { continue };
        let keep = match s {
            None => false,
            Some(s) if s.kind == Kind::Dir || d.kind == Kind::Dir => s.kind == d.kind,
            Some(_) => atomic,
        };
        if !keep {
            on_the_way(p)?;
            let gone = dst.join(p);
            in_parent(&gone, || remove(&gone))?;
        }
    }

    // 2. Bring in what `src` has, parents before children.
    for (p, (s, _)) in paths.iter().zip(&stamps) {
        let Some(s) = s else { continue };
        on_the_way(p)?;
        let to = dst.join(p);
        in_parent(&to, || {
            if s.kind == Kind::Dir {
                if Stamp::of(&to)?.is_none() {
                    fs::create_dir(&to).at(&to)?;
                }
            } else if atomic {
                let tmp = temp_beside(&to);
                let placed =
                    place_beneath(src, p, &tmp, s).and_then(|()| fs::rename(&tmp, &to).at(&to));
                if placed.is_err() {
                    let _ = fs::remove_file(&tmp);
                }
                placed?;
            } else {
                place_beneath(src, p, &to, s)?;
            }
            Ok(())
        })?;
    }

    // 3. Directory modes and times last: adding a child moves a directory's mtime.
    for (p, (s, _)) in paths.iter().zip(&stamps).rev() {
        if let Some(s) = s.as_ref().filter(|s| s.kind == Kind::Dir) {
            on_the_way(p)?;
            if fs::symlink_metadata(dst.join(p)).is_ok_and(|m| m.file_type().is_symlink()) {
                continue;
            }
            finish(&dst.join(p), s)?;
        }
    }

    // 4. The same goes for the parents of what changed, which are not in `paths`.
    copy_parent_times(src, dst, paths)
}

/// Gives the parent directories of `paths` (the root included) under `dst`
/// the modification times they have under `src`, where both are directories.
pub fn copy_parent_times(src: &Path, dst: &Path, paths: &[String]) -> Result<()> {
    let listed: BTreeSet<&str> = paths.iter().map(String::as_str).collect();
    let parents: BTreeSet<&str> = paths
        .iter()
        .filter_map(|p| Path::new(p).parent()?.to_str())
        .filter(|p| !listed.contains(p))
        .collect();
    for parent in parents {
        let (from, to) = (src.join(parent), dst.join(parent));
        if let (Some(s), Some(d)) = (Stamp::of(&from)?, Stamp::of(&to)?)
            && s.kind == Kind::Dir
            && d.kind == Kind::Dir
        {
            let mtime = FileTime::from_unix_time(s.mtime.0, s.mtime.1 as u32);
            filetime::set_file_mtime(&to, mtime).at(&to)?;
        }
    }
    Ok(())
}

/// Runs `f`, which adds or removes an entry in `path`'s directory, with that
/// directory writable for the moment if it is not (a read-only module cache,
/// a `chmod 555` build directory), and its mode put back afterwards.
fn in_parent<T>(path: &Path, f: impl FnOnce() -> Result<T>) -> Result<T> {
    let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) else {
        return f();
    };
    let mode = match fs::symlink_metadata(parent) {
        Ok(meta) if meta.is_dir() => meta.permissions().mode() & 0o7777,
        _ => return f(),
    };
    if mode & 0o200 != 0 {
        return f();
    }
    fs::set_permissions(parent, fs::Permissions::from_mode(mode | 0o700)).at(parent)?;
    let done = f();
    fs::set_permissions(parent, fs::Permissions::from_mode(mode)).at(parent)?;
    done
}

/// Puts the file or symlink described by `stamp` (read from `from`) at `to`,
/// which must not exist.
fn place(from: &Path, to: &Path, stamp: &Stamp) -> Result<()> {
    match stamp.kind {
        Kind::File => {
            reflink_copy::reflink_or_copy(from, to).at(from)?;
            finish(to, stamp)
        }
        Kind::Symlink => symlink(stamp.link.as_deref().unwrap_or_default(), to).at(to),
        Kind::Dir => fs::create_dir(to).at(to),
    }
}

/// [`place`] for the entry `rel` under `root`, read without following a link
/// at any step of its way: a file or link put in place of what `stamp`
/// describes, or a directory on the way swapped for a link, is refused, so
/// nothing from outside `root` can come along.
fn place_beneath(root: &Path, rel: &str, to: &Path, stamp: &Stamp) -> Result<()> {
    let from = root.join(rel);
    let changed = || Error::Invalid(format!("{rel} changed while it was being copied"));
    match (stamp.kind, beneath(root, rel).at(&from)?) {
        (Kind::File, Beneath::File(file)) => {
            copy_from(&file, to).at(to)?;
            finish(to, stamp)
        }
        (Kind::Symlink, Beneath::Link(target)) => symlink(target, to).at(to),
        (Kind::Dir, Beneath::Dir) => fs::create_dir(to).at(to),
        _ => Err(changed()),
    }
}

/// A new file at `to` with what `file` holds: cloned where the filesystem
/// allows it, copied otherwise.
fn copy_from(file: &File, to: &Path) -> io::Result<()> {
    use std::os::fd::AsRawFd;
    #[cfg(target_os = "macos")]
    {
        use std::os::unix::ffi::OsStrExt;
        let name = std::ffi::CString::new(to.as_os_str().as_bytes())?;
        // SAFETY: a descriptor we own and a NUL-terminated path.
        if unsafe { libc::fclonefileat(file.as_raw_fd(), libc::AT_FDCWD, name.as_ptr(), 0) } == 0 {
            return Ok(());
        }
    }
    let mut out = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(to)?;
    #[cfg(target_os = "linux")]
    {
        // SAFETY: two descriptors we own; FICLONE shares the source's extents.
        if unsafe { libc::ioctl(out.as_raw_fd(), libc::FICLONE, file.as_raw_fd()) } == 0 {
            return Ok(());
        }
    }
    let mut from = file;
    io::copy(&mut from, &mut out)?;
    Ok(())
}

/// What an entry is, found under a root without following a link at any
/// step of its path; a link on the way, or anything but a file, link or
/// directory, is `Missing`.
pub(crate) enum Beneath {
    File(File),
    Link(PathBuf),
    Dir,
    Missing,
}

pub(crate) fn beneath(root: &Path, rel: &str) -> io::Result<Beneath> {
    use std::os::fd::{AsRawFd, FromRawFd};
    use std::os::unix::ffi::{OsStrExt, OsStringExt};
    let (parent, name) = match rel.rsplit_once('/') {
        Some((parent, name)) => (Some(parent), name),
        None => (None, rel),
    };
    if matches!(name, "" | "." | "..") {
        return Ok(Beneath::Missing);
    }
    let dir = match parent {
        None => File::open(root)?,
        Some(parent) => match crate::outline::open_beneath(root, parent)? {
            Some(dir) => dir,
            None => return Ok(Beneath::Missing),
        },
    };
    if !dir.metadata()?.is_dir() {
        return Ok(Beneath::Missing);
    }
    let cname = std::ffi::CString::new(std::ffi::OsStr::new(name).as_bytes())?;
    let gone = |e: io::Error| match e.raw_os_error() {
        Some(libc::ENOENT | libc::ENOTDIR | libc::ELOOP | libc::ENXIO) => Ok(Beneath::Missing),
        _ => Err(e),
    };
    // SAFETY: an all-zero stat is valid, and fstatat fills it.
    let mut st: libc::stat = unsafe { std::mem::zeroed() };
    // SAFETY: a directory descriptor we own and a NUL-terminated name.
    if unsafe {
        libc::fstatat(
            dir.as_raw_fd(),
            cname.as_ptr(),
            &mut st,
            libc::AT_SYMLINK_NOFOLLOW,
        )
    } == -1
    {
        return gone(io::Error::last_os_error());
    }
    match st.st_mode & libc::S_IFMT {
        libc::S_IFDIR => Ok(Beneath::Dir),
        libc::S_IFLNK => {
            let mut buf = vec![0u8; libc::PATH_MAX as usize + 1];
            // SAFETY: reads at most buf.len() bytes into buf.
            let n = unsafe {
                libc::readlinkat(
                    dir.as_raw_fd(),
                    cname.as_ptr(),
                    buf.as_mut_ptr().cast(),
                    buf.len(),
                )
            };
            if n < 0 {
                return gone(io::Error::last_os_error());
            }
            buf.truncate(n as usize);
            Ok(Beneath::Link(PathBuf::from(std::ffi::OsString::from_vec(
                buf,
            ))))
        }
        libc::S_IFREG => {
            let flags = libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK;
            // SAFETY: openat reads the name and returns a new descriptor or -1.
            let fd = unsafe { libc::openat(dir.as_raw_fd(), cname.as_ptr(), flags) };
            if fd == -1 {
                return gone(io::Error::last_os_error());
            }
            // SAFETY: fd was just opened and is owned by nothing else.
            let file = unsafe { File::from_raw_fd(fd) };
            Ok(if file.metadata()?.is_file() {
                Beneath::File(file)
            } else {
                Beneath::Missing
            })
        }
        _ => Ok(Beneath::Missing),
    }
}

/// What the entry `rel` under `root` holds, hashed: a file's bytes or a
/// link's target, read as [`beneath`] finds it.
pub(crate) fn content_hash(root: &Path, rel: &str) -> Result<String> {
    use std::os::unix::ffi::OsStrExt;
    let at = root.join(rel);
    Ok(match beneath(root, rel).at(&at)? {
        Beneath::File(mut file) => {
            let mut hasher = blake3::Hasher::new();
            io::copy(&mut file, &mut hasher).at(&at)?;
            format!("file:{}", hasher.finalize().to_hex())
        }
        Beneath::Link(target) => format!(
            "link:{}",
            blake3::hash(target.as_os_str().as_bytes()).to_hex()
        ),
        Beneath::Dir => "dir".to_owned(),
        Beneath::Missing => "missing".to_owned(),
    })
}

fn finish(path: &Path, stamp: &Stamp) -> Result<()> {
    let mtime = FileTime::from_unix_time(stamp.mtime.0, stamp.mtime.1 as u32);
    filetime::set_file_mtime(path, mtime).at(path)?;
    fs::set_permissions(path, fs::Permissions::from_mode(stamp.mode)).at(path)
}

fn temp_beside(path: &Path) -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    path.with_file_name(format!(".kule-tmp-{}-{n}", std::process::id()))
}

/// Writes `bytes` to the file at `path` through a rename, keeping its mode;
/// its modification time moves, as for any write.
pub(crate) fn replace_file(path: &Path, bytes: &[u8]) -> Result<()> {
    let mode = fs::symlink_metadata(path).at(path)?.permissions().mode();
    let tmp = temp_beside(path);
    fs::write(&tmp, bytes).at(&tmp)?;
    fs::set_permissions(&tmp, fs::Permissions::from_mode(mode)).at(&tmp)?;
    fs::rename(&tmp, path).at(path)
}

/// Removes a file, symlink or whole directory; a missing path is not an error.
pub fn remove(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        // Under a file, nothing is there either.
        Err(e)
            if matches!(
                e.kind(),
                io::ErrorKind::NotFound | io::ErrorKind::NotADirectory
            ) =>
        {
            Ok(())
        }
        Err(e) => Err(e).at(path),
        Ok(meta) if meta.is_dir() => remove_dir_tree(path),
        Ok(_) => fs::remove_file(path).at(path),
    }
}

/// `remove_dir_all` that also gets through read-only directories.
pub fn remove_dir_tree(path: &Path) -> Result<()> {
    match fs::remove_dir_all(path) {
        Err(e) if e.kind() == io::ErrorKind::PermissionDenied => {
            for entry in WalkDir::new(path).follow_links(false).into_iter().flatten() {
                if entry.file_type().is_dir() {
                    let _ = fs::set_permissions(entry.path(), fs::Permissions::from_mode(0o700));
                }
            }
            fs::remove_dir_all(path).at(path)
        }
        other => other.at(path),
    }
}

/// Whether two files hold the same bytes. A file that disappeared in the
/// meantime does not hold them.
pub fn same_bytes(a: &Path, b: &Path) -> Result<bool> {
    let open = |path: &Path| match File::open(path) {
        Ok(file) => Ok(Some(file)),
        Err(e)
            if matches!(
                e.kind(),
                io::ErrorKind::NotFound | io::ErrorKind::NotADirectory
            ) =>
        {
            Ok(None)
        }
        Err(e) => Err(e).at(path),
    };
    let (Some(mut fa), Some(mut fb)) = (open(a)?, open(b)?) else {
        return Ok(false);
    };
    let (mut ba, mut bb) = (vec![0u8; 64 * 1024], vec![0u8; 64 * 1024]);
    loop {
        let na = read_full(&mut fa, &mut ba).at(a)?;
        let nb = read_full(&mut fb, &mut bb).at(b)?;
        if ba[..na] != bb[..nb] {
            return Ok(false);
        }
        if na == 0 {
            return Ok(true);
        }
    }
}

fn read_full(file: &mut File, buf: &mut [u8]) -> io::Result<usize> {
    let mut n = 0;
    while n < buf.len() {
        match file.read(&mut buf[n..])? {
            0 => break,
            k => n += k,
        }
    }
    Ok(n)
}

pub fn read_json<T: DeserializeOwned>(path: &Path) -> Result<T> {
    let text = fs::read(path).at(path)?;
    serde_json::from_slice(&text).map_err(|source| Error::State {
        path: path.to_path_buf(),
        source,
    })
}

/// Writes through a rename, so a crash leaves the old file or the new one.
pub fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let text = serde_json::to_vec_pretty(value).map_err(|source| Error::State {
        path: path.to_path_buf(),
        source,
    })?;
    let tmp = temp_beside(path);
    fs::write(&tmp, text).at(&tmp)?;
    fs::rename(&tmp, path).at(path)
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;

    /// A link on the way to a path under `dst` takes neither a write nor a
    /// removal anywhere else.
    #[test]
    fn nothing_is_written_or_removed_through_a_link_on_the_way() {
        let dir = TempDir::new().unwrap();
        let (src, dst, outside) = (
            dir.path().join("src"),
            dir.path().join("dst"),
            dir.path().join("outside"),
        );
        fs::create_dir_all(src.join("docs")).unwrap();
        fs::write(src.join("docs/new.txt"), "ours\n").unwrap();
        fs::create_dir_all(&dst).unwrap();
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("new.txt"), "theirs\n").unwrap();
        fs::write(outside.join("gone.txt"), "theirs\n").unwrap();
        std::os::unix::fs::symlink(&outside, dst.join("docs")).unwrap();

        let paths = ["docs/new.txt".to_owned(), "docs/gone.txt".to_owned()];
        assert!(mirror(&src, &dst, &paths, true).is_err());
        assert!(mirror(&src, &dst, &paths, false).is_err());
        assert_eq!(
            fs::read_to_string(outside.join("new.txt")).unwrap(),
            "theirs\n"
        );
        assert_eq!(
            fs::read_to_string(outside.join("gone.txt")).unwrap(),
            "theirs\n"
        );
    }

    #[test]
    fn a_tree_cloned_at_once_is_put_right_where_it_changed_meanwhile() {
        let dir = TempDir::new().unwrap();
        let (src, dst) = (dir.path().join("src"), dir.path().join("dst"));
        fs::create_dir_all(src.join("a")).unwrap();
        for name in ["a/same", "a/changed", "a/gone"] {
            fs::write(src.join(name), name).unwrap();
        }
        let before = scan(&src).unwrap();
        // The clone holds the tree as it was; then the tree moves on.
        clone_one_by_one(&src, &dst, (0, 0)).unwrap();
        fs::write(src.join("a/changed"), "changed again").unwrap();
        fs::remove_file(src.join("a/gone")).unwrap();
        fs::write(src.join("a/new"), "new").unwrap();

        let cloned = reconcile(&src, &dst, &before, (0, 0)).unwrap();
        let read = |rel: &str| fs::read_to_string(dst.join(rel)).ok();
        assert_eq!(read("a/same").as_deref(), Some("a/same"));
        assert_eq!(read("a/changed").as_deref(), Some("changed again"));
        assert_eq!(read("a/gone"), None);
        assert_eq!(read("a/new").as_deref(), Some("new"));
        // Each entry holds what its stamp says.
        for rel in ["a/same", "a/changed", "a/new"] {
            assert_eq!(
                Some(&cloned.taken[rel]),
                Stamp::of(&src.join(rel)).unwrap().as_ref(),
                "{rel}"
            );
        }
        assert!(!cloned.taken.contains_key("a/gone"));
    }
}
