//! The home layer: what a task's commands write in the person's home
//! directory, caught for review instead of refused.
//!
//! Commands get `HOME` pointing at a layer in the task's directory. Its
//! small files at the top (`.zshrc`, `.gitconfig`) and `.config` are
//! clones of the person's: a command changes those, and anything new it
//! makes at the top, in the layer. Every other top-level entry is a link to
//! the person's own, so tools find their toolchains, caches and settings
//! where they always are, and a write through such a link meets the
//! sandbox as before. The agent's own state is linked too, and what changes
//! there is never offered. Paths the sandbox hides (`.ssh`, `.aws`, …) are
//! not in the layer at all.
//!
//! A person sees the changes ([`Task::home_changes`]) and takes them to the
//! real home ([`Task::apply_home`]) or throws them away
//! ([`Task::discard_home`]). Applying checks that the real path is still
//! what it was when the layer was made, and keeps what it replaces.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use walkdir::WalkDir;

use crate::effects::Change;
use crate::error::{Error, IoContext, Result};
use crate::fsops::{self, read_json, write_json};
use crate::manifest::{Entry, Kind, Stamp, scan};
use crate::task::Task;
use crate::textdiff::{self, TextDiff};

/// Directories cloned whole into the layer, where tools keep settings.
const CAPTURED_DIRS: &[&str] = &[".config"];
/// A top-level file larger than this is linked, not cloned.
const MAX_FILE: u64 = 1024 * 1024;
/// A captured directory with more than this is linked instead: the layer is
/// made for every run.
const MAX_DIR_ENTRIES: usize = 20_000;
/// Where cloning a directory copies it (not APFS), the most it may hold.
const MAX_DIR_BYTES: u64 = 64 * 1024 * 1024;

/// What shells and tools rewrite on their own as they run (completion
/// dumps, histories): never offered as a change.
const NOISE: &[&str] = &[
    ".zcompdump",
    ".zsh_history",
    ".zsh_sessions",
    ".bash_history",
    ".bash_sessions",
    ".python_history",
    ".node_repl_history",
    ".lesshst",
    ".viminfo",
    ".wget-hsts",
    ".sqlite_history",
    ".psql_history",
    ".CFUserTextEncoding",
];

/// Files that shells, the session or everyday tools read and run as they
/// start: a change there runs every time, not once. A path matches a name
/// here exactly, or lies under one ending in `/`.
const STARTUP: &[&str] = &[
    ".zshrc",
    ".zshenv",
    ".zprofile",
    ".zlogin",
    ".zlogout",
    ".bashrc",
    ".bash_profile",
    ".bash_login",
    ".bash_logout",
    ".profile",
    ".kshrc",
    ".cshrc",
    ".tcshrc",
    ".xprofile",
    ".xinitrc",
    ".xsession",
    ".gitconfig",
    ".vimrc",
    ".config/git/config",
    ".config/fish/",
    ".config/nvim/",
    ".config/autostart/",
    ".config/systemd/user/",
    ".config/environment.d/",
];

/// Whether the file at `rel` runs as something starts (see [`STARTUP`]).
fn startup(rel: &str) -> bool {
    STARTUP.iter().any(|s| match s.strip_suffix('/') {
        Some(dir) => rel.starts_with(s) || rel == dir,
        None => rel == *s,
    })
}

/// What the layer started from.
#[derive(Debug, Default, Serialize, Deserialize)]
struct Layer {
    /// The person's home.
    real: PathBuf,
    /// Each captured path with what it was in the real home (`work`) and in
    /// the layer (`copy`) when the layer was made, or last applied.
    entries: BTreeMap<String, Entry>,
    /// Paths made links to the person's own.
    links: BTreeSet<String>,
    /// Names whose changes are the agent's own business: a path is left out
    /// when its first component starts with one (`.claude.json.tmp.1`).
    ignored: Vec<String>,
}

