//! The descriptor-relative calls [`super`] builds on, on Unix: `openat`
//! and its family, each with `O_NOFOLLOW`, so a link put on the way is
//! refused rather than followed.

use std::ffi::{CStr, CString};
use std::fs::{self, File};
use std::io;
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use crate::manifest::Kind;

/// One component of a path as an `*at` call takes it.
pub(crate) type Name = CString;

/// `name` for an `*at` call: one component, neither `.` nor `..`.
pub(crate) fn plain_name(name: &str) -> io::Result<Name> {
    if matches!(name, "" | "." | "..") || name.contains('/') {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{name:?} is not a plain name"),
        ));
    }
    Ok(CString::new(std::ffi::OsStr::new(name).as_bytes())?)
}

/// `name` as text, for messages.
pub(crate) fn name_text(name: &Name) -> String {
    String::from_utf8_lossy(name.to_bytes()).into_owned()
}

/// Whether `e` says the entry is a link, or something else than the folder
/// or file the call asked for. Linux says `ELOOP` for a link opened with
/// `O_NOFOLLOW`, macOS `ENOTDIR` as for a file.
pub(crate) fn is_link_or_other_kind(e: &io::Error) -> bool {
    matches!(e.raw_os_error(), Some(libc::ELOOP | libc::ENOTDIR))
}

pub(crate) fn is_missing(e: &io::Error) -> bool {
    e.raw_os_error() == Some(libc::ENOENT)
}

pub(crate) fn is_exists(e: &io::Error) -> bool {
    e.raw_os_error() == Some(libc::EEXIST)
}

/// Whether `e` says a rename found something in the way at its target.
pub(crate) fn is_in_the_way(e: &io::Error) -> bool {
    matches!(
        e.raw_os_error(),
        Some(libc::ENOTEMPTY | libc::EEXIST | libc::ENOTDIR | libc::EISDIR)
    )
}

/// The folder `name` in `dir`, opened without following a link.
pub(crate) fn open_dir_at(dir: &File, name: &Name) -> io::Result<File> {
    let flags = libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC;
    // SAFETY: openat reads the name and returns a new descriptor or -1.
    let fd = unsafe { libc::openat(dir.as_raw_fd(), name.as_ptr(), flags) };
    if fd == -1 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: fd was just opened and is owned by nothing else.
    Ok(unsafe { File::from_raw_fd(fd) })
}

/// The regular file `name` in `dir`, opened for reading without following
/// a link, and without blocking on a FIFO put in its place; `None` for a
/// link or anything else than a regular file.
pub(crate) fn open_file_at(dir: &File, name: &Name) -> io::Result<Option<File>> {
    let flags = libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK;
    // SAFETY: openat reads the name and returns a new descriptor or -1.
    let fd = unsafe { libc::openat(dir.as_raw_fd(), name.as_ptr(), flags) };
    if fd == -1 {
        let e = io::Error::last_os_error();
        return match e.raw_os_error() {
            Some(libc::ELOOP | libc::ENXIO) => Ok(None),
            _ => Err(e),
        };
    }
    // SAFETY: fd was just opened and is owned by nothing else.
    let file = unsafe { File::from_raw_fd(fd) };
    Ok(file.metadata()?.is_file().then_some(file))
}

/// The entry `name` in `dir`, whatever it is but a link, opened for
/// reading without following a link and without blocking on a FIFO; the
/// caller asks the open file what it is. `None` for a link.
pub(crate) fn open_any_at(dir: &File, name: &Name) -> io::Result<Option<File>> {
    let flags = libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK;
    // SAFETY: openat reads the name and returns a new descriptor or -1.
    let fd = unsafe { libc::openat(dir.as_raw_fd(), name.as_ptr(), flags) };
    if fd == -1 {
        let e = io::Error::last_os_error();
        return match e.raw_os_error() {
            Some(libc::ELOOP | libc::ENXIO) => Ok(None),
            _ => Err(e),
        };
    }
    // SAFETY: fd was just opened and is owned by nothing else.
    Ok(Some(unsafe { File::from_raw_fd(fd) }))
}

/// What the entry `name` in `dir` is, not following a link, with a
/// directory's permission bits; `None` when there is none.
pub(crate) fn stat_at(dir: &File, name: &Name) -> io::Result<Option<(Kind, u32)>> {
    // SAFETY: an all-zero stat is valid, and fstatat fills it.
    let mut st: libc::stat = unsafe { std::mem::zeroed() };
    // SAFETY: a directory descriptor we own and a NUL-terminated name.
    let found = unsafe {
        libc::fstatat(
            dir.as_raw_fd(),
            name.as_ptr(),
            &mut st,
            libc::AT_SYMLINK_NOFOLLOW,
        )
    };
    if found == -1 {
        let e = io::Error::last_os_error();
        return match e.raw_os_error() {
            Some(libc::ENOENT | libc::ENOTDIR) => Ok(None),
            _ => Err(e),
        };
    }
    let kind = match st.st_mode & libc::S_IFMT {
        libc::S_IFDIR => Kind::Dir,
        libc::S_IFLNK => Kind::Symlink,
        _ => Kind::File,
    };
    // mode_t is u16 on macOS, u32 on Linux.
    #[allow(clippy::useless_conversion)]
    let mode = u32::from(st.st_mode) & 0o7777;
    Ok(Some((kind, mode)))
}

