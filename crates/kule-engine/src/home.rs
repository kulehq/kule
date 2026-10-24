//! The home layer: what a task's commands write in the person's home
//! directory, caught for review instead of refused.
//!
//! Commands get `HOME` pointing at a layer in the task's directory. Its
//! small files at the top (`.zshrc`, `.gitconfig`) and `.config` are
//! clones of the person's: a command changes those, and anything new it
//! makes at the top, in the layer. Every other top-level entry is a link to
//! the person's own, so tools find their toolchains, caches and settings
//! where they always are, and a write through such a link meets the
//! sandbox as before. The agent's own state is cloned into the layer, never
//! linked: it writes there, what it writes is never offered, and nothing of
//! it reaches the person's home. Paths the sandbox hides (`.ssh`, `.aws`,
//! …) are not in the layer at all.
//!
//! A person sees the changes ([`Task::home_changes`]) and takes them to the
//! real home ([`Task::apply_home`]) or throws them away
//! ([`Task::discard_home`]). Applying checks that the real path is still
//! what it was when the layer was made, and keeps what it replaces.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use walkdir::WalkDir;

use crate::clock::Clock;
use crate::effects::Change;
use crate::error::{Error, IoContext, Result};
use crate::fsops::{self, read_json, write_json};
use crate::manifest::{Entry, Kind, Stamp, Tree, scan};
use crate::secrets::{self, TaskSecrets};
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

/// Whether the layer's file at `rel` is one whose settings that look like
/// secrets Kule masks, as in the copy's `.env` files: what shells and tools
/// read as they start (`export GITHUB_TOKEN=…` in `.zshrc`) and env files
/// (`~/.env`), where people keep tokens.
fn masks_home(rel: &str) -> bool {
    startup(rel) || secrets::is_env_file(rel)
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
    /// Paths whose changes are the agent's own business, and never offered:
    /// what is in one, and a name beside one that starts with its name and a
    /// dot (`.claude.json.tmp.1`).
    ignored: Vec<String>,
    /// Who the layer was made for: its state, hidden and ignored paths,
    /// hashed. Another agent's run never takes it over, with what the first
    /// wrote in it (its state, perhaps its login).
    #[serde(default)]
    made_for: String,
    /// Files Kule masked secrets in: the real ones are hidden from the
    /// commands, which read the masked clones through `HOME`.
    #[serde(default)]
    masked: BTreeSet<String>,
    /// Files Kule could not mask (not UTF-8, too large), with why: kept out
    /// of the layer and hidden where they are, so the commands read them
    /// nowhere.
    #[serde(default)]
    withheld: BTreeMap<String, String>,
}

/// The largest home settings file Kule masks; a larger one is withheld.
const MAX_MASKED: u64 = 1024 * 1024;

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
    /// Where a link the layer holds points, as written: a person sees where
    /// it leads before applying it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    /// What the layer holds at the path, hashed (its bytes and permission
    /// bits, a link's target, `missing` for a deletion): what an apply
    /// takes is what was reviewed.
    #[serde(default)]
    pub hash: String,
}

/// A hash of `changes` as a review lists them: an apply takes them only
/// while they are still these.
pub fn home_digest(changes: &[HomeChange]) -> String {
    let bytes = serde_json::to_vec(changes).unwrap_or_default();
    blake3::hash(&bytes).to_hex().to_string()
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
            || self.ignored.iter().any(|p| own_business(p, rel))
            || self
                .links
                .iter()
                .any(|l| rel == l || rel.starts_with(&format!("{l}/")))
    }
}

/// Whether `rel` is in the ignored path `ignored`, or a name beside it that
/// starts with its name and a dot, as tools write a file next to the one
/// they replace (`.claude.json.tmp.1`).
fn own_business(ignored: &str, rel: &str) -> bool {
    let ours: Vec<&str> = ignored.split('/').collect();
    let theirs: Vec<&str> = rel.split('/').collect();
    let Some((name, parents)) = ours.split_last() else {
        return false;
    };
    theirs.len() >= ours.len()
        && theirs[..parents.len()] == *parents
        && (theirs[parents.len()] == *name
            || theirs[parents.len()].starts_with(&format!("{name}.")))
}

