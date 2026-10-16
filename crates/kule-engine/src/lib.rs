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
//! M0 has no sandbox and no policy: a command run in the copy can still write
//! outside it. Those arrive in M1.

#[cfg(not(unix))]
compile_error!("Kule M0 supports macOS and Linux only");

mod checkpoint;
mod effects;
mod error;
mod fsops;
mod git;
mod manifest;
mod task;

pub use checkpoint::Checkpoint;
pub use effects::{Change, Effect, EffectSet};
pub use error::{Error, Result};
pub use git::{GitState, GitSummary, RefChange};
pub use manifest::Kind;
pub use task::{Task, TaskInfo};
