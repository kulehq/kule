//! Trying an agent before it has a profile: it runs once in the sandbox, in
//! an empty folder, opened no wider than its draft, and what it was refused
//! suggests what the profile should open. That is how Kule's own profiles
//! were found; a person adding an agent gets the same help.
//!
//! The refusals are only suggestions: the person picks from them, and the
//! profile is still checked and approved like any other. Only macOS reports
//! the files the sandbox refused; on Linux the hosts alone are suggested.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use kule_engine::{
    CacheSources, Connection, Denial, Error, Policy, Proxy, Result, Task, connections,
};
use serde::{Deserialize, Serialize};

use crate::profile::{Agents, Profile};
use crate::{Event, run};

/// How long a trial may take; the agent is stopped after that.
const TRIAL_TIME: Duration = Duration::from_secs(120);

const PROMPT: &str = "Kule is checking that you start. Reply with the single word: ready.";

/// How much of the agent's error output a trial shows.
const LOG_LINES: usize = 30;

/// Directories that hold many tools' state: a suggestion names the tool's
/// directory inside them, not the whole.
const SHARED: &[&str] = &[
    ".config",
    ".local/share",
    ".local/state",
    ".cache",
    "Library/Application Support",
    "Library/Caches",
    "Library/Preferences",
    "Library/Logs",
];

/// Files shells and REPLs write in the home directory, whoever starts them:
/// never an agent's own state.
const NOISE: &[&str] = &[
    ".zcompdump",
    ".zsh_history",
    ".bash_history",
    ".lesshst",
    ".viminfo",
    ".python_history",
    ".node_repl_history",
];

/// What a trial found.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Trial {
    /// What the agent answered, when it got that far.
    pub reply: String,
    /// Why the run ended badly, when it did.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Paths under the home directory it was refused writing to, as state
    /// to open.
    pub state: Vec<String>,
    /// HTTPS hosts it was refused, as hosts to open.
    pub hosts: Vec<String>,
    /// What in its state, the suggested state included, can make it run
    /// something the next time it starts outside Kule: to make read-only.
    #[serde(default)]
    pub protect: Vec<String>,
    /// Everything it was refused, as the sandbox and the proxy said it.
    pub refused: Vec<String>,
    /// The last lines the agent wrote to its error output.
    #[serde(default)]
    pub log: Vec<String>,
}

/// Runs `draft` once in an empty task and reads what it was refused.
pub fn trial(
    kule_home: &Path,
    agents: &Agents,
    draft: &Profile,
    person: Option<CacheSources>,
) -> Result<Trial> {
    agents
        .check_draft(kule_home, draft)
        .map_err(Error::Invalid)?;
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let folder = kule_home
        .join("trials")
        .join(format!("{}-{nanos}", draft.name));
    std::fs::create_dir_all(&folder).map_err(|e| Error::Io {
        path: folder.clone(),
        source: e,
    })?;
    let _ = std::fs::write(folder.join("README.md"), "Kule is trying an agent here.\n");
    // The person's policy, as for any task; the empty folder adds no forbids.
    let policy = Policy::load(Some(&kule_home.join("policy.cedar")), &folder);
    let outcome =
        policy.and_then(|policy| try_in(kule_home, &folder, agents, draft, policy, person));
    let _ = std::fs::remove_dir_all(&folder);
    outcome
}

