//! Kule's transactional engine (milestone M0).
//!
//! A [`Task`] works in a copy-on-write copy of a workspace. Whatever happens
//! inside the copy is one uncommitted transaction:
//!
//! - [`Task::effects`] reports it by comparing the copy with the baseline;
//! - [`Task::commit`] takes a checkpoint and applies it to the real workspace;
//! - [`Task::rollback`] throws it away;
//! - [`Task::undo`] reverts the last commit, in the workspace and the copy.
//!
//! Commands run through a [`Sandbox`]: writes outside the copy, reads of
//! credentials and, when asked, network connections are refused by the
//! operating system. Policy over the effect set arrives later in M1.

#[cfg(not(unix))]
compile_error!("Kule M0 supports macOS and Linux only");

mod approvals;
mod caches;
mod checkpoint;
mod clock;
mod effects;
mod error;
mod fsops;
mod git;
mod history;
mod home;
mod ignore;
mod manifest;
mod meet;
#[cfg(target_os = "linux")]
mod namespace;
mod network;
mod outline;
mod policy;
mod preview;
mod protect;
pub mod remembered;
mod sandbox;
mod secrets;
mod snapshot;
mod steps;
mod task;
mod tls;
mod usage;

pub use approvals::{
    Answer, Approvals, DEFAULT_TIMEOUT, MAX_TIMEOUT, MAX_WAITING, MIN_TIMEOUT, Question,
};
pub use caches::Sources as CacheSources;
pub use checkpoint::{Checkpoint, State as CheckpointState};
pub use effects::{Change, Effect, EffectSet, Move};
pub use error::{Error, Result};
pub use git::{GitState, GitSummary, RefChange};
pub use history::{Entry as HistoryEntry, Event as HistoryEvent};
pub use home::{HomeApplied, HomeChange};
pub use kule_entities as entities;
pub use manifest::Kind;
pub use meet::{Meeting, Touched, changed_entities, meet, touched};
pub use network::{Connection, Proxy, connections};
pub use outline::FileOutline;
pub use policy::{DEFAULT_POLICY, Judgement, Policy, REPO_POLICY, Ruling, Verdict};
pub use preview::{Picture, Preview, previewable};
pub use protect::{Kept, PutBack};
pub use remembered::{
    Allowance, Impact, REMEMBERABLE, REMEMBERED_FILE, Remembered, forget, impact, remember,
};
pub use sandbox::{
    Denial, HIDDEN_IN_HOME, Network, PASSED_ENV, Sandbox, denials, isolates_network,
    protects_in_place, record_denials,
};
pub use secrets::{SECRETS_FILE, SecretInfo, Vault, add_secret, remove_secret, secrets};
pub use steps::{Step, StepChange};
pub use task::{AgentAtWork, Task, TaskInfo};
pub use usage::{Money, RunUsage, Spent};
