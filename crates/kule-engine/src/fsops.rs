//! Filesystem moves shared by task creation, commit, rollback and undo.

use std::collections::{BTreeMap, BTreeSet};
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
    let start = clock.now()?;
    let Cloned {
        mut taken,
        mut dirs,
        racy,
    } = match clone_at_once(src, dst, start)? {
        Some(cloned) => cloned,
        None => clone_one_by_one(src, dst, start)?,
    };
    dirs.extend(pin_racy(src, dst, &mut taken, racy, clock)?);

    for (dir, stamp) in dirs.iter().rev() {
        finish(dir, stamp)?;
    }
    if let Some(root) = Stamp::of(src)? {
        finish(dst, &root)?;
    }
    Ok(taken)
}

/// Pins down the `racy` files of `taken`, cloned from `src` to `dst` within
/// the tick of `clock` their stamps were read in: each is compared with its
/// clone once the clock has moved on, and cloned again when they differ. One
/// that is never pinned down keeps a stamp that matches nothing. Returns the
/// directories that took a file's place meanwhile, whose times are set last.
pub(crate) fn pin_racy(
    src: &Path,
    dst: &Path,
    taken: &mut Tree,
    mut racy: Vec<String>,
    clock: &Clock,
) -> Result<Vec<(PathBuf, Stamp)>> {
    const ROUNDS: usize = 5;
    let mut dirs = Vec::new();
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
                // The source has moved past a stamp that changed; one that
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
    Ok(dirs)
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

/// Makes `rel` under `base` a file holding `bytes`, as [`base_one`] would
/// keep it: nothing there when it is larger than [`BASE_MAX`] or in a
/// directory tools rebuild.
pub(crate) fn base_put(base: &Path, rel: &str, bytes: &[u8]) -> Result<()> {
    let to = base.join(rel);
    remove(&to)?;
    let rebuilt = rel.split('/').any(|part| NOT_BASE.contains(&part));
    if rebuilt || bytes.len() as u64 > BASE_MAX {
        return Ok(());
    }
    if let Some(parent) = to.parent() {
        fs::create_dir_all(parent).at(parent)?;
    }
    fs::write(&to, bytes).at(&to)
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
///
/// Under `dst`, every directory on the way to a path is opened without
/// following a link, and each write, removal, mode and time goes through
/// the descriptor of the directory it is in: a link there (put in place
/// meanwhile, even while this runs, or a name that a case-insensitive disk
/// reads as another) takes nothing anywhere else, and is refused.
pub fn mirror(src: &Path, dst: &Path, paths: &[String], atomic: bool) -> Result<()> {
    let stamps = paths
        .iter()
        .map(|p| Ok((Stamp::of(&src.join(p))?, Stamp::of(&dst.join(p))?)))
        .collect::<Result<Vec<_>>>()?;
    let root = File::open(dst).at(dst)?;

    // 1. Clear what has to go, children before parents.
    for (p, (s, d)) in paths.iter().zip(&stamps).rev() {
        let Some(d) = d else { continue };
        let keep = match s {
            None => false,
            Some(s) if s.kind == Kind::Dir || d.kind == Kind::Dir => s.kind == d.kind,
            Some(_) => atomic,
        };
        if !keep {
            let (dir, name) = parent_beneath(&root, dst, p)?;
            let gone = dst.join(p);
            in_dir(&dir, &gone, || remove_tree_at(&dir, name).at(&gone))?;
        }
    }

    // 2. Bring in what `src` has, parents before children.
    for (p, (s, _)) in paths.iter().zip(&stamps) {
        let Some(s) = s else { continue };
        let (dir, name) = parent_beneath(&root, dst, p)?;
        let to = dst.join(p);
        in_dir(&dir, &to, || {
            if s.kind == Kind::Dir {
                if entry_at(&dir, name).at(&to)?.is_none() {
                    mkdir_at(&dir, name).at(&to)?;
                }
            } else if atomic {
                let tmp = temp_name();
                let placed = place_at(src, p, &dir, &tmp, s, &to)
                    .and_then(|()| rename_at(&dir, &tmp, name).at(&to));
                if placed.is_err() {
                    let _ = remove_at(&dir, &tmp);
                }
                placed?;
            } else {
                place_at(src, p, &dir, name, s, &to)?;
            }
            Ok(())
        })?;
    }

    // 3. Directory modes and times last: adding a child moves a directory's mtime.
    for (p, (s, _)) in paths.iter().zip(&stamps).rev() {
        if let Some(s) = s.as_ref().filter(|s| s.kind == Kind::Dir) {
            let (dir, name) = parent_beneath(&root, dst, p)?;
            let to = dst.join(p);
            if entry_at(&dir, name).at(&to)? != Some(Kind::Dir) {
                continue;
            }
            finish_at(&dir, name, s).at(&to)?;
        }
    }

    // 4. The same goes for the parents of what changed, which are not in `paths`.
    copy_parent_times(src, dst, paths)
}

/// Gives the parent directories of `paths` (the root included) under `dst`
/// the modification times they have under `src`, where both are
/// directories; under `dst`, through descriptors as [`mirror`] goes.
pub fn copy_parent_times(src: &Path, dst: &Path, paths: &[String]) -> Result<()> {
    let listed: BTreeSet<&str> = paths.iter().map(String::as_str).collect();
    let parents: BTreeSet<&str> = paths
        .iter()
        .filter_map(|p| Path::new(p).parent()?.to_str())
        .filter(|p| !listed.contains(p))
        .collect();
    let root = File::open(dst).at(dst)?;
    for parent in parents {
        let Some(s) = Stamp::of(&src.join(parent))?.filter(|s| s.kind == Kind::Dir) else {
            continue;
        };
        let to = dst.join(parent);
        if parent.is_empty() {
            set_mtime(&root, None, s.mtime).at(&to)?;
            continue;
        }
        let (dir, name) = parent_beneath(&root, dst, parent)?;
        if entry_at(&dir, name).at(&to)? == Some(Kind::Dir) {
            set_mtime(&dir, Some(name), s.mtime).at(&to)?;
        }
    }
    Ok(())
}

/// The directory `rel` is in under `dst` (open as `root`), opened one
/// component at a time without following a link, and `rel`'s own name in
/// it. A link on the way is refused.
fn parent_beneath<'a>(root: &File, dst: &Path, rel: &'a str) -> Result<(File, &'a str)> {
    use std::os::fd::{AsRawFd, FromRawFd};
    let (parent, name) = rel.rsplit_once('/').unwrap_or(("", rel));
    let mut dir = root.try_clone().at(dst)?;
    let mut at = dst.to_path_buf();
    for part in parent.split('/').filter(|p| !p.is_empty()) {
        at.push(part);
        let cname = plain_name(part).at(&at)?;
        let flags = libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC;
        // SAFETY: openat reads the name and returns a new descriptor or -1.
        let fd = unsafe { libc::openat(dir.as_raw_fd(), cname.as_ptr(), flags) };
        if fd == -1 {
            let e = io::Error::last_os_error();
            // Linux says ELOOP for a link, macOS ENOTDIR as for a file.
            if matches!(e.raw_os_error(), Some(libc::ELOOP | libc::ENOTDIR))
                && entry_at(&dir, part).ok().flatten() == Some(Kind::Symlink)
            {
                return Err(Error::Invalid(format!(
                    "{} is a link, which Kule does not write through",
                    at.display()
                )));
            }
            return Err(e).at(&at);
        }
        // SAFETY: fd was just opened and is owned by nothing else.
        dir = unsafe { File::from_raw_fd(fd) };
    }
    plain_name(name).at(&dst.join(rel))?;
    Ok((dir, name))
}

/// Runs `f`, which adds or removes an entry in `dir`, with `dir` writable
/// for the moment if it is not (a read-only module cache, a `chmod 555`
/// build directory), and its mode put back afterwards; `shown` names the
/// entry in errors.
fn in_dir<T>(dir: &File, shown: &Path, f: impl FnOnce() -> Result<T>) -> Result<T> {
    let mode = dir.metadata().at(shown)?.permissions().mode() & 0o7777;
    if mode & 0o200 != 0 {
        return f();
    }
    dir.set_permissions(fs::Permissions::from_mode(mode | 0o700))
        .at(shown)?;
    let done = f();
    dir.set_permissions(fs::Permissions::from_mode(mode))
        .at(shown)?;
    done
}

/// A name for a file on its way into a directory, renamed into place.
fn temp_name() -> String {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    format!(".kule-tmp-{}-{n}", std::process::id())
}

/// The folder that holds `rel` under the folder `root`, opened through
/// every folder on the way without following a link, and `rel`'s last name
/// in it: what a write through descriptors starts from.
pub(crate) fn open_parent<'a>(root: &Path, rel: &'a str) -> Result<(File, &'a str)> {
    let top = File::open(root).at(root)?;
    parent_beneath(&top, root, rel)
}

