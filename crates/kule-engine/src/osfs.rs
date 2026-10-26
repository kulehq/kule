//! What Kule and its crates ask of the operating system's filesystem and
//! processes, under one set of names on Unix and Windows: permission bits, links, private
//! files, whether a process is alive. Where Windows has no counterpart,
//! the function says what it does instead; nothing here weakens a check
//! silently.

use std::fs::{self, File, Metadata};
use std::io;
use std::path::Path;

/// The folder at `path`, open as a handle for calls relative to it. A
/// link in `path` is followed, as `File::open` follows it; the folders
/// beneath are then opened without following any.
pub fn open_dir(path: &Path) -> io::Result<File> {
    #[cfg(unix)]
    {
        File::open(path)
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows::Win32::Storage::FileSystem::FILE_FLAG_BACKUP_SEMANTICS;
        fs::OpenOptions::new()
            .read(true)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS.0)
            .open(path)
    }
}

/// The permission bits of `meta`. On Windows, where files carry no mode,
/// a stand-in from what Windows keeps: `0o644` for a file and `0o755` for
/// a folder, without the write bits when the read-only attribute is set,
/// `0o777` for a link.
pub fn mode_of(meta: &Metadata) -> u32 {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        meta.permissions().mode() & 0o7777
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        synthesized_mode(meta.file_attributes())
    }
}

/// [`mode_of`] from Windows file attributes.
#[cfg(windows)]
pub(crate) fn synthesized_mode(attributes: u32) -> u32 {
    use windows::Win32::Storage::FileSystem::{
        FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_READONLY, FILE_ATTRIBUTE_REPARSE_POINT,
    };
    let has = |flag: windows::Win32::Storage::FileSystem::FILE_FLAGS_AND_ATTRIBUTES| {
        attributes & flag.0 != 0
    };
    if has(FILE_ATTRIBUTE_REPARSE_POINT) {
        0o777
    } else if has(FILE_ATTRIBUTE_DIRECTORY) {
        if has(FILE_ATTRIBUTE_READONLY) {
            0o555
        } else {
            0o755
        }
    } else if has(FILE_ATTRIBUTE_READONLY) {
        0o444
    } else {
        0o644
    }
}

/// Gives the entry at `path` the permission bits `mode`. On Windows the
/// only bit kept is whether the owner may write: a file without it gets
/// the read-only attribute; a folder is left as it is, since Windows does
/// not stop writes into a read-only folder. Follows no link on Windows.
pub fn set_mode(path: &Path, mode: u32) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(mode))
    }
    #[cfg(windows)]
    {
        let file = win::open_attributes(path, true)?;
        set_file_mode(&file, mode)
    }
}

/// [`set_mode`] on an open file.
pub fn set_file_mode(file: &File, mode: u32) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(fs::Permissions::from_mode(mode))
    }
    #[cfg(windows)]
    {
        win::apply_mode(file, mode)
    }
}

/// Sets the modification time of the entry at `path`, a read-only file
/// included, leaving its other times.
pub fn set_mtime(path: &Path, mtime: (i64, i64)) -> io::Result<()> {
    #[cfg(unix)]
    {
        let time = filetime::FileTime::from_unix_time(mtime.0, mtime.1 as u32);
        filetime::set_file_mtime(path, time)
    }
    #[cfg(windows)]
    {
        use windows::Win32::Storage::FileSystem::FILE_BASIC_INFO;
        let file = win::open_attributes(path, true)?;
        let info = FILE_BASIC_INFO {
            CreationTime: 0,
            LastAccessTime: 0,
            LastWriteTime: win::file_time(mtime),
            ChangeTime: 0,
            FileAttributes: 0,
        };
        win::set_basic_info(&file, &info)
    }
}

/// Makes `link` a symbolic link to `target`. On Windows a link needs
/// Developer Mode or a privilege; without them the error says so, and
/// the caller decides what to do without the link.
pub fn symlink(target: &Path, link: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target, link)
    }
    #[cfg(windows)]
    {
        let resolved = if target.is_absolute() {
            target.to_path_buf()
        } else {
            link.parent().unwrap_or(Path::new(".")).join(target)
        };
        // A folder gets a junction, which needs no privilege and is read
        // back as a link to the same place; a file gets a symbolic link,
        // which Windows allows with Developer Mode or a privilege.
        if fs::metadata(&resolved).is_ok_and(|m| m.is_dir()) {
            let absolute = fs::canonicalize(&resolved).unwrap_or(resolved);
            return win::junction(&absolute, link);
        }
        std::os::windows::fs::symlink_file(target, link).map_err(win::explain_link_privilege)
    }
}