/// One change a task's commands made in the home layer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HomeChange {
    /// Relative to the home directory.
    pub path: String,
    pub change: Change,
    pub kind: Kind,
    /// A file read and run as shells, the session or tools start
    /// (`.zshrc`, `.gitconfig`): what it says runs every time.
    #[serde(default)]
    pub startup: bool,
}

/// What [`Task::apply_home`] did.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HomeApplied {
    pub paths: Vec<String>,
    /// Where what the real home held at those paths was kept.
    pub kept: Option<PathBuf>,
}

impl Layer {
    fn ignores(&self, rel: &str) -> bool {
        let first = rel.split('/').next().unwrap_or(rel);
        let last = rel.rsplit('/').next().unwrap_or(rel);
        NOISE.iter().any(|n| first.starts_with(n))
            || last == ".DS_Store"
            || self
                .ignored
                .iter()
                .any(|name| first.starts_with(name.as_str()))
            || self
                .links
                .iter()
                .any(|l| rel == l || rel.starts_with(&format!("{l}/")))
    }
}

/// The relative form of `path` under `root`, if it is there.
fn under(root: &Path, path: &Path) -> Option<String> {
    let rel = path.strip_prefix(root).ok()?;
    let text = rel.to_str()?;
    (!text.is_empty()).then(|| text.to_owned())
}

impl Task {
    /// Where the task's home layer lives.
    pub fn home_layer(&self) -> PathBuf {
        self.dir().join("home")
    }

    fn layer_path(&self) -> PathBuf {
        self.dir().join("home.json")
    }

    fn load_layer(&self) -> Result<Option<Layer>> {
        let path = self.layer_path();
        if !path.is_file() {
            return Ok(None);
        }
        read_json(&path).map(Some)
    }

