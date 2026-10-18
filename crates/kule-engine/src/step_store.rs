//! What each step left in the copy, so the copy can go back to any step.
//!
//! After a step, every path it changed is kept as it is then: a file is
//! cloned (free on APFS and btrfs, a copy elsewhere), a directory or a link
//! noted, a deletion noted as such, and a path back to what the workspace
//! holds noted as that. Going back to step N puts every path a later step
//! changed back to what the latest step up to N left there, or to what the
//! workspace holds, through the same mirror a rollback uses. Going back is
//! a step of its own, so it can be gone back on.
//!
//! The copy is the agent's to change, so it is read the way the window's
//! explorer reads it: no link followed at any step of a path, and a file
//! with a second name (a hard link to something outside) not kept.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Error, IoContext, Result};
use crate::fsops::{read_json, write_json};
use crate::outline::open_beneath;
use crate::steps::Step;
use crate::task::Task;

/// Where the steps' paths are kept, in the task's directory.
pub(crate) const STORE: &str = "steps-store";
/// Larger files are not kept: the step can then not be gone back past.
pub const MAX_KEPT_FILE: u64 = 64 << 20;
/// The most all kept files of one transaction may take.
pub const MAX_KEPT_TOTAL: u64 = 1 << 30;

/// A path as a step left it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Left {
    /// In the step's tree: a file, a directory or a link.
    Kept,
    /// Not there.
    Absent,
    /// As the workspace holds it.
    Workspace,
    /// Changed, but not kept: too large, or not a plain file.
    Unkept,
}

fn step_dir(task: &Task, id: u32) -> PathBuf {
    task.dir().join(STORE).join(id.to_string())
}

fn manifest(task: &Task, id: u32) -> PathBuf {
    task.dir().join(STORE).join(format!("{id}.json"))
}

/// Keeps what `step` changed as the copy holds it now. `kept` counts the
/// bytes kept so far in the transaction, and grows.
pub(crate) fn keep(task: &Task, step: &Step, kept: &mut u64) -> Result<()> {
    let copy = task.copy();
    let tree = step_dir(task, step.id).join("tree");
    let mut left = BTreeMap::new();
    for change in &step.changes {
        let state = match change.change {
            None => Left::Workspace,
            Some(crate::effects::Change::Deleted) => Left::Absent,
            Some(_) => keep_path(&copy, &change.path, &tree.join(&change.path), kept)?,
        };
        left.insert(change.path.clone(), state);
    }
    let store = task.dir().join(STORE);
    fs::create_dir_all(&store).at(&store)?;
    write_json(&manifest(task, step.id), &left)
}

fn keep_path(copy: &Path, rel: &str, to: &Path, kept: &mut u64) -> Result<Left> {
    // What the path is, read beside it, without following it.
    let meta = match fs::symlink_metadata(copy.join(rel)) {
        Ok(meta) => meta,
        Err(e)
            if matches!(
                e.kind(),
                io::ErrorKind::NotFound | io::ErrorKind::NotADirectory
            ) =>
        {
            return Ok(Left::Absent);
        }
        Err(e) => return Err(e).at(&copy.join(rel)),
    };
    if let Some(parent) = to.parent() {
        fs::create_dir_all(parent).at(parent)?;
    }
    if meta.is_dir() {
        fs::create_dir_all(to).at(to)?;
        return Ok(Left::Kept);
    }
    if meta.file_type().is_symlink() {
        let target = fs::read_link(copy.join(rel)).at(&copy.join(rel))?;
        std::os::unix::fs::symlink(&target, to).at(to)?;
        return Ok(Left::Kept);
    }
    // A file, opened without following a link anywhere on its way, so one
    // put there since the look above is not followed either.
    let Some(file) = open_beneath(copy, rel).at(&copy.join(rel))? else {
        return Ok(Left::Absent);
    };
    let meta = file.metadata().at(&copy.join(rel))?;
    if !meta.is_file() || meta.nlink() > 1 {
        return Ok(Left::Unkept);
    }
    if meta.len() > MAX_KEPT_FILE || *kept + meta.len() > MAX_KEPT_TOTAL {
        return Ok(Left::Unkept);
    }
    clone_from(&file, to).at(to)?;
    fs::set_permissions(to, meta.permissions()).at(to)?;
    *kept += meta.len();
    Ok(Left::Kept)
}

/// Clones the open file to `to`, which must not exist: a clone where the
/// filesystem makes one, a copy otherwise.
fn clone_from(file: &File, to: &Path) -> io::Result<()> {
    use std::os::fd::AsRawFd;
    #[cfg(target_os = "macos")]
    {
        use std::os::unix::ffi::OsStrExt;
        let name = std::ffi::CString::new(to.as_os_str().as_bytes())?;
        // SAFETY: the descriptor is open and the path NUL-terminated.
        if unsafe { libc::fclonefileat(file.as_raw_fd(), libc::AT_FDCWD, name.as_ptr(), 0) } == 0 {
            return Ok(());
        }
    }
    let out = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(to)?;
    #[cfg(target_os = "linux")]
    {
        // SAFETY: both descriptors are open; FICLONE only shares extents.
        if unsafe { libc::ioctl(out.as_raw_fd(), libc::FICLONE, file.as_raw_fd()) } == 0 {
            return Ok(());
        }
    }
    let mut from = file;
    let mut out = out;
    io::copy(&mut from, &mut out)?;
    Ok(())
}

