//! Values Kule masks in a task's settings files that the task's git
//! repository holds anyway: in its index, at a reference, in a commit of
//! its history or of a reference's log, or in an object git keeps outside
//! all of them. A command in the task reads those with git
//! (`git show HEAD:.env`), whatever the copy's file says, so nothing reads
//! the copy until the person accepts it for the folder (see
//! [`crate::Task::committed`]).
//!
//! Kule looks only at the files a value is masked in (`.env` and the like,
//! [`crate::secrets::is_env_file`]) in the history, as `git log --all --
//! <paths>` would, and at every object git keeps outside the history whole.
//! A look that does not end within its budget, or a repository Kule cannot
//! read, decides nothing: it is told and held as a finding is.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::error::{Error, IoContext, Result};
use crate::fsops::{Flock, read_json, write_json};
use crate::secrets::is_env_file;

/// How long one look may take before it stops and decides nothing.
pub(crate) const BUDGET: Duration = Duration::from_secs(5);

/// Where a value Kule masks was found.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Finding {
    /// The file it is in, from the repository's root (empty for an object
    /// git keeps outside the history, which has no name).
    pub path: String,
    /// Where git holds it: `the index`, a reference, `commit <id>`, or
    /// `an object outside the history`.
    pub at: String,
    /// The repository, from the copy's root (empty for the copy itself).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub repo: String,
    /// The value, as a keyed digest only (see [`Accepted::digest`]).
    pub value: String,
}

/// What a look found: where the masked values are, and why it decided
/// nothing, if it did not.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Look {
    pub found: Vec<Finding>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub undecided: Option<String>,
}

impl Look {
    /// Whether nothing was found and the look decided.
    pub fn clean(&self) -> bool {
        self.found.is_empty() && self.undecided.is_none()
    }

    /// Each file a value was found in, with the first place git holds it
    /// (`.env (commit 1a2b3c4d5e6f)`), and an object outside the history
    /// last: what to tell the person.
    pub fn files(&self) -> Vec<String> {
        let mut by_file: BTreeMap<String, &str> = BTreeMap::new();
        for f in &self.found {
            let file = match (f.repo.is_empty(), f.path.is_empty()) {
                (_, true) => continue,
                (true, false) => f.path.clone(),
                (false, false) => format!("{}/{}", f.repo, f.path),
            };
            by_file.entry(file).or_insert(&f.at);
        }
        let mut files: Vec<String> = by_file
            .iter()
            .map(|(file, at)| format!("{file} ({at})"))
            .collect();
        if self.found.iter().any(|f| f.path.is_empty()) {
            files.push("an object git keeps outside the history".to_owned());
        }
        files
    }
}

/// The values masked in the copy at `copy`, as `(digest, value)`, looked
/// for in every repository that holds a file `masked_in` names.
pub(crate) fn look(
    copy: &Path,
    masked_in: &BTreeSet<String>,
    values: &[(String, String)],
    budget: Duration,
) -> Look {
    let mut look = Look::default();
    if values.is_empty() {
        return look;
    }
    let started = Instant::now();
    for root in roots(copy, masked_in) {
        let repo = root
            .strip_prefix(copy)
            .map(|p| p.to_string_lossy().replace('\\', "/"))
            .unwrap_or_default();
        let left = budget.saturating_sub(started.elapsed());
        match look_in(&root, values, left) {
            Ok((found, undecided)) => {
                look.found.extend(found.into_iter().map(|f| Finding {
                    repo: repo.clone(),
                    ..f
                }));
                if let Some(why) = undecided {
                    look.undecided.get_or_insert(why);
                }
            }
            Err(e) => {
                look.undecided.get_or_insert(format!(
                    "Kule could not read the repository{}: {e}",
                    if repo.is_empty() {
                        String::new()
                    } else {
                        format!(" in {repo}")
                    }
                ));
            }
        }
    }
    look.found.sort();
    look.found.dedup();
    look
}

