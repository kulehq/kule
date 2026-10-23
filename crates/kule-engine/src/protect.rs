//! Protected paths the sandbox cannot keep read-only in place (Linux).
//!
//! Landlock only allows, and an allowance for a directory reaches every file
//! in it: a directory where an agent writes its state and keeps its
//! settings cannot be writable for one and not the other. Where the kernel
//! lets an unprivileged process have a mount namespace, the protected paths
//! that exist are mounted read-only for the command alone (see `namespace`)
//! and nothing needs putting back. What is left, a path that does not exist
//! yet, a link, or every protected path where namespaces are refused, is
//! kept the other way: noted when the first run that keeps it begins, and
//! put back, and said so, when each run ends. A change made in the meantime
//! only ever reaches the sandboxed agent itself, never what runs later
//! outside the sandbox.
//!
//! The notes live in Kule's home, where every Kule process finds them: a run
//! from the CLI and one from the daemon share the first one's note, never
//! what the other's agent made of the path since. A run whose process is
//! gone without putting back leaves its note to the next run. And what a
//! put-back replaces is moved aside first, not lost: it may be the person's
//! own change, made outside the sandbox while a run was going.

use std::collections::HashMap;
use std::ffi::OsString;
use std::fs::{self, File};
use std::io::Write;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::error::{Error, IoContext, Result};

/// The most a protected path may hold; more is refused, not noted.
const MAX_KEPT: usize = 64 << 20;

#[derive(Debug, Clone, PartialEq, Eq)]
enum Node {
    Absent,
    File {
        data: Vec<u8>,
        mode: u32,
    },
    Link(PathBuf),
    Dir {
        mode: u32,
        entries: HashMap<OsString, Node>,
    },
}

/// A protected path that a run left changed, and Kule put back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PutBack {
    pub path: PathBuf,
    /// Where what the path held instead was moved; `None` when it held
    /// nothing, the run having removed it.
    pub saved: Option<PathBuf>,
}

/// Protected paths noted for one run; [`Kept::restore`] puts them back
/// when the run is over. Dropped without that, it puts them back anyway.
#[derive(Debug)]
pub struct Kept {
    /// Where the notes are, shared by every Kule process of this home.
    dir: PathBuf,
    /// This run, among the runs that keep a path.
    holder: Holder,
    paths: Vec<PathBuf>,
}

impl Kept {
    /// Notes `paths` in `dir`, unless a run that keeps them is already
    /// going, or ended without putting back: then its note stands.
    pub(crate) fn new(dir: &Path, paths: Vec<PathBuf>) -> Result<Kept> {
        let holder = Holder::this_run();
        if !paths.is_empty() {
            let _lock = Lock::take(dir)?;
            for (i, path) in paths.iter().enumerate() {
                if let Err(e) = join(dir, path, &holder) {
                    for joined in &paths[..i] {
                        let _ = leave(dir, joined, &holder);
                    }
                    return Err(e);
                }
            }
        }
        Ok(Kept {
            dir: dir.to_owned(),
            holder,
            paths,
        })
    }

    /// Puts back each path that changed; returns those it put back. The
    /// agent must be gone: what it still runs could change them again.
    pub fn restore(mut self) -> Result<Vec<PutBack>> {
        let paths = std::mem::take(&mut self.paths);
        restore_all(&self.dir, &paths, &self.holder)
    }
}

impl Drop for Kept {
    fn drop(&mut self) {
        if !self.paths.is_empty() {
            let _ = restore_all(&self.dir, &self.paths, &self.holder);
        }
    }
}

fn restore_all(dir: &Path, paths: &[PathBuf], holder: &Holder) -> Result<Vec<PutBack>> {
    if paths.is_empty() {
        return Ok(Vec::new());
    }
    let _lock = Lock::take(dir)?;
    let aside = dir.join("replaced").join(stamp());
    let mut restored = Vec::new();
    let mut failed = None;
    for path in paths {
        match restore_one(dir, path, &aside) {
            Ok(Some(put_back)) => restored.push(put_back),
            Ok(None) => {}
            Err(e) => failed = failed.or(Some(e)),
        }
        if let Err(e) = leave(dir, path, holder) {
            failed = failed.or(Some(e));
        }
    }
    match failed {
        Some(e) => Err(e),
        None => Ok(restored),
    }
}

