//! Effect sets: what the copy holds that the baseline does not.

use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::fsops::{self, same_bytes};
use crate::git::{self, GitSummary};
use crate::ignore;
use crate::manifest::{Baseline, Entry, Kind, Stamp, Tree, scan};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Change {
    Created,
    Modified,
    Deleted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Effect {
    pub path: String,
    pub change: Change,
    /// The entry's type now, or before it was deleted.
    pub kind: Kind,
    /// The workspace's ignore rules exclude the path: a commit leaves it in
    /// the copy unless asked to take it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub ignored: bool,
}

impl Effect {
    /// Whether the path is inside the repository's `.git` directory. Those
    /// effects are read through [`EffectSet::git`] rather than one by one.
    pub fn in_git(&self) -> bool {
        git::classify(&self.path).is_some()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EffectSet {
    /// Every changed path, `.git` included, sorted so that a parent always
    /// comes before its children.
    pub effects: Vec<Effect>,
    /// What the changes under `.git` mean; `None` when they mean nothing,
    /// such as an index refresh by `git status`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git: Option<GitSummary>,
    /// Deleted paths that live on under another name: the same file (or
    /// directory) in the copy, renamed. A commit still deletes one path and
    /// writes the other; a person and the policy see a move.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub moves: Vec<Move>,
    /// The workspace is a git repository, which can bring back what a
    /// commit deletes; a plain folder has only the task's own undo.
    #[serde(default)]
    pub repository: bool,
    /// What each changed file holds and where each changed link points, by
    /// path and hashed, as read for a commit or its review. Part of the
    /// digest: a commit takes only the content that was reviewed.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub contents: BTreeMap<String, String>,
    /// Folders in the copy that hold a repository of their own without a
    /// `.git` in their name (a bare one: `HEAD`, `objects/`, `refs/`). Git
    /// run there reads its config and hooks; the policy judges what is in
    /// them as it judges what is in a `.git`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub git_dirs: Vec<String>,
}

/// A path the copy renamed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Move {
    pub from: String,
    pub to: String,
}

impl EffectSet {
    pub fn is_empty(&self) -> bool {
        self.effects.is_empty()
    }

    pub fn count(&self, change: Change) -> usize {
        self.effects.iter().filter(|e| e.change == change).count()
    }

    pub fn paths(&self) -> Vec<String> {
        self.effects.iter().map(|e| e.path.clone()).collect()
    }

    /// The effects outside `.git`.
    pub fn files(&self) -> impl Iterator<Item = &Effect> {
        self.effects.iter().filter(|e| !e.in_git())
    }

    /// The effects on paths the workspace ignores.
    pub fn ignored(&self) -> impl Iterator<Item = &Effect> {
        self.effects.iter().filter(|e| e.ignored)
    }

    /// A hash of the set, to tell whether what a person reviewed is still
    /// what a commit would take.
    pub fn digest(&self) -> String {
        let bytes = serde_json::to_vec(self).unwrap_or_default();
        blake3::hash(&bytes).to_hex().to_string()
    }

    /// What a commit takes: everything, or everything but the ignored paths.
    /// Git's lock files never go: one in the copy is a crashed process's.
    /// Nor what git writes beside its work, reflogs, `ORIG_HEAD` and
    /// `FETCH_HEAD`: each names commits a person could check out later
    /// (`HEAD@{1}`, `git reset ORIG_HEAD`) that no reference brought for
    /// review. The folder keeps its own.
    pub fn to_commit(&self, include_ignored: bool) -> EffectSet {
        let effects: Vec<Effect> = self
            .effects
            .iter()
            .filter(|e| include_ignored || !e.ignored)
            .filter(|e| {
                !matches!(
                    git::classify(&e.path),
                    Some(git::Part::Lock | git::Part::Derived)
                )
            })
            .cloned()
            .collect();
        let taken = |path: &str| effects.iter().any(|e| e.path == path);
        let moves = self
            .moves
            .iter()
            .filter(|m| taken(&m.from) && taken(&m.to))
            .cloned()
            .collect();
        let contents = self
            .contents
            .iter()
            .filter(|(path, _)| taken(path))
            .map(|(path, hash)| (path.clone(), hash.clone()))
            .collect();
        EffectSet {
            git: self.git.clone(),
            moves,
            repository: self.repository,
            effects,
            contents,
            git_dirs: self.git_dirs.clone(),
        }
    }

    /// Reads what each path of the set is now under `root` (the copy),
    /// without following a link on the way, into [`EffectSet::contents`]:
    /// a file's or link's content, a directory, or nothing (deleted).
    pub(crate) fn read_contents(&mut self, root: &Path) -> Result<()> {
        self.contents = self
            .effects
            .iter()
            .map(|e| Ok((e.path.clone(), fsops::content_hash(root, &e.path)?)))
            .collect::<Result<_>>()?;
        Ok(())
    }

    /// The deleted paths that are not moves: what is gone for good once
    /// the commit is in.
    pub fn deleted(&self) -> impl Iterator<Item = &Effect> {
        self.files()
            .filter(|e| e.change == Change::Deleted && !self.moves.iter().any(|m| m.from == e.path))
    }
}

pub(crate) fn compute(copy: &Path, work: &Path, baseline: &Baseline) -> Result<EffectSet> {
    let now = scan(copy)?;
    let mut effects = Vec::new();
    for (path, entry) in &baseline.entries {
        match now.get(path) {
            None => effects.push(Effect {
                path: path.clone(),
                change: Change::Deleted,
                kind: entry.copy.kind,
                ignored: false,
            }),
            Some(stamp)
                if !entry.copy.unchanged(stamp) && differs(copy, work, path, entry, stamp)? =>
            {
                effects.push(Effect {
                    path: path.clone(),
                    change: Change::Modified,
                    kind: stamp.kind,
                    ignored: false,
                })
            }
            Some(_) => {}
        }
    }
    for (path, stamp) in &now {
        if !baseline.entries.contains_key(path) {
            effects.push(Effect {
                path: path.clone(),
                change: Change::Created,
                kind: stamp.kind,
                ignored: false,
            });
        }
    }
    effects.sort_by(|a, b| a.path.cmp(&b.path));
    ignore::mark(work, &mut effects)?;
    let git = git::summarize(copy, work, baseline.git.as_ref(), &effects)?;
    let moves = moves(&effects, baseline, &now);
    Ok(EffectSet {
        effects,
        git,
        moves,
        repository: baseline.git.is_some(),
        contents: BTreeMap::new(),
        git_dirs: git_dirs(&now),
    })
}

/// The folders of `tree` that hold a repository's own files (`HEAD`, with
/// `objects/` and `refs/` beside it), but for the copy's own `.git`.
fn git_dirs(tree: &Tree) -> Vec<String> {
    let is = |path: &str, kind: Kind| tree.get(path).is_some_and(|s| s.kind == kind);
    tree.iter()
        .filter_map(|(path, stamp)| {
            let dir = path.strip_suffix("/HEAD")?;
            let own = dir == ".git" || dir.starts_with(".git/");
            (stamp.kind == Kind::File
                && !own
                && is(&format!("{dir}/objects"), Kind::Dir)
                && is(&format!("{dir}/refs"), Kind::Dir))
            .then(|| dir.to_owned())
        })
        .collect()
}

/// The deleted paths whose file lives on at a created or replaced one: the
/// same inode, made at the same time where an inode number is given again
/// (see [`Stamp::same_entry`]), and for a file the same size and
/// modification time, which a rename keeps and a write does not. Read
/// from metadata alone, so sorting
/// thousands of photos costs no reading; an agent that means to could pass
/// a deletion off as a move, which is why a move is still shown as one.
fn moves(effects: &[Effect], baseline: &Baseline, now: &Tree) -> Vec<Move> {
    let mut gone: HashMap<u64, (&str, &Stamp)> = HashMap::new();
    for effect in effects
        .iter()
        .filter(|e| e.change == Change::Deleted && !e.in_git())
    {
        if let Some(entry) = baseline.entries.get(&effect.path) {
            gone.insert(entry.copy.ino, (&effect.path, &entry.copy));
        }
    }
    let mut moves = Vec::new();
    for effect in effects
        .iter()
        .filter(|e| e.change != Change::Deleted && !e.in_git())
    {
        let Some(stamp) = now.get(&effect.path) else {
            continue;
        };
        let Some((from, before)) = gone.get(&stamp.ino) else {
            continue;
        };
        let same = before.kind == stamp.kind
            && before.same_entry(stamp)
            && (stamp.kind == Kind::Dir
                || (before.size == stamp.size && before.mtime == stamp.mtime));
        if same {
            moves.push(Move {
                from: (*from).to_owned(),
                to: effect.path.clone(),
            });
            gone.remove(&stamp.ino);
        }
    }
    moves.sort_by(|a, b| a.to.cmp(&b.to));
    moves
}

/// The copy's stamp moved; decides whether its type, mode or content did.
fn differs(copy: &Path, work: &Path, path: &str, entry: &Entry, now: &Stamp) -> Result<bool> {
    let before = &entry.copy;
    if before.kind != now.kind || before.mode != now.mode {
        return Ok(true);
    }
    match now.kind {
        Kind::Dir => Ok(false),
        Kind::Symlink => Ok(before.link != now.link),
        Kind::File if before.size != now.size => Ok(true),
        Kind::File => {
            // Same size: compare bytes with the workspace's file, which holds
            // the baseline content as long as nobody touched it.
            let work_file = work.join(path);
            match Stamp::of(&work_file)? {
                Some(w) if entry.work.unchanged(&w) => {
                    Ok(!same_bytes(&copy.join(path), &work_file)?)
                }
                _ => Ok(true),
            }
        }
    }
}

/// Inode numbers given again are Linux's: APFS never does.
#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use crate::manifest::Entry;

    fn stamp(ino: u64, born: Option<(i64, i64)>) -> Stamp {
        Stamp {
            kind: Kind::File,
            size: 2,
            mode: 0o644,
            mtime: (1_700_000_000, 0),
            ctime: (1_700_000_000, 0),
            ino,
            link: None,
            born,
        }
    }

    fn effect(path: &str, change: Change) -> Effect {
        Effect {
            path: path.to_owned(),
            change,
            kind: Kind::File,
            ignored: false,
        }
    }

    /// The moves found when `a`, made at `was`, is deleted and `b`, made at
    /// `now`, has its inode number, size and modification time.
    fn moved(was: Option<(i64, i64)>, now: Option<(i64, i64)>) -> Vec<Move> {
        let mut baseline = Baseline::default();
        baseline.entries.insert(
            "a".to_owned(),
            Entry {
                work: stamp(1, None),
                copy: stamp(7, was),
            },
        );
        let tree: Tree = [("b".to_owned(), stamp(7, now))].into_iter().collect();
        let effects = [effect("a", Change::Deleted), effect("b", Change::Created)];
        moves(&effects, &baseline, &tree)
    }

    /// Linux filesystems give a deleted file's inode number to the next one
    /// made (ext4 at once): when each was made tells them apart, and where
    /// that is not known, a deletion is not passed off as a move.
    #[test]
    fn an_inode_number_given_again_is_not_a_move() {
        assert_eq!(moved(Some((1, 0)), Some((1, 0))).len(), 1);
        assert!(moved(Some((1, 0)), Some((2, 0))).is_empty());
        assert!(moved(None, None).is_empty());
    }
}
