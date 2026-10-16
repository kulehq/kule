//! Agent profiles: how to start an agent over ACP, and the narrow extra room
//! each one needs in the sandbox.
//!
//! What a profile opens was found by running the agent in the sandbox and
//! reading what it was refused (`sandbox.jsonl`): its own state and login,
//! nothing of the workspace or of other tools.

use std::path::Path;
use std::process::Command;

use kule_engine::{CacheSources, Kept, Network, Policy, Proxy, Result, Sandbox, Task};

#[derive(Debug, Clone, Copy)]
pub struct Profile {
    pub name: &'static str,
    /// Starts the agent speaking ACP on stdio.
    pub command: &'static [&'static str],
    /// Paths under the home directory the agent writes its own state to.
    pub state: &'static [&'static str],
    /// Paths in that state that configure what runs outside the sandbox
    /// later (hooks, MCP servers, sandbox settings): readable, never
    /// writable.
    pub protected: &'static [&'static str],
    /// Its login inside that state, hidden from every other agent and from
    /// commands Kule runs itself.
    pub login: &'static [&'static str],
    /// Default-hidden paths under the home directory the agent needs to read
    /// (macOS only: the keychain holding its login).
    pub reveal_on_macos: &'static [&'static str],
    /// The session mode to start in, when the agent offers it: one that
    /// leaves sandboxing to Kule, for an agent whose own sandbox cannot
    /// start inside Kule's.
    pub mode: Option<&'static str>,
    /// Environment variables the agent is started with.
    pub env: &'static [(&'static str, &'static str)],
    /// The person's environment variables the agent may see: its own
    /// credentials, when it signs in by key. Others stay out.
    pub pass_env: &'static [&'static str],
    /// HTTPS hosts the agent needs for its login and model, beyond what the
    /// policy already lets through. Open only while this agent runs.
    pub hosts: &'static [&'static str],
}

pub const PROFILES: &[Profile] = &[
    Profile {
        name: "claude-code",
        mode: None,
        command: &["npx", "-y", "@agentclientprotocol/claude-agent-acp@0.92.0"],
        state: &[
            ".claude",
            ".claude.json",
            ".local/state/claude",
            "Library/Caches/claude-cli-nodejs",
        ],
        login: &[".claude/.credentials.json"],
        protected: &[
            ".claude/settings.json",
            ".claude/settings.local.json",
            ".claude/hooks",
            ".claude.json",
        ],
        reveal_on_macos: &["Library/Keychains"],
        env: &[],
        pass_env: &[
            "ANTHROPIC_API_KEY",
            "ANTHROPIC_AUTH_TOKEN",
            "CLAUDE_CODE_OAUTH_TOKEN",
        ],
        hosts: &[],
    },
    Profile {
        name: "gemini",
        mode: None,
        command: &["gemini", "--acp"],
        state: &[".gemini"],
        login: &[
            ".gemini/gemini-credentials.json",
            ".gemini/oauth_creds.json",
        ],
        protected: &[".gemini/settings.json"],
        reveal_on_macos: &[],
        // The keychain only works for Gemini when it can write a probe item
        // there, which the sandbox refuses; its own encrypted file under
        // ~/.gemini needs no more than the state above. Save the API key
        // once with `GEMINI_FORCE_FILE_STORAGE=true gemini` and `/auth`.
        env: &[("GEMINI_FORCE_FILE_STORAGE", "true")],
        pass_env: &["GEMINI_API_KEY", "GOOGLE_API_KEY"],
        // With an API key it talks to the public API, which the policy lets
        // through. Google no longer serves its Code Assist sign-in to
        // clients like this one.
        hosts: &[],
    },
    Profile {
        name: "codex",
        mode: Some("agent-full-access"),
        // Codex runs each command in its own Seatbelt or Landlock sandbox,
        // which cannot be set up inside Kule's; Kule's sandbox stands in.
        command: &[
            "npx",
            "-y",
            "@agentclientprotocol/codex-acp@2.2.2",
            "-c",
            "sandbox_mode=\"danger-full-access\"",
        ],
        state: &[".codex"],
        login: &[".codex/auth.json"],
        protected: &[".codex/config.toml"],
        reveal_on_macos: &[],
        env: &[],
        pass_env: &["OPENAI_API_KEY", "CODEX_HOME"],
        // A ChatGPT sign-in refreshes its token at auth.openai.com and
        // reaches the model through chatgpt.com, not the public API.
        hosts: &["auth.openai.com", "chatgpt.com"],
    },
];

pub fn profile(name: &str) -> Option<&'static Profile> {
    PROFILES.iter().find(|p| p.name == name)
}