    /// Makes the home layer over `real` for a run, or keeps the one there
    /// while it holds changes no one has looked at. `linked` are paths
    /// under `real` that stay the person's own (an agent's state); `hidden`
    /// ones are left out; `ignored` names are the agent's own business.
    /// Returns the layer, to be the commands' `HOME`.
    pub fn prepare_home(
        &self,
        real: &Path,
        linked: &[PathBuf],
        hidden: &[PathBuf],
        ignored: &[String],
    ) -> Result<PathBuf> {
        let real = fs::canonicalize(real).at(real)?;
        let dir = self.home_layer();
        if let Some(layer) = self.load_layer()?
            && layer.real == real
            && dir.is_dir()
            && !self.home_changes()?.is_empty()
        {
            return Ok(dir);
        }
        fsops::remove(&dir)?;
        fs::create_dir_all(&dir).at(&dir)?;
        let mut layer = Layer {
            real: real.clone(),
            ignored: ignored.to_vec(),
            ..Layer::default()
        };
        let canonical = |p: &PathBuf| fs::canonicalize(p).unwrap_or_else(|_| p.clone());
        let linked: Vec<PathBuf> = linked.iter().map(canonical).collect();
        let hidden: Vec<PathBuf> = hidden.iter().map(canonical).collect();
        let clock = crate::clock::Clock::new(self.dir(), &dir)?;
        let mut names: Vec<_> = fs::read_dir(&real)
            .at(&real)?
            .filter_map(|e| e.ok())
            .map(|e| e.file_name())
            .collect();
        names.sort();
        for name in names {
            let Some(text) = name.to_str().map(str::to_owned) else {
                continue;
            };
            let (from, to) = (real.join(&name), dir.join(&name));
            if hidden.contains(&from) {
                continue;
            }
            let Some(stamp) = Stamp::of(&from)? else {
                continue;
            };
            let is_linked = linked.contains(&from);
            match stamp.kind {
                Kind::Symlink => {
                    // As the person has it: the target is reached the same way.
                    let target = fs::read_link(&from).at(&from)?;
                    std::os::unix::fs::symlink(&target, &to).at(&to)?;
                    layer.links.insert(text);
                }
                Kind::File if !is_linked && stamp.size <= MAX_FILE => {
                    if let Some(work) = fsops::clone_entry(&from, &to)?
                        && let Some(copy) = Stamp::of(&to)?
                    {
                        layer.entries.insert(text, Entry { work, copy });
                    }
                }
                Kind::Dir if !is_linked && CAPTURED_DIRS.contains(&text.as_str()) => {
                    fs::create_dir(&to).at(&to)?;
                    if let Some(copy) = Stamp::of(&to)? {
                        layer
                            .entries
                            .insert(text.clone(), Entry { work: stamp, copy });
                    }
                    // Entry by entry: one the person cannot read (a root-owned
                    // directory) or too large to clone for every run is
                    // linked, and the rest is still caught.
                    let mut children: Vec<_> = fs::read_dir(&from)
                        .at(&from)?
                        .filter_map(|e| e.ok())
                        .map(|e| e.file_name())
                        .collect();
                    children.sort();
                    for child in children {
                        let Some(name) = child.to_str() else { continue };
                        let rel = format!("{text}/{name}");
                        let (from, to) = (from.join(&child), to.join(&child));
                        let Some(stamp) = Stamp::of(&from)? else {
                            continue;
                        };
                        let own = linked.contains(&from);
                        let caught = match stamp.kind {
                            _ if own || hidden.contains(&from) => false,
                            Kind::File if stamp.size <= MAX_FILE => {
                                match fsops::clone_entry(&from, &to) {
                                    Ok(Some(work)) => {
                                        if let Some(copy) = Stamp::of(&to)? {
                                            layer.entries.insert(rel.clone(), Entry { work, copy });
                                        }
                                        true
                                    }
                                    _ => {
                                        fsops::remove(&to)?;
                                        false
                                    }
                                }
                            }
                            Kind::Dir if capturable(&from) => {
                                match fsops::clone_tree(&from, &to, &clock) {
                                    Ok(taken) => {
                                        let made = scan(&to)?;
                                        if let Some(copy) = Stamp::of(&to)? {
                                            layer.entries.insert(
                                                rel.clone(),
                                                Entry {
                                                    work: stamp.clone(),
                                                    copy,
                                                },
                                            );
                                        }
                                        for (sub, work) in taken {
                                            if let Some(copy) = made.get(&sub) {
                                                layer.entries.insert(
                                                    format!("{rel}/{sub}"),
                                                    Entry {
                                                        work,
                                                        copy: copy.clone(),
                                                    },
                                                );
                                            }
                                        }
                                        true
                                    }
                                    Err(_) => {
                                        fsops::remove(&to)?;
                                        false
                                    }
                                }
                            }
                            _ => false,
                        };
                        if !caught && !hidden.contains(&from) {
                            let target = if stamp.kind == Kind::Symlink {
                                fs::read_link(&from).at(&from)?
                            } else {
                                from.clone()
                            };
                            std::os::unix::fs::symlink(target, &to).at(&to)?;
                            layer.links.insert(rel);
                        }
                    }
                    // What is hidden goes; what stays the person's own
                    // becomes a link to it.
                    for path in hidden.iter().chain(&linked) {
                        let Some(rel) =
                            under(&real, path).filter(|r| r.starts_with(&format!("{text}/")))
                        else {
                            continue;
                        };
                        fsops::remove(&dir.join(&rel))?;
                        layer
                            .entries
                            .retain(|p, _| !(*p == rel || p.starts_with(&format!("{rel}/"))));
                        if linked.contains(path) && path.exists() {
                            std::os::unix::fs::symlink(path, dir.join(&rel)).at(&dir.join(&rel))?;
                            layer.links.insert(rel);
                        }
                    }
                }
                _ => {
                    std::os::unix::fs::symlink(&from, &to).at(&to)?;
                    layer.links.insert(text);
                }
            }
        }
        // A write within the same tick of the clock as the clone would
        // leave the stamp as it was.
        let latest = layer
            .entries
            .values()
            .map(|e| e.copy.ctime)
            .max()
            .unwrap_or_default();
        if !clock.wait_past(latest)? {
            for entry in layer.entries.values_mut() {
                if entry.copy.ctime == latest {
                    entry.copy = entry.copy.clone().unknown();
                }
            }
        }
        write_json(&self.layer_path(), &layer)?;
        Ok(dir)
    }

