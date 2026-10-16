//! Protected paths where the sandbox cannot keep them read-only (Linux).
//!
//! Landlock only allows, and an allowance for a directory reaches every file
//! in it; a directory where an agent writes its state and keeps its settings
//! cannot be writable for one and not the other. So there the settings are
//! kept the other way: what they were when the first run began is noted, and
//! every run, when it ends, puts them back and says so. A change made in
//! the meantime only ever reaches the sandboxed agent itself, never what
//! runs later outside the sandbox.

use std::collections::{BTreeMap, HashMap};
use std::ffi::OsString;
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::error::{Error, IoContext, Result};

/// The most a protected path may hold; more is refused, not noted.
const MAX_KEPT: usize = 64 << 20;

/// What each protected path held before the first of the runs that keep it
/// began, and how many of those runs are still going. A run that starts
/// while another has the path open takes the first one's note, not what
/// that run's agent may have made of the path since.
static KEPT: Mutex<BTreeMap<PathBuf, (usize, Node)>> = Mutex::new(BTreeMap::new());

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

/// Protected paths noted for one run; [`Kept::restore`] puts them back
/// when the run is over. Dropped without that, it puts them back anyway.
#[derive(Debug)]
pub struct Kept {
    paths: Vec<PathBuf>,
}

impl Kept {
    /// Notes `paths`, unless a run that keeps them is already going.
    pub(crate) fn new(paths: Vec<PathBuf>) -> Result<Kept> {
        let mut kept = KEPT.lock().unwrap_or_else(|e| e.into_inner());
        let mut noted = Vec::new();
        for path in &paths {
            if let Some((runs, _)) = kept.get_mut(path) {
                *runs += 1;
            } else {
                match note(path) {
                    Ok(node) => {
                        kept.insert(path.clone(), (1, node));
                    }
                    Err(e) => {
                        release(&mut kept, &noted);
                        return Err(e);
                    }
                }
            }
            noted.push(path.clone());
        }
        Ok(Kept { paths })
    }

    /// Puts back each path that changed; returns those it put back. The
    /// agent must be gone: what it still runs could change them again.
    pub fn restore(mut self) -> Result<Vec<PathBuf>> {
        let paths = std::mem::take(&mut self.paths);
        restore_all(&paths)
    }
}

impl Drop for Kept {
    fn drop(&mut self) {
        if !self.paths.is_empty() {
            let _ = restore_all(&self.paths);
        }
    }
}

fn restore_all(paths: &[PathBuf]) -> Result<Vec<PathBuf>> {
    let mut kept = KEPT.lock().unwrap_or_else(|e| e.into_inner());
    let mut restored = Vec::new();
    let mut failed = None;
    for path in paths {
        let Some((_, node)) = kept.get(path) else {
            continue;
        };
        let now = note(path).ok();
        if now.as_ref() != Some(node) {
            match put_back(path, node) {
                Ok(()) => restored.push(path.clone()),
                Err(e) => failed = failed.or(Some(e)),
            }
        }
    }
    release(&mut kept, paths);
    match failed {
        Some(e) => Err(e),
        None => Ok(restored),
    }
}

fn release(kept: &mut BTreeMap<PathBuf, (usize, Node)>, paths: &[PathBuf]) {
    for path in paths {
        if let Some((runs, _)) = kept.get_mut(path) {
            *runs -= 1;
            if *runs == 0 {
                kept.remove(path);
            }
        }
    }
}

fn note(path: &Path) -> Result<Node> {
    let mut budget = MAX_KEPT;
    read(path, &mut budget)
}

