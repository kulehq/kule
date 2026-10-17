//! The daemon's wire format: one JSON object per line, each way. A client
//! sends a [`Request`] and reads one [`Response`]; after
//! [`Call::Subscribe`] the connection carries [`Notice`](crate::Notice)s
//! instead, one per line, until either side closes it.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Changes whenever a call, a result or a notice changes shape; a client
/// and a daemon that disagree on it cannot talk.
pub const PROTOCOL: u32 = 10;

/// What a daemon says about itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hello {
    pub protocol: u32,
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Request {
    pub id: u64,
    /// The daemon's token (see `token_path`); requests without it are
    /// refused.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
    #[serde(flatten)]
    pub call: Call,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "method", content = "params", rename_all = "snake_case")]
pub enum Call {
    /// The daemon's [`Hello`]; every client asks first.
    Hello,
    /// Stops the daemon, unless an agent is at work or a question waits.
    Shutdown,
    Agents,
    Tasks,
    CreateTask {
        workspace: PathBuf,
    },
    Review {
        task: String,
        include_ignored: bool,
    },
    /// What a changed picture was and is (see `Task::preview`).
    Preview {
        task: String,
        path: String,
    },
    Commit {
        task: String,
        approve: bool,
        include_ignored: bool,
        /// The digest of the review the person saw; refused if it changed.
        #[serde(default)]
        reviewed: Option<String>,
        /// With an approval: files whose question is not to be asked again
        /// on this folder, once the commit is in.
        #[serde(default)]
        remember: Vec<String>,
    },
    Rollback {
        task: String,
    },
    Undo {
        task: String,
    },
    Timeline {
        task: String,
    },
    RemoveTask {
        task: String,
    },
    CopyOf {
        task: String,
    },
    /// The copy, for the person's terminal; refused while the copy's git
    /// config or hooks hold changes (see `Task::terminal_dir`).
    TerminalDir {
        task: String,
    },
    StartRun {
        task: String,
        agent: String,
        prompt: String,
    },
    /// The latest events of the task's runs.
    Stream {
        task: String,
    },
    Questions,
    Answer {
        id: u64,
        approve: bool,
        /// With an approval: let this host and port through on the task's
        /// folder from now on.
        #[serde(default)]
        remember: bool,
    },
    SetApprovalTimeout {
        task: String,
        minutes: u64,
    },
    /// What the task's runs may cost together; `None` for no limit.
    SetBudget {
        task: String,
        budget: Option<kule_engine::Money>,
    },
    /// Every agent profile, the ones that cannot run included.
    AgentList,
    /// The ACP registry, fresh or as last read.
    AgentRegistry,
    /// Writes one of the person's profiles; it then waits for approval.
    SaveAgent {
        profile: kule_acp::Profile,
    },
    RemoveAgent {
        name: String,
    },
    /// Approves a profile, only if it still has the digest the person saw.
    ApproveAgent {
        name: String,
        digest: String,
    },
    RevokeAgent {
        name: String,
    },
    /// Runs a draft profile once in an empty folder; what it was refused.
    TryAgent {
        profile: kule_acp::Profile,
    },
    /// The approvals a person told Kule to remember.
    Rules,
    ForgetRule {
        id: u64,
    },
    /// What an allowance would have let through so far, before it is kept.
    RuleImpact {
        allowance: kule_engine::Allowance,
        #[serde(default)]
        workspace: Option<PathBuf>,
    },
    /// Takes the task's home changes at `paths` to the person's home.
    ApplyHome {
        task: String,
        paths: Vec<String>,
    },
    DiscardHome {
        task: String,
        paths: Vec<String>,
    },
    /// The person's secrets, without their values.
    Secrets,
    /// Keeps a secret for `hosts`, on `workspace` or every folder.
    AddSecret {
        name: String,
        value: String,
        hosts: Vec<String>,
        #[serde(default)]
        workspace: Option<PathBuf>,
    },
    RemoveSecret {
        name: String,
        #[serde(default)]
        workspace: Option<PathBuf>,
    },
    /// Turns the connection into a stream of notices.
    Subscribe,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Response {
    pub id: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}
