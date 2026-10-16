//! What a commit replaced, kept in a store of Kule's own: Lonca's object
//! store, git objects in a bare repository under the task. The file
//! contents are blobs; what git cannot hold (permission bits, times,
//! directories, paths such as `.git/...`) goes in a sidecar, `meta.json`.
//! Blobs are named by number, so no workspace path has to be valid in git.
//!
//! [`Snapshots::restore`] lays the snapshot out as a directory, the way the
//! checkpoint's `before/` used to hold it, so undo works on it unchanged.

use std::collections::BTreeSet;
use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use filetime::FileTime;
use lonca_core::store::CommitSpec;
use lonca_core::{FileEntry, FileMap, FileMode, GitId, Store};
use serde::{Deserialize, Serialize};

use crate::error::{Error, IoContext, Result};
use crate::manifest::{Kind, Stamp};

const META: &str = "meta.json";

#[derive(Debug, Serialize, Deserialize)]
struct Meta {
    entries: Vec<MetaEntry>,
    /// Parents of the paths the commit touched that were directories, with
    /// the time to give back to the ones the commit did not list itself.
    parents: Vec<Parent>,
}

#[derive(Debug, Serialize, Deserialize)]
struct MetaEntry {
    path: String,
    kind: Kind,
    mode: u32,
    mtime: (i64, i64),
    #[serde(default, skip_serializing_if = "Option::is_none")]
    link: Option<String>,
    /// The number of the blob holding a file's bytes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    blob: Option<usize>,
}

#[derive(Debug, Serialize, Deserialize)]
struct Parent {
    path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    mtime: Option<(i64, i64)>,
}

pub(crate) struct Snapshots {
    store: Store,
}

impl Snapshots {
    /// The store at `dir`, made on first use.
    pub fn open(dir: &Path) -> Result<Snapshots> {
        let store = if dir.join("HEAD").is_file() {
            Store::open(dir)
        } else {
            fs::create_dir_all(dir).at(dir)?;
            Store::init_bare(dir)
        }
        .map_err(|e| lonca(dir, e))?;
        Ok(Snapshots { store })
    }

    pub fn store(&self) -> &Store {
        &self.store
    }

    /// Keeps the workspace's version of `replaced` (the paths a commit is
    /// about to replace or delete) and the times of the parents of `all`
    /// (every path it touches); returns the snapshot's commit.
    pub fn save(
        &self,
        id: u32,
        work: &Path,
        replaced: &[String],
        all: &[String],
    ) -> Result<String> {
        let at = self.store.path().to_owned();
        let fail = |e| lonca(&at, e);
        let listed: BTreeSet<&str> = all.iter().map(String::as_str).collect();
        let mut parents = Vec::new();
        let mut seen = BTreeSet::new();
        for path in all {
            // The workspace's root is a parent too, as "".
            let Some(parent) = Path::new(path).parent().and_then(Path::to_str) else {
                continue;
            };
            if !seen.insert(parent.to_owned()) {
                continue;
            }
            let Some(stamp) = Stamp::of(&work.join(parent))?.filter(|s| s.kind == Kind::Dir) else {
                continue;
            };
            parents.push(Parent {
                path: parent.to_owned(),
                mtime: (!listed.contains(parent)).then_some(stamp.mtime),
            });
        }

        let mut files = FileMap::new();
        let mut entries = Vec::new();
        for path in replaced {
            let Some(stamp) = Stamp::of(&work.join(path))? else {
                continue;
            };
            let blob = if stamp.kind == Kind::File {
                let from = work.join(path);
                let bytes = fs::read(&from).at(&from)?;
                let n = files.len();
                files.insert(
                    format!("files/{n}"),
                    FileEntry {
                        mode: FileMode::Regular,
                        blob: self.store.write_blob(&bytes).map_err(fail)?,
                    },
                );
                Some(n)
            } else {
                None
            };
            entries.push(MetaEntry {
                path: path.clone(),
                kind: stamp.kind,
                mode: stamp.mode,
                mtime: stamp.mtime,
                link: stamp.link,
                blob,
            });
        }
        let meta =
            serde_json::to_vec(&Meta { entries, parents }).map_err(|source| Error::State {
                path: at.join(META),
                source,
            })?;
        files.insert(
            META.to_owned(),
            FileEntry {
                mode: FileMode::Regular,
                blob: self.store.write_blob(&meta).map_err(fail)?,
            },
        );
        let tree = self.store.write_tree(&files).map_err(fail)?;
        let commit = self
            .store
            .write_commit(&CommitSpec {
                tree,
                parents: Vec::new(),
                author_name: "Kule".to_owned(),
                author_email: "kule@localhost".to_owned(),
                time_ms: now_ms(),
                message: format!("checkpoint {id}: what the commit replaced\n"),
                headers: vec![("kule-checkpoint".to_owned(), id.to_string())],
            })
            .map_err(fail)?;
        self.store
            .ref_force(&format!("refs/kule/checkpoints/{id}"), &commit)
            .map_err(fail)?;
        Ok(commit.0)
    }