/// What the entry `name` in `dir` is and how large, not following a link;
/// `None` when there is none, or it is something Kule does not track.
pub(crate) fn entry_facts(dir: &File, name: &Name) -> io::Result<Option<(Kind, u64)>> {
    // SAFETY: an all-zero stat is valid, and fstatat fills it.
    let mut st: libc::stat = unsafe { std::mem::zeroed() };
    // SAFETY: a directory descriptor we own and a NUL-terminated name.
    let found = unsafe {
        libc::fstatat(
            dir.as_raw_fd(),
            name.as_ptr(),
            &mut st,
            libc::AT_SYMLINK_NOFOLLOW,
        )
    };
    if found == -1 {
        let e = io::Error::last_os_error();
        return match e.raw_os_error() {
            Some(libc::ENOENT | libc::ENOTDIR) => Ok(None),
            _ => Err(e),
        };
    }
    let kind = match st.st_mode & libc::S_IFMT {
        libc::S_IFDIR => Kind::Dir,
        libc::S_IFLNK => Kind::Symlink,
        libc::S_IFREG => Kind::File,
        _ => return Ok(None),
    };
    Ok(Some((kind, st.st_size as u64)))
}

/// Whether the entry `name` in `dir` is a file, a link or a directory;
/// `None` for a socket, a FIFO or a device, which Kule does not track.
pub(crate) fn is_tracked_kind(dir: &File, name: &Name) -> io::Result<bool> {
    // SAFETY: an all-zero stat is valid, and fstatat fills it.
    let mut st: libc::stat = unsafe { std::mem::zeroed() };
    // SAFETY: a directory descriptor we own and a NUL-terminated name.
    let found = unsafe {
        libc::fstatat(
            dir.as_raw_fd(),
            name.as_ptr(),
            &mut st,
            libc::AT_SYMLINK_NOFOLLOW,
        )
    };
    if found == -1 {
        return Err(io::Error::last_os_error());
    }
    let kind = st.st_mode & libc::S_IFMT;
    Ok([libc::S_IFDIR, libc::S_IFLNK, libc::S_IFREG].contains(&kind))
}

/// Where the link `name` in `dir` points.
pub(crate) fn readlink_at(dir: &File, name: &Name) -> io::Result<PathBuf> {
    let mut buf = vec![0u8; libc::PATH_MAX as usize + 1];
    // SAFETY: reads at most buf.len() bytes into buf.
    let n = unsafe {
        libc::readlinkat(
            dir.as_raw_fd(),
            name.as_ptr(),
            buf.as_mut_ptr().cast(),
            buf.len(),
        )
    };
    if n < 0 {
        return Err(io::Error::last_os_error());
    }
    buf.truncate(n as usize);
    Ok(PathBuf::from(std::ffi::OsString::from_vec(buf)))
}