fn restore_one(dir: &Path, path: &Path, aside: &Path) -> Result<Option<PutBack>> {
    let noted = load(&entry(dir, path))?;
    if note(path).ok().as_ref() == Some(&noted) {
        return Ok(None);
    }
    no_link_above(path)?;
    let saved = set_aside(path, aside)?;
    put_back(path, &noted)?;
    Ok(Some(PutBack {
        path: path.to_owned(),
        saved,
    }))
}

/// Moves what is at `path` under `aside`, at its own absolute path; `None`
/// when nothing is there.
fn set_aside(path: &Path, aside: &Path) -> Result<Option<PathBuf>> {
    match fs::symlink_metadata(path) {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e).at(path),
    }
    let target = aside.join(path.strip_prefix("/").unwrap_or(path));
    if let Some(parent) = target.parent() {
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(parent)
            .at(parent)?;
    }
    match fs::rename(path, &target) {
        Ok(()) => Ok(Some(target)),
        // Kule's home on another file system: copied instead, as far as
        // Kule keeps anything; put_back removes the original.
        Err(e) if e.raw_os_error() == Some(libc::EXDEV) => match note(path) {
            Ok(node) => write(&target, &node).map(|()| Some(target)),
            Err(_) => Ok(None),
        },
        Err(e) => Err(e).at(path),
    }
}

/// One run among those that keep a path: its process, told apart from a
/// later one with the same id by when it started.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Holder {
    pid: i32,
    started: u64,
    run: u64,
}

impl Holder {
    fn this_run() -> Holder {
        static RUNS: AtomicU64 = AtomicU64::new(0);
        let pid = std::process::id() as i32;
        Holder {
            pid,
            started: started(pid).unwrap_or(0),
            run: RUNS.fetch_add(1, Ordering::Relaxed),
        }
    }

    fn parse(line: &str) -> Option<Holder> {
        let mut parts = line.split(' ');
        let holder = Holder {
            pid: parts.next()?.parse().ok()?,
            started: parts.next()?.parse().ok()?,
            run: parts.next()?.parse().ok()?,
        };
        parts.next().is_none().then_some(holder)
    }

    fn line(&self) -> String {
        format!("{} {} {}", self.pid, self.started, self.run)
    }

    /// Whether its process still runs: a run whose process is gone will
    /// never put back.
    fn alive(&self) -> bool {
        // SAFETY: signal 0 only asks whether the process exists.
        let exists = unsafe { libc::kill(self.pid, 0) } == 0
            || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM);
        exists && started(self.pid).unwrap_or(0) == self.started
    }
}

/// When process `pid` started, in clock ticks since boot (Linux); elsewhere
/// `None`, and a process is told apart by its id alone.
fn started(pid: i32) -> Option<u64> {
    if !cfg!(target_os = "linux") {
        return None;
    }
    let stat = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    // The name, in parentheses, may hold spaces; the start time is the
    // 22nd field, the 20th after it.
    let (_, rest) = stat.rsplit_once(')')?;
    rest.split_whitespace().nth(19)?.parse().ok()
}

/// Kule's lock on the notes, across its processes; released when dropped.
struct Lock {
    _held: crate::fsops::Flock,
}

impl Lock {
    fn take(dir: &Path) -> Result<Lock> {
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)
            .at(dir)?;
        Ok(Lock {
            _held: crate::fsops::Flock::take(&dir.join("lock"))?,
        })
    }
}

/// Where the note for `path` is kept.
fn entry(dir: &Path, path: &Path) -> PathBuf {
    let hash = blake3::hash(path.as_os_str().as_bytes()).to_hex();
    dir.join(&hash[..32])
}

/// Adds `holder` to the runs keeping `path`, noting the path first when no
/// note is there.
fn join(dir: &Path, path: &Path, holder: &Holder) -> Result<()> {
    let entry = entry(dir, path);
    if fs::symlink_metadata(&entry).is_ok() {
        let mut holders = holders(&entry)?;
        holders.push(holder.clone());
        return write_holders(&entry, &holders);
    }
    let node = note(path)?;
    // Made whole beside it, then moved in place: a note is never half there.
    let staging = entry.with_extension("partial");
    remove_tree(&staging)?;
    fs::DirBuilder::new()
        .mode(0o700)
        .create(&staging)
        .at(&staging)?;
    fs::write(staging.join("path"), path.as_os_str().as_bytes()).at(&staging)?;
    match &node {
        Node::Absent => File::create(staging.join("absent"))
            .map(drop)
            .at(&staging)?,
        node => write(&staging.join("note"), node)?,
    }
    write_holders(&staging, std::slice::from_ref(holder))?;
    fs::rename(&staging, &entry).at(&entry)
}

