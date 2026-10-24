//! Package caches of a task's own.
//!
//! What a package manager finds in its cache it takes as already checked:
//! cargo builds an unpacked crate it finds under `registry/src`, pip
//! installs a cached wheel, Go links a cached object. A command that could
//! write to the person's caches could leave code there that runs later,
//! outside the sandbox. So the person's caches stay read-only, and the tools
//! are pointed at caches in the task's tmp instead, by the variables they
//! read. Where the filesystem clones a directory in one go (APFS), the
//! cargo and npm caches start as clones of the person's; elsewhere, and for
//! the others, they start empty and fill through the proxy.

use std::ffi::OsString;
use std::fs::File;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use crate::error::{Error, IoContext, Result};
use crate::fsops;
use crate::manifest::Kind;

/// Where the person's caches are, to read from or to clone.
#[derive(Debug, Clone)]
pub struct Sources {
    pub home: PathBuf,
    /// `CARGO_HOME`, or `~/.cargo`.
    pub cargo_home: PathBuf,
}

impl Sources {
    /// The person's: `HOME`, and `CARGO_HOME` where it is set.
    pub fn from_env() -> Option<Sources> {
        let home = PathBuf::from(std::env::var_os("HOME")?);
        let cargo_home = std::env::var_os("CARGO_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".cargo"));
        Some(Sources { home, cargo_home })
    }
}

/// Directory, under the task's tmp, for each variable that names a cache.
const PLAIN: &[(&str, &str)] = &[
    ("XDG_CACHE_HOME", "xdg"),
    ("PIP_CACHE_DIR", "pip"),
    ("UV_CACHE_DIR", "uv"),
    ("GOCACHE", "go-build"),
    ("GOMODCACHE", "go-mod"),
    ("YARN_CACHE_FOLDER", "yarn"),
    ("NUGET_PACKAGES", "nuget"),
];

/// pnpm's home, under the task's tmp: up to pnpm 10 its store is the
/// `store` folder there. pnpm 11 reads the store from its own variable, and
/// no longer from `npm_config_store_dir`, which npm 11 warns about.
const PNPM: &str = "pnpm";

/// What cargo needs from its home besides the caches: settings, never
/// `credentials.toml`.
const CARGO_SETTINGS: &[&str] = &["config.toml", "config"];

/// What a command that has to wait for its task's caches is told: another
/// Kule process (the app, as the task was made) is making them.
pub const STILL_PREPARING: &str = "Kule is still preparing this task's package caches (a copy of \
                                   yours); this starts once they are ready";

/// What a command is told as it makes its task's caches itself, when there
/// is something of the person's to copy.
pub const PREPARING: &str =
    "preparing this task's package caches (a copy of yours, once per task)…";

/// [`prepare`] under the task's `lock` (a file in the task's folder, out of
/// the sandbox's reach), so two Kule processes never make the same caches
/// at once: one that finds the lock held is told [`STILL_PREPARING`] and
/// waits, one that copies the person's caches itself is told [`PREPARING`].
pub(crate) fn prepare_locked(
    lock: &Path,
    tmp: &Path,
    sources: &Sources,
    tell: &mut dyn FnMut(&str),
) -> Result<Vec<(String, OsString)>> {
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(lock)
        .at(lock)?;
    if !flock(&file, false).at(lock)? {
        tell(STILL_PREPARING);
        flock(&file, true).at(lock)?;
    }
    if to_copy(tmp, sources) {
        tell(PREPARING);
    }
    // The lock goes with `file`, once the caches are in place.
    prepare(tmp, sources)
}

/// Takes `file`'s lock, which goes with the file or its process (flock):
/// waiting for it when `wait`, otherwise `false` when another holds it.
pub(crate) fn flock(file: &File, wait: bool) -> std::io::Result<bool> {
    use std::os::fd::AsRawFd;
    let how = if wait {
        libc::LOCK_EX
    } else {
        libc::LOCK_EX | libc::LOCK_NB
    };
    loop {
        // SAFETY: `file` is an open descriptor we own for the call.
        if unsafe { libc::flock(file.as_raw_fd(), how) } == 0 {
            return Ok(true);
        }
        let e = std::io::Error::last_os_error();
        match e.raw_os_error() {
            Some(libc::EINTR) => continue,
            Some(libc::EWOULDBLOCK) if !wait => return Ok(false),
            _ => return Err(e),
        }
    }
}

/// Whether making the caches under `tmp` copies some of the person's:
/// where the filesystem clones folders, for a cache not made yet whose
/// source is there.
fn to_copy(tmp: &Path, sources: &Sources) -> bool {
    let root = tmp.join("caches");
    let missing = |dir: &str| std::fs::symlink_metadata(root.join(dir)).is_err();
    let there = |path: PathBuf| std::fs::symlink_metadata(path).is_ok_and(|m| m.is_dir());
    cfg!(target_os = "macos")
        && ((missing("cargo")
            && (there(sources.cargo_home.join("registry"))
                || there(sources.cargo_home.join("git"))))
            || (missing("npm") && there(sources.home.join(".npm"))))
}

/// Prepares the caches under `tmp` (on first use) and returns the variables
/// that point the tools there. The tmp is the sandbox's to write to, so
/// every folder here is made and opened without following a link put on
/// the way, and written to through its descriptor.
pub(crate) fn prepare(tmp: &Path, sources: &Sources) -> Result<Vec<(String, OsString)>> {
    let root = tmp.join("caches");
    let caches = fsops::kept_dir(tmp, "caches")?;
    let mut env = Vec::new();

    let cargo = root.join("cargo");
    seed(&caches, &cargo, |staging| {
        for dir in ["registry", "git"] {
            clone_dir(&sources.cargo_home.join(dir), staging, dir, &cargo)?;
        }
        for file in CARGO_SETTINGS {
            let from = sources.cargo_home.join(file);
            if from.is_file() {
                let mode = std::fs::metadata(&from).at(&from)?.permissions().mode() & 0o777;
                let bytes = std::fs::read(&from).at(&from)?;
                fsops::put_at(staging, file, &bytes, mode, &cargo.join(file))?;
            }
        }
        Ok(())
    })?;
    env.push(("CARGO_HOME".to_owned(), cargo.into_os_string()));

    let npm = root.join("npm");
    // Cloned whole where it can be; otherwise it starts empty.
    seed_clone(&caches, &npm, &sources.home.join(".npm"))?;
    env.push(("npm_config_cache".to_owned(), npm.into_os_string()));

    for (var, dir) in PLAIN {
        let path = root.join(dir);
        fsops::kept_child(&caches, dir, &path)?;
        env.push(((*var).to_owned(), path.into_os_string()));
    }
    let pnpm = root.join(PNPM);
    let store = pnpm.join("store");
    fsops::kept_child(&fsops::kept_child(&caches, PNPM, &pnpm)?, "store", &store)?;
    env.push(("PNPM_HOME".to_owned(), pnpm.into_os_string()));
    env.push(("pnpm_config_store_dir".to_owned(), store.into_os_string()));
    // NuGet still finds what the person has: read-only, as a fallback.
    let nuget = sources.home.join(".nuget/packages");
    if nuget.is_dir() {
        env.push((
            "RestoreAdditionalProjectFallbackFolders".to_owned(),
            nuget.into_os_string(),
        ));
    }
    Ok(env)
}

/// Creates the folder `dir` in `caches` with `fill`, unless a folder is
/// there: filled under a staging name of its own beside it and renamed
/// into place, so a run that stops halfway leaves nothing half made.
fn seed(caches: &File, dir: &Path, fill: impl FnOnce(&File) -> Result<()>) -> Result<()> {
    let Some(staging) = staging(caches, dir)? else {
        return Ok(());
    };
    let kept = fsops::kept_child(caches, &staging, dir)?;
    fill(&kept)?;
    place(caches, &staging, dir)
}

/// [`seed`], with `dir` a clone of `from` where the filesystem clones a
/// folder in one go, and empty otherwise.
fn seed_clone(caches: &File, dir: &Path, from: &Path) -> Result<()> {
    let Some(staging) = staging(caches, dir)? else {
        return Ok(());
    };
    clone_dir(from, caches, &staging, dir)?;
    place(caches, &staging, dir)
}

/// The staging name for `dir` in `caches`, or `None` when the folder is
/// there already. A link or a file the sandbox put in its place is taken
/// out, and so is what a run that stopped halfway left at the staging name.
fn staging(caches: &File, dir: &Path) -> Result<Option<String>> {
    let name = file_name(dir)?;
    match fsops::entry_at(caches, name).at(dir)? {
        Some(Kind::Dir) => return Ok(None),
        Some(_) => fsops::remove_at(caches, name).at(dir)?,
        None => {}
    }
    let staging = format!("{name}.partial");
    fsops::remove_tree_at(caches, &staging).at(&dir.with_extension("partial"))?;
    Ok(Some(staging))
}

fn place(caches: &File, staging: &str, dir: &Path) -> Result<()> {
    fsops::rename_at(caches, staging, file_name(dir)?).at(dir)
}

fn file_name(dir: &Path) -> Result<&str> {
    dir.file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| Error::Invalid(format!("{} has no plain name", dir.display())))
}