/// The copy's own repository, and those that hold the files `paths` names:
/// the nearest folder above each, within `copy`, with a `.git` of its own
/// (a folder, or a file naming one elsewhere, as a worktree has). A secret
/// kept for the folder may be in the copy's history with no settings file
/// left in the copy.
fn roots(copy: &Path, paths: &BTreeSet<String>) -> BTreeSet<PathBuf> {
    let mut roots = BTreeSet::new();
    if std::fs::symlink_metadata(copy.join(".git")).is_ok() {
        roots.insert(copy.to_path_buf());
    }
    for path in paths {
        let mut dir = copy.join(path);
        while dir.pop() && dir.starts_with(copy) {
            if std::fs::symlink_metadata(dir.join(".git")).is_ok() {
                roots.insert(dir.clone());
                break;
            }
        }
    }
    roots
}

type Found = (Vec<Finding>, Option<String>);

fn look_in(root: &Path, values: &[(String, String)], budget: Duration) -> Result<Found> {
    let fail = |e: &dyn std::fmt::Display| Error::Git {
        path: root.to_path_buf(),
        message: e.to_string(),
    };
    let repo = crate::git::open(root)?;
    let mut walk = Walk {
        repo: &repo,
        started: Instant::now(),
        budget,
        trees: HashSet::new(),
        blobs: Vec::new(),
        seen: HashSet::new(),
        commits: 0,
    };
    if let Some(index) = repo.try_index().map_err(|e| fail(&e))? {
        for entry in index.entries() {
            let path = entry.path(&index).to_string();
            if is_env_file(&path) && walk.seen.insert(entry.id) {
                walk.blobs.push((entry.id, path, "the index".to_owned()));
            }
        }
    }
    // Every reference and what its log says it pointed to before: a
    // commit amended or reset away is still in the log, and git shows it.
    let mut tips: Vec<(String, gix::ObjectId)> = Vec::new();
    let mut logged = Vec::new();
    let mut references: Vec<gix::Reference<'_>> = Vec::new();
    if let Some(head) = repo.try_find_reference("HEAD").map_err(|e| fail(&e))? {
        references.push(head);
    }
    let platform = repo.references().map_err(|e| fail(&e))?;
    for reference in platform.all().map_err(|e| fail(&e))? {
        references.push(reference.map_err(|e| fail(&e))?);
    }
    for mut reference in references {
        let name = reference.name().as_bstr().to_string();
        if let Some(lines) = reference.log_iter().all().map_err(|e| fail(&e))? {
            for line in lines {
                let line = line.map_err(|e| fail(&e))?;
                logged.extend([line.previous_oid(), line.new_oid()]);
            }
        }
        if let Ok(id) = reference.peel_to_id() {
            tips.push((name, id.detach()));
        }
    }
    tips.extend(
        logged
            .into_iter()
            .map(|id| (format!("commit {}", short(&id)), id)),
    );
    let mut commits = Vec::new();
    for (at, id) in tips {
        if id.is_null() {
            continue;
        }
        match repo.find_object(id).map(|o| o.kind) {
            Ok(gix::object::Kind::Commit) => {
                let tree = repo
                    .find_object(id)
                    .map_err(|e| fail(&e))?
                    .try_into_commit()
                    .map_err(|e| fail(&e))?
                    .tree_id()
                    .map_err(|e| fail(&e))?
                    .detach();
                if !walk.tree(tree, "", &at)? {
                    return Ok(walk.cut_short());
                }
                commits.push(id);
            }
            Ok(gix::object::Kind::Tree) if !walk.tree(id, "", &at)? => {
                return Ok(walk.cut_short());
            }
            _ => {}
        }
    }
    let history = repo.rev_walk(commits).all().map_err(|e| fail(&e))?;
    for info in history {
        let info = info.map_err(|e| fail(&e))?;
        walk.commits += 1;
        let tree = info
            .object()
            .map_err(|e| fail(&e))?
            .tree_id()
            .map_err(|e| fail(&e))?
            .detach();
        if !walk.tree(tree, "", &format!("commit {}", short(&info.id)))? {
            return Ok(walk.cut_short());
        }
    }
    let mut found = Vec::new();
    for (id, path, at) in std::mem::take(&mut walk.blobs) {
        if walk.over() {
            return Ok((found, Some(walk.why())));
        }
        let data = repo.find_object(id).map_err(|e| fail(&e))?.detach().data;
        found.extend(holding(&data, values).map(|digest| Finding {
            path: path.clone(),
            at: at.clone(),
            repo: String::new(),
            value: digest,
        }));
    }
    // What git keeps outside the history (a loose object, a cruft pack's)
    // has no name: every blob there is looked into whole.
    for id in outside_history(&repo)? {
        if walk.over() {
            return Ok((found, Some(walk.why())));
        }
        if walk.seen.contains(&id) {
            continue;
        }
        let Ok(object) = repo.find_object(id) else {
            continue;
        };
        if object.kind != gix::object::Kind::Blob {
            continue;
        }
        found.extend(holding(&object.data, values).map(|digest| Finding {
            path: String::new(),
            at: "an object outside the history".to_owned(),
            repo: String::new(),
            value: digest,
        }));
    }
    Ok((found, None))
}