/// What is at `path`, symbolic links not followed; `budget` is what may
/// still be read.
fn read(path: &Path, budget: &mut usize) -> Result<Node> {
    let meta = match std::fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Node::Absent),
        Err(e) => return Err(e).at(path),
    };
    let mode = meta.permissions().mode() & 0o7777;
    let kind = meta.file_type();
    if kind.is_symlink() {
        return Ok(Node::Link(std::fs::read_link(path).at(path)?));
    }
    if kind.is_dir() {
        let mut entries = HashMap::new();
        for entry in std::fs::read_dir(path).at(path)? {
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
    let data = std::fs::read(path).at(path)?;
    Ok(Node::File { data, mode })
}

/// Replaces whatever is at `path` with `node`. Nothing is written through
/// a link: what the agent left there is removed, not followed.
fn put_back(path: &Path, node: &Node) -> Result<()> {
    for ancestor in path.ancestors().skip(1) {
        if std::fs::symlink_metadata(ancestor).is_ok_and(|m| m.file_type().is_symlink()) {
            return Err(Error::Invalid(format!(
                "cannot put back {}: {} became a link",
                path.display(),
                ancestor.display()
            )));
        }
    }
    match std::fs::symlink_metadata(path) {
        // remove_dir_all does not follow links inside the directory.
        Ok(meta) if meta.is_dir() => std::fs::remove_dir_all(path).at(path)?,
        Ok(_) => std::fs::remove_file(path).at(path)?,
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
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .custom_flags(libc::O_NOFOLLOW)
                .open(path)
                .at(path)?;
            file.write_all(data).at(path)?;
            file.set_permissions(std::fs::Permissions::from_mode(*mode))
                .at(path)
        }
        Node::Dir { mode, entries } => {
            std::fs::create_dir(path).at(path)?;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).at(path)?;
            for (name, entry) in entries {
                write(&path.join(name), entry)?;
            }
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(*mode)).at(path)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::*;

    fn real(dir: &TempDir) -> PathBuf {
        fs::canonicalize(dir.path()).unwrap()
    }

    #[test]
    fn a_changed_file_is_put_back_and_reported() {
        let dir = TempDir::new().unwrap();
        let settings = real(&dir).join("settings.json");
        fs::write(&settings, "{}\n").unwrap();
        let kept = Kept::new(vec![settings.clone()]).unwrap();
        fs::write(&settings, "{\"hooks\":1}\n").unwrap();
        assert_eq!(kept.restore().unwrap(), vec![settings.clone()]);
        assert_eq!(fs::read_to_string(&settings).unwrap(), "{}\n");
    }

    #[test]
    fn an_unchanged_path_is_left_alone() {
        let dir = TempDir::new().unwrap();
        let settings = real(&dir).join("settings.json");
        fs::write(&settings, "{}\n").unwrap();
        let kept = Kept::new(vec![settings.clone()]).unwrap();
        assert!(kept.restore().unwrap().is_empty());
    }

    #[test]
    fn a_path_the_agent_created_is_removed() {
        let dir = TempDir::new().unwrap();
        let hooks = real(&dir).join("hooks");
        let kept = Kept::new(vec![hooks.clone()]).unwrap();
        fs::create_dir_all(hooks.join("deep")).unwrap();
        fs::write(hooks.join("deep/run.sh"), "x").unwrap();
        assert_eq!(kept.restore().unwrap(), vec![hooks.clone()]);
        assert!(!hooks.exists());
    }

    #[test]
    fn a_directory_comes_back_whole() {
        let dir = TempDir::new().unwrap();
        let hooks = real(&dir).join("hooks");
        fs::create_dir(&hooks).unwrap();
        fs::write(hooks.join("pre.sh"), "echo pre\n").unwrap();
        fs::set_permissions(hooks.join("pre.sh"), fs::Permissions::from_mode(0o755)).unwrap();
        let kept = Kept::new(vec![hooks.clone()]).unwrap();
        fs::write(hooks.join("pre.sh"), "curl evil\n").unwrap();
        fs::write(hooks.join("post.sh"), "curl evil\n").unwrap();
        assert_eq!(kept.restore().unwrap(), vec![hooks.clone()]);
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
        let dir = TempDir::new().unwrap();
        let root = real(&dir);
        let (settings, elsewhere) = (root.join("settings.json"), root.join("elsewhere"));
        fs::write(&settings, "{}\n").unwrap();
        fs::write(&elsewhere, "mine\n").unwrap();
        let kept = Kept::new(vec![settings.clone()]).unwrap();
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
        let dir = TempDir::new().unwrap();
        let settings = real(&dir).join("settings.json");
        fs::write(&settings, "{}\n").unwrap();
        let first = Kept::new(vec![settings.clone()]).unwrap();
        // The first run's agent changes the file before the second starts.
        fs::write(&settings, "{\"hooks\":1}\n").unwrap();
        let second = Kept::new(vec![settings.clone()]).unwrap();
        first.restore().unwrap();
        fs::write(&settings, "{\"hooks\":2}\n").unwrap();
        assert_eq!(second.restore().unwrap(), vec![settings.clone()]);
        assert_eq!(fs::read_to_string(&settings).unwrap(), "{}\n");
        // Both runs over: the next one notes the file afresh.
        fs::write(&settings, "{\"theme\":1}\n").unwrap();
        let third = Kept::new(vec![settings.clone()]).unwrap();
        assert!(third.restore().unwrap().is_empty());
    }

    #[test]
    fn a_dropped_note_still_puts_back() {
        let dir = TempDir::new().unwrap();
        let settings = real(&dir).join("settings.json");
        fs::write(&settings, "{}\n").unwrap();
        {
            let _kept = Kept::new(vec![settings.clone()]).unwrap();
            fs::write(&settings, "{\"hooks\":1}\n").unwrap();
        }
        assert_eq!(fs::read_to_string(&settings).unwrap(), "{}\n");
    }
}