/// Takes `holder` off the runs keeping `path`; the note goes with the last
/// one still running.
fn leave(dir: &Path, path: &Path, holder: &Holder) -> Result<()> {
    let entry = entry(dir, path);
    if fs::symlink_metadata(&entry).is_err() {
        return Ok(());
    }
    let mut holders = holders(&entry)?;
    holders.retain(|h| h != holder);
    if holders.is_empty() {
        remove_tree(&entry)
    } else {
        write_holders(&entry, &holders)
    }
}

/// The runs keeping the path noted in `entry` that still run. When none
/// does, the note stays all the same, for the next run to put back.
fn holders(entry: &Path) -> Result<Vec<Holder>> {
    let path = entry.join("holders");
    let text = fs::read_to_string(&path).at(&path)?;
    Ok(text
        .lines()
        .filter_map(Holder::parse)
        .filter(Holder::alive)
        .collect())
}

fn write_holders(entry: &Path, holders: &[Holder]) -> Result<()> {
    let text: String = holders.iter().map(|h| h.line() + "\n").collect();
    let (path, partial) = (entry.join("holders"), entry.join("holders.partial"));
    fs::write(&partial, text).at(&partial)?;
    fs::rename(&partial, &path).at(&path)
}

/// The note made in `entry`.
fn load(entry: &Path) -> Result<Node> {
    let noted = entry.join("note");
    if fs::symlink_metadata(&noted).is_ok() {
        return note(&noted);
    }
    if fs::symlink_metadata(entry.join("absent")).is_ok() {
        return Ok(Node::Absent);
    }
    Err(Error::Invalid(format!(
        "the note in {} is incomplete",
        entry.display()
    )))
}

/// A name for one put-back's set-aside files: when, in UTC, and by whom.
fn stamp() -> String {
    static PUT_BACKS: AtomicU64 = AtomicU64::new(0);
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let (days, time) = (secs / 86_400, secs % 86_400);
    // Days since 1970-01-01 to a civil date (Howard Hinnant's algorithm).
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}{month:02}{day:02}T{:02}{:02}{:02}Z-{}-{}",
        time / 3600,
        time / 60 % 60,
        time % 60,
        std::process::id(),
        PUT_BACKS.fetch_add(1, Ordering::Relaxed)
    )
}

/// Removes `path` and all in it, whatever modes the note gave them.
fn remove_tree(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e).at(path),
        Ok(meta) if meta.is_dir() => {
            fs::set_permissions(path, fs::Permissions::from_mode(0o700)).at(path)?;
            for child in fs::read_dir(path).at(path)? {
                remove_tree(&child.at(path)?.path())?;
            }
            fs::remove_dir(path).at(path)
        }
        Ok(_) => fs::remove_file(path).at(path),
    }
}

/// Whether what is at `path` is small enough to note: a plugin directory
/// of hundreds of megabytes is not.
pub(crate) fn fits(path: &Path) -> bool {
    fn size(path: &Path, left: &mut u64) -> bool {
        let Ok(meta) = fs::symlink_metadata(path) else {
            return true;
        };
        if meta.is_dir() {
            let Ok(entries) = fs::read_dir(path) else {
                return true;
            };
            return entries.flatten().all(|e| size(&e.path(), left));
        }
        if meta.is_file() {
            match left.checked_sub(meta.len()) {
                Some(rest) => *left = rest,
                None => return false,
            }
        }
        true
    }
    let mut left = MAX_KEPT as u64;
    size(path, &mut left)
}

fn note(path: &Path) -> Result<Node> {
    let mut budget = MAX_KEPT;
    read(path, &mut budget)
}

