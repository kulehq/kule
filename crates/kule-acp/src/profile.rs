//! Agent profiles: how to start an agent over ACP, and the narrow extra room
//! each one needs in the sandbox.
//!
//! What a profile opens was found by running the agent in the sandbox and
//! reading what it was refused (`sandbox.jsonl`): its own state and login,
//! nothing of the workspace or of other tools.

use std::path::{Path, PathBuf};

use kule_engine::{Policy, Result, Sandbox};

#[derive(Debug, Clone, Copy)]
pub struct Profile {
    pub name: &'static str,
    /// Starts the agent speaking ACP on stdio.
    pub command: &'static [&'static str],
    /// Paths under the home directory the agent writes its own state to.
    pub state: &'static [&'static str],
    /// Default-hidden paths under the home directory the agent needs to read
    /// (macOS only: the keychain holding its login).
    pub reveal_on_macos: &'static [&'static str],
    /// HTTPS hosts the agent needs for its login and model, beyond what the
    /// policy already lets through. Open only while this agent runs.
    pub hosts: &'static [&'static str],
}

pub const PROFILES: &[Profile] = &[
    Profile {
        name: "claude-code",
        command: &["npx", "-y", "@agentclientprotocol/claude-agent-acp@0.92.0"],
        state: &[".claude", ".claude.json", ".local/state/claude"],
        reveal_on_macos: &["Library/Keychains"],
        hosts: &[],
    },
    Profile {
        name: "gemini",
        command: &["gemini", "--acp"],
        state: &[".gemini"],
        reveal_on_macos: &[],
        // A Google sign-in refreshes its token at oauth2 and reaches the
        // model through Code Assist (cloudcode-pa), not the public API.
        hosts: &["oauth2.googleapis.com", "cloudcode-pa.googleapis.com"],
    },
];

pub fn profile(name: &str) -> Option<&'static Profile> {
    PROFILES.iter().find(|p| p.name == name)
}

impl Profile {
    /// Opens the agent's own state and login in `sandbox`. Paths that do not
    /// exist are skipped: the agent has never run here.
    pub fn apply(&self, sandbox: &mut Sandbox) -> Result<()> {
        let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
            return Ok(());
        };
        for state in self.state {
            let path = home.join(state);
            if path.exists() {
                sandbox.allow_write(&path)?;
            }
        }
        if cfg!(target_os = "macos") {
            for path in self.reveal_on_macos {
                sandbox.reveal(&home.join(path))?;
            }
        }
        Ok(())
    }

    /// `policy` with this agent's hosts let through, under `profile-<name>`.
    pub fn open_hosts(&self, policy: Policy) -> Result<Policy> {
        policy.permit_hosts(&format!("profile-{}", self.name), self.hosts)
    }

    pub fn program(&self) -> &Path {
        Path::new(self.command[0])
    }

    pub fn args(&self) -> Vec<String> {
        self.command[1..].iter().map(|s| s.to_string()).collect()
    }
}