/// Whether `rel` is a folder on the way to an ignored path, which the layer
/// holds as a folder of its own only to reach that path.
fn on_the_way(ignored: &[String], rel: &str) -> bool {
    ignored.iter().any(|p| p.starts_with(&format!("{rel}/")))
}

/// Makes an agent's state at `rel` a clone in the layer `dir` of what it is
/// under `real`; where the person has none, the agent makes it in the layer.
/// Each folder on the way the layer holds as a link to the person's becomes
/// a folder of its own, whose other entries stay links. State too large or
/// unreadable to clone for every run, or behind a link of the person's own,
/// stays a link: the sandbox refuses the agent's writes there. A hidden
/// entry on the way is left out, unless something in it is `shown`: then
/// it is a link too, and the sandbox lets only that be read.
fn capture_state(
    real: &Path,
    dir: &Path,
    rel: &str,
    hidden: &[PathBuf],
    shown: &[PathBuf],
    layer: &mut Layer,
    clock: &Clock,
) -> Result<()> {
    let parts: Vec<&str> = rel.split('/').collect();
    for depth in 1..parts.len() {
        let on = parts[..depth].join("/");
        let (from, to) = (real.join(&on), dir.join(&on));
        match fs::symlink_metadata(&to) {
            Ok(m) if m.is_dir() => {}
            Ok(_) if layer.links.contains(&on) => {
                if !fs::symlink_metadata(&from).is_ok_and(|m| m.is_dir()) {
                    return Ok(());
                }
                fs::remove_file(&to).at(&to)?;
                layer.links.remove(&on);
                fs::create_dir(&to).at(&to)?;
                let mut children: Vec<_> = fs::read_dir(&from)
                    .at(&from)?
                    .filter_map(|e| e.ok())
                    .map(|e| e.file_name())
                    .collect();
                children.sort();
                for child in children {
                    let Some(name) = child.to_str() else { continue };
                    let from = from.join(&child);
                    let concealed =
                        hidden.contains(&from) && !shown.iter().any(|s| s.starts_with(&from));
                    if name == parts[depth] || concealed {
                        continue;
                    }
                    let target = if fs::symlink_metadata(&from).at(&from)?.is_symlink() {
                        fs::read_link(&from).at(&from)?
                    } else {
                        from.clone()
                    };
                    let to = to.join(&child);
                    std::os::unix::fs::symlink(target, &to).at(&to)?;
                    layer.links.insert(format!("{on}/{name}"));
                }
            }
            Ok(_) => return Ok(()),
            Err(_) => fs::create_dir(&to).at(&to)?,
        }
    }
    let caught = |p: &String| *p == rel || p.starts_with(&format!("{rel}/"));
    if layer.entries.keys().any(caught) {
        // Cloned already, as a small file at the top or in `.config`.
        return Ok(());
    }
    let (from, to) = (real.join(rel), dir.join(rel));
    if fs::symlink_metadata(&to).is_ok() {
        fsops::remove(&to)?;
        layer.links.remove(rel);
    }
    let Some(stamp) = Stamp::of(&from)? else {
        return Ok(());
    };
    let cloned = match stamp.kind {
        Kind::Dir if capturable(&from) => fsops::clone_tree(&from, &to, clock).is_ok(),
        Kind::File if stamp.size <= MAX_DIR_BYTES => {
            matches!(fsops::clone_entry(&from, &to), Ok(Some(_)))
        }
        _ => false,
    };
    if !cloned {
        fsops::remove(&to)?;
        let target = if stamp.kind == Kind::Symlink {
            fs::read_link(&from).at(&from)?
        } else {
            from
        };
        std::os::unix::fs::symlink(target, &to).at(&to)?;
        layer.links.insert(rel.to_owned());
    }
    Ok(())
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
    /// while it holds changes no one has looked at. `state` are paths under
    /// `real` an agent writes its own state to: cloned into the layer, so
    /// what it writes there never reaches the person's home. `hidden` ones
    /// are left out, but for what `shown` names inside a hidden directory
    /// the layer clones (`.config`); `ignored` paths (relative to `real`)
    /// are the agent's own business, never offered. Returns the layer, to
    /// be the commands' `HOME`.
    pub fn prepare_home(
        &self,
        real: &Path,
        state: &[PathBuf],
        hidden: &[PathBuf],
        shown: &[PathBuf],
        ignored: &[String],
    ) -> Result<PathBuf> {
        let given = real.to_path_buf();
        let real = fs::canonicalize(real).at(real)?;
        let dir = self.home_layer();
        let canonical = |p: &PathBuf| fs::canonicalize(p).unwrap_or_else(|_| p.clone());
        let made_for = {
            let mut hasher = blake3::Hasher::new();
            for (what, paths) in [("state", state), ("hidden", hidden)] {
                let mut paths: Vec<String> = paths
                    .iter()
                    .map(|p| canonical(p).display().to_string())
                    .collect();
                paths.sort();
                hasher.update(format!("{what}:{paths:?}\n").as_bytes());
            }
            let mut ignored = ignored.to_vec();
            ignored.sort();
            hasher.update(format!("ignored:{ignored:?}\n").as_bytes());
            hasher.finalize().to_hex().to_string()
        };
        if let Some(layer) = self.load_layer()?
            && layer.real == real
            && dir.is_dir()
            && !self.home_changes()?.is_empty()
        {
            // Changes waiting in a layer made for another agent: neither
            // given to this one nor thrown away.
            if layer.made_for != made_for {
                return Err(Error::Invalid(
                    "changes another agent made in your home wait for review in this task; apply or discard them first".to_owned(),
                ));
            }
            return Ok(dir);
        }
        fsops::remove(&dir)?;
        fs::create_dir_all(&dir).at(&dir)?;
        let mut layer = Layer {
            real: real.clone(),
            ignored: ignored.to_vec(),
            made_for,
            ..Layer::default()
        };
        let hidden: Vec<PathBuf> = hidden.iter().map(canonical).collect();
        let shown: Vec<PathBuf> = shown.iter().map(canonical).collect();
        // Inside a directory hidden whole, what is shown there, or holds
        // what is, alone.
        let shows = |path: &Path| shown.iter().any(|s| s.starts_with(path));
        // The person's files are stamped by the clock of the real home's
        // disk, the clones by the layer's.
        let real_clock = self.clock(&real)?;
        let layer_clock = self.clock(&dir)?;
        let start = real_clock.now()?;
        // Files the person wrote within the tick their stamps were read in,
        // which a write in that tick could leave as they were.
        let mut racy = Vec::new();
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
            let captured = CAPTURED_DIRS.contains(&text.as_str());
            let hidden_dir = hidden.contains(&from);
            if hidden_dir && !(captured && shows(&from)) {
                continue;
            }
            let Some(stamp) = Stamp::of(&from)? else {
                continue;
            };
            match stamp.kind {
                Kind::Symlink => {
                    // As the person has it: the target is reached the same way.
                    let target = fs::read_link(&from).at(&from)?;
                    std::os::unix::fs::symlink(&target, &to).at(&to)?;
                    layer.links.insert(text);
                }
                Kind::File if stamp.size <= MAX_FILE => {
                    if let Some(work) = fsops::clone_entry(&from, &to)?
                        && let Some(copy) = Stamp::of(&to)?
                    {
                        if work.ctime >= start {
                            racy.push(text.clone());
                        }
                        layer.entries.insert(text, Entry { work, copy });
                    }
                }
                // Too large to mask: a link would show its values.
                Kind::File if masks_home(&text) => {
                    layer.withheld.insert(text, "larger than 1 MB".to_owned());
                }
                Kind::Dir if captured => {
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
                        let concealed = hidden.contains(&from) || (hidden_dir && !shows(&from));
                        let caught = match stamp.kind {
                            _ if concealed => false,
                            Kind::File if stamp.size <= MAX_FILE => {
                                match fsops::clone_entry(&from, &to) {
                                    Ok(Some(work)) => {
                                        if let Some(copy) = Stamp::of(&to)? {
                                            if work.ctime >= start {
                                                racy.push(rel.clone());
                                            }
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
                            Kind::File if masks_home(&rel) => {
                                layer
                                    .withheld
                                    .insert(rel.clone(), "larger than 1 MB".to_owned());
                                true
                            }
                            Kind::Dir if capturable(&from) => {
                                match fsops::clone_tree(&from, &to, &real_clock) {
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
                        if !caught && !concealed {
                            let target = if stamp.kind == Kind::Symlink {
                                fs::read_link(&from).at(&from)?
                            } else {
                                from.clone()
                            };
                            std::os::unix::fs::symlink(target, &to).at(&to)?;
                            layer.links.insert(rel);
                        }
                    }
                    // What is hidden goes.
                    for path in &hidden {
                        let Some(rel) =
                            under(&real, path).filter(|r| r.starts_with(&format!("{text}/")))
                        else {
                            continue;
                        };
                        fsops::remove(&dir.join(&rel))?;
                        layer
                            .entries
                            .retain(|p, _| !(*p == rel || p.starts_with(&format!("{rel}/"))));
                    }
                }
                _ => {
                    std::os::unix::fs::symlink(&from, &to).at(&to)?;
                    layer.links.insert(text);
                }
            }
        }
        for path in state {
            if let Some(rel) = under(&given, path).or_else(|| under(&real, path)) {
                capture_state(&real, &dir, &rel, &hidden, &shown, &mut layer, &real_clock)?;
            }
        }
        // Each is compared with its clone once the clock has moved on, before
        // masking changes the clones (see `fsops::clone_tree`).
        racy.retain(|rel| layer.entries.contains_key(rel));
        let mut taken: Tree = racy
            .iter()
            .map(|rel| (rel.clone(), layer.entries[rel].work.clone()))
            .collect();
        fsops::pin_racy(&real, &dir, &mut taken, racy.clone(), &real_clock)?;
        for rel in racy {
            let (from, to) = (real.join(&rel), dir.join(&rel));
            match (taken.remove(&rel), Stamp::of(&to)?) {
                (Some(work), Some(copy)) if work.kind == Kind::File => {
                    layer.entries.insert(rel, Entry { work, copy });
                }
                // Gone meanwhile, or no longer a file: what is there now
                // is the person's own, as any entry not caught.
                (taken, _) => {
                    layer.entries.remove(&rel);
                    fsops::remove(&to)?;
                    if taken.is_some() {
                        std::os::unix::fs::symlink(&from, &to).at(&to)?;
                        layer.links.insert(rel);
                    }
                }
            }
        }
        self.mask_layer(&mut layer, None)?;
        // A write within the same tick of the clock as the clone would
        // leave the stamp as it was.
        let latest = layer
            .entries
            .values()
            .map(|e| e.copy.ctime)
            .max()
            .unwrap_or_default();
        if !layer_clock.wait_past(latest)? {
            for entry in layer.entries.values_mut() {
                if entry.copy.ctime == latest {
                    entry.copy = entry.copy.clone().unknown();
                }
            }
        }
        write_json(&self.layer_path(), &layer)?;
        Ok(dir)
    }

    /// What the task's commands changed in the home layer, by path, each
    /// with what it holds hashed (see [`home_digest`]).
    pub fn home_changes(&self) -> Result<Vec<HomeChange>> {
        self.home_changes_hashed(true)
    }

    /// How many changes [`Task::home_changes`] lists, without reading what
    /// they hold: for a list that only counts them.
    pub fn home_change_count(&self) -> Result<usize> {
        Ok(self.home_changes_hashed(false)?.len())
    }

    fn home_changes_hashed(&self, hashed: bool) -> Result<Vec<HomeChange>> {
        let hash = |dir: &Path, rel: &str| -> Result<String> {
            if hashed {
                fsops::content_hash(dir, rel)
            } else {
                Ok(String::new())
            }
        };
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
            if now.kind == Kind::Dir && on_the_way(&layer.ignored, &rel) {
                continue;
            }
            match layer.entries.get(&rel) {
                Some(was) if was.copy.unchanged(&now) => {}
                Some(_) => changes.push(HomeChange {
                    startup: startup(&rel),
                    hash: hash(&dir, &rel)?,
                    path: rel,
                    change: Change::Modified,
                    kind: now.kind,
                    target: now.link,
                }),
                None => changes.push(HomeChange {
                    startup: startup(&rel),
                    hash: hash(&dir, &rel)?,
                    path: rel,
                    change: Change::Created,
                    kind: now.kind,
                    target: now.link,
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
                    target: None,
                    hash: "missing".to_owned(),
                });
            }
        }
        changes.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(changes)
    }

    /// Takes the changes at `paths` from the layer to the real home. Every
    /// one of them is refused when any real path is no longer what it was
    /// when the layer was made; what is replaced or removed is kept first.
    pub fn apply_home(&self, paths: &[String], reviewed: &str) -> Result<HomeApplied> {
        let Some(mut layer) = self.load_layer()? else {
            return Ok(HomeApplied::default());
        };
        let _lock = self.lock()?;
        // Nothing left running in the task's sandbox changes the layer
        // while it is applied.
        self.quiet_sandbox("apply its home changes")?;
        let changes = self.home_changes()?;
        if home_digest(&changes) != reviewed {
            return Err(Error::Invalid(
                "the home changes are not the ones you reviewed any more; review them again".into(),
            ));
        }
        let wanted: Vec<HomeChange> = changes
            .into_iter()
            .filter(|c| paths.contains(&c.path))
            .collect();
        if wanted.is_empty() {
            return Ok(HomeApplied::default());
        }
        // As a commit: nothing comes to run as its owner or group where
        // what it replaces did not.
        for c in &wanted {
            let gains = fsops::hash_mode(&c.hash).unwrap_or(0) & fsops::RUNS_AS_OWNER;
            let had = Stamp::of(&layer.real.join(&c.path))?
                .filter(|s| s.kind == Kind::File)
                .map_or(0, |s| s.mode);
            if gains & !had != 0 {
                return Err(Error::Invalid(format!(
                    "~/{} would run as its owner or group (setuid or setgid), which Kule never brings into your home; nothing was applied",
                    c.path
                )));
            }
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
        // What is applied lands where the review showed it: no folder on
        // the way there in the real home is a link, unless this apply
        // makes it a folder first.
        let made_dirs: BTreeSet<&str> = wanted
            .iter()
            .filter(|c| c.kind == Kind::Dir && c.change != Change::Deleted)
            .map(|c| c.path.as_str())
            .collect();
        for c in &wanted {
            if let Some(link) = link_on_the_way(&layer.real, &c.path)
                && !made_dirs.contains(link.as_str())
            {
                return Err(through_link(&link));
            }
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
            if let Some(link) = link_on_the_way(&layer.real, &c.path) {
                return Err(through_link(&link));
            }
            let (from, to) = (dir.join(&c.path), layer.real.join(&c.path));
            let held = Stamp::of(&to)?;
            let keeps =
                c.change != Change::Created && held.as_ref().is_some_and(|s| s.kind != Kind::Dir);
            if keeps {
                keep(&layer.real, &c.path, &kept.join(&c.path))?;
                kept_any = true;
            }
            let before = layer.entries.get(&c.path).cloned();
            // Written through the folders on the way, opened without
            // following a link: what is applied lands where the review
            // showed it, whatever is put on the way meanwhile.
            let (home_dir, name) = in_home(&layer.real, &c.path)?;
            let there = fsops::entry_at(&home_dir, name).at(&to)?;
            match c.change {
                Change::Deleted => match there {
                    Some(Kind::Dir) => fsops::rmdir_at(&home_dir, name).at(&to)?,
                    Some(_) => fsops::remove_at(&home_dir, name).at(&to)?,
                    None => {}
                },
                _ => match c.kind {
                    Kind::Dir => {
                        if there != Some(Kind::Dir) {
                            fsops::remove_at(&home_dir, name).at(&to)?;
                            fsops::mkdir_at(&home_dir, name).at(&to)?;
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
                        if there == Some(Kind::Dir) {
                            fsops::remove_tree_at(&home_dir, name).at(&to)?;
                        }
                        fsops::link_at(&home_dir, name, &target).at(&to)?;
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
                        let bytes = self.unmasked(&layer.real, &c.path, bytes)?;
                        fsops::put_at(&home_dir, name, &bytes, mode.mode(), &to)?;
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
    /// Masks, in the layer, the settings that look like secrets in the files
    /// [`masks_home`] names (all, or those in `only`), as the copy's `.env`
    /// files are masked: each placeholder is kept with the path the value
    /// has in the real home, and goes back only there, as that setting
    /// (see [`Task::apply_home`]). The values are kept before any file is
    /// written; the entries take the masked files' stamps, so masking is no
    /// change.
    fn mask_layer(&self, layer: &mut Layer, only: Option<&[String]>) -> Result<()> {
        let dir = self.home_layer();
        let candidates: Vec<String> = layer
            .entries
            .iter()
            .filter(|(rel, e)| e.copy.kind == Kind::File && masks_home(rel))
            .filter(|(rel, _)| only.is_none_or(|only| only.contains(rel)))
            .map(|(rel, _)| rel.clone())
            .collect();
        if candidates.is_empty() {
            return Ok(());
        }
        let vault = self.vault()?;
        let mut kept = TaskSecrets::load(self.dir(), &self.home())?;
        let mut writes = Vec::new();
        for rel in candidates {
            let file = dir.join(&rel);
            let text = match read_settings(&dir, &rel) {
                Ok(text) => text,
                Err(why) => {
                    // Its values would be read in the clear: neither the
                    // clone nor the real file is shown.
                    fsops::remove(&file)?;
                    layer.entries.remove(&rel);
                    layer.withheld.insert(rel, why.to_owned());
                    continue;
                }
            };
            let real = layer.real.join(&rel).display().to_string();
            let masked = secrets::mask_with(&real, &text, &mut kept, &vault)?;
            if masked != text {
                writes.push((rel, file, masked));
            }
        }
        if writes.is_empty() {
            return Ok(());
        }
        // The values first: a masked file whose values were lost could only
        // ever be applied as placeholders.
        kept.save(self.dir(), &self.home())?;
        for (rel, file, masked) in writes {
            fsops::replace_file(&file, masked.as_bytes())?;
            if let (Some(entry), Some(now)) = (layer.entries.get_mut(&rel), Stamp::of(&file)?) {
                entry.copy = now;
            }
            layer.masked.insert(rel);
        }
        Ok(())
    }

    /// The files in the person's home whose secrets the layer holds masked
    /// (see [`Task::mask_layer`]): a sandbox that gives commands the layer
    /// hides them, so the values are read nowhere but where they were
    /// masked.
    pub fn home_masked(&self) -> Result<Vec<PathBuf>> {
        Ok(self
            .load_layer()?
            .map(|layer| {
                layer
                    .masked
                    .iter()
                    .chain(layer.withheld.keys())
                    .map(|rel| layer.real.join(rel))
                    .collect()
            })
            .unwrap_or_default())
    }

    /// What to tell the person of each file [`Task::home_withheld`] names,
    /// wherever the commands run: a run's notes, a command's output, a
    /// terminal.
    pub fn home_withheld_notes(&self) -> Result<Vec<String>> {
        Ok(self
            .home_withheld()?
            .into_iter()
            .map(|(path, why)| {
                format!(
                    "~/{path} is hidden from the commands here: it is {why}, so its secrets \
                     could not be masked"
                )
            })
            .collect())
    }

    /// The home's settings files Kule withheld from the commands, each with
    /// why: it could not mask their secrets (see [`Task::mask_layer`]).
    pub fn home_withheld(&self) -> Result<Vec<(String, String)>> {
        Ok(self
            .load_layer()?
            .map(|layer| layer.withheld.into_iter().collect())
            .unwrap_or_default())
    }

    /// `bytes`, the layer's file at `rel`, with the values Kule masked there
    /// put back, as the real home is to hold them; a placeholder anywhere
    /// else stays one.
    fn unmasked(&self, real: &Path, rel: &str, bytes: Vec<u8>) -> Result<Vec<u8>> {
        if !masks_home(rel) {
            return Ok(bytes);
        }
        let text = match String::from_utf8(bytes) {
            Ok(text) => text,
            Err(e) => return Ok(e.into_bytes()),
        };
        let path = real.join(rel).display().to_string();
        Ok(self.vault()?.unmask(&path, &text).into_bytes())
    }

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
        // What came back from the real home is masked again.
        let back: Vec<String> = wanted
            .iter()
            .filter(|c| c.change != Change::Created)
            .map(|c| c.path.clone())
            .collect();
        self.mask_layer(&mut layer, Some(&back))?;
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

fn changed(path: &str) -> Error {
    Error::Invalid(format!("~/{path} changed while it was being applied"))
}

/// The first folder on the way to `rel` under `real` that is a link, as a
/// path relative to `real`.
fn link_on_the_way(real: &Path, rel: &str) -> Option<String> {
    let mut at = String::new();
    for part in Path::new(rel).parent()?.components() {
        if !at.is_empty() {
            at.push('/');
        }
        at.push_str(&part.as_os_str().to_string_lossy());
        if fs::symlink_metadata(real.join(&at)).is_ok_and(|m| m.file_type().is_symlink()) {
            return Some(at);
        }
    }
    None
}

fn through_link(link: &str) -> Error {
    Error::Invalid(format!(
        "~/{link} is a link in your home, which Kule does not apply through: what goes there would land where it leads"
    ))
}

/// The folder in the real home `real` that holds `rel`, opened through
/// every folder on the way without following a link, and `rel`'s name in
/// it. A link on the way is refused as [`through_link`] says.
fn in_home<'a>(real: &Path, rel: &'a str) -> Result<(fs::File, &'a str)> {
    fsops::open_parent(real, rel).map_err(|e| match link_on_the_way(real, rel) {
        Some(link) => through_link(&link),
        None => e,
    })
}

/// Saves the file or link at `rel` in the real home `real` as `to`, read
/// without following a link on the way.
fn keep(real: &Path, rel: &str, to: &Path) -> Result<()> {
    if let Some(parent) = to.parent() {
        fs::create_dir_all(parent).at(parent)?;
    }
    let from = real.join(rel);
    match fsops::beneath(real, rel).at(&from)? {
        fsops::Beneath::Link(target) => std::os::unix::fs::symlink(target, to).at(to)?,
        fsops::Beneath::File(mut file) => {
            let mode = file.metadata().at(&from)?.permissions();
            let mut out = fs::File::create(to).at(to)?;
            std::io::copy(&mut file, &mut out).at(to)?;
            out.set_permissions(mode).at(to)?;
        }
        _ => return Err(changed(rel)),
    }
    Ok(())
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
        // As it would land: what Kule masked is no change.
        let after = match textdiff::side(&copy).at(&copy)? {
            Some(bytes) => Some(self.unmasked(&layer.real, path, bytes)?),
            None => None,
        };
        // Compared as they are, shown with what looks secret hidden on both
        // sides alike: a diff goes to a terminal or a window, not only to
        // the person.
        let (vault, key) = (self.vault()?, secrets::view_key()?);
        let shown = |side: Option<Vec<u8>>| match side {
            Some(bytes) => match String::from_utf8(bytes) {
                Ok(text) => Some(secrets::shown(&text, &vault, &key).into_bytes()),
                Err(e) => Some(e.into_bytes()),
            },
            None => None,
        };
        let (before, after) = (shown(before), shown(after));
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
        let _lock = self.lock()?;
        // The layer takes the changes back, so nothing in the sandbox
        // changes it meanwhile, as when they were applied.
        self.quiet_sandbox("undo its home changes")?;
        let kept = record.with_extension("");
        let made: BTreeSet<&str> = applied
            .paths
            .iter()
            .filter(|p| p.held.is_none())
            .map(|p| p.path.as_str())
            .collect();
        // As an apply: nothing is put back through a link on the way.
        for p in &applied.paths {
            if let Some(link) = link_on_the_way(&layer.real, &p.path) {
                return Err(through_link(&link));
            }
        }
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
            // Through the folders on the way, as the apply wrote.
            let (home_dir, name) = in_home(&layer.real, &p.path)?;
            let there = fsops::entry_at(&home_dir, name).at(&to)?;
            let clear = |there: Option<Kind>| match there {
                Some(Kind::Dir) => fsops::remove_tree_at(&home_dir, name).at(&to),
                Some(_) => fsops::remove_at(&home_dir, name).at(&to),
                None => Ok(()),
            };
            match p.held {
                _ if p.kept => {
                    // What the apply kept is Kule's own, in the task.
                    let from = kept.join(&p.path);
                    let meta = fs::symlink_metadata(&from).at(&from)?;
                    if meta.file_type().is_symlink() {
                        let target = fs::read_link(&from).at(&from)?;
                        if there == Some(Kind::Dir) {
                            clear(there)?;
                        }
                        fsops::link_at(&home_dir, name, &target).at(&to)?;
                    } else {
                        let bytes = fs::read(&from).at(&from)?;
                        if there == Some(Kind::Dir) {
                            clear(there)?;
                        }
                        fsops::put_at(&home_dir, name, &bytes, meta.permissions().mode(), &to)?;
                    }
                }
                Some(Kind::Dir) => {
                    if there.is_none() {
                        fsops::mkdir_at(&home_dir, name).at(&to)?;
                    }
                }
                Some(_) => {}
                None if p.kind == Kind::Dir => fsops::rmdir_at(&home_dir, name).at(&to)?,
                None => clear(there)?,
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

/// The layer's settings file `rel` under `dir` as text, read without
/// following a link; why not, where it cannot be masked.
fn read_settings(dir: &Path, rel: &str) -> std::result::Result<String, &'static str> {
    let Ok(fsops::Beneath::File(file)) = fsops::beneath(dir, rel) else {
        return Err("unreadable");
    };
    let mut bytes = Vec::new();
    std::io::Read::read_to_end(&mut std::io::Read::take(file, MAX_MASKED + 1), &mut bytes)
        .map_err(|_| "unreadable")?;
    if bytes.len() as u64 > MAX_MASKED {
        return Err("larger than 1 MB");
    }
    String::from_utf8(bytes).map_err(|_| "not UTF-8")
}