/// The regular file at `path`, open for reading, no link followed and no
/// FIFO blocking the read; `None` when nothing is there, or a link, or
/// anything but a regular file.
pub fn open_regular(path: &Path) -> io::Result<Option<File>> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        let file = match fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(path)
        {
            Ok(f) => f,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(e) if matches!(e.raw_os_error(), Some(libc::ELOOP | libc::ENXIO)) => {
                return Ok(None);
            }
            Err(e) => return Err(e),
        };
        Ok(file.metadata()?.is_file().then_some(file))
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT;
        let file = match fs::OpenOptions::new()
            .read(true)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT.0)
            .open(path)
        {
            Ok(f) => f,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e),
        };
        let meta = file.metadata()?;
        Ok((meta.is_file() && !win::is_reparse(&meta)).then_some(file))
    }
}

/// A new file at `path` that only its owner can read or write, open for
/// writing; one already there, or a link in its place, is refused.
pub fn create_private(path: &Path) -> io::Result<File> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(path)
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows::Win32::Storage::FileSystem::{FILE_FLAG_OPEN_REPARSE_POINT, WRITE_DAC};
        let file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .access_mode(win::GENERIC_WRITE | WRITE_DAC.0)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT.0)
            .open(path)?;
        win::make_private(&file, false)?;
        Ok(file)
    }
}

/// Makes the folder `path`, which must not exist, readable and writable by
/// its owner only.
pub fn create_private_dir(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        fs::DirBuilder::new().mode(0o700).create(path)
    }
    #[cfg(windows)]
    {
        fs::create_dir(path)?;
        let dir = win::open_attributes_for_dacl(path)?;
        win::make_private(&dir, true)
    }
}

/// Makes the folder `path` and the folders on the way to it, each private
/// to its owner; one already there is left as it is.
pub fn create_private_dir_all(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(path)
    }
    #[cfg(windows)]
    {
        let mut made = Vec::new();
        for ancestor in path.ancestors() {
            if ancestor.as_os_str().is_empty() || fs::symlink_metadata(ancestor).is_ok() {
                break;
            }
            made.push(ancestor.to_path_buf());
        }
        fs::create_dir_all(path)?;
        for dir in made {
            make_private(&dir)?;
        }
        Ok(())
    }
}

/// The process group of `pid`, where the system has them (Unix); `None`
/// on Windows, where a process is ended by its handle, not its group.
pub fn process_group(pid: i32) -> Option<i32> {
    #[cfg(unix)]
    {
        // SAFETY: getpgid only reads.
        let group = unsafe { libc::getpgid(pid) };
        (group != -1).then_some(group)
    }
    #[cfg(windows)]
    {
        let _ = pid;
        None
    }
}

/// What counting disk use needs of the entry `meta` describes at `path`:
/// how many names it has, its device and identity (to count a file linked
/// from several places once) and the bytes allocated to it.
pub fn disk_facts(path: &Path, meta: &Metadata) -> Option<(u64, u64, u64, u64)> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let _ = path;
        Some((
            meta.nlink(),
            meta.dev(),
            meta.ino(),
            meta.blocks().saturating_mul(512),
        ))
    }
    #[cfg(windows)]
    {
        if meta.is_dir() {
            return Some((1, 0, 0, 0));
        }
        let file = win::open_attributes(path, false).ok()?;
        let standard = win::standard_info(&file).ok()?;
        let (volume, id) = win::identity(&file).ok()?;
        Some((
            u64::from(standard.NumberOfLinks),
            volume,
            u64::from_le_bytes(id[..8].try_into().ok()?),
            standard.AllocationSize.max(0) as u64,
        ))
    }
}

/// Makes what is at `path` (a file or a folder, no link followed) readable
/// and writable by its owner only, whatever it allowed before.
pub fn make_private(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let meta = fs::symlink_metadata(path)?;
        let mode = if meta.is_dir() { 0o700 } else { 0o600 };
        fs::set_permissions(path, fs::Permissions::from_mode(mode))
    }
    #[cfg(windows)]
    {
        let handle = win::open_attributes_for_dacl(path)?;
        let is_dir = handle.metadata()?.is_dir();
        win::make_private(&handle, is_dir)
    }
}

/// The device (Unix) or volume (Windows) that holds `path`, by number.
pub fn device_of(path: &Path) -> io::Result<u64> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        Ok(fs::metadata(path)?.dev())
    }
    #[cfg(windows)]
    {
        let file = win::open_attributes(path, false)?;
        Ok(win::identity(&file)?.0)
    }
}

/// When the open entry's content or attributes last changed, as the
/// filesystem keeps it: what a stamp's change time holds.
pub fn change_time(file: &File) -> io::Result<(i64, i64)> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let meta = file.metadata()?;
        Ok((meta.ctime(), meta.ctime_nsec()))
    }
    #[cfg(windows)]
    {
        Ok(win::unix_time(win::basic_info(file)?.ChangeTime))
    }
}

/// How many names the open file has (hard links).
pub fn links_of(file: &File) -> io::Result<u64> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        Ok(file.metadata()?.nlink())
    }
    #[cfg(windows)]
    {
        Ok(u64::from(win::standard_info(file)?.NumberOfLinks))
    }
}