/// Makes `name` in `parent` a clone of the folder `from` where the
/// filesystem does that in one go, and an empty folder otherwise or when
/// `from` is missing. `shown` names it in errors.
fn clone_dir(from: &Path, parent: &File, name: &str, shown: &Path) -> Result<()> {
    if std::fs::symlink_metadata(from).is_ok_and(|m| m.is_dir())
        && platform_clone(from, parent, name)
    {
        return Ok(());
    }
    fsops::kept_child(parent, name, shown).map(drop)
}

#[cfg(target_os = "macos")]
fn platform_clone(from: &Path, parent: &File, name: &str) -> bool {
    use std::ffi::CString;
    use std::os::fd::AsRawFd;
    use std::os::unix::ffi::OsStrExt;

    let (Ok(src), Ok(dst)) = (
        CString::new(from.as_os_str().as_bytes()),
        CString::new(name.as_bytes()),
    ) else {
        return false;
    };
    // From <sys/clonefile.h>: links inside are cloned as links, not followed.
    const CLONE_NOFOLLOW: u32 = 0x0001;
    // SAFETY: both are valid NUL-terminated paths for the call's duration,
    // and `parent` a directory descriptor we own. Another volume, or no
    // APFS, fails: the folder then starts empty.
    unsafe {
        libc::clonefileat(
            libc::AT_FDCWD,
            src.as_ptr(),
            parent.as_raw_fd(),
            dst.as_ptr(),
            CLONE_NOFOLLOW,
        ) == 0
    }
}

