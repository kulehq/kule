//! Kule's daemon: one process per home that owns tasks, agent runs and the
//! questions they wait on, so they outlive any window. The app and the CLI
//! talk to it over a Unix socket that only the same user can open.

mod alerts;
mod client;
pub mod editors;
pub mod extensions;
pub mod files;
pub mod layout;
pub mod outside;
mod protocol;
mod server;
mod service;
pub mod settings_profile;
pub mod shell;
pub mod shortcuts;
pub mod themes;

use std::path::PathBuf;

pub use alerts::{Alert, Alerts};
pub use client::{Client, spawn_daemon};
pub use kule_acp::Profile as AgentProfile;
pub use kule_engine::{
    BudgetLeft, BudgetScope, Group, GroupPolicy, GroupSpent, Limits, Money, PolicyChoice, Question,
    Spent, TaskColor, TaskLimits, public_ip,
};
pub use kule_view::refs::Reference;
pub use protocol::{Call, Hello, PROTOCOL, Redacted, Request, Response};
pub use server::{MAX_REQUEST, bind, dispatch, new_token, run, serve, socket_path, token_path};
pub use service::{
    Behind, Commit, DailyBudget, GroupImpact, GroupPolicyText, IsolationView, Notice, Outside,
    Overlap, Result, Review, RunEvent, Sequenced, Service, SharedBudget, TaskChange, TaskView,
    Timeline,
};

/// Kule's home: `KULE_HOME`, else `~/.kule`.
pub fn default_home() -> Result<PathBuf> {
    if let Some(home) = std::env::var_os("KULE_HOME") {
        return Ok(PathBuf::from(home));
    }
    std::env::var_os("HOME")
        .map(|h| PathBuf::from(h).join(".kule"))
        .ok_or_else(|| "HOME is not set; set KULE_HOME".to_owned())
}