    /// What the task's commands changed in the home layer, by path.
    pub fn home_changes(&self) -> Result<Vec<HomeChange>> {
        let Some(layer) = self.load_layer()? else {
            return Ok(Vec::new());
        };
        let dir = self.home_layer();
        if !dir.is_dir() {
            return Ok(Vec::new());
        }
        let mut seen = BTreeSet::new();
        let mut changes = Vec::new();
        let mut walk = WalkDir::new(&dir)
            .min_depth(1)
            .follow_links(false)
            .into_iter();
        while let Some(entry) = walk.next() {
            let entry = match entry {
                Ok(e) => e,
                Err(e) => return Err(Error::Invalid(format!("home layer: {e}"))),
            };
            let Some(rel) = under(&dir, entry.path()) else {
                continue;
            };
            if layer.ignores(&rel) {
                if entry.file_type().is_dir() {
                    walk.skip_current_dir();
                }
                continue;
            }
            let Some(now) = Stamp::of(entry.path())? else {
                continue;
            };
            seen.insert(rel.clone());
            match layer.entries.get(&rel) {
                Some(was) if was.copy.unchanged(&now) => {}
                Some(_) => changes.push(HomeChange {
                    startup: startup(&rel),
                    path: rel,
                    change: Change::Modified,
                    kind: now.kind,
                }),
                None => changes.push(HomeChange {
                    startup: startup(&rel),
                    path: rel,
                    change: Change::Created,
                    kind: now.kind,
                }),
            }
        }
        for (rel, was) in &layer.entries {
            if !seen.contains(rel) && !layer.ignores(rel) {
                changes.push(HomeChange {
                    path: rel.clone(),
                    change: Change::Deleted,
                    kind: was.copy.kind,
                    startup: startup(rel),
                });
            }
        }
        changes.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(changes)
    }