/// Makes the directory `name` in `dir`, with `mode` before the umask.
pub(crate) fn mkdir_at(dir: &File, name: &Name, mode: u32) -> io::Result<()> {
    let mode = libc::mode_t::try_from(mode).map_err(io::Error::other)?;
    // SAFETY: a directory descriptor we own and a NUL-terminated name.
    if unsafe { libc::mkdirat(dir.as_raw_fd(), name.as_ptr(), mode) } == -1 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// Makes `name` in `dir` a link to `target`.
pub(crate) fn symlink_at(dir: &File, name: &Name, target: &Path) -> io::Result<()> {
    let to = CString::new(target.as_os_str().as_bytes())?;
    // SAFETY: NUL-terminated strings and a directory descriptor we own.
    if unsafe { libc::symlinkat(to.as_ptr(), dir.as_raw_fd(), name.as_ptr()) } == -1 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// Removes the entry `name` in `dir` that is not a folder; one already gone
/// is not an error.
pub(crate) fn unlink_at(dir: &File, name: &Name) -> io::Result<()> {
    // SAFETY: a directory descriptor we own and a NUL-terminated name.
    if unsafe { libc::unlinkat(dir.as_raw_fd(), name.as_ptr(), 0) } == -1 {
        let e = io::Error::last_os_error();
        if !is_missing(&e) {
            return Err(e);
        }
    }
    Ok(())
}

/// Removes the empty folder `name` in `dir`; one already gone is not an
/// error.
pub(crate) fn rmdir_at(dir: &File, name: &Name) -> io::Result<()> {
    // SAFETY: a directory descriptor we own and a NUL-terminated name.
    if unsafe { libc::unlinkat(dir.as_raw_fd(), name.as_ptr(), libc::AT_REMOVEDIR) } == -1 {
        let e = io::Error::last_os_error();
        if !is_missing(&e) {
            return Err(e);
        }
    }
    Ok(())
}

/// Renames `from` to `to`, both in `dir`, over a file or link at `to`.
pub(crate) fn rename_at(dir: &File, from: &Name, to: &Name) -> io::Result<()> {
    // SAFETY: both names are NUL-terminated and relative to `dir`.
    let done =
        unsafe { libc::renameat(dir.as_raw_fd(), from.as_ptr(), dir.as_raw_fd(), to.as_ptr()) };
    if done == -1 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// A new file `name` in `dir`, open for writing, with `mode` before the
/// umask: never one already there, nor a link followed.
pub(crate) fn create_new_at(dir: &File, name: &Name, mode: u32) -> io::Result<File> {
    let flags = libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC;
    // SAFETY: openat reads the name and returns a new descriptor or -1.
    let fd = unsafe { libc::openat(dir.as_raw_fd(), name.as_ptr(), flags, mode as libc::c_uint) };
    if fd == -1 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: fd was just opened and is owned by nothing else.
    Ok(unsafe { File::from_raw_fd(fd) })
}

/// A new file, `name` in the directory `dir`, with what `file` holds:
/// cloned where the filesystem allows it, copied otherwise; never one
/// already there, nor a link followed.
pub(crate) fn copy_into(file: &File, dir: &File, name: &Name) -> io::Result<()> {
    #[cfg(target_os = "macos")]
    {
        // SAFETY: descriptors we own and a NUL-terminated name.
        if unsafe { libc::fclonefileat(file.as_raw_fd(), dir.as_raw_fd(), name.as_ptr(), 0) } == 0 {
            return Ok(());
        }
    }
    let mut out = create_new_at(dir, name, 0o666)?;
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

/// Gives the entry `name` in `dir` the permission bits `mode`, not
/// following a link.
pub(crate) fn set_mode_at(dir: &File, name: &Name, mode: u32) -> io::Result<()> {
    // mode_t is u16 on macOS, u32 on Linux.
    #[allow(clippy::useless_conversion)]
    let mode = libc::mode_t::try_from(mode & 0o7777).map_err(io::Error::other)?;
    // SAFETY: a directory descriptor we own and a NUL-terminated name.
    let done = unsafe {
        libc::fchmodat(
            dir.as_raw_fd(),
            name.as_ptr(),
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
pub(crate) fn set_mtime_at(dir: &File, name: Option<&Name>, mtime: (i64, i64)) -> io::Result<()> {
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
        // SAFETY: a directory descriptor we own, a NUL-terminated name
        // and two timespecs.
        Some(name) => unsafe {
            libc::utimensat(
                dir.as_raw_fd(),
                name.as_ptr(),
                times.as_ptr(),
                libc::AT_SYMLINK_NOFOLLOW,
            )
        },
    };
    if done == -1 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// The names in the folder `dir`, but `.` and `..`.
pub(crate) fn names_in(dir: &File) -> io::Result<Vec<Name>> {
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
        let name = unsafe { CStr::from_ptr((*entry).d_name.as_ptr()) };
        if !matches!(name.to_bytes(), b"." | b"..") {
            names.push(name.to_owned());
        }
    }
    // SAFETY: closes the stream and the descriptor it took.
    unsafe { libc::closedir(stream) };
    Ok(names)
}

/// Makes `dir` writable for the moment if it is not (a read-only module
/// cache, a `chmod 555` build directory); returns what to give
/// [`restore_writable`] afterwards, `None` when nothing was changed.
pub(crate) fn make_writable(dir: &File) -> io::Result<Option<u32>> {
    let mode = dir.metadata()?.permissions().mode() & 0o7777;
    if mode & 0o200 != 0 {
        return Ok(None);
    }
    dir.set_permissions(fs::Permissions::from_mode(mode | 0o700))?;
    Ok(Some(mode))
}

pub(crate) fn restore_writable(dir: &File, mode: u32) -> io::Result<()> {
    dir.set_permissions(fs::Permissions::from_mode(mode))
}

/// Clones directory `src` to `dst`, which must not exist, in one call.
#[cfg(target_os = "macos")]
pub(crate) fn clone_dir_at_once(src: &Path, dst: &Path) -> bool {
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
pub(crate) fn clone_dir_at_once(_src: &Path, _dst: &Path) -> bool {
    false
}

/// The file a lock across Kule's processes is taken on: made if missing,
/// private to the user, never followed if a link.
pub(crate) fn open_lock_file(path: &Path) -> io::Result<File> {
    fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
}

/// Takes `file`'s lock, which goes with the file or its process (flock):
/// waiting for it when `wait`, otherwise `false` when another holds it.
pub(crate) fn lock(file: &File, wait: bool) -> io::Result<bool> {
    let how = if wait {
        libc::LOCK_EX
    } else {
        libc::LOCK_EX | libc::LOCK_NB
    };
    loop {
        // SAFETY: flock on a descriptor the caller owns.
        if unsafe { libc::flock(file.as_raw_fd(), how) } == 0 {
            return Ok(true);
        }
        let e = io::Error::last_os_error();
        match e.raw_os_error() {
            Some(libc::EINTR) => continue,
            Some(libc::EWOULDBLOCK) if !wait => return Ok(false),
            _ => return Err(e),
        }
    }
}
