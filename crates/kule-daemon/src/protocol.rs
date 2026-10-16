//! The daemon's wire format: one JSON object per line, each way. A client
//! sends a [`Request`] and reads one [`Response`]; after
//! [`Call::Subscribe`] the connection carries [`Notice`](crate::Notice)s
//! instead, one per line, until either side closes it.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Changes whenever a call, a result or a notice changes shape; a client
/// and a daemon that disagree on it cannot talk.
pub const PROTOCOL: u32 = 2;

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
    Commit {
        task: String,
        approve: bool,
        include_ignored: bool,
        /// The digest of the review the person saw; refused if it changed.
        #[serde(default)]
        reviewed: Option<String>,
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
    },
    SetApprovalTimeout {
        task: String,
        minutes: u64,
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