/// Removes the empty folder `name` in `dir`; one already gone is not an
/// error.
pub(crate) fn rmdir_at(dir: &File, name: &str) -> io::Result<()> {
    use std::os::fd::AsRawFd;
    let cname = plain_name(name)?;
    // SAFETY: a directory descriptor we own and a NUL-terminated name.
    if unsafe { libc::unlinkat(dir.as_raw_fd(), cname.as_ptr(), libc::AT_REMOVEDIR) } == -1 {
        let e = io::Error::last_os_error();
        if e.raw_os_error() != Some(libc::ENOENT) {
            return Err(e);
        }
    }
    Ok(())
}

/// Makes `name` in `dir` a link to `target`: made aside and renamed over
/// what is there (a file or a link), in one step.
pub(crate) fn link_at(dir: &File, name: &str, target: &Path) -> io::Result<()> {
    use std::os::fd::AsRawFd;
    use std::os::unix::ffi::OsStrExt;
    let to = std::ffi::CString::new(target.as_os_str().as_bytes())?;
    let tmp = temp_name();
    let ctmp = plain_name(&tmp)?;
    // SAFETY: NUL-terminated strings and a directory descriptor we own.
    if unsafe { libc::symlinkat(to.as_ptr(), dir.as_raw_fd(), ctmp.as_ptr()) } == -1 {
        return Err(io::Error::last_os_error());
    }
    let placed = rename_at(dir, &tmp, name);
    if placed.is_err() {
        let _ = unlink_at(dir, &ctmp);
    }
    placed
}