    /// Takes the changes at `paths` from the layer to the real home. Every
    /// one of them is refused when any real path is no longer what it was
    /// when the layer was made; what is replaced or removed is kept first.
    pub fn apply_home(&self, paths: &[String]) -> Result<HomeApplied> {
        let Some(mut layer) = self.load_layer()? else {
            return Ok(HomeApplied::default());
        };
        // Nothing left running in the task's sandbox changes the layer
        // while it is applied.
        if let Ok(sandbox) = crate::Sandbox::new(self) {
            sandbox.end_strays();
        }
        let wanted: Vec<HomeChange> = self
            .home_changes()?
            .into_iter()
            .filter(|c| paths.contains(&c.path))
            .collect();
        if wanted.is_empty() {
            return Ok(HomeApplied::default());
        }
        let dir = self.home_layer();
        let mut moved = Vec::new();
        for c in &wanted {
            let now = Stamp::of(&layer.real.join(&c.path))?;
            let expected = layer.entries.get(&c.path).map(|e| &e.work);
            let same = match (expected, &now) {
                (Some(was), Some(now)) => was.unchanged(now) && !was.is_unknown(),
                (None, None) => true,
                _ => false,
            };
            if !same {
                moved.push(format!("~/{}", c.path));
            }
        }
        if !moved.is_empty() {
            return Err(Error::Invalid(format!(
                "changed in your home since the task's layer was made, so not applied: {}",
                moved.join(", ")
            )));
        }
        let kept_root = self.dir().join("home-kept");
        fs::create_dir_all(&kept_root).at(&kept_root)?;
        // Named so they sort by when: the last one is what an undo takes back.
        let mut id = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_millis());
        while kept_root.join(format!("{id:020}")).exists()
            || kept_root.join(format!("{id:020}.json")).exists()
        {
            id += 1;
        }
        let id = format!("{id:020}");
        let kept = kept_root.join(&id);
        let mut kept_any = false;
        let mut record = Vec::new();
        // Parents before what is in them, and removals deepest first.
        let (mut removals, mut writes): (Vec<&HomeChange>, Vec<&HomeChange>) =
            wanted.iter().partition(|c| c.change == Change::Deleted);
        writes.sort_by(|a, b| a.path.cmp(&b.path));
        removals.sort_by(|a, b| b.path.cmp(&a.path));
        for c in writes.iter().chain(&removals) {
            let (from, to) = (dir.join(&c.path), layer.real.join(&c.path));
            let held = Stamp::of(&to)?;
            let keeps =
                c.change != Change::Created && held.as_ref().is_some_and(|s| s.kind != Kind::Dir);
            if keeps {
                keep(&to, &kept.join(&c.path))?;
                kept_any = true;
            }
            let before = layer.entries.get(&c.path).cloned();
            match c.change {
                Change::Deleted => match Stamp::of(&to)?.map(|s| s.kind) {
                    Some(Kind::Dir) => fs::remove_dir(&to).at(&to)?,
                    Some(_) => fs::remove_file(&to).at(&to)?,
                    None => {}
                },
                _ => match c.kind {
                    Kind::Dir => {
                        if Stamp::of(&to)?.is_none_or(|s| s.kind != Kind::Dir) {
                            fsops::remove(&to)?;
                            fs::create_dir(&to).at(&to)?;
                        }
                    }
                    // Read from the layer without following a link at any
                    // step: what reaches the home is what the layer holds.
                    Kind::Symlink => {
                        let fsops::Beneath::Link(target) =
                            fsops::beneath(&dir, &c.path).at(&from)?
                        else {
                            return Err(changed(&c.path));
                        };
                        fsops::remove(&to)?;
                        std::os::unix::fs::symlink(target, &to).at(&to)?;
                    }
                    Kind::File => {
                        let fsops::Beneath::File(mut file) =
                            fsops::beneath(&dir, &c.path).at(&from)?
                        else {
                            return Err(changed(&c.path));
                        };
                        let mode = file.metadata().at(&from)?.permissions();
                        let mut bytes = Vec::new();
                        std::io::Read::read_to_end(&mut file, &mut bytes).at(&from)?;
                        put(&to, &bytes, mode)?;
                    }
                },
            }
            let left = Stamp::of(&to)?;
            match (&left, Stamp::of(&from)?) {
                (Some(work), Some(copy)) => {
                    let work = work.clone();
                    layer.entries.insert(c.path.clone(), Entry { work, copy });
                }
                _ => {
                    layer.entries.remove(&c.path);
                }
            }
            record.push(AppliedPath {
                path: c.path.clone(),
                change: c.change,
                kind: c.kind,
                before,
                held: held.map(|s| s.kind),
                kept: keeps,
                left,
            });
        }
        write_json(
            &kept_root.join(format!("{id}.json")),
            &Applied { paths: record },
        )?;
        write_json(&self.layer_path(), &layer)?;
        let mut applied: Vec<String> = wanted.into_iter().map(|c| c.path).collect();
        applied.sort();
        Ok(HomeApplied {
            paths: applied,
            kept: kept_any.then_some(kept),
        })
    }

    /// Throws the changes at `paths` away: the layer gets back what the
    /// real home holds there.
    pub fn discard_home(&self, paths: &[String]) -> Result<Vec<String>> {
        let Some(mut layer) = self.load_layer()? else {
            return Ok(Vec::new());
        };
        let dir = self.home_layer();
        let mut wanted: Vec<HomeChange> = self
            .home_changes()?
            .into_iter()
            .filter(|c| paths.contains(&c.path))
            .collect();
        // What is in a directory before the directory, for removals; parents
        // first for what comes back.
        wanted.sort_by(|a, b| a.path.cmp(&b.path));
        for c in wanted.iter().rev().filter(|c| c.change == Change::Created) {
            fsops::remove(&dir.join(&c.path))?;
        }
        for c in wanted.iter().filter(|c| c.change != Change::Created) {
            let (real, to) = (layer.real.join(&c.path), dir.join(&c.path));
            fsops::remove(&to)?;
            if let Some(work) = fsops::clone_entry(&real, &to)?
                && let Some(copy) = Stamp::of(&to)?
            {
                layer.entries.insert(c.path.clone(), Entry { work, copy });
            }
        }
        write_json(&self.layer_path(), &layer)?;
        Ok(wanted.into_iter().map(|c| c.path).collect())
    }
}