#[cfg(not(target_os = "macos"))]
fn platform_clone(_from: &Path, _parent: &File, _name: &str) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::*;

    fn person() -> (TempDir, Sources) {
        let home = TempDir::new().unwrap();
        let root = fs::canonicalize(home.path()).unwrap();
        let cargo_home = root.join(".cargo");
        fs::create_dir_all(cargo_home.join("registry/src/serde-1.0.0")).unwrap();
        fs::write(cargo_home.join("registry/src/serde-1.0.0/lib.rs"), "real\n").unwrap();
        fs::write(cargo_home.join("config.toml"), "[net]\n").unwrap();
        fs::write(cargo_home.join("credentials.toml"), "token = \"secret\"\n").unwrap();
        fs::create_dir_all(root.join(".npm/_cacache")).unwrap();
        (
            home,
            Sources {
                home: root,
                cargo_home,
            },
        )
    }

    fn var<'a>(env: &'a [(String, OsString)], name: &str) -> &'a Path {
        Path::new(&env.iter().find(|(n, _)| n == name).unwrap().1)
    }

    #[test]
    fn every_cache_variable_points_into_the_task() {
        let (_home, sources) = person();
        let tmp = TempDir::new().unwrap();
        let env = prepare(tmp.path(), &sources).unwrap();
        for (name, value) in &env {
            if name == "RestoreAdditionalProjectFallbackFolders" {
                continue;
            }
            assert!(Path::new(value).starts_with(tmp.path()), "{name}");
            assert!(Path::new(value).is_dir(), "{name}");
        }
        for name in [
            "CARGO_HOME",
            "npm_config_cache",
            "XDG_CACHE_HOME",
            "GOCACHE",
            "PNPM_HOME",
            "pnpm_config_store_dir",
        ] {
            assert!(env.iter().any(|(n, _)| n == name), "{name}");
        }
        // npm warns about every variable of its prefix it does not know.
        assert!(!env.iter().any(|(n, _)| n == "npm_config_store_dir"));
    }

    #[test]
    fn cargo_gets_its_settings_but_never_the_credentials() {
        let (_home, sources) = person();
        let tmp = TempDir::new().unwrap();
        let env = prepare(tmp.path(), &sources).unwrap();
        let cargo = var(&env, "CARGO_HOME");
        assert_eq!(
            fs::read_to_string(cargo.join("config.toml")).unwrap(),
            "[net]\n"
        );
        assert!(!cargo.join("credentials.toml").exists());
    }

    #[test]
    fn a_write_to_the_tasks_cache_leaves_the_persons_alone() {
        let (_home, sources) = person();
        let tmp = TempDir::new().unwrap();
        let env = prepare(tmp.path(), &sources).unwrap();
        let cargo = var(&env, "CARGO_HOME");
        let unpacked = cargo.join("registry/src/serde-1.0.0/lib.rs");
        if cfg!(target_os = "macos") {
            // Cloned from the person's, then changed in the task only.
            assert_eq!(fs::read_to_string(&unpacked).unwrap(), "real\n");
        }
        fs::create_dir_all(unpacked.parent().unwrap()).unwrap();
        fs::write(&unpacked, "poisoned\n").unwrap();
        assert_eq!(
            fs::read_to_string(sources.cargo_home.join("registry/src/serde-1.0.0/lib.rs")).unwrap(),
            "real\n"
        );
    }

    #[test]
    fn the_caches_are_made_once_per_task() {
        let (_home, sources) = person();
        let tmp = TempDir::new().unwrap();
        let env = prepare(tmp.path(), &sources).unwrap();
        let marker = var(&env, "CARGO_HOME").join("downloaded");
        fs::write(&marker, "x").unwrap();
        prepare(tmp.path(), &sources).unwrap();
        assert!(marker.exists());
    }

    #[test]
    fn a_half_made_cache_is_made_again() {
        let (_home, sources) = person();
        let tmp = TempDir::new().unwrap();
        let staging = tmp.path().join("caches/cargo.partial");
        fs::create_dir_all(&staging).unwrap();
        fs::write(staging.join("leftover"), "x").unwrap();
        let env = prepare(tmp.path(), &sources).unwrap();
        assert!(!staging.exists());
        assert!(!var(&env, "CARGO_HOME").join("leftover").exists());
    }

    /// One that finds the caches being made waits for them, and is told;
    /// one that copies the person's itself is told that, once.
    #[test]
    fn a_wait_for_the_caches_is_told() {
        let (_home, sources) = person();
        let (dir, tmp) = (TempDir::new().unwrap(), TempDir::new().unwrap());
        let lock = dir.path().join("caches.lock");
        let held = File::create(&lock).unwrap();
        assert!(flock(&held, false).unwrap());
        let (tx, rx) = std::sync::mpsc::channel::<String>();
        let waiting = {
            let (lock, tmp, sources) = (lock.clone(), tmp.path().to_owned(), sources.clone());
            std::thread::spawn(move || {
                prepare_locked(&lock, &tmp, &sources, &mut |m| {
                    let _ = tx.send(m.to_owned());
                })
                .unwrap();
            })
        };
        let told = rx.recv_timeout(std::time::Duration::from_secs(10)).unwrap();
        assert_eq!(told, STILL_PREPARING);
        // It waits while the lock is held.
        std::thread::sleep(std::time::Duration::from_millis(300));
        assert!(!waiting.is_finished());
        assert!(!tmp.path().join("caches/npm").exists());
        drop(held);
        waiting.join().unwrap();
        let rest: Vec<String> = rx.try_iter().collect();
        if cfg!(target_os = "macos") {
            assert_eq!(rest, [PREPARING]);
        } else {
            // Nothing of the person's is copied there: nothing to tell.
            assert!(rest.is_empty(), "{rest:?}");
        }
        assert!(tmp.path().join("caches/npm").is_dir());
        // Made once: the next one is told nothing.
        let mut told = Vec::new();
        prepare_locked(&lock, tmp.path(), &sources, &mut |m| {
            told.push(m.to_owned())
        })
        .unwrap();
        assert!(told.is_empty(), "{told:?}");
    }

    /// Two made at once for the same task make it once, whole.
    #[test]
    fn two_at_once_make_the_caches_once() {
        let (_home, sources) = person();
        let (dir, tmp) = (TempDir::new().unwrap(), TempDir::new().unwrap());
        let lock = dir.path().join("caches.lock");
        let both: Vec<_> = (0..2)
            .map(|_| {
                let (lock, tmp, sources) = (lock.clone(), tmp.path().to_owned(), sources.clone());
                std::thread::spawn(move || prepare_locked(&lock, &tmp, &sources, &mut |_| {}))
            })
            .collect();
        for one in both {
            one.join().unwrap().unwrap();
        }
        let names: Vec<String> = fs::read_dir(tmp.path().join("caches"))
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert!(!names.iter().any(|n| n.ends_with(".partial")), "{names:?}");
        if cfg!(target_os = "macos") {
            let lib = tmp
                .path()
                .join("caches/cargo/registry/src/serde-1.0.0/lib.rs");
            assert_eq!(fs::read_to_string(lib).unwrap(), "real\n");
        }
    }
}
