//! The handle-relative calls [`super`] builds on, on Windows: `NtCreateFile`
//! with a root directory handle and one name at a time, each open with
//! `FILE_OPEN_REPARSE_POINT`, so a link (a symbolic link or a junction) put
//! on the way is seen as itself and refused rather than followed.
//!
//! A reparse point that is not a name surrogate (a cloud placeholder, a
//! deduplicated file) is not a link: it is opened again, following its
//! reparse, and the file identity is checked to be the same, so nothing
//! swapped in between is followed.

use std::fs::File;
use std::io::{self, Write};
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::path::{Path, PathBuf};

use windows::Wdk::Foundation::OBJECT_ATTRIBUTES;
use windows::Wdk::Storage::FileSystem::{
    FILE_CREATE, FILE_DIRECTORY_FILE, FILE_NON_DIRECTORY_FILE, FILE_OPEN, FILE_OPEN_REPARSE_POINT,
    FILE_SYNCHRONOUS_IO_NONALERT, NTCREATEFILE_CREATE_DISPOSITION, NTCREATEFILE_CREATE_OPTIONS,
    NtCreateFile,
};
use windows::Win32::Foundation::{
    HANDLE, NTSTATUS, OBJ_CASE_INSENSITIVE, RtlNtStatusToDosError, UNICODE_STRING,
};
use windows::Win32::Storage::FileSystem::{
    DELETE, FILE_ACCESS_RIGHTS, FILE_ATTRIBUTE_NORMAL, FILE_ATTRIBUTE_READONLY,
    FILE_ATTRIBUTE_REPARSE_POINT, FILE_ATTRIBUTE_TAG_INFO, FILE_BASIC_INFO,
    FILE_DISPOSITION_INFO_EX, FILE_DISPOSITION_INFO_EX_FLAGS, FILE_END_OF_FILE_INFO,
    FILE_FLAGS_AND_ATTRIBUTES, FILE_GENERIC_READ, FILE_GENERIC_WRITE, FILE_ID_BOTH_DIR_INFO,
    FILE_LIST_DIRECTORY, FILE_READ_ATTRIBUTES, FILE_RENAME_INFO, FILE_SHARE_DELETE,
    FILE_SHARE_READ, FILE_SHARE_WRITE, FILE_TRAVERSE, FILE_WRITE_ATTRIBUTES, FileDispositionInfoEx,
    FileEndOfFileInfo, FileIdBothDirectoryInfo, FileIdBothDirectoryRestartInfo,
    GetFileInformationByHandleEx, LOCKFILE_EXCLUSIVE_LOCK, LOCKFILE_FAIL_IMMEDIATELY, LockFileEx,
    SYNCHRONIZE, SetFileInformationByHandle,
};
use windows::Win32::System::IO::{DeviceIoControl, IO_STATUS_BLOCK, OVERLAPPED};
use windows::Win32::System::Ioctl::{
    DUPLICATE_EXTENTS_DATA, FSCTL_DUPLICATE_EXTENTS_TO_FILE, FSCTL_GET_REPARSE_POINT,
    FSCTL_SET_REPARSE_POINT,
};
use windows::Win32::System::SystemServices::{IO_REPARSE_TAG_MOUNT_POINT, IO_REPARSE_TAG_SYMLINK};
use windows::core::PWSTR;

use crate::manifest::Kind;
use crate::osfs::win::{self, io_error};

/// One component of a path as `NtCreateFile` takes it, in UTF-16.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Name(Vec<u16>);

/// `name` for a handle-relative call: one component, neither `.` nor `..`.
pub(crate) fn plain_name(name: &str) -> io::Result<Name> {
    if matches!(name, "" | "." | "..") || name.contains(['/', '\\', '\0']) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{name:?} is not a plain name"),
        ));
    }
    let wide: Vec<u16> = name.encode_utf16().collect();
    if wide.len() > u16::MAX as usize / 2 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{name:?} is too long for a name"),
        ));
    }
    Ok(Name(wide))
}

/// `name` as text, for messages.
pub(crate) fn name_text(name: &Name) -> String {
    String::from_utf16_lossy(&name.0)
}

/// The mark on an error that says the entry is a link (a name surrogate
/// reparse point) where a folder or a file was asked for.
#[derive(Debug)]
struct LinkOnTheWay;

impl std::fmt::Display for LinkOnTheWay {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("a link stands where a folder or file was expected")
    }
}

impl std::error::Error for LinkOnTheWay {}

fn link_error() -> io::Error {
    io::Error::other(LinkOnTheWay)
}