/// What one [`Task::apply_home`] did, for an undo: kept next to what it
/// replaced, as `home-kept/<id>.json` beside `home-kept/<id>/`.
#[derive(Debug, Serialize, Deserialize)]
struct Applied {
    paths: Vec<AppliedPath>,
}

#[derive(Debug, Serialize, Deserialize)]
struct AppliedPath {
    path: String,
    change: Change,
    kind: Kind,
    /// The layer's entry before: an undo offers the change again from it.
    before: Option<Entry>,
    /// What the real home held there, if anything.
    held: Option<Kind>,
    /// Whether that was kept (a file or a link).
    kept: bool,
    /// What the apply left there; an undo needs it unchanged.
    left: Option<Stamp>,
}

/// Saves the file or link at `from` as `to`.
fn changed(path: &str) -> Error {
    Error::Invalid(format!("~/{path} changed while it was being applied"))
}

fn keep(from: &Path, to: &Path) -> Result<()> {
    if let Some(parent) = to.parent() {
        fs::create_dir_all(parent).at(parent)?;
    }
    if fs::symlink_metadata(from)
        .at(from)?
        .file_type()
        .is_symlink()
    {
        let target = fs::read_link(from).at(from)?;
        std::os::unix::fs::symlink(target, to).at(to)?;
    } else {
        fs::copy(from, to).at(to)?;
    }
    Ok(())
}

/// Puts `bytes` at `to` in one step, with `mode`; the content is never
/// readable with wider permissions than that on the way.
fn put(to: &Path, bytes: &[u8], mode: fs::Permissions) -> Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let tmp = to.with_file_name(format!(
        ".{}.kule-{}",
        to.file_name().and_then(|n| n.to_str()).unwrap_or("new"),
        std::process::id()
    ));
    let _ = fs::remove_file(&tmp);
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&tmp)
        .at(&tmp)?;
    file.write_all(bytes).at(&tmp)?;
    drop(file);
    fs::set_permissions(&tmp, mode).at(&tmp)?;
    fs::rename(&tmp, to).at(to)
}

impl Task {
    /// What the change waiting at `path` would do to the file in the
    /// person's home, line by line.
    pub fn home_diff(&self, path: &str) -> Result<TextDiff> {
        let Some(layer) = self.load_layer()? else {
            return Err(Error::Invalid(format!("~/{path} has no change waiting")));
        };
        let change = self
            .home_changes()?
            .into_iter()
            .find(|c| c.path == path)
            .ok_or_else(|| Error::Invalid(format!("~/{path} has no change waiting")))?;
        if change.kind != Kind::File {
            return Err(Error::Invalid(format!("~/{path} is not a file")));
        }
        let (real, copy) = (layer.real.join(path), self.home_layer().join(path));
        let before = textdiff::side(&real).at(&real)?;
        let after = textdiff::side(&copy).at(&copy)?;
        Ok(textdiff::diff(before.as_deref(), after.as_deref()))
    }