/// What is at `path`, symbolic links not followed; `budget` is what may
/// still be read.
fn read(path: &Path, budget: &mut usize) -> Result<Node> {
    let meta = match fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Node::Absent),
        Err(e) => return Err(e).at(path),
    };
    let mode = meta.permissions().mode() & 0o7777;
    let kind = meta.file_type();
    if kind.is_symlink() {
        return Ok(Node::Link(fs::read_link(path).at(path)?));
    }
    if kind.is_dir() {
        let mut entries = HashMap::new();
        for entry in fs::read_dir(path).at(path)? {
            let entry = entry.at(path)?;
            entries.insert(entry.file_name(), read(&entry.path(), budget)?);
        }
        return Ok(Node::Dir { mode, entries });
    }
    if !kind.is_file() {
        return Err(Error::Invalid(format!(
            "{} is protected, and neither a file, a directory nor a link",
            path.display()
        )));
    }
    let size = usize::try_from(meta.len()).unwrap_or(usize::MAX);
    *budget = budget.checked_sub(size).ok_or_else(|| {
        Error::Invalid(format!(
            "{} is protected and holds more than Kule keeps ({} MiB)",
            path.display(),
            MAX_KEPT >> 20
        ))
    })?;
    let data = fs::read(path).at(path)?;
    Ok(Node::File { data, mode })
}

/// Refuses `path` when a directory on the way to it became a link: nothing
/// is moved or written through one.
fn no_link_above(path: &Path) -> Result<()> {
    for ancestor in path.ancestors().skip(1) {
        if fs::symlink_metadata(ancestor).is_ok_and(|m| m.file_type().is_symlink()) {
            return Err(Error::Invalid(format!(
                "cannot put back {}: {} became a link",
                path.display(),
                ancestor.display()
            )));
        }
    }
    Ok(())
}

/// Replaces whatever is at `path` with `node`. Nothing is written through
/// a link: what the agent left there is removed, not followed.
fn put_back(path: &Path, node: &Node) -> Result<()> {
    no_link_above(path)?;
    match fs::symlink_metadata(path) {
        // remove_dir_all does not follow links inside the directory.
        Ok(meta) if meta.is_dir() => fs::remove_dir_all(path).at(path)?,
        Ok(_) => fs::remove_file(path).at(path)?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e).at(path),
    }
    write(path, node)
}