/// What tells one version of the file at `path` from the next: its device
/// or volume, its identity and its modification time; `None` when nothing
/// is there. No link is followed.
pub fn mark_of(path: &Path) -> Option<(u64, u64, i64, i64)> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let m = fs::symlink_metadata(path).ok()?;
        Some((m.dev(), m.ino(), m.mtime(), m.mtime_nsec()))
    }
    #[cfg(windows)]
    {
        let file = win::open_attributes(path, false).ok()?;
        let (volume, id) = win::identity(&file).ok()?;
        let mtime = win::unix_time(win::basic_info(&file).ok()?.LastWriteTime);
        Some((
            volume,
            u64::from_le_bytes(id[..8].try_into().ok()?),
            mtime.0,
            mtime.1,
        ))
    }
}

/// The file at `path`, open for appending and made if missing, that only
/// its owner can read.
pub fn append_private(path: &Path) -> io::Result<File> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        fs::OpenOptions::new()
            .create(true)
            .append(true)
            .mode(0o600)
            .open(path)
    }
    #[cfg(windows)]
    {
        let file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)?;
        make_private(path)?;
        Ok(file)
    }
}

/// Whether `e` says a rename or link crossed from one filesystem to
/// another, where it cannot go.
pub fn is_cross_device(e: &io::Error) -> bool {
    #[cfg(unix)]
    {
        e.raw_os_error() == Some(libc::EXDEV)
    }
    #[cfg(windows)]
    {
        const ERROR_NOT_SAME_DEVICE: i32 = 17;
        e.raw_os_error() == Some(ERROR_NOT_SAME_DEVICE)
    }
}

/// Whether `path` is a file a command can start: one with an execute bit
/// on Unix, one whose extension `PATHEXT` lists on Windows.
pub fn executable_at(path: &Path) -> bool {
    let Ok(meta) = fs::metadata(path) else {
        return false;
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        meta.is_file() && meta.permissions().mode() & 0o111 != 0
    }
    #[cfg(windows)]
    {
        meta.is_file()
            && path
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| path_extensions().iter().any(|x| x.eq_ignore_ascii_case(e)))
    }
}

/// The names a bare program name may stand for on disk: itself, and on
/// Windows itself with each `PATHEXT` extension when it has none.
pub fn executable_candidates(path: &Path) -> Vec<std::path::PathBuf> {
    #[allow(unused_mut)]
    let mut out = vec![path.to_path_buf()];
    #[cfg(windows)]
    {
        if path.extension().is_none() {
            for ext in path_extensions() {
                let mut with = path.as_os_str().to_owned();
                with.push(".");
                with.push(&ext);
                out.push(std::path::PathBuf::from(with));
            }
        }
    }
    out
}

/// The extensions Windows runs as programs, from `PATHEXT`, without dots.
#[cfg(windows)]
fn path_extensions() -> Vec<String> {
    std::env::var("PATHEXT")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| ".COM;.EXE;.BAT;.CMD".to_owned())
        .split(';')
        .map(|e| e.trim().trim_start_matches('.').to_owned())
        .filter(|e| !e.is_empty())
        .collect()
}

/// What tells one version of an entry from the next (see [`version_mark`]):
/// size, device, identity, modification time, change time.
pub type VersionMark = (u64, u64, u64, (i64, i64), (i64, i64));

/// What tells one version of the entry at `path` from the next, no link
/// followed: its size, device, identity, modification and change times.
pub fn version_mark(path: &Path) -> Option<VersionMark> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let m = fs::symlink_metadata(path).ok()?;
        Some((
            m.len(),
            m.dev(),
            m.ino(),
            (m.mtime(), m.mtime_nsec()),
            (m.ctime(), m.ctime_nsec()),
        ))
    }
    #[cfg(windows)]
    {
        let file = win::open_attributes(path, false).ok()?;
        let basic = win::basic_info(&file).ok()?;
        let standard = win::standard_info(&file).ok()?;
        let (volume, id) = win::identity(&file).ok()?;
        Some((
            standard.EndOfFile as u64,
            volume,
            u64::from_le_bytes(id[..8].try_into().ok()?),
            win::unix_time(basic.LastWriteTime),
            win::unix_time(basic.ChangeTime),
        ))
    }
}

/// Takes `file`'s lock without waiting: `false` when another process
/// holds it. Held until the file is closed.
pub fn try_lock(file: &File) -> io::Result<bool> {
    crate::fsops::lock_file(file, false)
}

/// Whether this user owns the entry at `path` (no link followed).
pub fn owned_by_me(path: &Path) -> io::Result<bool> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        // SAFETY: geteuid has no preconditions and cannot fail.
        Ok(fs::symlink_metadata(path)?.uid() == unsafe { libc::geteuid() })
    }
    #[cfg(windows)]
    {
        win::owned_by_me(path)
    }
}

/// Whether the entry at `path` lets anyone in besides its owner: group or
/// world bits on Unix, access entries for others on Windows. The text
/// says what was found, for a note.
pub fn open_to_others(path: &Path) -> io::Result<Option<String>> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let mode = fs::symlink_metadata(path)?.mode() & 0o7777;
        Ok((mode & 0o077 != 0).then(|| format!("{mode:o}")))
    }
    #[cfg(windows)]
    {
        Ok(win::others_allowed(path)?.then(|| "an access list naming others".to_owned()))
    }
}