    /// The last apply from this task's layer not undone yet.
    fn last_applied(&self) -> Result<Option<(PathBuf, Applied)>> {
        let root = self.dir().join("home-kept");
        let mut records: Vec<PathBuf> = match fs::read_dir(&root) {
            Ok(dir) => dir
                .filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|e| e == "json"))
                .collect(),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e).at(&root),
        };
        records.sort();
        let Some(last) = records.pop() else {
            return Ok(None);
        };
        let applied = read_json(&last)?;
        Ok(Some((last, applied)))
    }

    /// What [`Task::undo_home`] would take back.
    pub fn home_last_applied(&self) -> Result<Option<HomeApplied>> {
        Ok(self.last_applied()?.map(|(record, applied)| {
            let kept = record.with_extension("");
            HomeApplied {
                paths: applied.paths.into_iter().map(|p| p.path).collect(),
                kept: kept.is_dir().then_some(kept),
            }
        }))
    }

    /// Takes the last apply back: the person's home gets what it held
    /// before, and the changes wait in the layer again. Refused when any of
    /// those paths changed in the home since.
    pub fn undo_home(&self) -> Result<HomeApplied> {
        let nothing = || Error::Invalid("nothing applied from this task's home to undo".into());
        let mut layer = self.load_layer()?.ok_or_else(nothing)?;
        let (record, applied) = self.last_applied()?.ok_or_else(nothing)?;
        let kept = record.with_extension("");
        let made: BTreeSet<&str> = applied
            .paths
            .iter()
            .filter(|p| p.held.is_none())
            .map(|p| p.path.as_str())
            .collect();
        let mut moved = Vec::new();
        for p in &applied.paths {
            let to = layer.real.join(&p.path);
            let now = Stamp::of(&to)?;
            let mut same = match (&p.left, &now) {
                (Some(left), Some(now)) => left.unchanged(now),
                (None, None) => true,
                _ => false,
            };
            // A directory the apply made goes only with nothing else in it.
            if same && p.held.is_none() && p.kind == Kind::Dir {
                same = fs::read_dir(&to).at(&to)?.filter_map(|e| e.ok()).all(|e| {
                    let name = e.file_name();
                    name.to_str()
                        .is_some_and(|n| made.contains(format!("{}/{n}", p.path).as_str()))
                });
            }
            if !same {
                moved.push(format!("~/{}", p.path));
            }
        }
        if !moved.is_empty() {
            return Err(Error::Invalid(format!(
                "changed in your home since it was applied, so not undone: {}",
                moved.join(", ")
            )));
        }
        // The apply's order backwards: what is in a directory goes before it.
        for p in applied.paths.iter().rev() {
            let to = layer.real.join(&p.path);
            match p.held {
                _ if p.kept => {
                    let from = kept.join(&p.path);
                    fsops::remove(&to)?;
                    if fs::symlink_metadata(&from)
                        .at(&from)?
                        .file_type()
                        .is_symlink()
                    {
                        let target = fs::read_link(&from).at(&from)?;
                        std::os::unix::fs::symlink(target, &to).at(&to)?;
                    } else {
                        let bytes = fs::read(&from).at(&from)?;
                        put(&to, &bytes, fs::metadata(&from).at(&from)?.permissions())?;
                    }
                }
                Some(Kind::Dir) => {
                    if Stamp::of(&to)?.is_none() {
                        fs::create_dir(&to).at(&to)?;
                    }
                }
                Some(_) => {}
                None if p.kind == Kind::Dir => fs::remove_dir(&to).at(&to)?,
                None => fsops::remove(&to)?,
            }
            match (&p.before, Stamp::of(&to)?) {
                (Some(before), Some(work)) => {
                    let copy = before.copy.clone();
                    layer.entries.insert(p.path.clone(), Entry { work, copy });
                }
                _ => {
                    layer.entries.remove(&p.path);
                }
            }
        }
        write_json(&self.layer_path(), &layer)?;
        fsops::remove(&kept)?;
        fs::remove_file(&record).at(&record)?;
        Ok(HomeApplied {
            paths: applied.paths.into_iter().map(|p| p.path).collect(),
            kept: None,
        })
    }
}

/// Whether a directory is small enough to clone for every run,
/// and readable whole.
fn capturable(dir: &Path) -> bool {
    let Ok(tree) = scan(dir) else {
        return false;
    };
    tree.len() <= MAX_DIR_ENTRIES
        && (cfg!(target_os = "macos")
            || tree.values().map(|s| s.size).sum::<u64>() <= MAX_DIR_BYTES)
}
