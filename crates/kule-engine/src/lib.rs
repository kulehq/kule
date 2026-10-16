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

mod checkpoint;
mod clock;
mod effects;
mod error;
mod fsops;
mod git;
mod manifest;
mod network;
mod policy;
mod sandbox;
mod steps;
mod task;

pub use checkpoint::{Checkpoint, State as CheckpointState};
pub use effects::{Change, Effect, EffectSet};
pub use error::{Error, Result};
pub use git::{GitState, GitSummary, RefChange};
pub use manifest::Kind;
pub use network::{Connection, Proxy, connections};
pub use policy::{DEFAULT_POLICY, Judgement, Policy, REPO_POLICY, Ruling, Verdict};
pub use sandbox::{Denial, Network, Sandbox, denials, record_denials};
pub use steps::{Step, StepChange};
pub use task::{Task, TaskInfo};