fn read_manifest(task: &Task, id: u32) -> Result<BTreeMap<String, Left>> {
    let path = manifest(task, id);
    if path.is_file() {
        read_json(&path)
    } else {
        // A step from before steps were kept: none of its paths can be put back.
        Ok(BTreeMap::new())
    }
}

/// What every path changed after step `id` should hold to be as it was at
/// `id`: the latest state up to `id`, with the step that kept it.
fn targets(task: &Task, steps: &[Step], id: u32) -> Result<BTreeMap<String, (Left, u32)>> {
    let later: BTreeSet<&str> = steps
        .iter()
        .filter(|s| s.id > id)
        .flat_map(|s| s.changes.iter().map(|c| c.path.as_str()))
        .collect();
    let mut targets: BTreeMap<String, (Left, u32)> = later
        .iter()
        .map(|p| ((*p).to_owned(), (Left::Workspace, 0)))
        .collect();
    for step in steps.iter().filter(|s| s.id <= id) {
        let left = read_manifest(task, step.id)?;
        for change in &step.changes {
            if let Some(t) = targets.get_mut(&change.path) {
                // A step whose paths were not noted keeps none of them.
                *t = (
                    left.get(&change.path).copied().unwrap_or(Left::Unkept),
                    step.id,
                );
            }
        }
    }
    Ok(targets)
}

/// Why the copy cannot go back to step `id` (`0`, before the first): a
/// path a later step changed whose state then was not kept.
pub(crate) fn blocker(task: &Task, steps: &[Step], id: u32) -> Result<Option<String>> {
    Ok(targets(task, steps, id)?
        .into_iter()
        .find(|(_, (left, _))| *left == Left::Unkept)
        .map(|(path, _)| path))
}

impl Task {
    /// Whether the copy can go back to each step (and to before the
    /// first, `0`): `None` when it can, else the path that keeps it from it.
    pub fn step_blockers(&self) -> Result<BTreeMap<u32, Option<String>>> {
        let steps = self.steps()?;
        let mut out = BTreeMap::new();
        for id in std::iter::once(0).chain(steps.iter().map(|s| s.id)) {
            out.insert(id, blocker(self, &steps, id)?);
        }
        Ok(out)
    }

    /// Puts the copy back as it was after step `id` (`0`: before the first
    /// step), and records that as a step. Changes made since the last step
    /// are kept as a step first, so nothing is lost on the way.
    pub fn back_to_step(&self, id: u32) -> Result<Step> {
        let mut steps = self.steps()?;
        if id != 0 && !steps.iter().any(|s| s.id == id) {
            return Err(Error::Invalid(format!("the task has no step {id}")));
        }
        if self.unstepped()? {
            steps.push(self.step("Changes before going back")?);
        }
        let targets = targets(self, &steps, id)?;
        if let Some((path, _)) = targets.iter().find(|(_, (left, _))| *left == Left::Unkept) {
            return Err(Error::Invalid(format!(
                "{path} changed after step {id} and was not kept then (too large, or not a plain file), so the task cannot go back to step {id}"
            )));
        }
        // The paths' states gathered in one tree, so one mirror puts them
        // in place: what is absent there goes from the copy.
        let staging = self.dir().join(STORE).join("going-back");
        if staging.exists() {
            crate::fsops::remove_dir_tree(&staging)?;
        }
        fs::create_dir_all(&staging).at(&staging)?;
        let staged = (|| {
            for (path, (left, step)) in &targets {
                let to = staging.join(path);
                match left {
                    Left::Absent | Left::Unkept => {}
                    Left::Workspace => self.stage_synced(path, &to)?,
                    Left::Kept => {
                        let kept = step_dir(self, *step).join("tree").join(path);
                        if let Some(parent) = to.parent() {
                            fs::create_dir_all(parent).at(parent)?;
                        }
                        let meta = fs::symlink_metadata(&kept).at(&kept)?;
                        if meta.is_dir() {
                            fs::create_dir_all(&to).at(&to)?;
                        } else if meta.file_type().is_symlink() {
                            std::os::unix::fs::symlink(fs::read_link(&kept).at(&kept)?, &to)
                                .at(&to)?;
                        } else {
                            reflink_copy::reflink_or_copy(&kept, &to).at(&kept)?;
                        }
                    }
                }
            }
            let paths: Vec<String> = targets.keys().cloned().collect();
            self.put_back(&staging, &paths)
        })();
        let _ = crate::fsops::remove_dir_tree(&staging);
        staged?;
        self.step(&if id == 0 {
            "Back to before the first step".to_owned()
        } else {
            format!("Back to step {id}")
        })
    }
}