const ERROR_FILE_NOT_FOUND: i32 = 2;
const ERROR_PATH_NOT_FOUND: i32 = 3;
const ERROR_FILE_EXISTS: i32 = 80;
const ERROR_INVALID_PARAMETER: i32 = 87;
const ERROR_DIR_NOT_EMPTY: i32 = 145;
const ERROR_ALREADY_EXISTS: i32 = 183;
const ERROR_DIRECTORY: i32 = 267;
const ERROR_DIRECTORY_NOT_SUPPORTED: i32 = 336;
const ERROR_LOCK_VIOLATION: i32 = 33;
const ERROR_NO_MORE_FILES: i32 = 18;
const ERROR_ACCESS_DENIED: i32 = 5;

/// Whether `e` says the entry is a link, or something else than the folder
/// or file the call asked for.
pub(crate) fn is_link_or_other_kind(e: &io::Error) -> bool {
    e.get_ref().is_some_and(|inner| inner.is::<LinkOnTheWay>())
        || matches!(
            e.raw_os_error(),
            Some(ERROR_DIRECTORY | ERROR_DIRECTORY_NOT_SUPPORTED)
        )
}

pub(crate) fn is_missing(e: &io::Error) -> bool {
    matches!(
        e.raw_os_error(),
        Some(ERROR_FILE_NOT_FOUND | ERROR_PATH_NOT_FOUND)
    )
}

pub(crate) fn is_exists(e: &io::Error) -> bool {
    matches!(
        e.raw_os_error(),
        Some(ERROR_FILE_EXISTS | ERROR_ALREADY_EXISTS)
    )
}

/// Whether `e` says a rename found something in the way at its target.
pub(crate) fn is_in_the_way(e: &io::Error) -> bool {
    matches!(
        e.raw_os_error(),
        Some(
            ERROR_DIR_NOT_EMPTY
                | ERROR_FILE_EXISTS
                | ERROR_ALREADY_EXISTS
                | ERROR_DIRECTORY
                | ERROR_DIRECTORY_NOT_SUPPORTED
        )
    )
}

fn nt_error(status: NTSTATUS) -> io::Error {
    // SAFETY: a pure conversion of a status code.
    io::Error::from_raw_os_error(unsafe { RtlNtStatusToDosError(status) } as i32)
}

/// Opens `name` in `dir` (the directory itself when `name` is `None`), with
/// `access`, `disposition` and `options`, never following a reparse point.
fn nt_open(
    dir: &File,
    name: Option<&Name>,
    access: FILE_ACCESS_RIGHTS,
    disposition: NTCREATEFILE_CREATE_DISPOSITION,
    options: NTCREATEFILE_CREATE_OPTIONS,
    attributes: FILE_FLAGS_AND_ATTRIBUTES,
) -> io::Result<File> {
    let empty: [u16; 0] = [];
    let wide: &[u16] = name.map_or(&empty[..], |n| n.0.as_slice());
    let bytes = (wide.len() * 2) as u16;
    let unicode = UNICODE_STRING {
        Length: bytes,
        MaximumLength: bytes,
        Buffer: PWSTR(wide.as_ptr().cast_mut()),
    };
    let attrs = OBJECT_ATTRIBUTES {
        Length: std::mem::size_of::<OBJECT_ATTRIBUTES>() as u32,
        RootDirectory: HANDLE(dir.as_raw_handle()),
        ObjectName: &unicode,
        Attributes: OBJ_CASE_INSENSITIVE,
        SecurityDescriptor: std::ptr::null(),
        SecurityQualityOfService: std::ptr::null(),
    };
    let mut handle = HANDLE::default();
    let mut status_block = IO_STATUS_BLOCK::default();
    // SAFETY: every pointer is to a live local for the call's duration; the
    // handle written is owned below.
    let status = unsafe {
        NtCreateFile(
            &mut handle,
            access | SYNCHRONIZE,
            &attrs,
            &mut status_block,
            None,
            attributes,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            disposition,
            options | FILE_OPEN_REPARSE_POINT | FILE_SYNCHRONOUS_IO_NONALERT,
            None,
            0,
        )
    };
    if status.is_err() {
        return Err(nt_error(status));
    }
    // SAFETY: a handle the call just opened, owned by nothing else.
    Ok(File::from(unsafe {
        OwnedHandle::from_raw_handle(handle.0)
    }))
}

/// Whether an open entry is a link: a reparse point whose tag is a name
/// surrogate (a symbolic link, a junction, a mount point), which redirects
/// the name to somewhere else.
fn is_link(info: &FILE_ATTRIBUTE_TAG_INFO) -> bool {
    const NAME_SURROGATE: u32 = 0x2000_0000;
    info.FileAttributes & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0
        && info.ReparseTag & NAME_SURROGATE != 0
}

/// Whether an open entry is a reparse point that is not a link (a cloud
/// placeholder): one that reads as a plain file or folder once followed.
fn is_placeholder(info: &FILE_ATTRIBUTE_TAG_INFO) -> bool {
    info.FileAttributes & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0 && !is_link(info)
}

