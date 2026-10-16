//! A task's history as a signed chain: each commit and undo is an entry
//! naming the one before it by hash and signed with the key of Kule's home,
//! appended to `refs/kule/log` in the task's checkpoint store. Changing,
//! dropping or reordering an entry breaks the chain, which
//! [`History::read`] checks entry by entry.
//!
//! The chain is as safe as the key: it lives in Kule's home, which the
//! sandbox hides from what a task runs.

use std::path::Path;

use lonca_core::canonical::digest_of;
use lonca_core::keys::{KeyPair, verify_hex};
use lonca_core::store::CommitSpec;
use lonca_core::{FileEntry, FileMap, FileMode, Store};
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::snapshot::{lonca, now_ms};

pub(crate) const LOG_REF: &str = "refs/kule/log";
const GENESIS: &str = "genesis";
const ENTRY: &str = "entry.json";

/// One thing that happened to the workspace through the task.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum Event {
    /// Checkpoint `checkpoint` applied `changes` changes; `snapshot` is
    /// what they replaced and `effects` the hash of the effect set.
    Commit {
        checkpoint: u32,
        changes: usize,
        snapshot: String,
        effects: String,
    },
    Undo {
        checkpoint: u32,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub seq: u64,
    /// The hash of the entry before, or `genesis`.
    pub prev: String,
    pub time_ms: i64,
    #[serde(flatten)]
    pub event: Event,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Signed {
    entry: Entry,
    hash: String,
    /// The signer's public key, which has to be the home's.
    key: String,
    sig: String,
}

pub(crate) struct History<'a> {
    store: &'a Store,
}

impl<'a> History<'a> {
    pub fn new(store: &'a Store) -> History<'a> {
        History { store }
    }

    /// Appends `event`, signed with `key`, after checking the chain so far.
    pub fn append(&self, key: &KeyPair, event: Event) -> Result<Entry> {
        let fail = |e| lonca(self.store.path(), e);
        let so_far = self.read(&key.public_hex())?;
        let head = self.store.ref_get(LOG_REF).map_err(fail)?;
        let entry = Entry {
            seq: so_far.len() as u64,
            prev: so_far
                .last()
                .map_or(GENESIS.to_owned(), |(_, hash)| hash.clone()),
            time_ms: now_ms(),
            event,
        };
        let hash = digest_of(&entry);
        let signed = Signed {
            sig: key.sign_hex(hash.as_bytes()),
            key: key.public_hex(),
            hash,
            entry: entry.clone(),
        };
        let bytes = serde_json::to_vec(&signed).map_err(|e| Error::Invalid(e.to_string()))?;
        let mut files = FileMap::new();
        files.insert(
            ENTRY.to_owned(),
            FileEntry {
                mode: FileMode::Regular,
                blob: self.store.write_blob(&bytes).map_err(fail)?,
            },
        );
        let tree = self.store.write_tree(&files).map_err(fail)?;
        let commit = self
            .store
            .write_commit(&CommitSpec {
                tree,
                parents: head.iter().cloned().collect(),
                author_name: "Kule".to_owned(),
                author_email: "kule@localhost".to_owned(),
                time_ms: entry.time_ms,
                message: format!("kule history {}\n", entry.seq),
                headers: Vec::new(),
            })
            .map_err(fail)?;
        // Fails when someone appended meanwhile; the task's lock prevents it.
        self.store
            .ref_cas(LOG_REF, &commit, head.as_ref())
            .map_err(fail)?;
        Ok(entry)
    }

    /// Every entry, oldest first, each with its hash, after checking that
    /// the chain is whole and every entry is signed by `key`.
    pub fn read(&self, key: &str) -> Result<Vec<(Entry, String)>> {
        let fail = |e| lonca(self.store.path(), e);
        let mut newest_first = Vec::new();
        let mut at = self.store.ref_get(LOG_REF).map_err(fail)?;
        while let Some(commit) = at {
            let info = self.store.read_commit(&commit).map_err(fail)?;
            let bytes = self
                .store
                .read_file(&commit, ENTRY)
                .map_err(fail)?
                .ok_or_else(|| broken(self.store.path(), "an entry without entry.json"))?;
            let signed: Signed = serde_json::from_slice(&bytes)
                .map_err(|_| broken(self.store.path(), "an entry that does not parse"))?;
            newest_first.push(signed);
            at = info.parents.first().cloned();
        }
        let mut entries = Vec::new();
        let mut prev = GENESIS.to_owned();
        for (seq, signed) in newest_first.into_iter().rev().enumerate() {
            let entry = &signed.entry;
            let whole = entry.seq == seq as u64
                && entry.prev == prev
                && digest_of(entry) == signed.hash
                && signed.key == key
                && verify_hex(key, signed.hash.as_bytes(), &signed.sig);
            if !whole {
                return Err(broken(
                    self.store.path(),
                    &format!("entry {seq} is not the one that was signed"),
                ));
            }
            prev = signed.hash.clone();
            entries.push((signed.entry, signed.hash));
        }
        Ok(entries)
    }
}

fn broken(store: &Path, what: &str) -> Error {
    Error::Invalid(format!(
        "the task's history in {} does not verify: {what}",
        store.display()
    ))
}