/// The digests of the values `data` holds.
fn holding<'a>(
    data: &'a [u8],
    values: &'a [(String, String)],
) -> impl Iterator<Item = String> + 'a {
    values
        .iter()
        .filter(|(_, value)| !value.is_empty() && contains(data, value.as_bytes()))
        .map(|(digest, _)| digest.clone())
}

fn contains(data: &[u8], needle: &[u8]) -> bool {
    data.windows(needle.len()).any(|w| w == needle)
}

fn short(id: &gix::oid) -> String {
    id.to_hex_with_len(12).to_string()
}

/// The objects git keeps loose, and those in cruft packs (where `git gc`
/// puts what nothing reaches any more): neither is where the history is.
fn outside_history(repo: &gix::Repository) -> Result<Vec<gix::ObjectId>> {
    let objects = repo.objects.store_ref().path().to_owned();
    let mut ids = Vec::new();
    let Ok(dirs) = std::fs::read_dir(&objects) else {
        return Ok(ids);
    };
    for dir in dirs.flatten() {
        let name = dir.file_name().to_string_lossy().into_owned();
        if name.len() != 2 || !name.bytes().all(|b| b.is_ascii_hexdigit()) {
            continue;
        }
        for file in std::fs::read_dir(dir.path()).at(&dir.path())?.flatten() {
            let rest = file.file_name().to_string_lossy().into_owned();
            if let Ok(id) = gix::ObjectId::from_hex(format!("{name}{rest}").as_bytes()) {
                ids.push(id);
            }
        }
    }
    let packs = objects.join("pack");
    if let Ok(entries) = std::fs::read_dir(&packs) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "mtimes") {
                let index = path.with_extension("idx");
                let index =
                    gix::odb::pack::index::File::at(&index, repo.object_hash()).map_err(|e| {
                        Error::Git {
                            path: index.clone(),
                            message: e.to_string(),
                        }
                    })?;
                ids.extend(index.iter().map(|e| e.oid));
            }
        }
    }
    Ok(ids)
}

struct Walk<'r> {
    repo: &'r gix::Repository,
    started: Instant,
    budget: Duration,
    /// Trees already looked at: a tree two commits share is looked at once,
    /// so a commit costs what it changed.
    trees: HashSet<gix::ObjectId>,
    /// Settings files to look into, the first place each was met.
    blobs: Vec<(gix::ObjectId, String, String)>,
    seen: HashSet<gix::ObjectId>,
    commits: usize,
}

impl Walk<'_> {
    fn over(&self) -> bool {
        self.started.elapsed() > self.budget
    }

    fn why(&self) -> String {
        format!(
            "Kule looked through {} commits of the repository's history in {} seconds and stopped before the end",
            self.commits,
            self.started.elapsed().as_secs().max(1)
        )
    }

    fn cut_short(&mut self) -> Found {
        (Vec::new(), Some(self.why()))
    }

    /// Notes the settings files in tree `id` under `prefix`; `false` once
    /// the budget is spent.
    fn tree(&mut self, id: gix::ObjectId, prefix: &str, at: &str) -> Result<bool> {
        use gix::objs::tree::EntryKind;
        if !self.trees.insert(id) {
            return Ok(true);
        }
        if self.over() {
            return Ok(false);
        }
        let fail = |e: &dyn std::fmt::Display| Error::Invalid(format!("tree {id}: {e}"));
        let object = self.repo.find_object(id).map_err(|e| fail(&e))?;
        let tree = object.try_into_tree().map_err(|e| fail(&e))?;
        let entries: Vec<(String, EntryKind, gix::ObjectId)> = tree
            .decode()
            .map_err(|e| fail(&e))?
            .entries
            .iter()
            .map(|e| (e.filename.to_string(), e.mode.kind(), e.oid.to_owned()))
            .collect();
        for (name, kind, oid) in entries {
            let path = if prefix.is_empty() {
                name
            } else {
                format!("{prefix}/{name}")
            };
            match kind {
                EntryKind::Tree => {
                    if !self.tree(oid, &path, at)? {
                        return Ok(false);
                    }
                }
                EntryKind::Blob | EntryKind::BlobExecutable
                    if is_env_file(&path) && self.seen.insert(oid) =>
                {
                    self.blobs.push((oid, path, at.to_owned()));
                }
                _ => {}
            }
        }
        Ok(true)
    }
}

