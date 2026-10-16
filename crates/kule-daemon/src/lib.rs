//! Kule's daemon: one process per home that owns tasks, agent runs and the
//! questions they wait on, so they outlive any window. The app and the CLI
//! talk to it over a Unix socket that only the same user can open.

mod client;
mod protocol;
mod server;
mod service;

use std::path::PathBuf;

pub use client::{Client, spawn_daemon};
pub use kule_engine::Question;
pub use protocol::{Call, Hello, PROTOCOL, Request, Response};
pub use server::{MAX_REQUEST, bind, dispatch, new_token, run, serve, socket_path, token_path};
pub use service::{Commit, Notice, Result, Review, RunEvent, Service, TaskView, Timeline};

/// Kule's home: `KULE_HOME`, else `~/.kule`.
pub fn default_home() -> Result<PathBuf> {
    if let Some(home) = std::env::var_os("KULE_HOME") {
        return Ok(PathBuf::from(home));
    }
    std::env::var_os("HOME")
        .map(|h| PathBuf::from(h).join(".kule"))
        .ok_or_else(|| "HOME is not set; set KULE_HOME".to_owned())
}