impl Profile {
    /// Opens the agent's own state and login in `sandbox`. Paths that do not
    /// exist are skipped: the agent has never run here.
    /// Other agents' state, their logins and what the person said to them,
    /// is hidden. `home` is the person's home directory.
    pub fn apply(&self, sandbox: &mut Sandbox, home: &Path) -> Result<()> {
        for other in PROFILES.iter().filter(|p| p.name != self.name) {
            for state in other.state {
                sandbox.hide(&home.join(state))?;
            }
        }
        for state in self.state {
            let path = home.join(state);
            if path.exists() {
                sandbox.allow_write(&path)?;
            }
        }
        for path in self.login {
            sandbox.reveal(&home.join(path))?;
        }
        for path in self.protected {
            sandbox.protect(&home.join(path))?;
        }
        if cfg!(target_os = "macos") {
            for path in self.reveal_on_macos {
                sandbox.reveal(&home.join(path))?;
            }
        }
        for name in self.pass_env {
            sandbox.pass_env(name);
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

    /// Prepares the agent for `task`: the task's sandbox with this agent's
    /// state opened, its network through `proxy` (started with
    /// [`Profile::open_hosts`]).
    pub fn launch(&self, task: &Task, proxy: &Proxy) -> Result<Launch> {
        self.launch_for(task, proxy, CacheSources::from_env())
    }

    /// [`Profile::launch`] for the person whose home and caches `person`
    /// names.
    pub fn launch_for(
        &self,
        task: &Task,
        proxy: &Proxy,
        person: Option<CacheSources>,
    ) -> Result<Launch> {
        let home = person.as_ref().map(|p| p.home.clone());
        let mut sandbox = Sandbox::for_task_with(task, person)?;
        if let Some(home) = home {
            self.apply(&mut sandbox, &home)?;
        }
        sandbox.set_network(Network::Proxy(proxy.port()));
        let kept = sandbox.keep_protected()?;
        let mut command = sandbox.command(&self.program().to_string_lossy(), &self.args())?;
        command.envs(self.env.iter().copied());
        // Started from the task's tmp, not the copy: `npx` would otherwise
        // take the repository's node_modules or .npmrc for the agent's own.
        // The agent still works in the copy, which its session names.
        command.current_dir(task.tmp()?);
        Ok(Launch {
            command,
            sandbox,
            kept,
            mode: self.mode,
        })
    }
}

/// An agent ready to start, and the sandbox it runs in, which reports what
/// it refused once the agent is done.
pub struct Launch {
    pub command: Command,
    pub sandbox: Sandbox,
    /// The agent's settings, to put back once it is gone where the sandbox
    /// could not keep them read-only.
    pub kept: Kept,
    /// The session mode to ask for (see [`Profile::mode`]).
    pub mode: Option<&'static str>,
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;

    use tempfile::TempDir;

    use super::*;

    #[test]
    fn every_profile_names_hosts_the_policy_accepts() {
        for profile in PROFILES {
            assert!(
                profile.open_hosts(Policy::builtin()).is_ok(),
                "{}",
                profile.name
            );
        }
    }

    #[test]
    fn an_agent_starts_with_its_profiles_environment() {
        let (home, ws) = (TempDir::new().unwrap(), TempDir::new().unwrap());
        let task = Task::create(home.path(), ws.path()).unwrap();
        let proxy = Proxy::start(&task, Policy::builtin()).unwrap();
        let launch = profile("gemini")
            .unwrap()
            .launch_for(&task, &proxy, None)
            .unwrap();
        let env: Vec<_> = launch.command.get_envs().collect();
        assert!(env.contains(&(
            OsStr::new("GEMINI_FORCE_FILE_STORAGE"),
            Some(OsStr::new("true"))
        )));
        // Not from the copy, where the repository could stand in for npx's
        // package.
        assert_eq!(
            launch.command.get_current_dir(),
            Some(task.tmp().unwrap().as_path())
        );
    }

    /// One agent's login and transcripts are none of another's business.
    #[test]
    fn an_agent_sees_its_own_login_and_no_other_agents_state() {
        let (home, ws, person) = (
            TempDir::new().unwrap(),
            TempDir::new().unwrap(),
            TempDir::new().unwrap(),
        );
        let task = Task::create(home.path(), ws.path()).unwrap();
        let person = std::fs::canonicalize(person.path()).unwrap();
        for (dir, file) in [
            (".claude", ".credentials.json"),
            (".claude/projects", "session.jsonl"),
            (".codex", "auth.json"),
            (".gemini", "gemini-credentials.json"),
        ] {
            std::fs::create_dir_all(person.join(dir)).unwrap();
            std::fs::write(person.join(dir).join(file), "secret\n").unwrap();
        }
        let read = |profile: &Profile, path: &str| {
            let mut sandbox = Sandbox::for_task_with(
                &task,
                Some(CacheSources {
                    home: person.clone(),
                    cargo_home: person.join(".cargo"),
                }),
            )
            .unwrap();
            profile.apply(&mut sandbox, &person).unwrap();
            let target = person.join(path);
            sandbox
                .command("cat", &[target.display().to_string()])
                .unwrap()
                .output()
                .unwrap()
                .status
                .success()
        };
        let (claude, codex) = (profile("claude-code").unwrap(), profile("codex").unwrap());
        assert!(read(claude, ".claude/.credentials.json"));
        assert!(!read(claude, ".codex/auth.json"));
        assert!(!read(claude, ".gemini/gemini-credentials.json"));
        assert!(read(codex, ".codex/auth.json"));
        assert!(!read(codex, ".claude/.credentials.json"));
        assert!(!read(codex, ".claude/projects/session.jsonl"));
    }
}
