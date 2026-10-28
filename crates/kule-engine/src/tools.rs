//! The folders a person, or an administrator, lets every task read: a
//! toolchain Kule does not know (`/opt/corp/sdk`), named once after a
//! command was refused it. Read-only, for every folder's tasks; a project
//! beside a workspace is opened with `kule reveal` instead, for that folder
//! alone.
//!
//! The person's list lives in Kule's home, which no sandbox can see. The
//! administrator's lives in a file of the system's (see [`admin_file`]),
//! read only while root owns it and no one else can write it, or its
//! folder: a file anyone could edit would let anyone widen every task. Both
//! go through the one validator ([`crate::grants::validate`]) when they
//! are added and again before every run, so neither can name a path that
//! weakens Kule.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::fsops::{read_json, write_json};
use crate::grants::{Bounds, Source, validate};

pub const TOOLS_FILE: &str = "tools.json";

/// One folder the person lets every task read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tool {
    pub id: u64,
    /// Resolved when it was added; resolved again before each run.
    pub path: PathBuf,
    pub added_unix: u64,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct File {
    #[serde(default)]
    next: u64,
    #[serde(default)]
    tools: Vec<Tool>,
}

fn read(home: &Path) -> Result<File> {
    let path = home.join(TOOLS_FILE);
    if !path.is_file() {
        return Ok(File::default());
    }
    read_json(&path)
}

/// Every folder the person added in Kule's home `home`.
pub fn tools(home: &Path) -> Result<Vec<Tool>> {
    Ok(read(home)?.tools)
}

/// Lets every task read `path`, for the person whose home is `person`:
/// refused, with why, where the validator refuses it.
pub fn add_tool(home: &Path, path: &Path, person: &Path) -> Result<Tool> {
    let path = if path.is_absolute() {
        path.to_owned()
    } else {
        std::env::current_dir()
            .map_err(|e| Error::Invalid(format!("the current directory: {e}")))?
            .join(path)
    };
    let bounds = Bounds::for_person(home, person);
    let real = validate(&path, Source::Person, &bounds)
        .map_err(|why| Error::Invalid(format!("{}: {why}", path.display())))?;
    let mut file = read(home)?;
    if let Some(have) = file.tools.iter().find(|t| t.path == real) {
        return Ok(have.clone());
    }
    file.next += 1;
    let tool = Tool {
        id: file.next,
        path: real,
        added_unix: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
    };
    file.tools.push(tool.clone());
    std::fs::create_dir_all(home)
        .map_err(|e| Error::Invalid(format!("{}: {e}", home.display())))?;
    write_json(&home.join(TOOLS_FILE), &file)?;
    Ok(tool)
}

/// Takes the folder added as `id` back: whether there was one.
pub fn remove_tool(home: &Path, id: u64) -> Result<bool> {
    let mut file = read(home)?;
    let before = file.tools.len();
    file.tools.retain(|t| t.id != id);
    if file.tools.len() == before {
        return Ok(false);
    }
    write_json(&home.join(TOOLS_FILE), &file)?;
    Ok(true)
}

/// Where an administrator names the folders every task on this computer
/// may read.
pub fn admin_file() -> PathBuf {
    #[cfg(target_os = "macos")]
    return PathBuf::from("/Library/Application Support/Kule/tools.toml");
    #[cfg(target_os = "linux")]
    return PathBuf::from("/etc/kule/tools.toml");
    #[cfg(windows)]
    return std::env::var_os("ProgramData")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("C:\\ProgramData"))
        .join("Kule")
        .join("tools.toml");
    #[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
    return PathBuf::from("/etc/kule/tools.toml");
}

/// What the administrator's file names, or why it was not read.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AdminTools {
    pub paths: Vec<PathBuf>,
    /// The file is there but was not read, and why: shown by `kule doctor`
    /// and `kule tools list`.
    pub ignored: Option<String>,
}

/// The administrator's file.
#[derive(Debug, Deserialize)]
struct AdminFile {
    #[serde(default)]
    paths: Vec<PathBuf>,
}

/// [`admin_file`] read, where it may be trusted.
pub fn admin_tools() -> AdminTools {
    admin_tools_at(&admin_file())
}

/// The administrator's file at `path`: read only when root (an
/// administrator on Windows) owns it and its folder and no one else can
/// write either; otherwise left unread, with why.
pub(crate) fn admin_tools_at(path: &Path) -> AdminTools {
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return AdminTools::default();
    };
    let ignore = |why: &str| AdminTools {
        paths: Vec::new(),
        ignored: Some(format!("{} was not read: {why}", path.display())),
    };
    if meta.file_type().is_symlink() {
        return ignore("it is a link");
    }
    let Some(dir) = path.parent() else {
        return ignore("it is in no folder");
    };
    if let Err(why) = owned_by_admin(path, &meta) {
        return ignore(&why);
    }
    match std::fs::symlink_metadata(dir) {
        Ok(dir_meta) => {
            if let Err(why) = owned_by_admin(dir, &dir_meta) {
                return ignore(&format!("its folder {why}"));
            }
        }
        Err(e) => return ignore(&format!("its folder: {e}")),
    }
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) => return ignore(&e.to_string()),
    };
    match toml::from_str::<AdminFile>(&text) {
        Ok(file) => AdminTools {
            paths: file.paths,
            ignored: None,
        },
        Err(e) => ignore(&format!("not readable as TOML ({})", e.message())),
    }
}