/// `opened` again, this time following its reparse point, when it is a
/// placeholder; refused unless what comes back is the same entry.
fn through_placeholder(
    dir: &File,
    name: &Name,
    opened: File,
    access: FILE_ACCESS_RIGHTS,
    options: NTCREATEFILE_CREATE_OPTIONS,
) -> io::Result<File> {
    let before = win::identity(&opened)?;
    let wide = &name.0;
    let bytes = (wide.len() * 2) as u16;
    let unicode = UNICODE_STRING {
        Length: bytes,
        MaximumLength: bytes,
        Buffer: PWSTR(wide.as_ptr().cast_mut()),
    };
    let attrs = OBJECT_ATTRIBUTES {
        Length: std::mem::size_of::<OBJECT_ATTRIBUTES>() as u32,
        RootDirectory: HANDLE(dir.as_raw_handle()),
        ObjectName: &unicode,
        Attributes: OBJ_CASE_INSENSITIVE,
        SecurityDescriptor: std::ptr::null(),
        SecurityQualityOfService: std::ptr::null(),
    };
    let mut handle = HANDLE::default();
    let mut status_block = IO_STATUS_BLOCK::default();
    // SAFETY: as in `nt_open`.
    let status = unsafe {
        NtCreateFile(
            &mut handle,
            access | SYNCHRONIZE,
            &attrs,
            &mut status_block,
            None,
            FILE_FLAGS_AND_ATTRIBUTES(0),
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            FILE_OPEN,
            options | FILE_SYNCHRONOUS_IO_NONALERT,
            None,
            0,
        )
    };
    if status.is_err() {
        return Err(nt_error(status));
    }
    // SAFETY: a handle the call just opened, owned by nothing else.
    let followed = File::from(unsafe { OwnedHandle::from_raw_handle(handle.0) });
    if win::identity(&followed)? != before {
        return Err(link_error());
    }
    Ok(followed)
}

const DIR_ACCESS: FILE_ACCESS_RIGHTS = FILE_ACCESS_RIGHTS(
    FILE_LIST_DIRECTORY.0 | FILE_TRAVERSE.0 | FILE_READ_ATTRIBUTES.0 | FILE_WRITE_ATTRIBUTES.0,
);

/// The folder `name` in `dir`, opened without following a link.
pub(crate) fn open_dir_at(dir: &File, name: &Name) -> io::Result<File> {
    let opened = match nt_open(
        dir,
        Some(name),
        DIR_ACCESS,
        FILE_OPEN,
        FILE_DIRECTORY_FILE,
        FILE_FLAGS_AND_ATTRIBUTES(0),
    ) {
        Ok(opened) => opened,
        // Without the right to change attributes, still a folder to read.
        Err(e) if e.raw_os_error() == Some(ERROR_ACCESS_DENIED) => nt_open(
            dir,
            Some(name),
            FILE_ACCESS_RIGHTS(FILE_LIST_DIRECTORY.0 | FILE_TRAVERSE.0 | FILE_READ_ATTRIBUTES.0),
            FILE_OPEN,
            FILE_DIRECTORY_FILE,
            FILE_FLAGS_AND_ATTRIBUTES(0),
        )?,
        Err(e) => return Err(e),
    };
    let info = win::tag_info(&opened)?;
    if is_link(&info) {
        return Err(link_error());
    }
    if is_placeholder(&info) {
        return through_placeholder(dir, name, opened, DIR_ACCESS, FILE_DIRECTORY_FILE);
    }
    Ok(opened)
}

/// The regular file `name` in `dir`, opened for reading without following
/// a link; `None` for a link or anything else than a regular file.
pub(crate) fn open_file_at(dir: &File, name: &Name) -> io::Result<Option<File>> {
    let opened = match nt_open(
        dir,
        Some(name),
        FILE_GENERIC_READ,
        FILE_OPEN,
        FILE_NON_DIRECTORY_FILE,
        FILE_FLAGS_AND_ATTRIBUTES(0),
    ) {
        Ok(opened) => opened,
        Err(e) if is_link_or_other_kind(&e) => return Ok(None),
        Err(e) => return Err(e),
    };
    let info = win::tag_info(&opened)?;
    if is_link(&info) {
        return Ok(None);
    }
    let file = if is_placeholder(&info) {
        through_placeholder(
            dir,
            name,
            opened,
            FILE_GENERIC_READ,
            FILE_NON_DIRECTORY_FILE,
        )?
    } else {
        opened
    };
    Ok(file.metadata()?.is_file().then_some(file))
}