/// What Kule's home keeps of what the person accepted, per folder.
const ACCEPTED_FILE: &str = "committed.json";

#[derive(Debug, Default, Serialize, Deserialize)]
struct Accepted {
    /// The key to [`Accepted::digest`]: values are kept as digests only.
    #[serde(default)]
    key: String,
    #[serde(default)]
    folders: Vec<Folder>,
}

/// What the person accepted for one folder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Folder {
    pub workspace: PathBuf,
    /// `(path, value digest)` of each finding accepted.
    pub values: BTreeSet<(String, String)>,
    /// Whether a look that decided nothing was accepted too.
    pub undecided: bool,
    pub added_unix: u64,
}

impl Accepted {
    fn read(home: &Path) -> Result<Accepted> {
        let path = home.join(ACCEPTED_FILE);
        if !path.is_file() {
            return Ok(Accepted::default());
        }
        read_json(&path)
    }

    fn write(&self, home: &Path) -> Result<()> {
        write_json(&home.join(ACCEPTED_FILE), self)
    }

    fn holds(&self, workspace: &Path, look: &Look) -> bool {
        self.folders.iter().any(|f| {
            f.workspace == workspace
                && (look.undecided.is_none() || f.undecided)
                && look
                    .found
                    .iter()
                    .all(|x| f.values.contains(&(x.path.clone(), x.value.clone())))
        })
    }
}