/// The person's home folder: `HOME` on Unix, `USERPROFILE` on Windows.
pub fn person_home() -> Option<std::path::PathBuf> {
    #[cfg(unix)]
    let var = "HOME";
    #[cfg(windows)]
    let var = "USERPROFILE";
    std::env::var_os(var)
        .filter(|h| !h.is_empty())
        .map(std::path::PathBuf::from)
}

/// The shell a person gets by default: `SHELL` when it names a program by
/// an absolute path, else `/bin/sh` (Unix); PowerShell 7, Windows
/// PowerShell or `COMSPEC` (Windows).
pub fn default_shell() -> std::ffi::OsString {
    #[cfg(unix)]
    {
        std::env::var_os("SHELL")
            .filter(|s| Path::new(s).is_absolute())
            .unwrap_or_else(|| "/bin/sh".into())
    }
    #[cfg(windows)]
    {
        let on_path = |name: &str| {
            std::env::var_os("PATH").and_then(|path| {
                std::env::split_paths(&path)
                    .map(|dir| dir.join(name))
                    .find(|candidate| candidate.is_file())
            })
        };
        on_path("pwsh.exe")
            .or_else(|| on_path("powershell.exe"))
            .map(std::path::PathBuf::into_os_string)
            .or_else(|| std::env::var_os("COMSPEC"))
            .unwrap_or_else(|| "cmd.exe".into())
    }
}

/// Whether the process `pid` is alive, or alive under another user.
pub fn alive(pid: u32) -> bool {
    #[cfg(unix)]
    {
        let Ok(pid) = i32::try_from(pid) else {
            return false;
        };
        // SAFETY: signal 0 only asks whether the process exists.
        pid > 0
            && (unsafe { libc::kill(pid, 0) } == 0
                || io::Error::last_os_error().raw_os_error() == Some(libc::EPERM))
    }
    #[cfg(windows)]
    {
        win::alive(pid)
    }
}

/// Fills `buf` with random bytes from the system.
pub fn random_bytes(buf: &mut [u8]) -> io::Result<()> {
    use ring::rand::SecureRandom;
    ring::rand::SystemRandom::new()
        .fill(buf)
        .map_err(|_| io::Error::other("the system gave no random bytes"))
}

#[cfg(windows)]
pub mod win {
    use std::fs::{self, File, Metadata};
    use std::io;
    use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
    use std::os::windows::io::AsRawHandle;
    use std::path::Path;