/// Makes the directory `name` in `dir`.
pub(crate) fn mkdir_at(dir: &File, name: &str) -> io::Result<()> {
    use std::os::fd::AsRawFd;
    let cname = plain_name(name)?;
    // SAFETY: a directory descriptor we own and a NUL-terminated name.
    if unsafe { libc::mkdirat(dir.as_raw_fd(), cname.as_ptr(), 0o777) } == -1 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// Makes `name` in `dir` what `stamp` describes, read from `rel` under
/// `root` without following a link at any step of its way: a file or link
/// put in place of what `stamp` describes, or a directory on the way
/// swapped for a link, is refused, so nothing from outside `root` can come
/// along. `to` names it in errors.
fn place_at(
    root: &Path,
    rel: &str,
    dir: &File,
    name: &str,
    stamp: &Stamp,
    to: &Path,
) -> Result<()> {
    use std::os::fd::AsRawFd;
    use std::os::unix::ffi::OsStrExt;
    let from = root.join(rel);
    let changed = || Error::Invalid(format!("{rel} changed while it was being copied"));
    let cname = plain_name(name).at(to)?;
    match (stamp.kind, beneath(root, rel).at(&from)?) {
        (Kind::File, Beneath::File(file)) => {
            copy_into(&file, dir.as_raw_fd(), &cname).at(to)?;
            finish_at(dir, name, stamp).at(to)
        }
        (Kind::Symlink, Beneath::Link(target)) => {
            let target = std::ffi::CString::new(target.as_os_str().as_bytes())
                .map_err(io::Error::other)
                .at(to)?;
            // SAFETY: two NUL-terminated strings and a directory descriptor we own.
            if unsafe { libc::symlinkat(target.as_ptr(), dir.as_raw_fd(), cname.as_ptr()) } == -1 {
                return Err(io::Error::last_os_error()).at(to);
            }
            Ok(())
        }
        (Kind::Dir, Beneath::Dir(_)) => mkdir_at(dir, name).at(to),
        _ => Err(changed()),
    }
}

/// [`finish`] for the entry `name` in `dir`, not following a link.
fn finish_at(dir: &File, name: &str, stamp: &Stamp) -> io::Result<()> {
    use std::os::fd::AsRawFd;
    set_mtime(dir, Some(name), stamp.mtime)?;
    let cname = plain_name(name)?;
    // mode_t is u16 on macOS, u32 on Linux.
    #[allow(clippy::useless_conversion)]
    let mode = libc::mode_t::try_from(stamp.mode & 0o7777).map_err(io::Error::other)?;
    // SAFETY: a directory descriptor we own and a NUL-terminated name.
    let done = unsafe {
        libc::fchmodat(
            dir.as_raw_fd(),
            cname.as_ptr(),
            mode,
            libc::AT_SYMLINK_NOFOLLOW,
        )
    };
    if done == -1 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// Sets the modification time of the entry `name` in `dir` (of `dir`
/// itself without one), leaving its access time, not following a link.
fn set_mtime(dir: &File, name: Option<&str>, mtime: (i64, i64)) -> io::Result<()> {
    use std::os::fd::AsRawFd;
    let times = [
        libc::timespec {
            tv_sec: 0,
            tv_nsec: libc::UTIME_OMIT,
        },
        libc::timespec {
            tv_sec: mtime.0 as libc::time_t,
            tv_nsec: mtime.1 as libc::c_long,
        },
    ];
    let done = match name {
        // SAFETY: a descriptor we own and two timespecs.
        None => unsafe { libc::futimens(dir.as_raw_fd(), times.as_ptr()) },
        Some(name) => {
            let cname = plain_name(name)?;
            // SAFETY: a directory descriptor we own, a NUL-terminated name
            // and two timespecs.
            unsafe {
                libc::utimensat(
                    dir.as_raw_fd(),
                    cname.as_ptr(),
                    times.as_ptr(),
                    libc::AT_SYMLINK_NOFOLLOW,
                )
            }
        }
    };
    if done == -1 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
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

/// A new file, `name` in the directory `dir`, with what `file` holds:
/// cloned where the filesystem allows it, copied otherwise; never one
/// already there, nor a link followed.
fn copy_into(file: &File, dir: std::os::fd::RawFd, name: &std::ffi::CStr) -> io::Result<()> {
    use std::os::fd::{AsRawFd, FromRawFd};
    #[cfg(target_os = "macos")]
    {
        // SAFETY: descriptors we own and a NUL-terminated name.
        if unsafe { libc::fclonefileat(file.as_raw_fd(), dir, name.as_ptr(), 0) } == 0 {
            return Ok(());
        }
    }
    let flags = libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC;
    // SAFETY: openat reads the name and returns a new descriptor or -1.
    let fd = unsafe { libc::openat(dir, name.as_ptr(), flags, 0o666 as libc::c_uint) };
    if fd == -1 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: fd was just opened and is owned by nothing else.
    let mut out = unsafe { File::from_raw_fd(fd) };
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
    /// With its permission bits.
    Dir(u32),
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
        libc::S_IFDIR => {
            // mode_t is u16 on macOS, u32 on Linux.
            #[allow(clippy::useless_conversion)]
            let mode = u32::from(st.st_mode) & 0o7777;
            Ok(Beneath::Dir(mode))
        }
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

/// What the entry `rel` under `root` holds, hashed: a file's bytes and
/// permission bits, a link's target, a directory's permission bits, read as
/// [`beneath`] finds it.
pub(crate) fn content_hash(root: &Path, rel: &str) -> Result<String> {
    content_hash_at_most(root, rel, HASH_MOST)
}

/// The largest file Kule hashes for a review: one past it is refused,
/// saying so, rather than read without end.
pub const HASH_MOST: u64 = 8 << 30;

fn content_hash_at_most(root: &Path, rel: &str, most: u64) -> Result<String> {
    use std::os::unix::ffi::OsStrExt;
    let at = root.join(rel);
    Ok(match beneath(root, rel).at(&at)? {
        Beneath::File(file) => {
            let mode = file.metadata().at(&at)?.permissions().mode();
            let mut hasher = blake3::Hasher::new();
            let read = io::copy(&mut file.take(most + 1), &mut hasher).at(&at)?;
            if read > most {
                return Err(Error::Invalid(format!(
                    "{rel} is larger than {}, more than Kule reads for a review",
                    size_shown(most)
                )));
            }
            file_hash(hasher.finalize(), mode)
        }
        Beneath::Link(target) => format!(
            "link:{}",
            blake3::hash(target.as_os_str().as_bytes()).to_hex()
        ),
        Beneath::Dir(mode) => format!("dir:{mode:o}"),
        Beneath::Missing => "missing".to_owned(),
    })
}

/// `bytes` as a person reads a size.
fn size_shown(bytes: u64) -> String {
    match bytes {
        b if b >= 1 << 30 => format!("{} GiB", b >> 30),
        b if b >= 1 << 20 => format!("{} MiB", b >> 20),
        b => format!("{b} bytes"),
    }
}

/// [`content_hash`] of a file whose bytes hash to `hash`, with `mode`.
pub(crate) fn file_hash(hash: blake3::Hash, mode: u32) -> String {
    format!("file:{}:{:o}", hash.to_hex(), mode & 0o7777)
}

/// The permission bits in a file's [`content_hash`]; `None` for anything
/// but a file.
pub(crate) fn hash_mode(hash: &str) -> Option<u32> {
    let (_, mode) = hash.strip_prefix("file:")?.rsplit_once(':')?;
    u32::from_str_radix(mode, 8).ok()
}

/// Permission bits that make a file run as its owner or group: a commit
/// never brings them into the folder where it had none.
pub(crate) const RUNS_AS_OWNER: u32 = 0o6000;

/// Extended attributes macOS sets on its own on what a sandboxed process
/// writes, saying where it came from, never what it holds: left out of
/// what a review compares, or every file the agent writes would be asked
/// about.
#[cfg(target_os = "macos")]
const SET_BY_THE_SYSTEM: &[&str] = &["com.apple.provenance"];

/// The most of a file's extended attributes read for a review, together.
#[cfg(target_os = "macos")]
const XATTRS_MAX: usize = 64 << 20;

/// The extended attributes of the file `rel` under `root`, as [`beneath`]
/// finds it: each name with a hash of its value, but those the system sets
/// on its own. Empty for anything but a file, and off macOS, where a commit
/// carries none into the folder.
pub(crate) fn xattrs(root: &Path, rel: &str) -> Result<BTreeMap<String, String>> {
    let at = root.join(rel);
    let Beneath::File(file) = beneath(root, rel).at(&at)? else {
        return Ok(BTreeMap::new());
    };
    file_xattrs(&file).at(&at)
}

#[cfg(target_os = "macos")]
fn file_xattrs(file: &File) -> io::Result<BTreeMap<String, String>> {
    use std::os::fd::AsRawFd;
    let fd = file.as_raw_fd();
    // SAFETY: the descriptor is open; a null buffer asks for the size.
    let size = unsafe { libc::flistxattr(fd, std::ptr::null_mut(), 0, 0) };
    if size < 0 {
        return Err(io::Error::last_os_error());
    }
    let mut names = vec![0u8; size as usize];
    // SAFETY: the descriptor is open and `names` valid for its length.
    let n = unsafe { libc::flistxattr(fd, names.as_mut_ptr().cast(), names.len(), 0) };
    if n < 0 {
        return Err(io::Error::last_os_error());
    }
    names.truncate(n as usize);
    let mut out = BTreeMap::new();
    let mut read = 0usize;
    for name in names.split(|b| *b == 0).filter(|n| !n.is_empty()) {
        let shown = String::from_utf8_lossy(name).into_owned();
        if SET_BY_THE_SYSTEM.contains(&shown.as_str()) {
            continue;
        }
        let cname = std::ffi::CString::new(name)?;
        // SAFETY: as above, with a NUL-terminated name.
        let len = unsafe { libc::fgetxattr(fd, cname.as_ptr(), std::ptr::null_mut(), 0, 0, 0) };
        if len < 0 {
            return Err(io::Error::last_os_error());
        }
        read = read.saturating_add(len as usize);
        if read > XATTRS_MAX {
            return Err(io::Error::other(format!(
                "its extended attributes take more than {} MiB, more than Kule reads for a review",
                XATTRS_MAX >> 20
            )));
        }
        let mut value = vec![0u8; len as usize];
        // SAFETY: as above, `value` valid for its length.
        let got = unsafe {
            libc::fgetxattr(
                fd,
                cname.as_ptr(),
                value.as_mut_ptr().cast(),
                value.len(),
                0,
                0,
            )
        };
        if got < 0 {
            return Err(io::Error::last_os_error());
        }
        value.truncate(got as usize);
        out.insert(shown, blake3::hash(&value).to_hex().to_string());
    }
    Ok(out)
}

#[cfg(not(target_os = "macos"))]
fn file_xattrs(_file: &File) -> io::Result<BTreeMap<String, String>> {
    Ok(BTreeMap::new())
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

/// A folder Kule keeps at `rel` under `root`, in a place a sandbox writes
/// to as well (the task's tmp): opened one component at a time without
/// following a link, made where it is missing, and made anew where a link
/// or a file stands in its place. Kule writes in it through the returned
/// descriptor, by name, so nothing the sandbox puts on the way, then or
/// later, leads a write elsewhere.
pub(crate) fn kept_dir(root: &Path, rel: &str) -> Result<File> {
    let shown = root.join(rel);
    let mut dir = File::open(root).at(root)?;
    for part in rel.split('/').filter(|p| !p.is_empty()) {
        dir = kept_child(&dir, part, &shown)?;
    }
    Ok(dir)
}

/// The folder `name` in `dir`, as [`kept_dir`] keeps it; `shown` names it
/// in errors.
pub(crate) fn kept_child(dir: &File, name: &str, shown: &Path) -> Result<File> {
    use std::os::fd::{AsRawFd, FromRawFd};
    let cname = plain_name(name).at(shown)?;
    for _ in 0..KEPT_TRIES {
        // SAFETY: a directory descriptor we own and a NUL-terminated name.
        if unsafe { libc::mkdirat(dir.as_raw_fd(), cname.as_ptr(), 0o755) } == -1 {
            let e = io::Error::last_os_error();
            if e.raw_os_error() != Some(libc::EEXIST) {
                return Err(e).at(shown);
            }
        }
        let flags = libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC;
        // SAFETY: openat reads the name and returns a new descriptor or -1.
        let fd = unsafe { libc::openat(dir.as_raw_fd(), cname.as_ptr(), flags) };
        if fd != -1 {
            // SAFETY: fd was just opened and is owned by nothing else.
            return Ok(unsafe { File::from_raw_fd(fd) });
        }
        let e = io::Error::last_os_error();
        match e.raw_os_error() {
            // A link or a file in its place: taken out, and made again.
            Some(libc::ELOOP | libc::ENOTDIR) => unlink_at(dir, &cname).at(shown)?,
            // Gone in between: made again.
            Some(libc::ENOENT) => {}
            _ => return Err(e).at(shown),
        }
    }
    Err(Error::Invalid(format!(
        "{} kept being replaced while Kule made it",
        shown.display()
    )))
}

/// Writes `bytes` as the file `name` in `dir`, with `mode`: a new file,
/// renamed over whatever is there, so a link at that name is replaced,
/// never written through, and a reader sees the old file or the new one.
/// `shown` names it in errors.
pub(crate) fn put_at(dir: &File, name: &str, bytes: &[u8], mode: u32, shown: &Path) -> Result<()> {
    use std::io::Write;
    use std::os::fd::{AsRawFd, FromRawFd};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let cname = plain_name(name).at(shown)?;
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let ctmp = plain_name(&format!(".kule-tmp-{}-{n}", std::process::id())).at(shown)?;
    let flags = libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC;
    // SAFETY: openat reads the name and returns a new descriptor or -1.
    let fd = unsafe { libc::openat(dir.as_raw_fd(), ctmp.as_ptr(), flags, 0o600 as libc::c_uint) };
    if fd == -1 {
        return Err(io::Error::last_os_error()).at(shown);
    }
    // SAFETY: fd was just opened and is owned by nothing else.
    let mut file = unsafe { File::from_raw_fd(fd) };
    let written = file
        .write_all(bytes)
        .and_then(|()| file.set_permissions(fs::Permissions::from_mode(mode)));
    drop(file);
    let placed = written.and_then(|()| {
        // SAFETY: both names are NUL-terminated and relative to `dir`.
        let done = unsafe {
            libc::renameat(
                dir.as_raw_fd(),
                ctmp.as_ptr(),
                dir.as_raw_fd(),
                cname.as_ptr(),
            )
        };
        if done == -1 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    });
    if placed.is_err() {
        let _ = unlink_at(dir, &ctmp);
    }
    placed.at(shown)
}

/// Writes `bytes` as the file `rel` under `root`, as [`put_at`] does, in a
/// folder kept as [`kept_dir`] keeps it.
pub(crate) fn put_beneath(root: &Path, rel: &str, bytes: &[u8], mode: u32) -> Result<()> {
    let (parent, name) = rel.rsplit_once('/').unwrap_or(("", rel));
    let dir = kept_dir(root, parent)?;
    put_at(&dir, name, bytes, mode, &root.join(rel))
}

/// The most entries [`carry`] copies.
const CARRY_ENTRIES: usize = 20_000;
/// The most bytes [`carry`] copies.
const CARRY_BYTES: u64 = 64 * 1024 * 1024;

/// Makes `rel` under `root` exactly what `from` is: a copy of the file,
/// folder or link there, or nothing where there is none. `root` is a place a
/// sandbox writes to (an agent's home of its own): the copy is made aside
/// and renamed into place through folders opened as [`kept_dir`] opens them,
/// so nothing put on the way leads it elsewhere, and whatever stood at `rel`
/// is replaced, a link never written through. Links in `from` are copied as
/// links. More than 20,000 entries or 64 MB is refused.
pub fn carry(from: &Path, root: &Path, rel: &str) -> Result<()> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let shown = root.join(rel);
    let (parent, name) = rel.rsplit_once('/').unwrap_or(("", rel));
    let dir = kept_dir(root, parent)?;
    let Some(stamp) = Stamp::of(from)? else {
        return remove_tree_at(&dir, name).at(&shown);
    };
    let (mut entries, mut bytes) = (0, 0);
    for entry in WalkDir::new(from).follow_links(false) {
        let entry = entry.map_err(|e| Error::Invalid(format!("{}: {e}", from.display())))?;
        entries += 1;
        if entry.file_type().is_file() {
            bytes += entry.metadata().map(|m| m.len()).unwrap_or(0);
        }
        if entries > CARRY_ENTRIES || bytes > CARRY_BYTES {
            return Err(Error::Invalid(format!(
                "{} is too large to copy (more than {CARRY_ENTRIES} entries or {} MB)",
                from.display(),
                CARRY_BYTES / 1024 / 1024
            )));
        }
    }
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let aside = format!(".kule-carry-{}-{n}", std::process::id());
    remove_tree_at(&dir, &aside).at(&shown)?;
    let made = copy_at(from, stamp.kind, &dir, &aside, &shown).and_then(|()| {
        // Something put at `name` in between is taken out again.
        for _ in 0..KEPT_TRIES {
            remove_tree_at(&dir, name).at(&shown)?;
            match rename_at(&dir, &aside, name) {
                Ok(()) => return Ok(()),
                Err(e)
                    if matches!(
                        e.raw_os_error(),
                        Some(libc::ENOTEMPTY | libc::EEXIST | libc::ENOTDIR | libc::EISDIR)
                    ) => {}
                Err(e) => return Err(e).at(&shown),
            }
        }
        Err(Error::Invalid(format!(
            "{} kept being replaced while Kule copied it",
            shown.display()
        )))
    });
    if made.is_err() {
        let _ = remove_tree_at(&dir, &aside);
    }
    made
}

/// Removes `rel` under `root`, a whole folder included, through folders
/// opened as [`kept_dir`] opens them: nothing a sandbox put on the way, or
/// in it, is followed.
pub fn remove_beneath(root: &Path, rel: &str) -> Result<()> {
    let (parent, name) = rel.rsplit_once('/').unwrap_or(("", rel));
    let dir = kept_dir(root, parent)?;
    remove_tree_at(&dir, name).at(&root.join(rel))
}

/// Copies `from`, which is a `kind`, to `name` in `dir`, nothing in `dir`
/// followed; `shown` names it in errors.
fn copy_at(from: &Path, kind: Kind, dir: &File, name: &str, shown: &Path) -> Result<()> {
    use std::os::fd::AsRawFd;
    match kind {
        Kind::Symlink => {
            let target = fs::read_link(from).at(from)?;
            let ctarget = std::ffi::CString::new(target.as_os_str().as_encoded_bytes())
                .map_err(|_| Error::Invalid(format!("{}: a link with a NUL", from.display())))?;
            let cname = plain_name(name).at(shown)?;
            // SAFETY: both strings are NUL-terminated; `dir` is a descriptor we own.
            if unsafe { libc::symlinkat(ctarget.as_ptr(), dir.as_raw_fd(), cname.as_ptr()) } == -1 {
                return Err(io::Error::last_os_error()).at(shown);
            }
            Ok(())
        }
        Kind::File => {
            let bytes = fs::read(from).at(from)?;
            let mode = fs::symlink_metadata(from).at(from)?.permissions().mode() & 0o777;
            put_at(dir, name, &bytes, mode, shown)
        }
        Kind::Dir => {
            let made = kept_child(dir, name, shown)?;
            let mut children: Vec<_> = fs::read_dir(from)
                .at(from)?
                .filter_map(|e| e.ok())
                .map(|e| e.file_name())
                .collect();
            children.sort();
            for child in children {
                let Some(child) = child.to_str() else {
                    continue;
                };
                let from = from.join(child);
                if let Some(stamp) = Stamp::of(&from)? {
                    copy_at(&from, stamp.kind, &made, child, &shown.join(child))?;
                }
            }
            Ok(())
        }
    }
}

/// How many times a kept folder is made again when something keeps taking
/// its place.
const KEPT_TRIES: usize = 8;

/// `name` for an `*at` call: one component, neither `.` nor `..`.
fn plain_name(name: &str) -> io::Result<std::ffi::CString> {
    use std::os::unix::ffi::OsStrExt;
    if matches!(name, "" | "." | "..") || name.contains('/') {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{name:?} is not a plain name"),
        ));
    }
    Ok(std::ffi::CString::new(
        std::ffi::OsStr::new(name).as_bytes(),
    )?)
}

/// Removes the entry `name` in `dir` that is not a folder; one already gone
/// is not an error.
fn unlink_at(dir: &File, name: &std::ffi::CStr) -> io::Result<()> {
    use std::os::fd::AsRawFd;
    // SAFETY: a directory descriptor we own and a NUL-terminated name.
    if unsafe { libc::unlinkat(dir.as_raw_fd(), name.as_ptr(), 0) } == -1 {
        let e = io::Error::last_os_error();
        if e.raw_os_error() != Some(libc::ENOENT) {
            return Err(e);
        }
    }
    Ok(())
}

/// What the entry `name` in `dir` is, not following a link; `None` when
/// there is none.
pub(crate) fn entry_at(dir: &File, name: &str) -> io::Result<Option<Kind>> {
    use std::os::fd::AsRawFd;
    let cname = plain_name(name)?;
    // SAFETY: an all-zero stat is valid, and fstatat fills it.
    let mut st: libc::stat = unsafe { std::mem::zeroed() };
    // SAFETY: a directory descriptor we own and a NUL-terminated name.
    let found = unsafe {
        libc::fstatat(
            dir.as_raw_fd(),
            cname.as_ptr(),
            &mut st,
            libc::AT_SYMLINK_NOFOLLOW,
        )
    };
    if found == -1 {
        let e = io::Error::last_os_error();
        return match e.raw_os_error() {
            Some(libc::ENOENT) => Ok(None),
            _ => Err(e),
        };
    }
    Ok(Some(match st.st_mode & libc::S_IFMT {
        libc::S_IFDIR => Kind::Dir,
        libc::S_IFLNK => Kind::Symlink,
        _ => Kind::File,
    }))
}

/// Removes the entry `name` in `dir` that is not a folder (a file, a link,
/// anything else); one already gone is not an error.
pub(crate) fn remove_at(dir: &File, name: &str) -> io::Result<()> {
    unlink_at(dir, &plain_name(name)?)
}

/// How deep [`remove_tree_at`] goes before it gives up.
const REMOVE_DEPTH: usize = 512;

/// Removes the entry `name` in `dir`, a whole folder included, through
/// descriptors only: nothing in it is followed, whatever a sandbox puts
/// there meanwhile. A folder deeper than Kule ever makes is refused.
pub(crate) fn remove_tree_at(dir: &File, name: &str) -> io::Result<()> {
    use std::os::fd::AsRawFd;
    /// A folder on the way down: its descriptor, the names left to remove
    /// in it, and its own name in the folder above.
    struct Frame {
        dir: File,
        left: Vec<std::ffi::CString>,
        name: std::ffi::CString,
    }
    let top = plain_name(name)?;
    let Some(first) = open_dir_at(dir, &top)? else {
        return Ok(());
    };
    let mut stack = vec![Frame {
        left: names_in(&first)?,
        dir: first,
        name: top,
    }];
    while let Some(frame) = stack.last_mut() {
        if let Some(next) = frame.left.pop() {
            if let Some(sub) = open_dir_at(&frame.dir, &next)? {
                if stack.len() >= REMOVE_DEPTH {
                    return Err(io::Error::other("a folder too deep to remove"));
                }
                stack.push(Frame {
                    left: names_in(&sub)?,
                    dir: sub,
                    name: next,
                });
            }
            continue;
        }
        let done = stack.pop().expect("looked at just now");
        let parent = stack.last().map_or(dir, |f| &f.dir);
        // SAFETY: a directory descriptor we own and a NUL-terminated name.
        let removed =
            unsafe { libc::unlinkat(parent.as_raw_fd(), done.name.as_ptr(), libc::AT_REMOVEDIR) };
        if removed == -1 {
            let e = io::Error::last_os_error();
            if e.raw_os_error() != Some(libc::ENOENT) {
                return Err(e);
            }
        }
    }
    Ok(())
}

/// The folder `name` in `dir`, opened without following a link; `None`
/// when there is none, or when the entry is not a folder, which is then
/// removed.
fn open_dir_at(dir: &File, name: &std::ffi::CStr) -> io::Result<Option<File>> {
    use std::os::fd::{AsRawFd, FromRawFd};
    let flags = libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC;
    // SAFETY: openat reads the name and returns a new descriptor or -1.
    let fd = unsafe { libc::openat(dir.as_raw_fd(), name.as_ptr(), flags) };
    if fd == -1 {
        let e = io::Error::last_os_error();
        return match e.raw_os_error() {
            Some(libc::ELOOP | libc::ENOTDIR) => unlink_at(dir, name).map(|()| None),
            Some(libc::ENOENT) => Ok(None),
            _ => Err(e),
        };
    }
    // SAFETY: fd was just opened and is owned by nothing else.
    Ok(Some(unsafe { File::from_raw_fd(fd) }))
}

/// The names in the folder `dir`, but `.` and `..`.
fn names_in(dir: &File) -> io::Result<Vec<std::ffi::CString>> {
    use std::os::fd::AsRawFd;
    // fdopendir takes the descriptor it is given: a duplicate of ours.
    // SAFETY: fcntl returns a new descriptor or -1.
    let fd = unsafe { libc::fcntl(dir.as_raw_fd(), libc::F_DUPFD_CLOEXEC, 0) };
    if fd == -1 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: fd is ours to give; on failure it is still ours to close.
    let stream = unsafe { libc::fdopendir(fd) };
    if stream.is_null() {
        let e = io::Error::last_os_error();
        // SAFETY: fdopendir did not take it.
        unsafe { libc::close(fd) };
        return Err(e);
    }
    // SAFETY: a stream fdopendir just made; read from its start.
    unsafe { libc::rewinddir(stream) };
    let mut names = Vec::new();
    loop {
        // SAFETY: the stream is open until closedir below.
        let entry = unsafe { libc::readdir(stream) };
        if entry.is_null() {
            break;
        }
        // SAFETY: the entry holds a NUL-terminated name until the next
        // call; it is copied out first.
        let name = unsafe { std::ffi::CStr::from_ptr((*entry).d_name.as_ptr()) };
        if !matches!(name.to_bytes(), b"." | b"..") {
            names.push(name.to_owned());
        }
    }
    // SAFETY: closes the stream and the descriptor it took.
    unsafe { libc::closedir(stream) };
    Ok(names)
}

/// Renames `from` to `to`, both in `dir`.
pub(crate) fn rename_at(dir: &File, from: &str, to: &str) -> io::Result<()> {
    use std::os::fd::AsRawFd;
    let (cfrom, cto) = (plain_name(from)?, plain_name(to)?);
    // SAFETY: both names are NUL-terminated and relative to `dir`.
    let done = unsafe {
        libc::renameat(
            dir.as_raw_fd(),
            cfrom.as_ptr(),
            dir.as_raw_fd(),
            cto.as_ptr(),
        )
    };
    if done == -1 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
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

/// A lock across Kule's processes (the daemon, the CLI) on the file at
/// `path`, made if missing and never followed if a link; held until
/// dropped, waited for while another holds it.
pub(crate) struct Flock {
    _held: File,
}

impl Flock {
    pub(crate) fn take(path: &Path) -> Result<Flock> {
        use std::os::fd::AsRawFd;
        use std::os::unix::fs::OpenOptionsExt;
        let file = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(path)
            .at(path)?;
        loop {
            // SAFETY: flock on a descriptor this function owns.
            if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) } == 0 {
                return Ok(Flock { _held: file });
            }
            let e = io::Error::last_os_error();
            if e.kind() != io::ErrorKind::Interrupted {
                return Err(e).at(path);
            }
        }
    }
}

#[cfg(test)]
mod tests {

    /// A write through descriptors never follows a link on the way: the
    /// folder that holds the path is opened one folder at a time.
    #[test]
    fn a_link_on_the_way_is_not_opened_through() {
        let (root, elsewhere) = (TempDir::new().unwrap(), TempDir::new().unwrap());
        std::fs::create_dir(root.path().join("real")).unwrap();
        std::os::unix::fs::symlink(elsewhere.path(), root.path().join("linked")).unwrap();
        let (dir, name) = open_parent(root.path(), "real/a").unwrap();
        assert_eq!(name, "a");
        put_at(&dir, name, b"x", 0o644, &root.path().join("real/a")).unwrap();
        assert!(root.path().join("real/a").exists());
        let e = open_parent(root.path(), "linked/a")
            .unwrap_err()
            .to_string();
        assert!(e.contains("is a link"), "{e}");
        let (dir, _) = open_parent(root.path(), "x").unwrap();
        link_at(&dir, "l", Path::new("real")).unwrap();
        assert_eq!(
            std::fs::read_link(root.path().join("l")).unwrap(),
            Path::new("real")
        );
        rmdir_at(&dir, "real").unwrap_err();
        std::fs::remove_file(root.path().join("real/a")).unwrap();
        rmdir_at(&dir, "real").unwrap();
        assert!(!root.path().join("real").exists());
        assert!(
            std::fs::read_dir(elsewhere.path())
                .unwrap()
                .next()
                .is_none()
        );
    }

    /// A file past the most Kule hashes for a review is refused, saying so.
    #[test]
    fn a_file_past_the_most_hashed_is_refused() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::write(dir.path().join("big"), vec![1u8; 2048]).unwrap();
        assert!(
            content_hash_at_most(dir.path(), "big", 2048)
                .unwrap()
                .starts_with("file:")
        );
        let e = content_hash_at_most(dir.path(), "big", 2047)
            .unwrap_err()
            .to_string();
        assert!(e.contains("big is larger than 2047 bytes"), "{e}");
    }

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

    /// A folder on the way swapped for a link while `mirror` runs, over and
    /// over, as a process left running could: nothing outside is written,
    /// changed or removed, whichever moment the swap lands in.
    #[test]
    fn a_folder_swapped_for_a_link_meanwhile_takes_nothing_elsewhere() {
        use std::sync::atomic::AtomicBool;
        let dir = TempDir::new().unwrap();
        let (src, dst, outside) = (
            dir.path().join("src"),
            dir.path().join("dst"),
            dir.path().join("outside"),
        );
        fs::create_dir_all(src.join("d/sub")).unwrap();
        fs::write(src.join("d/new.txt"), "ours\n").unwrap();
        fs::create_dir_all(dst.join("d")).unwrap();
        fs::create_dir_all(&outside).unwrap();
        for name in ["gone.txt", "new.txt"] {
            fs::write(outside.join(name), "theirs\n").unwrap();
        }
        fs::create_dir(outside.join("sub")).unwrap();
        fs::set_permissions(outside.join("sub"), fs::Permissions::from_mode(0o700)).unwrap();
        let theirs = || {
            let mut names: Vec<_> = fs::read_dir(&outside)
                .unwrap()
                .map(|e| e.unwrap().file_name())
                .collect();
            names.sort();
            let mode = fs::metadata(outside.join("sub"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777;
            (
                names,
                fs::read_to_string(outside.join("gone.txt")).ok(),
                fs::read_to_string(outside.join("new.txt")).ok(),
                mode,
            )
        };
        let before = theirs();

        let stop = std::sync::Arc::new(AtomicBool::new(false));
        // Held by the test while it puts `dst` right between runs, so only
        // `mirror` ever meets the link.
        let still = std::sync::Arc::new(std::sync::Mutex::new(()));
        let swapper = {
            let (dst, outside, stop, still) =
                (dst.clone(), outside.clone(), stop.clone(), still.clone());
            std::thread::spawn(move || {
                let (d, real) = (dst.join("d"), dst.join("d.real"));
                while !stop.load(Ordering::Relaxed) {
                    let _still = still.lock().unwrap();
                    if fs::rename(&d, &real).is_ok() {
                        let _ = symlink(&outside, &d);
                        std::thread::yield_now();
                        let _ = fs::remove_file(&d);
                        let _ = fs::rename(&real, &d);
                    }
                }
            })
        };
        let paths = [
            "d/gone.txt".to_owned(),
            "d/new.txt".to_owned(),
            "d/sub".to_owned(),
        ];
        let start = std::time::Instant::now();
        while start.elapsed() < std::time::Duration::from_secs(3) {
            for atomic in [true, false] {
                {
                    let _still = still.lock().unwrap();
                    fs::write(dst.join("d/gone.txt"), "copy\n").unwrap();
                    let _ = fs::remove_file(dst.join("d/new.txt"));
                    let _ = fs::remove_dir(dst.join("d/sub"));
                }
                let _ = mirror(&src, &dst, &paths, atomic);
            }
        }
        stop.store(true, Ordering::Relaxed);
        swapper.join().unwrap();
        assert_eq!(theirs(), before);
    }

    /// A mode or a time Kule gives an entry it put in place goes to that
    /// entry: a link standing there takes neither to what it points at.
    #[test]
    fn modes_and_times_never_follow_a_link() {
        let root = TempDir::new().unwrap();
        let outside = TempDir::new().unwrap();
        let victim = outside.path().join("victim");
        fs::write(&victim, "mine\n").unwrap();
        fs::set_permissions(&victim, fs::Permissions::from_mode(0o600)).unwrap();
        let before = fs::metadata(&victim).unwrap().modified().unwrap();
        symlink(&victim, root.path().join("entry")).unwrap();
        let dir = File::open(root.path()).unwrap();
        let mut stamp = Stamp::of(&victim).unwrap().unwrap();
        stamp.mode = 0o777;
        stamp.mtime = (1, 0);
        let _ = finish_at(&dir, "entry", &stamp);
        let _ = set_mtime(&dir, Some("entry"), (1, 0));
        let after = fs::metadata(&victim).unwrap();
        assert_eq!(after.permissions().mode() & 0o777, 0o600);
        assert_eq!(after.modified().unwrap(), before);
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

    /// What Kule keeps in a place a sandbox writes to: a link put in its
    /// place, or inside it, is replaced or removed, never followed.
    #[test]
    fn kept_folders_and_files_never_follow_a_link() {
        let root = TempDir::new().unwrap();
        let outside = TempDir::new().unwrap();
        let victim = outside.path().join("victim");
        fs::write(&victim, "mine\n").unwrap();
        symlink(outside.path(), root.path().join("dir")).unwrap();
        let dir = kept_dir(root.path(), "dir/sub").unwrap();
        assert!(
            fs::symlink_metadata(root.path().join("dir"))
                .unwrap()
                .is_dir()
        );
        symlink(&victim, root.path().join("dir/sub/file")).unwrap();
        put_at(&dir, "file", b"kule\n", 0o644, Path::new("file")).unwrap();
        assert_eq!(fs::read_to_string(&victim).unwrap(), "mine\n");
        assert_eq!(
            fs::read_to_string(root.path().join("dir/sub/file")).unwrap(),
            "kule\n"
        );
        assert_eq!(fs::read_dir(outside.path()).unwrap().count(), 1);
    }

    #[test]
    fn a_tree_is_removed_without_following_its_links() {
        let root = TempDir::new().unwrap();
        let outside = TempDir::new().unwrap();
        fs::write(outside.path().join("kept"), "mine\n").unwrap();
        let tree = root.path().join("tree");
        fs::create_dir_all(tree.join("a/b")).unwrap();
        fs::write(tree.join("a/b/f"), "x").unwrap();
        symlink(outside.path(), tree.join("a/out")).unwrap();
        symlink(outside.path().join("kept"), tree.join("file-link")).unwrap();
        let dir = File::open(root.path()).unwrap();
        remove_tree_at(&dir, "tree").unwrap();
        assert!(!tree.exists());
        assert_eq!(
            fs::read_to_string(outside.path().join("kept")).unwrap(),
            "mine\n"
        );
        // A link in place of the tree goes as a link.
        symlink(outside.path(), &tree).unwrap();
        remove_tree_at(&dir, "tree").unwrap();
        assert!(fs::symlink_metadata(&tree).is_err());
        assert!(outside.path().join("kept").exists());
        remove_tree_at(&dir, "gone").unwrap();

        // Deeper than a path can name: made one folder at a time, as a
        // command can.
        let mut deep = kept_dir(root.path(), "deep").unwrap();
        for _ in 0..REMOVE_DEPTH + 1 {
            deep = kept_child(&deep, "d", Path::new("d")).unwrap();
        }
        assert!(remove_tree_at(&dir, "deep").is_err());
    }
}