fn write(path: &Path, node: &Node) -> Result<()> {
    match node {
        Node::Absent => Ok(()),
        Node::Link(target) => std::os::unix::fs::symlink(target, path).at(path),
        Node::File { data, mode } => {
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .custom_flags(libc::O_NOFOLLOW)
                .open(path)
                .at(path)?;
            file.write_all(data).at(path)?;
            file.set_permissions(fs::Permissions::from_mode(*mode))
                .at(path)
        }
        Node::Dir { mode, entries } => {
            fs::create_dir(path).at(path)?;
            fs::set_permissions(path, fs::Permissions::from_mode(0o700)).at(path)?;
            for (name, entry) in entries {
                write(&path.join(name), entry)?;
            }
            fs::set_permissions(path, fs::Permissions::from_mode(*mode)).at(path)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::ffi::OsStringExt;
    use std::process::Command;

    use tempfile::TempDir;

    use super::*;

    #[test]
    fn a_directory_too_large_to_note_is_told_apart() {
        let dir = TempDir::new().unwrap();
        let plugins = dir.path().join("plugins");
        fs::create_dir_all(plugins.join("cache")).unwrap();
        fs::write(plugins.join("installed.json"), "{}").unwrap();
        assert!(fits(&plugins));
        // Sparse: as large as Kule reads, without the disk.
        let big = fs::File::create(plugins.join("cache/bundle.js")).unwrap();
        big.set_len(MAX_KEPT as u64 + 1).unwrap();
        assert!(!fits(&plugins));
        assert!(fits(&dir.path().join("absent")));
    }

    /// A protected path and where Kule keeps its notes, both on the real
    /// file system.
    struct Setup {
        _dir: TempDir,
        root: PathBuf,
    }

    impl Setup {
        fn new() -> Setup {
            let dir = TempDir::new().unwrap();
            let root = fs::canonicalize(dir.path()).unwrap();
            Setup { _dir: dir, root }
        }

        fn notes(&self) -> PathBuf {
            self.root.join("kept")
        }

        fn path(&self, name: &str) -> PathBuf {
            self.root.join(name)
        }

        fn keep(&self, path: &Path) -> Kept {
            Kept::new(&self.notes(), vec![path.to_owned()]).unwrap()
        }

        /// The notes still standing, by the path each one keeps.
        fn standing(&self) -> Vec<PathBuf> {
            let Ok(entries) = fs::read_dir(self.notes()) else {
                return Vec::new();
            };
            entries
                .flatten()
                .filter_map(|e| fs::read(e.path().join("path")).ok())
                .map(|bytes| PathBuf::from(OsString::from_vec(bytes)))
                .collect()
        }

        /// Runs this test binary's `child` test in a process of its own,
        /// doing `what` with `path`.
        fn in_another_process(&self, what: &str, path: &Path) {
            let out = Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "protect::tests::child", "--nocapture"])
                .env("KULE_KEPT_CHILD", what)
                .env("KULE_KEPT_NOTES", self.notes())
                .env("KULE_KEPT_PATH", path)
                .output()
                .unwrap();
            assert!(out.status.success(), "{out:?}");
        }
    }

    /// What [`Setup::in_another_process`] runs; nothing on its own.
    #[test]
    fn child() {
        let Ok(what) = std::env::var("KULE_KEPT_CHILD") else {
            return;
        };
        let notes = PathBuf::from(std::env::var_os("KULE_KEPT_NOTES").unwrap());
        let path = PathBuf::from(std::env::var_os("KULE_KEPT_PATH").unwrap());
        let kept = Kept::new(&notes, vec![path.clone()]).unwrap();
        match what.as_str() {
            "run" => {
                kept.restore().unwrap();
            }
            // The agent changes the path and its run's process dies
            // before putting it back.
            "crash" => {
                fs::write(&path, "{\"hooks\":\"crash\"}\n").unwrap();
                std::mem::forget(kept);
            }
            other => panic!("{other}"),
        }
    }

    #[test]
    fn a_changed_file_is_put_back_and_what_it_held_is_saved() {
        let s = Setup::new();
        let settings = s.path("settings.json");
        fs::write(&settings, "{}\n").unwrap();
        let kept = s.keep(&settings);
        fs::write(&settings, "{\"theme\":\"mine\"}\n").unwrap();
        let put_back = kept.restore().unwrap();
        assert_eq!(fs::read_to_string(&settings).unwrap(), "{}\n");
        assert_eq!(put_back.len(), 1);
        assert_eq!(put_back[0].path, settings);
        let saved = put_back[0].saved.as_ref().unwrap();
        assert!(saved.starts_with(s.notes().join("replaced")), "{saved:?}");
        assert!(saved.ends_with(settings.strip_prefix("/").unwrap()));
        assert_eq!(fs::read_to_string(saved).unwrap(), "{\"theme\":\"mine\"}\n");
        assert!(s.standing().is_empty());
    }

    #[test]
    fn an_unchanged_path_is_left_alone() {
        let s = Setup::new();
        let settings = s.path("settings.json");
        fs::write(&settings, "{}\n").unwrap();
        let kept = s.keep(&settings);
        assert_eq!(s.standing(), std::slice::from_ref(&settings));
        assert!(kept.restore().unwrap().is_empty());
        assert!(s.standing().is_empty());
        assert!(!s.notes().join("replaced").exists());
    }

    #[test]
    fn a_path_the_agent_created_is_removed() {
        let s = Setup::new();
        let hooks = s.path("hooks");
        let kept = s.keep(&hooks);
        fs::create_dir_all(hooks.join("deep")).unwrap();
        fs::write(hooks.join("deep/run.sh"), "x").unwrap();
        let put_back = kept.restore().unwrap();
        assert_eq!(put_back.len(), 1);
        assert!(!hooks.exists());
        let saved = put_back[0].saved.as_ref().unwrap();
        assert_eq!(fs::read_to_string(saved.join("deep/run.sh")).unwrap(), "x");
    }

    #[test]
    fn a_path_the_agent_removed_comes_back_with_nothing_to_save() {
        let s = Setup::new();
        let settings = s.path("settings.json");
        fs::write(&settings, "{}\n").unwrap();
        let kept = s.keep(&settings);
        fs::remove_file(&settings).unwrap();
        let put_back = kept.restore().unwrap();
        assert_eq!(
            put_back,
            [PutBack {
                path: settings.clone(),
                saved: None
            }]
        );
        assert_eq!(fs::read_to_string(&settings).unwrap(), "{}\n");
    }

    #[test]
    fn a_directory_comes_back_whole() {
        let s = Setup::new();
        let hooks = s.path("hooks");
        fs::create_dir(&hooks).unwrap();
        fs::write(hooks.join("pre.sh"), "echo pre\n").unwrap();
        fs::set_permissions(hooks.join("pre.sh"), fs::Permissions::from_mode(0o755)).unwrap();
        let kept = s.keep(&hooks);
        fs::write(hooks.join("pre.sh"), "curl evil\n").unwrap();
        fs::write(hooks.join("post.sh"), "curl evil\n").unwrap();
        assert_eq!(kept.restore().unwrap().len(), 1);
        assert_eq!(
            fs::read_to_string(hooks.join("pre.sh")).unwrap(),
            "echo pre\n"
        );
        let mode = fs::metadata(hooks.join("pre.sh"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o755);
        assert!(!hooks.join("post.sh").exists());
    }

    #[test]
    fn a_file_replaced_by_a_link_is_not_written_through() {
        let s = Setup::new();
        let (settings, elsewhere) = (s.path("settings.json"), s.path("elsewhere"));
        fs::write(&settings, "{}\n").unwrap();
        fs::write(&elsewhere, "mine\n").unwrap();
        let kept = s.keep(&settings);
        fs::remove_file(&settings).unwrap();
        std::os::unix::fs::symlink(&elsewhere, &settings).unwrap();
        kept.restore().unwrap();
        assert!(
            !fs::symlink_metadata(&settings)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(fs::read_to_string(&settings).unwrap(), "{}\n");
        assert_eq!(fs::read_to_string(&elsewhere).unwrap(), "mine\n");
    }

    #[test]
    fn a_run_that_starts_later_keeps_what_the_first_one_noted() {
        let s = Setup::new();
        let settings = s.path("settings.json");
        fs::write(&settings, "{}\n").unwrap();
        let first = s.keep(&settings);
        // The first run's agent changes the file before the second starts.
        fs::write(&settings, "{\"hooks\":1}\n").unwrap();
        let second = s.keep(&settings);
        first.restore().unwrap();
        fs::write(&settings, "{\"hooks\":2}\n").unwrap();
        assert_eq!(second.restore().unwrap().len(), 1);
        assert_eq!(fs::read_to_string(&settings).unwrap(), "{}\n");
        // Both runs over: the next one notes the file afresh.
        fs::write(&settings, "{\"theme\":1}\n").unwrap();
        let third = s.keep(&settings);
        assert!(third.restore().unwrap().is_empty());
    }

    /// A run from the CLI and one from the daemon are two processes: the
    /// later one takes the earlier one's note, not what that run's agent
    /// made of the path since.
    #[test]
    fn a_run_in_another_process_keeps_what_the_first_one_noted() {
        let s = Setup::new();
        let settings = s.path("settings.json");
        fs::write(&settings, "{}\n").unwrap();
        let first = s.keep(&settings);
        fs::write(&settings, "{\"hooks\":1}\n").unwrap();
        s.in_another_process("run", &settings);
        assert_eq!(fs::read_to_string(&settings).unwrap(), "{}\n");
        assert_eq!(s.standing(), std::slice::from_ref(&settings));
        assert!(first.restore().unwrap().is_empty());
        assert!(s.standing().is_empty());
    }

    /// A run whose process died never put back; its note stands until the
    /// next run ends.
    #[test]
    fn a_run_whose_process_is_gone_leaves_its_note_to_the_next() {
        let s = Setup::new();
        let settings = s.path("settings.json");
        fs::write(&settings, "{}\n").unwrap();
        s.in_another_process("crash", &settings);
        assert_eq!(s.standing(), std::slice::from_ref(&settings));
        let next = s.keep(&settings);
        let put_back = next.restore().unwrap();
        assert_eq!(put_back.len(), 1);
        assert_eq!(fs::read_to_string(&settings).unwrap(), "{}\n");
        assert!(s.standing().is_empty());
    }

    #[test]
    fn a_dropped_note_still_puts_back() {
        let s = Setup::new();
        let settings = s.path("settings.json");
        fs::write(&settings, "{}\n").unwrap();
        {
            let _kept = s.keep(&settings);
            fs::write(&settings, "{\"hooks\":1}\n").unwrap();
        }
        assert_eq!(fs::read_to_string(&settings).unwrap(), "{}\n");
    }
}