    use windows::Win32::Foundation::{ERROR_ACCESS_DENIED, HANDLE, STILL_ACTIVE};
    use windows::Win32::Storage::FileSystem::{
        FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_NORMAL, FILE_ATTRIBUTE_READONLY,
        FILE_ATTRIBUTE_REPARSE_POINT, FILE_ATTRIBUTE_TAG_INFO, FILE_BASIC_INFO,
        FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_ID_INFO,
        FILE_READ_ATTRIBUTES, FILE_STANDARD_INFO, FILE_WRITE_ATTRIBUTES, FileAttributeTagInfo,
        FileBasicInfo, FileIdInfo, FileStandardInfo, GetFileInformationByHandleEx, READ_CONTROL,
        SetFileInformationByHandle, WRITE_DAC,
    };
    use windows::Win32::System::Threading::{
        GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    pub(crate) const GENERIC_WRITE: u32 = 0x4000_0000;

    pub(crate) fn handle(file: &File) -> HANDLE {
        HANDLE(file.as_raw_handle())
    }

    pub(crate) fn is_reparse(meta: &Metadata) -> bool {
        meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0
    }

    /// `path` opened for reading and, with `write`, writing its attributes
    /// only; a link is opened as itself, never followed.
    pub(crate) fn open_attributes(path: &Path, write: bool) -> io::Result<File> {
        let mut access = FILE_READ_ATTRIBUTES.0;
        if write {
            access |= FILE_WRITE_ATTRIBUTES.0;
        }
        fs::OpenOptions::new()
            .access_mode(access)
            .custom_flags((FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS).0)
            .open(path)
    }

    /// `path` opened with the right to change who may use it.
    pub(crate) fn open_attributes_for_dacl(path: &Path) -> io::Result<File> {
        fs::OpenOptions::new()
            .access_mode(FILE_READ_ATTRIBUTES.0 | READ_CONTROL.0 | WRITE_DAC.0)
            .custom_flags((FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS).0)
            .open(path)
    }

    /// The attributes and reparse tag of an open entry.
    pub(crate) fn tag_info(file: &File) -> io::Result<FILE_ATTRIBUTE_TAG_INFO> {
        let mut info = FILE_ATTRIBUTE_TAG_INFO::default();
        // SAFETY: the handle is open and the buffer is the size the class needs.
        unsafe {
            GetFileInformationByHandleEx(
                handle(file),
                FileAttributeTagInfo,
                (&mut info as *mut FILE_ATTRIBUTE_TAG_INFO).cast(),
                std::mem::size_of::<FILE_ATTRIBUTE_TAG_INFO>() as u32,
            )
        }
        .map_err(io_error)?;
        Ok(info)
    }

    /// The volume serial number and 128-bit file identity of an open entry.
    pub(crate) fn identity(file: &File) -> io::Result<(u64, [u8; 16])> {
        let mut info = FILE_ID_INFO::default();
        // SAFETY: the handle is open and the buffer is the size the class needs.
        unsafe {
            GetFileInformationByHandleEx(
                handle(file),
                FileIdInfo,
                (&mut info as *mut FILE_ID_INFO).cast(),
                std::mem::size_of::<FILE_ID_INFO>() as u32,
            )
        }
        .map_err(io_error)?;
        Ok((info.VolumeSerialNumber, info.FileId.Identifier))
    }

    /// Size, link count and kind of an open entry.
    pub(crate) fn standard_info(file: &File) -> io::Result<FILE_STANDARD_INFO> {
        let mut info = FILE_STANDARD_INFO::default();
        // SAFETY: the handle is open and the buffer is the size the class needs.
        unsafe {
            GetFileInformationByHandleEx(
                handle(file),
                FileStandardInfo,
                (&mut info as *mut FILE_STANDARD_INFO).cast(),
                std::mem::size_of::<FILE_STANDARD_INFO>() as u32,
            )
        }
        .map_err(io_error)?;
        Ok(info)
    }

    pub(crate) fn basic_info(file: &File) -> io::Result<FILE_BASIC_INFO> {
        let mut info = FILE_BASIC_INFO::default();
        // SAFETY: the handle is open and the buffer is the size the class needs.
        unsafe {
            GetFileInformationByHandleEx(
                handle(file),
                FileBasicInfo,
                (&mut info as *mut FILE_BASIC_INFO).cast(),
                std::mem::size_of::<FILE_BASIC_INFO>() as u32,
            )
        }
        .map_err(io_error)?;
        Ok(info)
    }

    pub(crate) fn set_basic_info(file: &File, info: &FILE_BASIC_INFO) -> io::Result<()> {
        // SAFETY: the handle is open and the buffer is the size the class needs.
        unsafe {
            SetFileInformationByHandle(
                handle(file),
                FileBasicInfo,
                (info as *const FILE_BASIC_INFO).cast(),
                std::mem::size_of::<FILE_BASIC_INFO>() as u32,
            )
        }
        .map_err(io_error)
    }

    /// [`super::set_file_mode`]: the read-only attribute follows the
    /// owner's write bit; a folder or a link is left alone.
    pub(crate) fn apply_mode(file: &File, mode: u32) -> io::Result<()> {
        let current = basic_info(file)?;
        let attributes = current.FileAttributes;
        if attributes & (FILE_ATTRIBUTE_DIRECTORY | FILE_ATTRIBUTE_REPARSE_POINT).0 != 0 {
            return Ok(());
        }
        let wanted = if mode & 0o200 == 0 {
            attributes | FILE_ATTRIBUTE_READONLY.0
        } else {
            attributes & !FILE_ATTRIBUTE_READONLY.0
        };
        if wanted == attributes {
            return Ok(());
        }
        // Zero times leave the times alone; an attribute set must not be
        // empty, which is what NORMAL means.
        let info = FILE_BASIC_INFO {
            CreationTime: 0,
            LastAccessTime: 0,
            LastWriteTime: 0,
            ChangeTime: 0,
            FileAttributes: if wanted == 0 {
                FILE_ATTRIBUTE_NORMAL.0
            } else {
                wanted
            },
        };
        set_basic_info(file, &info)
    }

    /// Gives `file` (a folder when `dir`) an access list naming only its
    /// owner and the system, inheritance from above cut off.
    pub fn make_private(file: &File, dir: bool) -> io::Result<()> {
        use windows::Win32::Security::Authorization::{
            ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1, SE_FILE_OBJECT,
            SetSecurityInfo,
        };
        use windows::Win32::Security::{
            DACL_SECURITY_INFORMATION, GetSecurityDescriptorDacl,
            PROTECTED_DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR,
        };
        use windows::core::PCWSTR;

        let sid = current_user_sid()?;
        let inherit = if dir { "OICI" } else { "" };
        let sddl = format!("D:PAI(A;{inherit};FA;;;{sid})(A;{inherit};FA;;;SY)");
        let wide: Vec<u16> = sddl.encode_utf16().chain(std::iter::once(0)).collect();
        unsafe {
            // SAFETY: the descriptor is allocated by the call and freed below.
            let mut descriptor = PSECURITY_DESCRIPTOR::default();
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                PCWSTR(wide.as_ptr()),
                SDDL_REVISION_1,
                &mut descriptor,
                None,
            )
            .map_err(io_error)?;
            let mut present = windows::core::BOOL::default();
            let mut defaulted = windows::core::BOOL::default();
            let mut dacl = std::ptr::null_mut();
            let got =
                GetSecurityDescriptorDacl(descriptor, &mut present, &mut dacl, &mut defaulted);
            let set = got.map_err(io_error).and_then(|()| {
                let err = SetSecurityInfo(
                    handle(file),
                    SE_FILE_OBJECT,
                    DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                    None,
                    None,
                    Some(dacl),
                    None,
                );
                if err.is_ok() {
                    Ok(())
                } else {
                    Err(io::Error::from_raw_os_error(err.0 as i32))
                }
            });
            let _ = windows::Win32::Foundation::LocalFree(Some(
                windows::Win32::Foundation::HLOCAL(descriptor.0),
            ));
            set
        }
    }