/// The entry `name` in `dir`, a file or a folder, opened for reading
/// without following a link; `None` for a link.
pub(crate) fn open_any_at(dir: &File, name: &Name) -> io::Result<Option<File>> {
    let opened = nt_open(
        dir,
        Some(name),
        FILE_GENERIC_READ,
        FILE_OPEN,
        NTCREATEFILE_CREATE_OPTIONS(0),
        FILE_FLAGS_AND_ATTRIBUTES(0),
    )?;
    let info = win::tag_info(&opened)?;
    if is_link(&info) {
        return Ok(None);
    }
    if is_placeholder(&info) {
        return through_placeholder(
            dir,
            name,
            opened,
            FILE_GENERIC_READ,
            NTCREATEFILE_CREATE_OPTIONS(0),
        )
        .map(Some);
    }
    Ok(Some(opened))
}

/// An entry opened to read or change its attributes only.
fn open_attributes_at(dir: &File, name: Option<&Name>, write: bool) -> io::Result<File> {
    let mut access = FILE_READ_ATTRIBUTES;
    if write {
        access |= FILE_WRITE_ATTRIBUTES;
    }
    nt_open(
        dir,
        name,
        access,
        FILE_OPEN,
        NTCREATEFILE_CREATE_OPTIONS(0),
        FILE_FLAGS_AND_ATTRIBUTES(0),
    )
}

/// What the entry `name` in `dir` is, not following a link, with a
/// directory's permission bits; `None` when there is none.
pub(crate) fn stat_at(dir: &File, name: &Name) -> io::Result<Option<(Kind, u32)>> {
    let opened = match open_attributes_at(dir, Some(name), false) {
        Ok(opened) => opened,
        Err(e) if is_missing(&e) => return Ok(None),
        Err(e) => return Err(e),
    };
    let info = win::tag_info(&opened)?;
    let kind = if is_link(&info) {
        Kind::Symlink
    } else if info.FileAttributes & windows::Win32::Storage::FileSystem::FILE_ATTRIBUTE_DIRECTORY.0
        != 0
    {
        Kind::Dir
    } else {
        Kind::File
    };
    Ok(Some((
        kind,
        crate::osfs::synthesized_mode(info.FileAttributes),
    )))
}

/// What the entry `name` in `dir` is and how large, not following a link;
/// `None` when there is none.
pub(crate) fn entry_facts(dir: &File, name: &Name) -> io::Result<Option<(Kind, u64)>> {
    let opened = match open_attributes_at(dir, Some(name), false) {
        Ok(opened) => opened,
        Err(e) if is_missing(&e) => return Ok(None),
        Err(e) => return Err(e),
    };
    let info = win::tag_info(&opened)?;
    let kind = if is_link(&info) {
        Kind::Symlink
    } else if info.FileAttributes & windows::Win32::Storage::FileSystem::FILE_ATTRIBUTE_DIRECTORY.0
        != 0
    {
        Kind::Dir
    } else {
        Kind::File
    };
    let size = match kind {
        Kind::File => win::standard_info(&opened)?.EndOfFile.max(0) as u64,
        _ => 0,
    };
    Ok(Some((kind, size)))
}

/// Whether the entry `name` in `dir` is a file, a link or a directory:
/// on Windows everything in a folder is one of the three.
pub(crate) fn is_tracked_kind(_dir: &File, _name: &Name) -> io::Result<bool> {
    Ok(true)
}

/// Where the link `name` in `dir` points.
pub(crate) fn readlink_at(dir: &File, name: &Name) -> io::Result<PathBuf> {
    let opened = open_attributes_at(dir, Some(name), false)?;
    let mut buf = vec![0u8; 16 * 1024];
    let mut returned = 0u32;
    // SAFETY: the handle is open and the buffer valid for its length.
    unsafe {
        DeviceIoControl(
            win::handle(&opened),
            FSCTL_GET_REPARSE_POINT,
            None,
            0,
            Some(buf.as_mut_ptr().cast()),
            buf.len() as u32,
            Some(&mut returned),
            None,
        )
    }
    .map_err(io_error)?;
    parse_reparse_target(&buf[..returned as usize]).ok_or_else(link_error)
}

/// The target a symbolic link or junction reparse buffer names, from its
/// print name (what the person wrote) or, without one, its substitute name
/// with the `\??\` prefix removed.
fn parse_reparse_target(buf: &[u8]) -> Option<PathBuf> {
    let u32_at = |i: usize| Some(u32::from_le_bytes(buf.get(i..i + 4)?.try_into().ok()?));
    let u16_at = |i: usize| Some(u16::from_le_bytes(buf.get(i..i + 2)?.try_into().ok()?));
    let tag = u32_at(0)?;
    // ReparseTag u32, ReparseDataLength u16, Reserved u16, then the
    // tag-specific part: four u16 offsets and lengths, flags for a symlink,
    // then the path buffer.
    let (names_at, flags) = match tag {
        IO_REPARSE_TAG_SYMLINK => (20usize, u32_at(16)?),
        IO_REPARSE_TAG_MOUNT_POINT => (16usize, 0),
        _ => return None,
    };
    let substitute = (u16_at(8)? as usize, u16_at(10)? as usize);
    let print = (u16_at(12)? as usize, u16_at(14)? as usize);
    let (offset, len) = if print.1 > 0 { print } else { substitute };
    let start = names_at + offset;
    let bytes = buf.get(start..start + len)?;
    let wide: Vec<u16> = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|c| u16::from_le_bytes(*c))
        .collect();
    let mut text = String::from_utf16_lossy(&wide);
    if let Some(stripped) = text.strip_prefix(r"\??\") {
        text = stripped.to_owned();
    }
    let _ = flags;
    Some(PathBuf::from(text))
}