fn try_in(
    kule_home: &Path,
    folder: &Path,
    agents: &Agents,
    draft: &Profile,
    policy: Policy,
    person: Option<CacheSources>,
) -> Result<Trial> {
    let home = person.as_ref().map(|p| p.home.clone());
    let task = Task::create(kule_home, folder)?;
    let profile = agents.draft(draft);
    let started = Instant::now();
    let outcome = (|| {
        let proxy = Proxy::start(&task, profile.open_hosts(policy)?)?;
        let launch = profile.launch_for(&task, &proxy, person)?;
        let (tx, rx) = futures::channel::mpsc::unbounded();
        let runtime = tokio::runtime::Runtime::new().map_err(|e| Error::Invalid(e.to_string()))?;
        let ran = runtime.block_on(async {
            tokio::time::timeout(
                TRIAL_TIME,
                run(&task, launch.command, PROMPT, launch.mode.as_deref(), tx),
            )
            .await
        });
        let events: Vec<Event> = runtime.block_on(futures::StreamExt::collect(rx));
        let _ = launch.kept.restore();
        proxy.stop();
        let reply: String = events
            .iter()
            .filter_map(|e| match e {
                Event::Message { text } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        let error = match ran {
            Ok(Ok(_)) => None,
            Ok(Err(e)) => Some(e.to_string()),
            Err(_) => Some(format!(
                "it did not answer within {} seconds",
                TRIAL_TIME.as_secs()
            )),
        };
        // The system's log takes a moment to show the last refusals.
        if cfg!(target_os = "macos") {
            std::thread::sleep(Duration::from_secs(2));
        }
        let denials = launch
            .sandbox
            .denials(started.elapsed())?
            .unwrap_or_default();
        Ok::<_, Error>((reply, error, denials))
    })();
    let refused_connections = connections(&task).unwrap_or_default();
    let log = std::fs::read_to_string(task.copy().with_file_name("agent.log")).unwrap_or_default();
    let log: Vec<String> = log
        .lines()
        .rev()
        .take(LOG_LINES)
        .map(|l| l.chars().take(300).collect())
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    let _ = task.remove();
    let _ = Task::empty_trash(kule_home);
    let (reply, error, denials) = outcome?;
    let mut trial = suggest(
        home.as_deref(),
        &denials,
        &refused_connections,
        agents,
        draft,
        kule_home,
    );
    trial.reply = reply.trim().chars().take(500).collect();
    trial.error = error;
    trial.log = log;
    if let Some(home) = &home {
        let mut probe = draft.clone();
        probe.state.extend(trial.state.iter().cloned());
        trial.protect = probe.unprotected(home);
    }
    Ok(trial)
}

/// What `denials` and `connections` suggest opening for `draft`.
pub fn suggest(
    home: Option<&Path>,
    denials: &[Denial],
    connections: &[Connection],
    agents: &Agents,
    draft: &Profile,
    kule_home: &Path,
) -> Trial {
    let mut trial = Trial::default();
    for denial in denials {
        trial.refused.push(format!(
            "{} {} ({})",
            denial.operation, denial.path, denial.process
        ));
        if !denial.operation.starts_with("file-write") {
            continue;
        }
        let Some(rel) = home.and_then(|h| Path::new(&denial.path).strip_prefix(h).ok()) else {
            continue;
        };
        let Some(state) = state_for(rel) else {
            continue;
        };
        // Only what the profile could open at all.
        let mut probe = draft.clone();
        probe.state = vec![state.clone()];
        probe.login.clear();
        probe.protected.clear();
        if agents.check_draft(kule_home, &probe).is_ok() && !trial.state.contains(&state) {
            trial.state.push(state);
        }
    }
    for c in connections.iter().filter(|c| c.outcome == "refused") {
        trial.refused.push(format!(
            "connect {}:{} ({})",
            c.host,
            c.port,
            c.rules.join(", ")
        ));
        let domain =
            c.host.contains('.') && !c.host.chars().all(|ch| ch.is_ascii_digit() || ch == '.');
        if c.port == 443 && domain && !trial.hosts.contains(&c.host) {
            let mut probe = draft.clone();
            probe.hosts = vec![c.host.clone()];
            if agents.check_draft(kule_home, &probe).is_ok() {
                trial.hosts.push(c.host.clone());
            }
        }
    }
    trial.refused.dedup();
    trial
}

/// The state directory a refused write at `rel` (relative to the home
/// directory) points to: the tool's own directory, inside a shared one when
/// it is in one.
fn state_for(rel: &Path) -> Option<String> {
    let text = rel.to_str()?;
    if NOISE.iter().any(|n| text.starts_with(n)) {
        return None;
    }
    let parts: Vec<&str> = text.split('/').collect();
    for shared in SHARED {
        let depth = shared.split('/').count();
        if text.starts_with(&format!("{shared}/")) && parts.len() > depth {
            return Some(parts[..=depth].join("/"));
        }
    }
    let first = parts.first()?;
    // A file next to its directory, written through a temporary name
    // (`.tool.json.tmp.123`), is the file.
    let first = match first.find(".tmp") {
        Some(at) if at > 0 => &first[..at],
        _ => first,
    };
    Some(first.trim_end_matches(".lock").to_owned())
}

/// Where the person's home is, for a trial run by this process.
pub fn person_home() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use kule_engine::Verdict;

    use super::*;

    fn denial(operation: &str, path: &str) -> Denial {
        Denial {
            process: "node".into(),
            operation: operation.into(),
            path: path.into(),
            saved: None,
        }
    }

    fn refused(host: &str, port: u16) -> Connection {
        Connection {
            time_unix: 0,
            host: host.into(),
            port,
            verdict: Verdict::Ask,
            rules: vec!["no-rule-lets-it-pass".into()],
            outcome: "refused".into(),
            answer: None,
        }
    }

    #[test]
    fn refusals_suggest_the_agents_own_directories_and_hosts() {
        let agents = Agents::built_in();
        let draft = Profile::bare(
            "opencode",
            vec!["opencode".into(), "acp".into()],
            BTreeMap::new(),
        );
        let home = Path::new("/Users/me");
        let denials = [
            denial(
                "file-write-create",
                "/Users/me/.local/share/opencode/log/1.txt",
            ),
            denial(
                "file-write-create",
                "/Users/me/.local/share/opencode/auth.json",
            ),
            denial("file-write-data", "/Users/me/.config/opencode/config.json"),
            denial("file-write-create", "/Users/me/.opencode.json.tmp.41.ab"),
            denial("file-write-create", "/Users/me/.zcompdump-host-5.9"),
            denial("file-write-create", "/Users/me/.claude/x"),
            denial("file-write-create", "/Users/me/.ssh/known_hosts"),
            denial("file-read-data", "/Users/me/.config/gh/hosts.yml"),
            denial("file-write-create", "/private/var/folders/x"),
        ];
        let connections = [
            refused("openrouter.ai", 443),
            refused("openrouter.ai", 443),
            refused("127.0.0.1", 443),
            refused("example.com", 8080),
        ];
        let trial = suggest(
            Some(home),
            &denials,
            &connections,
            &agents,
            &draft,
            Path::new("/k"),
        );
        assert_eq!(
            trial.state,
            [
                ".local/share/opencode",
                ".config/opencode",
                ".opencode.json"
            ]
        );
        assert_eq!(trial.hosts, ["openrouter.ai"]);
        assert!(trial.refused.iter().any(|r| r.contains(".ssh/known_hosts")));
    }
}
