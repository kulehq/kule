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
mod beside;
#[cfg(target_os = "linux")]
mod bind_filter;
mod caches;
mod checkpoint;
mod clienthello;
mod clock;
mod effects;
mod error;
mod fsops;
mod git;
mod groups;
mod history;
mod home;
mod ignore;
mod isolation;
mod labels;
mod limits;
mod loopback;
mod manifest;
mod meet;
#[cfg(target_os = "linux")]
mod namespace;
mod network;
mod outline;
mod policy;
mod ports;
mod preview;
mod protect;
pub mod remembered;
pub mod resources;
mod sandbox;
mod sealing;
mod secrets;
#[cfg(target_os = "macos")]
pub mod shield;
mod snapshot;
mod step_store;
mod steps;
mod sync;
mod task;
mod textdiff;
mod tls;
mod usage;

pub use approvals::{
    Answer, Approvals, DEFAULT_TIMEOUT, MAX_TIMEOUT, MAX_WAITING, MIN_TIMEOUT, Question,
};
pub use beside::{
    BESIDE_FILE, Beside, add_beside, beside_unopened, besides, remove_beside, suggest_beside,
};
pub use caches::Sources as CacheSources;
pub use checkpoint::{Checkpoint, State as CheckpointState};
pub use effects::{Change, Effect, EffectSet, Move};
pub use error::{Error, Result};
pub use fsops::{carry, remove_beneath};
pub use git::{Carried, CarriedEntry, GitState, GitSummary, RefChange};
pub use groups::{
    GROUP_POLICY_DIR, GROUPS_FILE, Group, GroupPolicy, MAX_GROUP_POLICY, MAX_GROUPS, create_group,
    delete_group, group_policy_text, groups, set_group_budget, set_group_limits, set_group_order,
    set_group_policy, update_group,
};
pub use history::{Entry as HistoryEntry, Event as HistoryEvent};
pub use home::{HomeApplied, HomeChange};
pub use isolation::{Isolation, isolation, isolation_gaps, set_isolation};
pub use kule_entities as entities;
pub use labels::{MAX_LABEL, ORDER_FILE, TaskColor, arranged, set_task_order, task_order};
pub use limits::{Limits, TaskLimits, limits, set_limits};
pub use manifest::Kind;
pub use meet::{Meeting, Touched, changed_entities, meet, touched};
pub use network::{Connection, Proxy, connections, public_ip};
pub use outline::{FileOutline, open_beneath};
pub use policy::{
    CAREFUL_POLICY, DEFAULT_POLICY, Judgement, Policy, PolicyChoice, REPO_POLICY, Ruling,
    USER_POLICY, Verdict, choose_policy, policy_choice,
};
pub use ports::{Listener, listening_in};
pub use preview::{Picture, Preview, previewable};
pub use protect::{Kept, PutBack};
pub use remembered::{
    Allowance, Impact, REMEMBERABLE, REMEMBERED_FILE, Remembered, forget, impact, remember,
};
#[cfg(target_os = "macos")]
pub use sandbox::ProcessFinder;
pub use sandbox::{
    Denial, Ended, HIDDEN_IN_HOME, Network, PASSED_ENV, PERSONAL_IN_HOME, Sandbox, UNREPORTED,
    denials, isolates_network, own_processes, protects_in_place, record_denials,
};
pub use sealing::KeyHome;
pub use secrets::{
    AgentKey, SECRETS_FILE, SecretInfo, Vault, add_secret, remove_secret, secrets, secrets_key_home,
};
pub use steps::{Step, StepChange};
pub use sync::{Behind, Synced};
pub use task::{AgentAtWork, OpenTerminal, OpenedOutside, Task, TaskInfo};
pub use textdiff::{DiffSkip, Hunk, Line, LineKind, TextDiff};
pub use usage::{
    BudgetLeft, BudgetScope, GroupSpent, Money, Reserved, RunUsage, Spent, daily_budget,
    daily_left, groups_spent, set_daily_budget, spent_today,
};
