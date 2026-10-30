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

mod approvals;
mod beside;
#[cfg(target_os = "linux")]
mod bind_filter;

/// Whether the filter that holds a command's servers to loopback, and
/// refuses the calls that reach past it, can be used here, where a command
/// has no network of its own (Linux).
#[cfg(target_os = "linux")]
pub fn bind_filter_available() -> bool {
    bind_filter::available()
}
mod caches;
mod checkpoint;
mod clienthello;
mod clock;
mod committed;
mod effects;
mod error;
mod fsops;
mod git;
pub mod grants;
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
pub mod osfs;
mod outline;
mod policy;
mod ports;
mod preview;
mod protect;
mod proxy_core;
mod proxy_pipe;
pub mod proxy_program;
#[cfg(target_os = "macos")]
mod refusal_log;
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
pub mod tools;
mod usage;
#[cfg(windows)]
pub mod windows;

pub use approvals::{
    AfterApproval, Answer, Approvals, DEFAULT_TIMEOUT, Inside, MAX_TIMEOUT, MAX_WAITING,
    MIN_TIMEOUT, Question, Settled,
};
pub use beside::{
    BESIDE_FILE, Beside, add_beside, beside_unopened, besides, remove_beside, suggest_beside,
};
pub use caches::{
    PREPARING as CACHES_PREPARING, STILL_PREPARING as CACHES_STILL_PREPARING,
    Sources as CacheSources,
};
pub use checkpoint::{Checkpoint, State as CheckpointState};
pub use committed::{
    Finding as CommittedFinding, Folder as CommittedFolder, Look as CommittedLook,
    committed_accepted, forget_committed,
};
pub use effects::{Change, Effect, EffectSet, Move};
pub use error::{Error, Result};
pub use fsops::{carry, entries_in, open_beneath, remove_beneath};
pub use git::{Carried, CarriedEntry, GitState, GitSummary, RefChange};
pub use grants::{Access, Bounds, Grant, Refused, Source};
pub use groups::{
    GROUP_POLICY_DIR, GROUPS_FILE, Group, GroupPolicy, MAX_GROUP_POLICY, MAX_GROUPS, create_group,
    delete_group, group_policy_text, groups, set_group_budget, set_group_limits, set_group_order,
    set_group_policy, update_group,
};
pub use history::{Entry as HistoryEntry, Event as HistoryEvent};
pub use home::{HomeApplied, HomeChange, home_digest};
pub use isolation::{Isolation, isolation, isolation_gaps, set_isolation};
pub use kule_entities as entities;
pub use labels::{
    MAX_LABEL, ORDER_FILE, TaskColor, arranged, arranged_told, set_task_order, task_order,
};
pub use limits::{Limits, TaskLimits, limits, set_limits};
pub use manifest::Kind;
pub use meet::{Meeting, Touched, changed_entities, meet, touched};
pub use network::{Connection, Proxy, connections, public_ip};
pub use outline::FileOutline;
pub use policy::{
    CAREFUL_POLICY, DEFAULT_POLICY, Held, Judgement, Policy, PolicyChoice, REPO_POLICY, Ruling,
    USER_POLICY, Verdict, choose_policy, dotted, file_kind, policy_choice, policy_text, reason,
};
pub use ports::{Listener, listening_in};
pub use preview::{Picture, Preview, previewable};
pub use protect::{Kept, PutBack};
pub use remembered::{
    Allowance, Impact, REMEMBERABLE, REMEMBERED_FILE, Remembered, UNFAMILIAR_KINDS, allowances,
    forget, impact, remember,
};
#[cfg(target_os = "macos")]
pub use sandbox::ProcessFinder;
pub use sandbox::{
    Denial, Ended, HIDDEN_IN_HOME, INCOMPLETE, Io, Network, PASSED_ENV, PASSED_ENV_WINDOWS,
    PERSONAL_IN_HOME, Process, Refusals, Sandbox, Start, UNREPORTED, check_hints,
    checked_read_hint, denials, isolates_network, own_processes, protects_in_place, record_denials,
    traced_denials,
};
#[cfg(windows)]
pub use sandbox::{Terminal, TerminalInput, TerminalStart};

/// Why this computer gives a task's commands no sandbox at all, one
/// sentence each, or nothing: on Windows, a build before the one Kule
/// measured, a session that is no desktop session, or a process container
/// that cannot keep a path from a command (see `windows::preconditions`).
/// Unlike [`isolation_gaps`], nothing here is accepted: commands are
/// refused until it is closed.
pub fn sandbox_refusals() -> Vec<String> {
    #[cfg(windows)]
    return windows::preconditions::check().err().unwrap_or_default();
    #[cfg(not(windows))]
    Vec::new()
}
pub use sealing::KeyHome;
pub use secrets::{
    AgentKey, SECRETS_FILE, SecretInfo, Vault, add_secret, remove_secret, secrets, secrets_key_home,
};
pub use steps::{Step, StepChange};
pub use sync::{Behind, Synced};
pub use task::{
    AgentAtWork, AgentLogMark, MEASURING_CACHES, OpenTerminal, OpenedOutside, Task, TaskInfo,
    Unreadable,
};
pub use textdiff::{DiffSkip, Hunk, Line, LineKind, TextDiff};
pub use tools::{AdminTools, Tool, add_tool, admin_file, admin_tools, remove_tool, tools};
pub use usage::{
    BudgetLeft, BudgetScope, GroupSpent, Money, Reserved, RunUsage, Spent, daily_budget,
    daily_left, groups_spent, set_daily_budget, spent_today,
};