/// Whether `meta` (of `path`) says root owns it and only root may write it.
#[cfg(unix)]
fn owned_by_admin(_path: &Path, meta: &std::fs::Metadata) -> std::result::Result<(), String> {
    use std::os::unix::fs::MetadataExt;
    if meta.uid() != 0 {
        return Err(format!("is owned by uid {}, not root", meta.uid()));
    }
    if meta.mode() & 0o022 != 0 {
        return Err("can be written by others than root".to_owned());
    }
    Ok(())
}

/// Whether an administrator (the Administrators group or SYSTEM) owns
/// `path`: a file a person made under `ProgramData` is theirs, and is not
/// read.
#[cfg(windows)]
fn owned_by_admin(path: &Path, _meta: &std::fs::Metadata) -> std::result::Result<(), String> {
    use windows::Win32::Foundation::{HLOCAL, LocalFree};
    use windows::Win32::Security::Authorization::{GetNamedSecurityInfoW, SE_FILE_OBJECT};
    use windows::Win32::Security::{
        IsWellKnownSid, OWNER_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, PSID,
        WinBuiltinAdministratorsSid, WinLocalSystemSid,
    };
    use windows::core::PCWSTR;
    let wide: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let mut owner = PSID::default();
    let mut descriptor = PSECURITY_DESCRIPTOR::default();
    // SAFETY: the name is NUL-terminated; the descriptor the call allocates
    // is freed below, after the owner it points into was read.
    let admin = unsafe {
        let status = GetNamedSecurityInfoW(
            PCWSTR(wide.as_ptr()),
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION,
            Some(&mut owner),
            None,
            None,
            None,
            &mut descriptor,
        );
        if status.is_err() {
            return Err(format!("its owner could not be read ({})", status.0));
        }
        let admin = IsWellKnownSid(owner, WinBuiltinAdministratorsSid).as_bool()
            || IsWellKnownSid(owner, WinLocalSystemSid).as_bool();
        let _ = LocalFree(Some(HLOCAL(descriptor.0)));
        admin
    };
    if admin {
        Ok(())
    } else {
        Err("is not owned by an administrator".to_owned())
    }
}

#[cfg(windows)]
use std::os::windows::ffi::OsStrExt;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_the_person_owns_is_not_read() {
        let dir = tempfile::TempDir::new().unwrap();
        let file = dir.path().join("tools.toml");
        std::fs::write(&file, "paths = [\"/opt/sdk\"]\n").unwrap();
        let read = admin_tools_at(&file);
        // A file this test made is the person's, unless the test runs as an
        // administrator on Windows, whose files are the administrators'.
        let admins = owned_by_admin(&file, &std::fs::metadata(&file).unwrap()).is_ok()
            && owned_by_admin(dir.path(), &std::fs::metadata(dir.path()).unwrap()).is_ok();
        if admins {
            assert_eq!(read.paths, vec![PathBuf::from("/opt/sdk")]);
            assert!(read.ignored.is_none(), "{read:?}");
        } else {
            assert!(read.paths.is_empty());
            assert!(
                read.ignored
                    .as_deref()
                    .is_some_and(|w| w.contains("not read")),
                "{read:?}"
            );
        }
        // No file: nothing, and nothing to say.
        assert_eq!(
            admin_tools_at(&dir.path().join("none.toml")),
            AdminTools::default()
        );
    }

    #[cfg(unix)]
    #[test]
    fn roots_file_must_not_be_writable_by_others() {
        use std::os::unix::fs::MetadataExt;
        let dir = tempfile::TempDir::new().unwrap();
        let file = dir.path().join("tools.toml");
        std::fs::write(&file, "").unwrap();
        let meta = std::fs::metadata(&file).unwrap();
        // Our own file: not root's.
        assert!(
            owned_by_admin(&file, &meta)
                .unwrap_err()
                .contains("not root")
        );
        // Root's files on this system: `/etc/passwd` is root's and not
        // writable by others; a world-writable path (`/tmp`) is refused.
        if let Ok(passwd) = std::fs::metadata("/etc/passwd")
            && passwd.uid() == 0
        {
            assert!(owned_by_admin(Path::new("/etc/passwd"), &passwd).is_ok());
        }
        if let Ok(tmp) = std::fs::metadata("/tmp")
            && tmp.uid() == 0
        {
            assert!(
                owned_by_admin(Path::new("/tmp"), &tmp)
                    .unwrap_err()
                    .contains("written by others")
            );
        }
    }

    #[test]
    fn the_admin_file_is_toml_with_paths() {
        let file: AdminFile = toml::from_str("paths = [\"/opt/sdk\", \"/srv/tools\"]\n").unwrap();
        assert_eq!(
            file.paths,
            vec![PathBuf::from("/opt/sdk"), PathBuf::from("/srv/tools")]
        );
        let empty: AdminFile = toml::from_str("").unwrap();
        assert!(empty.paths.is_empty());
    }
}