    pub fn alive(pid: u32) -> bool {
        // SAFETY: a query-only handle, closed below.
        unsafe {
            match OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
                Ok(process) => {
                    let mut code = 0u32;
                    let running = GetExitCodeProcess(process, &mut code).is_ok()
                        && code == STILL_ACTIVE.0 as u32;
                    let _ = windows::Win32::Foundation::CloseHandle(process);
                    running
                }
                // Alive under another user.
                Err(e) => e.code() == ERROR_ACCESS_DENIED.to_hresult(),
            }
        }
    }

    /// The SID of the user `token` runs as, as text.
    fn token_user_sid(token: HANDLE) -> io::Result<String> {
        use windows::Win32::Security::Authorization::ConvertSidToStringSidW;
        use windows::Win32::Security::{GetTokenInformation, TOKEN_USER, TokenUser};
        use windows::core::PWSTR;
        // SAFETY: sizes are asked first; the buffer outlives the pointer read.
        unsafe {
            let mut needed = 0u32;
            let _ = GetTokenInformation(token, TokenUser, None, 0, &mut needed);
            let mut buf = vec![0u8; needed.max(1) as usize];
            GetTokenInformation(
                token,
                TokenUser,
                Some(buf.as_mut_ptr().cast()),
                needed,
                &mut needed,
            )
            .map_err(io_error)?;
            let user = &*(buf.as_ptr() as *const TOKEN_USER);
            let mut text = PWSTR::null();
            ConvertSidToStringSidW(user.User.Sid, &mut text).map_err(io_error)?;
            let sid = text
                .to_string()
                .map_err(|e| io::Error::other(e.to_string()));
            let _ = windows::Win32::Foundation::LocalFree(Some(
                windows::Win32::Foundation::HLOCAL(text.0.cast()),
            ));
            sid
        }
    }

    /// The SID of the user this process runs as, as text.
    pub fn current_user_sid() -> io::Result<String> {
        use windows::Win32::Security::TOKEN_QUERY;
        use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
        // SAFETY: the token handle is closed below.
        unsafe {
            let mut token = HANDLE::default();
            OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).map_err(io_error)?;
            let sid = token_user_sid(token);
            let _ = windows::Win32::Foundation::CloseHandle(token);
            sid
        }
    }

    /// The token of process `pid`, open to be asked about.
    fn token_of(pid: u32) -> io::Result<HANDLE> {
        use windows::Win32::Security::TOKEN_QUERY;
        use windows::Win32::System::Threading::OpenProcessToken;
        // SAFETY: the process handle is closed here; the token by the caller.
        unsafe {
            let process =
                OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).map_err(io_error)?;
            let mut token = HANDLE::default();
            let opened = OpenProcessToken(process, TOKEN_QUERY, &mut token).map_err(io_error);
            let _ = windows::Win32::Foundation::CloseHandle(process);
            opened.map(|()| token)
        }
    }

    /// The SID of the user process `pid` runs as, as text.
    pub fn user_sid_of(pid: u32) -> io::Result<String> {
        let token = token_of(pid)?;
        let sid = token_user_sid(token);
        // SAFETY: opened by token_of.
        unsafe {
            let _ = windows::Win32::Foundation::CloseHandle(token);
        }
        sid
    }

    /// Whether process `pid` runs inside an app container (a sandboxed
    /// command, never one of Kule's own).
    pub fn is_app_container(pid: u32) -> io::Result<bool> {
        use windows::Win32::Security::{GetTokenInformation, TokenIsAppContainer};
        let token = token_of(pid)?;
        let mut flag = 0u32;
        let mut needed = 0u32;
        // SAFETY: the token is open; the buffer is a u32 as the class wants.
        let asked = unsafe {
            GetTokenInformation(
                token,
                TokenIsAppContainer,
                Some((&mut flag as *mut u32).cast()),
                std::mem::size_of::<u32>() as u32,
                &mut needed,
            )
        };
        // SAFETY: opened by token_of.
        unsafe {
            let _ = windows::Win32::Foundation::CloseHandle(token);
        }
        asked.map_err(io_error)?;
        Ok(flag != 0)
    }

    /// The owner of `path` (no link followed) and its access list as SDDL
    /// text.
    fn owner_and_dacl(path: &Path) -> io::Result<(String, String)> {
        use windows::Win32::Security::Authorization::{
            ConvertSecurityDescriptorToStringSecurityDescriptorW, ConvertSidToStringSidW,
            GetSecurityInfo, SDDL_REVISION_1, SE_FILE_OBJECT,
        };
        use windows::Win32::Security::{
            DACL_SECURITY_INFORMATION, OWNER_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, PSID,
        };
        use windows::core::PWSTR;
        let file = fs::OpenOptions::new()
            .access_mode(FILE_READ_ATTRIBUTES.0 | READ_CONTROL.0)
            .custom_flags((FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS).0)
            .open(path)?;
        // SAFETY: the descriptor and strings are allocated by the calls and
        // freed below.
        unsafe {
            let mut owner = PSID::default();
            let mut descriptor = PSECURITY_DESCRIPTOR::default();
            let err = GetSecurityInfo(
                handle(&file),
                SE_FILE_OBJECT,
                OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
                Some(&mut owner),
                None,
                None,
                None,
                Some(&mut descriptor),
            );
            if err.is_err() {
                return Err(io::Error::from_raw_os_error(err.0 as i32));
            }
            let mut owner_text = PWSTR::null();
            let owner_sid = ConvertSidToStringSidW(owner, &mut owner_text)
                .map_err(io_error)
                .and_then(|()| {
                    owner_text
                        .to_string()
                        .map_err(|e| io::Error::other(e.to_string()))
                });
            let _ = windows::Win32::Foundation::LocalFree(Some(
                windows::Win32::Foundation::HLOCAL(owner_text.0.cast()),
            ));
            let mut dacl_text = PWSTR::null();
            let dacl = ConvertSecurityDescriptorToStringSecurityDescriptorW(
                descriptor,
                SDDL_REVISION_1,
                DACL_SECURITY_INFORMATION,
                &mut dacl_text,
                None,
            )
            .map_err(io_error)
            .and_then(|()| {
                dacl_text
                    .to_string()
                    .map_err(|e| io::Error::other(e.to_string()))
            });
            let _ = windows::Win32::Foundation::LocalFree(Some(
                windows::Win32::Foundation::HLOCAL(dacl_text.0.cast()),
            ));
            let _ = windows::Win32::Foundation::LocalFree(Some(
                windows::Win32::Foundation::HLOCAL(descriptor.0),
            ));
            Ok((owner_sid?, dacl?))
        }
    }

    /// Whether this user owns `path`. What an administrator makes while
    /// elevated is owned by the Administrators group, which counts as
    /// theirs when their token carries that group.
    pub fn owned_by_me(path: &Path) -> io::Result<bool> {
        const ADMINISTRATORS: &str = "S-1-5-32-544";
        let owner = owner_and_dacl(path)?.0;
        Ok(owner == current_user_sid()?
            || (owner == ADMINISTRATORS && token_has_group(ADMINISTRATORS)?))
    }

    /// Whether this process's token carries `sid` as an enabled group.
    fn token_has_group(sid: &str) -> io::Result<bool> {
        use windows::Win32::Security::Authorization::ConvertSidToStringSidW;
        use windows::Win32::Security::{
            GetTokenInformation, TOKEN_GROUPS, TOKEN_QUERY, TokenGroups,
        };
        use windows::Win32::System::SystemServices::{
            SE_GROUP_ENABLED, SE_GROUP_USE_FOR_DENY_ONLY,
        };
        use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
        use windows::core::PWSTR;
        // SAFETY: sizes are asked first; every handle and string is freed.
        unsafe {
            let mut token = HANDLE::default();
            OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).map_err(io_error)?;
            let mut needed = 0u32;
            let _ = GetTokenInformation(token, TokenGroups, None, 0, &mut needed);
            let mut buf = vec![0u8; needed.max(1) as usize];
            let got = GetTokenInformation(
                token,
                TokenGroups,
                Some(buf.as_mut_ptr().cast()),
                needed,
                &mut needed,
            );
            let _ = windows::Win32::Foundation::CloseHandle(token);
            got.map_err(io_error)?;
            let groups = &*(buf.as_ptr() as *const TOKEN_GROUPS);
            let count = groups.GroupCount as usize;
            let first = groups.Groups.as_ptr();
            for i in 0..count {
                let group = &*first.add(i);
                let enabled = group.Attributes & SE_GROUP_ENABLED as u32 != 0
                    && group.Attributes & SE_GROUP_USE_FOR_DENY_ONLY as u32 == 0;
                if !enabled {
                    continue;
                }
                let mut text = PWSTR::null();
                if ConvertSidToStringSidW(group.Sid, &mut text).is_err() {
                    continue;
                }
                let same = text.to_string().is_ok_and(|t| t == sid);
                let _ = windows::Win32::Foundation::LocalFree(Some(
                    windows::Win32::Foundation::HLOCAL(text.0.cast()),
                ));
                if same {
                    return Ok(true);
                }
            }
            Ok(false)
        }
    }

    /// Whether `path`'s access list lets anyone in besides its owner, the
    /// system and administrators.
    pub fn others_allowed(path: &Path) -> io::Result<bool> {
        let (owner, dacl) = owner_and_dacl(path)?;
        let me = current_user_sid()?;
        // "(A;flags;rights;object;inherit;sid)": an allow entry for a SID.
        for ace in dacl.split('(').skip(1) {
            let ace = ace.trim_end_matches(')');
            let fields: Vec<&str> = ace.split(';').collect();
            if fields.first() != Some(&"A") {
                continue;
            }
            let Some(sid) = fields.get(5) else { continue };
            if ![owner.as_str(), me.as_str(), "SY", "BA", "OW", "CO"].contains(sid)
                && *sid != "S-1-5-18"
                && *sid != "S-1-5-32-544"
            {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Makes `link` a junction to the folder `target` (an absolute path):
    /// an empty folder made first, then given the reparse data through its
    /// own handle, so nothing is written through a link at `link`.
    pub fn junction(target: &Path, link: &Path) -> io::Result<()> {
        use std::os::windows::io::AsRawHandle;
        use windows::Win32::Storage::FileSystem::{
            FILE_FLAG_OPEN_REPARSE_POINT, FILE_WRITE_ATTRIBUTES,
        };
        use windows::Win32::System::IO::DeviceIoControl;
        use windows::Win32::System::Ioctl::FSCTL_SET_REPARSE_POINT;
        use windows::Win32::System::SystemServices::IO_REPARSE_TAG_MOUNT_POINT;
        fs::create_dir(link)?;
        let made = fs::OpenOptions::new()
            .access_mode(FILE_WRITE_ATTRIBUTES.0)
            .custom_flags((FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS).0)
            .open(link)?;
        let text = target.to_string_lossy().replace('/', "\\");
        let print: Vec<u16> = text.encode_utf16().collect();
        let substitute: Vec<u16> = {
            let stripped = text.trim_start_matches(r"\\?\");
            format!(r"\??\{stripped}").encode_utf16().collect()
        };
        // REPARSE_DATA_BUFFER for a mount point: tag, data length, reserved,
        // substitute offset and length, print offset and length, the two
        // paths each with a NUL.
        let mut data: Vec<u8> = Vec::new();
        let path_bytes = (substitute.len() + 1 + print.len() + 1) * 2;
        data.extend_from_slice(&IO_REPARSE_TAG_MOUNT_POINT.to_le_bytes());
        data.extend_from_slice(&((8 + path_bytes) as u16).to_le_bytes());
        data.extend_from_slice(&0u16.to_le_bytes());
        data.extend_from_slice(&0u16.to_le_bytes());
        data.extend_from_slice(&((substitute.len() * 2) as u16).to_le_bytes());
        data.extend_from_slice(&(((substitute.len() + 1) * 2) as u16).to_le_bytes());
        data.extend_from_slice(&((print.len() * 2) as u16).to_le_bytes());
        for unit in substitute
            .iter()
            .chain(std::iter::once(&0))
            .chain(print.iter())
            .chain(std::iter::once(&0))
        {
            data.extend_from_slice(&unit.to_le_bytes());
        }
        // SAFETY: the handle is open and the buffer valid for its length.
        let set = unsafe {
            DeviceIoControl(
                HANDLE(made.as_raw_handle()),
                FSCTL_SET_REPARSE_POINT,
                Some(data.as_ptr().cast()),
                data.len() as u32,
                None,
                0,
                None,
                None,
            )
        };
        if let Err(e) = set {
            drop(made);
            let _ = fs::remove_dir(link);
            return Err(io_error(e));
        }
        Ok(())
    }

    /// Windows needs Developer Mode or a privilege to make a link; the
    /// error says so instead of a bare code.
    pub(crate) fn explain_link_privilege(e: io::Error) -> io::Error {
        const ERROR_PRIVILEGE_NOT_HELD: i32 = 1314;
        if e.raw_os_error() == Some(ERROR_PRIVILEGE_NOT_HELD) {
            io::Error::new(
                io::ErrorKind::PermissionDenied,
                "Windows does not let Kule make a link here: links need Developer Mode or an administrator",
            )
        } else {
            e
        }
    }

    pub fn io_error(e: windows::core::Error) -> io::Error {
        io::Error::from_raw_os_error(e.code().0 & 0xFFFF)
    }

    /// A Windows time (100 ns ticks since 1601) as seconds and nanoseconds
    /// since 1970, as a stamp keeps it.
    pub(crate) fn unix_time(filetime: i64) -> (i64, i64) {
        const EPOCH_DIFFERENCE: i64 = 116_444_736_000_000_000;
        let ticks = filetime - EPOCH_DIFFERENCE;
        (
            ticks.div_euclid(10_000_000),
            ticks.rem_euclid(10_000_000) * 100,
        )
    }

    /// The reverse of [`unix_time`].
    pub(crate) fn file_time(unix: (i64, i64)) -> i64 {
        const EPOCH_DIFFERENCE: i64 = 116_444_736_000_000_000;
        unix.0 * 10_000_000 + unix.1 / 100 + EPOCH_DIFFERENCE
    }
}