    /// Lays snapshot `commit` out at `dst`, which must not exist: the files
    /// with their modes and times, symlinks, directories, and the parents'
    /// times.
    pub fn restore(&self, commit: &str, dst: &Path) -> Result<()> {
        let at = self.store.path().to_owned();
        let fail = |e| lonca(&at, e);
        let files = self
            .store
            .commit_files(&GitId::parse(commit).map_err(fail)?)
            .map_err(fail)?;
        let blob = |name: &str| -> Result<Vec<u8>> {
            let entry = files
                .get(name)
                .ok_or_else(|| Error::Invalid(format!("snapshot {commit} has no {name}")))?;
            self.store.read_blob(&entry.blob).map_err(|e| lonca(&at, e))
        };
        let meta: Meta = serde_json::from_slice(&blob(META)?).map_err(|source| Error::State {
            path: at.join(META),
            source,
        })?;

        fs::create_dir(dst).at(dst)?;
        for parent in &meta.parents {
            let dir = dst.join(&parent.path);
            fs::create_dir_all(&dir).at(&dir)?;
        }
        let mut entries: Vec<&MetaEntry> = meta.entries.iter().collect();
        entries.sort_by(|a, b| a.path.cmp(&b.path));
        for entry in &entries {
            let to = dst.join(&entry.path);
            match entry.kind {
                Kind::Dir => fs::create_dir_all(&to).at(&to)?,
                Kind::Symlink => symlink(entry.link.as_deref().unwrap_or_default(), &to).at(&to)?,
                Kind::File => {
                    let n = entry.blob.ok_or_else(|| {
                        Error::Invalid(format!("snapshot {commit}: no bytes for {}", entry.path))
                    })?;
                    fs::write(&to, blob(&format!("files/{n}"))?).at(&to)?;
                    finish(&to, entry.mode, entry.mtime)?;
                }
            }
        }
        // Directories last: what goes into them moves their times.
        for entry in entries.iter().rev().filter(|e| e.kind == Kind::Dir) {
            finish(&dst.join(&entry.path), entry.mode, entry.mtime)?;
        }
        for parent in &meta.parents {
            if let Some((secs, nanos)) = parent.mtime {
                let dir = dst.join(&parent.path);
                let time = FileTime::from_unix_time(secs, nanos as u32);
                filetime::set_file_mtime(&dir, time).at(&dir)?;
            }
        }
        Ok(())
    }
}

fn finish(path: &Path, mode: u32, (secs, nanos): (i64, i64)) -> Result<()> {
    filetime::set_file_mtime(path, FileTime::from_unix_time(secs, nanos as u32)).at(path)?;
    fs::set_permissions(path, fs::Permissions::from_mode(mode)).at(path)
}

pub(crate) fn lonca(path: &Path, e: lonca_core::Error) -> Error {
    Error::Invalid(format!("checkpoint store {}: {e}", path.display()))
}

pub(crate) fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}