/// Makes the directory `name` in `dir`.
pub(crate) fn mkdir_at(dir: &File, name: &Name, _mode: u32) -> io::Result<()> {
    nt_open(
        dir,
        Some(name),
        FILE_ACCESS_RIGHTS(FILE_LIST_DIRECTORY.0),
        FILE_CREATE,
        FILE_DIRECTORY_FILE,
        FILE_FLAGS_AND_ATTRIBUTES(0),
    )
    .map(drop)
}

/// Makes `name` in `dir` a symbolic link to `target`: an empty file or
/// folder made in place, then given the link's reparse data through its
/// own handle. Windows lets a link be made with Developer Mode or a
/// privilege; without them the error says so.
pub(crate) fn symlink_at(dir: &File, name: &Name, target: &Path) -> io::Result<()> {
    let text = target.to_string_lossy().replace('/', "\\");
    let relative = !target.is_absolute();
    let to_dir = {
        let resolved = plain_name(&text).ok().and_then(|n| {
            open_attributes_at(dir, Some(&n), false)
                .ok()
                .and_then(|f| f.metadata().ok())
        });
        match resolved {
            Some(meta) => meta.is_dir(),
            None => std::fs::metadata(target).is_ok_and(|m| m.is_dir()),
        }
    };
    let placeholder = nt_open(
        dir,
        Some(name),
        FILE_ACCESS_RIGHTS(FILE_WRITE_ATTRIBUTES.0 | DELETE.0),
        FILE_CREATE,
        if to_dir {
            FILE_DIRECTORY_FILE
        } else {
            FILE_NON_DIRECTORY_FILE
        },
        FILE_ATTRIBUTE_NORMAL,
    )?;
    let wide: Vec<u16> = text.encode_utf16().collect();
    let substitute: Vec<u16> = if relative {
        wide.clone()
    } else {
        format!(r"\??\{text}").encode_utf16().collect()
    };
    // REPARSE_DATA_BUFFER for a symbolic link: tag, data length, reserved,
    // substitute offset and length, print offset and length, flags, paths.
    let mut data: Vec<u8> = Vec::new();
    let path_bytes = (substitute.len() + wide.len()) * 2;
    data.extend_from_slice(&IO_REPARSE_TAG_SYMLINK.to_le_bytes());
    data.extend_from_slice(&((12 + path_bytes) as u16).to_le_bytes());
    data.extend_from_slice(&0u16.to_le_bytes());
    data.extend_from_slice(&0u16.to_le_bytes());
    data.extend_from_slice(&((substitute.len() * 2) as u16).to_le_bytes());
    data.extend_from_slice(&((substitute.len() * 2) as u16).to_le_bytes());
    data.extend_from_slice(&((wide.len() * 2) as u16).to_le_bytes());
    const SYMLINK_FLAG_RELATIVE: u32 = 1;
    data.extend_from_slice(&(if relative { SYMLINK_FLAG_RELATIVE } else { 0 }).to_le_bytes());
    for unit in substitute.iter().chain(wide.iter()) {
        data.extend_from_slice(&unit.to_le_bytes());
    }
    // SAFETY: the handle is open and the buffer valid for its length.
    let set = unsafe {
        DeviceIoControl(
            win::handle(&placeholder),
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
        let _ = dispose(&placeholder);
        return Err(win::explain_link_privilege(io_error(e)));
    }
    Ok(())
}

/// Marks an open entry for removal when its last handle closes.
fn dispose(file: &File) -> io::Result<()> {
    const DELETE_FLAG: u32 = 1;
    const POSIX_SEMANTICS: u32 = 2;
    let info = FILE_DISPOSITION_INFO_EX {
        Flags: FILE_DISPOSITION_INFO_EX_FLAGS(DELETE_FLAG | POSIX_SEMANTICS),
    };
    // SAFETY: the handle is open and the buffer is the size the class needs.
    let posix = unsafe {
        SetFileInformationByHandle(
            win::handle(file),
            FileDispositionInfoEx,
            (&info as *const FILE_DISPOSITION_INFO_EX).cast(),
            std::mem::size_of::<FILE_DISPOSITION_INFO_EX>() as u32,
        )
    };
    match posix.map_err(io_error) {
        Ok(()) => Ok(()),
        // A filesystem without POSIX removal: the older way.
        Err(e) if e.raw_os_error() == Some(ERROR_INVALID_PARAMETER) => {
            let delete: u8 = 1;
            // SAFETY: FILE_DISPOSITION_INFO is one BOOLEAN.
            unsafe {
                SetFileInformationByHandle(
                    win::handle(file),
                    windows::Win32::Storage::FileSystem::FileDispositionInfo,
                    (&delete as *const u8).cast(),
                    1,
                )
            }
            .map_err(io_error)
        }
        Err(e) => Err(e),
    }
}

/// Removes the entry `name` in `dir` that is `options` says (a folder or
/// not); one already gone is not an error. A read-only entry has that
/// attribute cleared first, as `rm -f` would.
fn remove_entry(dir: &File, name: &Name, options: NTCREATEFILE_CREATE_OPTIONS) -> io::Result<()> {
    let opened = match nt_open(
        dir,
        Some(name),
        FILE_ACCESS_RIGHTS(DELETE.0 | FILE_READ_ATTRIBUTES.0 | FILE_WRITE_ATTRIBUTES.0),
        FILE_OPEN,
        options,
        FILE_FLAGS_AND_ATTRIBUTES(0),
    ) {
        Ok(opened) => opened,
        Err(e) if is_missing(&e) => return Ok(()),
        Err(e) => return Err(e),
    };
    match dispose(&opened) {
        Ok(()) => Ok(()),
        Err(e) if e.raw_os_error() == Some(ERROR_ACCESS_DENIED) => {
            let current = win::basic_info(&opened)?;
            if current.FileAttributes & FILE_ATTRIBUTE_READONLY.0 == 0 {
                return Err(e);
            }
            let cleared = current.FileAttributes & !FILE_ATTRIBUTE_READONLY.0;
            win::set_basic_info(
                &opened,
                &FILE_BASIC_INFO {
                    CreationTime: 0,
                    LastAccessTime: 0,
                    LastWriteTime: 0,
                    ChangeTime: 0,
                    FileAttributes: if cleared == 0 {
                        FILE_ATTRIBUTE_NORMAL.0
                    } else {
                        cleared
                    },
                },
            )?;
            dispose(&opened)
        }
        Err(e) => Err(e),
    }
}

/// Removes the entry `name` in `dir` that is not a folder: a file, or a
/// link, a junction included, which Windows keeps as a folder and removes
/// as one. One already gone is not an error; a real folder is refused.
pub(crate) fn unlink_at(dir: &File, name: &Name) -> io::Result<()> {
    match remove_entry(dir, name, FILE_NON_DIRECTORY_FILE) {
        Ok(()) => Ok(()),
        Err(e) if e.raw_os_error() == Some(ERROR_ACCESS_DENIED) => {
            let is_link = match open_attributes_at(dir, Some(name), false) {
                Ok(opened) => is_link(&win::tag_info(&opened)?),
                Err(missing) if is_missing(&missing) => return Ok(()),
                Err(_) => false,
            };
            if is_link {
                remove_entry(dir, name, FILE_DIRECTORY_FILE)
            } else {
                Err(e)
            }
        }
        Err(e) => Err(e),
    }
}

/// Removes the empty folder `name` in `dir` (a junction counts as one and
/// goes as itself); one already gone is not an error.
pub(crate) fn rmdir_at(dir: &File, name: &Name) -> io::Result<()> {
    remove_entry(dir, name, FILE_DIRECTORY_FILE)
}

/// Renames `from` to `to`, both in `dir`, over a file or link at `to`:
/// through the NT call, which takes the folder's handle as the root the
/// new name is relative to.
pub(crate) fn rename_at(dir: &File, from: &Name, to: &Name) -> io::Result<()> {
    use windows::Wdk::Storage::FileSystem::{FILE_INFORMATION_CLASS, NtSetInformationFile};
    const FILE_RENAME_INFORMATION: FILE_INFORMATION_CLASS = FILE_INFORMATION_CLASS(10);
    const FILE_RENAME_INFORMATION_EX: FILE_INFORMATION_CLASS = FILE_INFORMATION_CLASS(65);
    let opened = nt_open(
        dir,
        Some(from),
        FILE_ACCESS_RIGHTS(DELETE.0),
        FILE_OPEN,
        NTCREATEFILE_CREATE_OPTIONS(0),
        FILE_FLAGS_AND_ATTRIBUTES(0),
    )?;
    const REPLACE_IF_EXISTS: u32 = 1;
    const POSIX_SEMANTICS: u32 = 2;
    let name_bytes = to.0.len() * 2;
    let header = std::mem::offset_of!(FILE_RENAME_INFO, FileName);
    let mut buf = vec![0u8; header + name_bytes + 2];
    // SAFETY: the buffer is at least a FILE_RENAME_INFO long; the fields
    // written are read by the kernel as plain bytes.
    unsafe {
        let info = buf.as_mut_ptr() as *mut FILE_RENAME_INFO;
        (*info).Anonymous.Flags = REPLACE_IF_EXISTS | POSIX_SEMANTICS;
        (*info).RootDirectory = HANDLE(dir.as_raw_handle());
        (*info).FileNameLength = name_bytes as u32;
        std::ptr::copy_nonoverlapping(
            to.0.as_ptr().cast::<u8>(),
            buf.as_mut_ptr().add(header),
            name_bytes,
        );
    }
    let set = |class: FILE_INFORMATION_CLASS, buf: &[u8]| {
        let mut status_block = IO_STATUS_BLOCK::default();
        // SAFETY: the handle is open and the buffer holds the structure.
        let status = unsafe {
            NtSetInformationFile(
                win::handle(&opened),
                &mut status_block,
                buf.as_ptr().cast(),
                buf.len() as u32,
                class,
            )
        };
        if status.is_err() {
            Err(nt_error(status))
        } else {
            Ok(())
        }
    };
    match set(FILE_RENAME_INFORMATION_EX, &buf) {
        Ok(()) => Ok(()),
        // A filesystem without POSIX renames: the older way, which still
        // replaces a file in the way.
        Err(e) if e.raw_os_error() == Some(ERROR_INVALID_PARAMETER) => {
            // SAFETY: as above; the first field reads as a BOOLEAN here.
            unsafe {
                let info = buf.as_mut_ptr() as *mut FILE_RENAME_INFO;
                (*info).Anonymous.Flags = 0;
                (*info).Anonymous.ReplaceIfExists = true;
            }
            set(FILE_RENAME_INFORMATION, &buf)
        }
        Err(e) => Err(e),
    }
}

/// A new file `name` in `dir`, open for writing: never one already there,
/// nor a link followed.
pub(crate) fn create_new_at(dir: &File, name: &Name, _mode: u32) -> io::Result<File> {
    nt_open(
        dir,
        Some(name),
        FILE_ACCESS_RIGHTS(FILE_GENERIC_WRITE.0 | FILE_READ_ATTRIBUTES.0 | DELETE.0),
        FILE_CREATE,
        FILE_NON_DIRECTORY_FILE,
        FILE_ATTRIBUTE_NORMAL,
    )
}

/// A new file, `name` in the directory `dir`, with what `file` holds:
/// cloned where the filesystem allows it (ReFS, a Dev Drive), copied
/// otherwise; never one already there, nor a link followed.
pub(crate) fn copy_into(file: &File, dir: &File, name: &Name) -> io::Result<()> {
    let mut out = create_new_at(dir, name, 0o666)?;
    let size = file.metadata()?.len();
    if size > 0 && clone_extents(file, &out, size).is_ok() {
        return Ok(());
    }
    let mut from = file;
    out.set_len(0)?;
    io::copy(&mut from, &mut out)?;
    out.flush()
}

/// Shares `size` bytes of `from`'s extents with `to`: a clone on a
/// filesystem that keeps extents per block (ReFS).
fn clone_extents(from: &File, to: &File, size: u64) -> io::Result<()> {
    let eof = FILE_END_OF_FILE_INFO {
        EndOfFile: size as i64,
    };
    // SAFETY: the handle is open and the buffer is the size the class needs.
    unsafe {
        SetFileInformationByHandle(
            win::handle(to),
            FileEndOfFileInfo,
            (&eof as *const FILE_END_OF_FILE_INFO).cast(),
            std::mem::size_of::<FILE_END_OF_FILE_INFO>() as u32,
        )
    }
    .map_err(io_error)?;
    // Whole clusters: the largest cluster ReFS uses is 64 KiB.
    const CLUSTER: u64 = 64 * 1024;
    let data = DUPLICATE_EXTENTS_DATA {
        FileHandle: win::handle(from),
        SourceFileOffset: 0,
        TargetFileOffset: 0,
        ByteCount: size.div_ceil(CLUSTER).saturating_mul(CLUSTER) as i64,
    };
    // SAFETY: both handles are open and the buffer is the structure.
    unsafe {
        DeviceIoControl(
            win::handle(to),
            FSCTL_DUPLICATE_EXTENTS_TO_FILE,
            Some((&data as *const DUPLICATE_EXTENTS_DATA).cast()),
            std::mem::size_of::<DUPLICATE_EXTENTS_DATA>() as u32,
            None,
            0,
            None,
            None,
        )
    }
    .map_err(io_error)
}

/// Gives the entry `name` in `dir` the permission bits `mode`, as Windows
/// keeps them (see `osfs::set_mode`), not following a link.
pub(crate) fn set_mode_at(dir: &File, name: &Name, mode: u32) -> io::Result<()> {
    let opened = open_attributes_at(dir, Some(name), true)?;
    win::apply_mode(&opened, mode)
}

/// Sets the modification time of the entry `name` in `dir` (of `dir`
/// itself without one), leaving the other times, not following a link.
pub(crate) fn set_mtime_at(dir: &File, name: Option<&Name>, mtime: (i64, i64)) -> io::Result<()> {
    let opened = open_attributes_at(dir, name, true)?;
    let info = FILE_BASIC_INFO {
        CreationTime: 0,
        LastAccessTime: 0,
        LastWriteTime: win::file_time(mtime),
        ChangeTime: 0,
        FileAttributes: 0,
    };
    win::set_basic_info(&opened, &info)
}

/// The names in the folder `dir`, but `.` and `..`.
pub(crate) fn names_in(dir: &File) -> io::Result<Vec<Name>> {
    let mut names = Vec::new();
    let mut buf = vec![0u8; 64 * 1024];
    let mut class = FileIdBothDirectoryRestartInfo;
    loop {
        // SAFETY: the handle is open and the buffer valid for its length.
        let got = unsafe {
            GetFileInformationByHandleEx(
                win::handle(dir),
                class,
                buf.as_mut_ptr().cast(),
                buf.len() as u32,
            )
        };
        if let Err(e) = got {
            let e = io_error(e);
            if e.raw_os_error() == Some(ERROR_NO_MORE_FILES) {
                return Ok(names);
            }
            return Err(e);
        }
        class = FileIdBothDirectoryInfo;
        let mut at = 0usize;
        loop {
            let header = std::mem::offset_of!(FILE_ID_BOTH_DIR_INFO, FileName);
            if at + header > buf.len() {
                break;
            }
            let u32_at = |i: usize| u32::from_le_bytes(buf[at + i..at + i + 4].try_into().unwrap());
            let next = u32_at(0) as usize;
            let name_len =
                u32_at(std::mem::offset_of!(FILE_ID_BOTH_DIR_INFO, FileNameLength)) as usize;
            let start = at + header;
            let end = (start + name_len).min(buf.len());
            let wide: Vec<u16> = buf[start..end]
                .as_chunks::<2>()
                .0
                .iter()
                .map(|c| u16::from_le_bytes(*c))
                .collect();
            if wide != [b'.' as u16] && wide != [b'.' as u16, b'.' as u16] {
                names.push(Name(wide));
            }
            if next == 0 {
                break;
            }
            at += next;
        }
    }
}

/// Windows does not stop writes into a read-only folder, so there is
/// nothing to make writable.
pub(crate) fn make_writable(_dir: &File) -> io::Result<Option<u32>> {
    Ok(None)
}

pub(crate) fn restore_writable(_dir: &File, _mode: u32) -> io::Result<()> {
    Ok(())
}

/// Windows has no call that clones a directory.
pub(crate) fn clone_dir_at_once(_src: &Path, _dst: &Path) -> bool {
    false
}

/// The file a lock across Kule's processes is taken on: made if missing,
/// private to the user, never followed if a link.
pub(crate) fn open_lock_file(path: &Path) -> io::Result<File> {
    use std::os::windows::fs::OpenOptionsExt;
    use windows::Win32::Storage::FileSystem::{FILE_FLAG_OPEN_REPARSE_POINT, WRITE_DAC};
    let open = |dacl: bool| {
        let mut access = FILE_GENERIC_READ.0 | FILE_GENERIC_WRITE.0;
        if dacl {
            access |= WRITE_DAC.0;
        }
        std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .access_mode(access)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT.0)
            .open(path)
    };
    let (file, owned) = match open(true) {
        Ok(file) => (file, true),
        Err(_) => (open(false)?, false),
    };
    if win::is_reparse(&file.metadata()?) {
        return Err(link_error());
    }
    if owned {
        win::make_private(&file, false)?;
    }
    Ok(file)
}

/// Takes `file`'s lock, which goes with the handle: waiting for it when
/// `wait`, otherwise `false` when another holds it.
pub(crate) fn lock(file: &File, wait: bool) -> io::Result<bool> {
    let mut flags = LOCKFILE_EXCLUSIVE_LOCK;
    if !wait {
        flags |= LOCKFILE_FAIL_IMMEDIATELY;
    }
    let mut overlapped = OVERLAPPED::default();
    // SAFETY: the handle is open; the overlapped block lives for the call.
    let locked = unsafe { LockFileEx(win::handle(file), flags, None, 1, 0, &mut overlapped) };
    match locked {
        Ok(()) => Ok(true),
        Err(e) => {
            let e = io_error(e);
            if !wait && e.raw_os_error() == Some(ERROR_LOCK_VIOLATION) {
                Ok(false)
            } else {
                Err(e)
            }
        }
    }
}