fn lock(home: &Path) -> Result<Flock> {
    Flock::take(&home.join("committed.lock"))
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// `(digest, value)` for each value: a digest keyed by Kule's home, made
/// once, so what is kept of a value opens nothing and matches only here.
pub(crate) fn digests(home: &Path, values: &[String]) -> Result<Vec<(String, String)>> {
    let _lock = lock(home)?;
    let mut accepted = Accepted::read(home)?;
    if accepted.key.is_empty() {
        let mut bytes = [0u8; 32];
        crate::osfs::random_bytes(&mut bytes).at(Path::new("random"))?;
        accepted.key = bytes.iter().map(|b| format!("{b:02x}")).collect();
        accepted.write(home)?;
    }
    let mut key = [0u8; 32];
    for (i, b) in key.iter_mut().enumerate() {
        *b =
            u8::from_str_radix(accepted.key.get(i * 2..i * 2 + 2).unwrap_or("00"), 16).unwrap_or(0);
    }
    Ok(values
        .iter()
        .map(|v| {
            (
                blake3::keyed_hash(&key, v.as_bytes()).to_hex()[..32].to_owned(),
                v.clone(),
            )
        })
        .collect())
}

/// Whether the person accepted `look` for `workspace`.
pub(crate) fn accepted(home: &Path, workspace: &Path, look: &Look) -> Result<bool> {
    Ok(Accepted::read(home)?.holds(workspace, look))
}

/// Keeps `look` as accepted for `workspace`, beside what was accepted there.
pub(crate) fn accept(home: &Path, workspace: &Path, look: &Look) -> Result<()> {
    let _lock = lock(home)?;
    let mut accepted = Accepted::read(home)?;
    let values = look.found.iter().map(|f| (f.path.clone(), f.value.clone()));
    match accepted
        .folders
        .iter_mut()
        .find(|f| f.workspace == workspace)
    {
        Some(folder) => {
            folder.values.extend(values);
            folder.undecided |= look.undecided.is_some();
        }
        None => accepted.folders.push(Folder {
            workspace: workspace.to_path_buf(),
            values: values.collect(),
            undecided: look.undecided.is_some(),
            added_unix: now(),
        }),
    }
    accepted.write(home)
}

/// Takes back what was accepted for `workspace`; whether there was any.
pub fn forget_committed(home: &Path, workspace: &Path) -> Result<bool> {
    let workspace = std::fs::canonicalize(workspace).at(workspace)?;
    let _lock = lock(home)?;
    let mut accepted = Accepted::read(home)?;
    let before = accepted.folders.len();
    accepted.folders.retain(|f| f.workspace != workspace);
    let gone = accepted.folders.len() != before;
    if gone {
        accepted.write(home)?;
    }
    Ok(gone)
}

/// The folders whose committed values the person accepted.
pub fn committed_accepted(home: &Path) -> Result<Vec<Folder>> {
    Ok(Accepted::read(home)?.folders)
}

/// The sentence that says where a look's findings are, for a refusal or a
/// note: the files, and the first place git holds each.
pub(crate) fn described(look: &Look) -> String {
    look.files().join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;
    use tempfile::TempDir;

    fn git(dir: &Path, args: &[&str]) {
        let out = Command::new("git")
            .args(["-c", "user.name=T", "-c", "user.email=t@example.com"])
            .args([
                "-c",
                "commit.gpgsign=false",
                "-c",
                "init.defaultBranch=main",
            ])
            .args(args)
            .current_dir(dir)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    /// A repository whose history holds `value` in `.env`, one commit back.
    fn repository(value: &str) -> TempDir {
        let dir = TempDir::new().unwrap();
        git(dir.path(), &["init", "-q"]);
        std::fs::write(dir.path().join(".env"), format!("KEY={value}\n")).unwrap();
        git(dir.path(), &["add", ".env"]);
        git(dir.path(), &["commit", "-qm", "a"]);
        git(dir.path(), &["rm", "-q", "--cached", ".env"]);
        git(dir.path(), &["commit", "-qm", "b"]);
        dir
    }

    fn values(value: &str) -> Vec<(String, String)> {
        vec![("d1".to_owned(), value.to_owned())]
    }

    #[test]
    fn a_look_finds_the_value_and_not_another() {
        let dir = repository("s3cret-value-1");
        let paths = BTreeSet::from([".env".to_owned()]);
        let look = look(dir.path(), &paths, &values("s3cret-value-1"), BUDGET);
        assert_eq!(look.undecided, None);
        assert!(
            look.found
                .iter()
                .any(|f| f.path == ".env" && f.value == "d1"),
            "{look:?}"
        );
        let look = super::look(dir.path(), &paths, &values("another-value-2"), BUDGET);
        assert!(look.clean(), "{look:?}");
    }

    /// Out of time, a look decides nothing and says so; it is never taken
    /// for a look that found nothing.
    #[test]
    fn a_look_out_of_time_decides_nothing() {
        let dir = repository("s3cret-value-1");
        let paths = BTreeSet::from([".env".to_owned()]);
        let look = look(
            dir.path(),
            &paths,
            &values("another-value-2"),
            Duration::ZERO,
        );
        assert!(!look.clean());
        assert!(look.undecided.is_some_and(|why| why.contains("stopped")));
    }

    /// An acceptance holds for its folder and what it was given: not for a
    /// value found since, nor for a look that decided nothing unless that
    /// was accepted too.
    #[test]
    fn an_acceptance_holds_for_what_was_accepted() {
        let home = TempDir::new().unwrap();
        let ws = Path::new("/w");
        let finding = |value: &str| Finding {
            path: ".env".to_owned(),
            at: "the index".to_owned(),
            repo: String::new(),
            value: value.to_owned(),
        };
        let found = Look {
            found: vec![finding("d1")],
            undecided: None,
        };
        accept(home.path(), ws, &found).unwrap();
        assert!(accepted(home.path(), ws, &found).unwrap());
        assert!(!accepted(home.path(), Path::new("/other"), &found).unwrap());
        let more = Look {
            found: vec![finding("d1"), finding("d2")],
            undecided: None,
        };
        assert!(!accepted(home.path(), ws, &more).unwrap());
        let undecided = Look {
            found: vec![finding("d1")],
            undecided: Some("cut short".to_owned()),
        };
        assert!(!accepted(home.path(), ws, &undecided).unwrap());
        accept(home.path(), ws, &undecided).unwrap();
        assert!(accepted(home.path(), ws, &undecided).unwrap());
    }
}
