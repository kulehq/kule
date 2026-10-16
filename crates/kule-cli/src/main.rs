use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::time::Instant;

use anyhow::{Context, Result, anyhow};
use clap::{Parser, Subcommand};
use kule_engine::{
    Change, Connection, DEFAULT_POLICY, Denial, Effect, EffectSet, Error, GitSummary, Kind,
    Network, Policy, Proxy, RefChange, Ruling, Sandbox, Task, Verdict, connections, record_denials,
};

/// Kule engine: run commands in a sandbox on a copy-on-write copy of a
/// workspace, then commit or roll back what they did.
#[derive(Parser)]
#[command(name = "kule", version)]
struct Cli {
    /// Where Kule keeps tasks [default: ~/.kule]
    #[arg(long, env = "KULE_HOME", global = true)]
    home: Option<PathBuf>,
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Create a task: a copy-on-write copy of WORKSPACE.
    New { workspace: PathBuf },
    /// List tasks.
    List,
    /// Open your shell in the task's copy, outside the sandbox. What you
    /// change there is reviewed and committed like the agent's work.
    Shell { task: String },
    /// Print the path of a task's copy.
    Path { task: String },
    /// Run a command inside the task's copy, in the sandbox, then show what
    /// it changed. The sandbox lets it write only to the copy, the task's
    /// TMPDIR and package caches, and hides credentials (~/.ssh, ~/.aws, …).
    /// Its network goes through the task's proxy, which lets through what
    /// the policy allows and logs every connection.
    Exec {
        task: String,
        /// Also let the command write under PATH (repeatable).
        #[arg(long = "allow-write", value_name = "PATH")]
        allow_write: Vec<PathBuf>,
        /// Also hide PATH from the command (repeatable).
        #[arg(long, value_name = "PATH")]
        hide: Vec<PathBuf>,
        /// Let the command read PATH although it is hidden by default, such
        /// as ~/Library/Keychains for an agent's own login (repeatable).
        #[arg(long, value_name = "PATH")]
        reveal: Vec<PathBuf>,
        /// Pass environment variable NAME on to the command (repeatable). Only
        /// what tools need to find themselves is passed otherwise; tokens
        /// and agent sockets in the environment stay out.
        #[arg(long = "pass-env", value_name = "NAME")]
        pass_env: Vec<String>,
        /// Shut TCP networking for the command, localhost included.
        #[arg(long, conflicts_with = "open_network")]
        no_network: bool,
        /// Let the command connect anywhere, bypassing the proxy and policy.
        #[arg(long)]
        open_network: bool,
        /// Run without the sandbox: the command can write anywhere you can,
        /// and reach any host; the sandbox's options mean nothing then.
        #[arg(long, conflicts_with_all = [
            "allow_write", "hide", "reveal", "pass_env", "no_network", "open_network",
        ])]
        no_sandbox: bool,
        #[arg(last = true, required = true)]
        command: Vec<String>,
    },
    /// Give a coding agent a prompt: it runs over ACP inside the sandbox,
    /// with its network through the task's proxy, and every tool call it
    /// makes becomes a step. Nothing reaches the workspace until commit.
    Run {
        task: String,
        /// claude-code, codex or gemini
        #[arg(long, default_value = "claude-code")]
        agent: String,
        prompt: String,
    },
    /// List the steps of the task's uncommitted work: what each tool call changed.
    Steps { task: String },
    /// Show the task's uncommitted changes and what the policy says about them.
    Status {
        task: String,
        #[arg(long)]
        json: bool,
    },
    /// Apply the task's changes to the workspace, keeping a checkpoint.
    /// The policy decides: changes that wait for a person need --approve,
    /// denied ones never pass.
    Commit {
        task: String,
        /// Approve the changes the policy puts to a person.
        #[arg(long)]
        approve: bool,
        /// Also take the paths the workspace's .gitignore excludes, which
        /// otherwise stay in the copy. The policy judges them too.
        #[arg(long)]
        include_ignored: bool,
    },
    /// Print the built-in policy. Save it as <KULE_HOME>/policy.cedar to
    /// replace it; a repository adds forbid rules in .kule/policy.cedar.
    Policy,
    /// Throw the task's uncommitted changes away.
    Rollback { task: String },
    /// Revert the task's newest commit in the workspace and the copy.
    Undo { task: String },
    /// Show the task's signed history (every commit and undo), checked
    /// entry by entry; fails if anything in it was changed.
    History { task: String },
    /// Delete a task and its copy. The workspace is not touched.
    Rm { task: String },
    /// Run the Kule daemon in the foreground: it owns agent runs and the
    /// questions they wait on, for the app. The app starts it when needed.
    Daemon {
        /// Exit after this many seconds with no client, no agent at work
        /// and no question waiting.
        #[arg(long, default_value_t = 600)]
        idle_secs: u64,
    },
}

fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(e) => {
            eprintln!("kule: {e:#}");
            if let Some(Error::Conflict(_)) = e.downcast_ref::<Error>() {
                eprintln!("hint: roll back, or rerun the task on the current workspace");
            }
            ExitCode::from(2)
        }
    }
}

fn run() -> Result<ExitCode> {
    let cli = Cli::parse();
    let home = match cli.home {
        Some(home) => home,
        None => std::env::var_os("HOME")
            .map(|h| PathBuf::from(h).join(".kule"))
            .ok_or_else(|| anyhow!("HOME is not set; pass --home or KULE_HOME"))?,
    };
    match cli.command {
        Cmd::New { workspace } => {
            let task = Task::create(&home, &workspace)?;
            println!("task {} · copy at {}", task.id(), task.copy().display());
        }
        Cmd::List => {
            for info in Task::list(&home)? {
                println!("{}  {}", info.id, info.workspace.display());
            }
        }
        Cmd::Shell { task } => {
            let task = Task::open(&home, &task)?;
            let dir = task.terminal_dir()?;
            let shell = std::env::var_os("SHELL").unwrap_or_else(|| "/bin/sh".into());
            eprintln!(
                "kule: in task {}'s copy, outside the sandbox; exit to come back",
                task.id()
            );
            let status = Command::new(&shell)
                .current_dir(dir)
                .env("KULE_TASK", task.id())
                .status()
                .with_context(|| format!("could not start {}", shell.to_string_lossy()))?;
            print_set(&task.effects()?);
            return Ok(match status.code() {
                Some(0) => ExitCode::SUCCESS,
                Some(code) => ExitCode::from(code.clamp(1, 255) as u8),
                None => ExitCode::FAILURE,
            });
        }
        Cmd::Path { task } => println!("{}", Task::open(&home, &task)?.copy().display()),
        Cmd::Exec {
            task,
            allow_write,
            hide,
            reveal,
            pass_env,
            no_network,
            open_network,
            no_sandbox,
            command,
        } => {
            let task = Task::open(&home, &task)?;
            // Held while the command runs: commit and rollback wait for it.
            let _at_work = task.agent_at_work()?;
            let logged_before = connections(&task)?.len();
            let started = Instant::now();
            let mut used = None;
            let mut proxy = None;
            let mut child = if no_sandbox {
                eprintln!("kule: running without the sandbox");
                let mut child = Command::new(&command[0]);
                child.args(&command[1..]).current_dir(task.copy());
                child
            } else {
                let mut sandbox = Sandbox::for_task(&task)?;
                for path in &allow_write {
                    sandbox.allow_write(path)?;
                }
                for path in &hide {
                    sandbox.hide(path)?;
                }
                for path in &reveal {
                    sandbox.reveal(path)?;
                }
                for name in &pass_env {
                    sandbox.pass_env(name);
                }
                let network = if no_network {
                    Network::Deny
                } else if open_network {
                    eprintln!("kule: network open, bypassing the proxy and the policy");
                    Network::Allow
                } else {
                    let started = Proxy::start(&task, load_policy(&home, &task)?)?;
                    let port = started.port();
                    proxy = Some(started);
                    Network::Proxy(port)
                };
                if network != Network::Allow {
                    warn_if_network_not_isolated();
                }
                sandbox.set_network(network);
                let child = sandbox.command(&command[0], &command[1..])?;
                used = Some(sandbox);
                child
            };
            let status = child
                .env("KULE_TASK", task.id())
                .status()
                .with_context(|| format!("could not run {}", command[0]))?;
            // Cuts what the command left open and logs it, so the list below
            // is complete.
            if let Some(proxy) = proxy {
                proxy.stop();
            }
            print_set(&task.effects()?);
            print_connections(&connections(&task)?[logged_before..]);
            if let Some(sandbox) = used
                && let Some(denials) = sandbox.denials(started.elapsed())?
            {
                record_denials(&task, &denials)?;
                for d in &denials {
                    println!(
                        "sandbox: refused {} {} ({})",
                        d.operation, d.path, d.process
                    );
                }
            }
            return Ok(match status.code() {
                Some(0) => ExitCode::SUCCESS,
                Some(code) => ExitCode::from(code.clamp(1, 255) as u8),
                None => ExitCode::FAILURE,
            });
        }
        Cmd::Run {
            task,
            agent,
            prompt,
        } => {
            let task = Task::open(&home, &task)?;
            let profile = kule_acp::profile(&agent).ok_or_else(|| {
                let known: Vec<&str> = kule_acp::PROFILES.iter().map(|p| p.name).collect();
                anyhow!("no agent profile {agent}; known: {}", known.join(", "))
            })?;
            let _at_work = task.agent_at_work()?;
            let started = Instant::now();
            let logged_before = connections(&task)?.len();
            warn_if_network_not_isolated();
            let proxy = Proxy::start(&task, profile.open_hosts(load_policy(&home, &task)?)?)?;
            let kule_acp::Launch {
                command,
                sandbox,
                kept,
                mode,
            } = profile.launch(&task, &proxy)?;
            let (tx, rx) = futures::channel::mpsc::unbounded();
            let runtime = tokio::runtime::Runtime::new()?;
            let printer = runtime.spawn(print_events(rx));
            let outcome = runtime.block_on(kule_acp::run(&task, command, &prompt, mode, tx));
            let _ = runtime.block_on(printer);
            // The agent's process group is gone by now.
            let put_back = kept.restore()?;
            outcome.map_err(|e| {
                anyhow!(
                    "{e} (see {})",
                    task.copy().with_file_name("agent.log").display()
                )
            })?;
            println!();
            proxy.stop();
            print_set(&task.effects()?);
            print_connections(&connections(&task)?[logged_before..]);
            if let Some(denials) = sandbox.denials(started.elapsed())? {
                record_denials(&task, &denials)?;
                for d in &denials {
                    println!(
                        "sandbox: refused {} {} ({})",
                        d.operation, d.path, d.process
                    );
                }
            }
            let put_back: Vec<Denial> = put_back.iter().map(|p| Denial::put_back(p)).collect();
            record_denials(&task, &put_back)?;
            for d in &put_back {
                println!("sandbox: the agent changed {}; Kule put it back", d.path);
            }
            print_ruling(&load_policy(&home, &task)?.judge(&task.effects()?.to_commit(false)));
        }
        Cmd::Steps { task } => {
            for step in Task::open(&home, &task)?.steps()? {
                println!("{:>3}. {}", step.id, step.label);
                for change in &step.changes {
                    let mark = match change.change {
                        Some(Change::Created) => "+",
                        Some(Change::Modified) => "~",
                        Some(Change::Deleted) => "-",
                        None => "↺",
                    };
                    println!("       {mark} {}", change.path);
                }
            }
        }
        Cmd::Status { task, json } => {
            let task = Task::open(&home, &task)?;
            let set = task.effects()?;
            // On what a plain commit would take: ignored paths stay behind.
            let committed = set.to_commit(false);
            let ruling = load_policy(&home, &task)?.judge(&committed);
            if json {
                let both = serde_json::json!({ "effects": set, "ruling": ruling });
                println!("{}", serde_json::to_string_pretty(&both)?);
            } else {
                print_set(&set);
                if !committed.is_empty() {
                    print_ruling(&ruling);
                }
            }
        }
        Cmd::Commit {
            task,
            approve,
            include_ignored,
        } => {
            let task = Task::open(&home, &task)?;
            let policy = load_policy(&home, &task)?;
            match task.commit_with(include_ignored, policy.gate(approve)) {
                Ok(Some(cp)) => println!(
                    "committed {} changes · checkpoint {}",
                    cp.effects.len(),
                    cp.id
                ),
                Ok(None) => println!("nothing to commit"),
                Err(Error::Held(ruling)) => {
                    print_ruling(&ruling);
                    if ruling.verdict == Verdict::Ask {
                        eprintln!(
                            "kule: not committed; rerun with --approve to approve the changes above"
                        );
                    } else {
                        eprintln!("kule: not committed; the policy denies the changes above");
                    }
                    return Ok(ExitCode::from(3));
                }
                Err(e) => return Err(e.into()),
            }
        }
        Cmd::Policy => print!("{DEFAULT_POLICY}"),
        Cmd::Rollback { task } => {
            let set = Task::open(&home, &task)?.rollback()?;
            println!("rolled back {} changes", set.effects.len());
        }
        Cmd::Undo { task } => match Task::open(&home, &task)?.undo()? {
            Some(cp) => println!("undid checkpoint {} ({} changes)", cp.id, cp.effects.len()),
            None => println!("nothing to undo"),
        },
        Cmd::History { task } => {
            let history = Task::open(&home, &task)?.history()?;
            for entry in &history {
                let what = match &entry.event {
                    kule_engine::HistoryEvent::Commit {
                        checkpoint,
                        changes,
                        snapshot,
                        ..
                    } => format!(
                        "commit {checkpoint} · {changes} changes · snapshot {}",
                        &snapshot[..snapshot.len().min(12)]
                    ),
                    kule_engine::HistoryEvent::Undo { checkpoint } => {
                        format!("undo {checkpoint}")
                    }
                };
                println!("{:>3}. {what}", entry.seq);
            }
            println!("history verified: {} entries, signed", history.len());
        }
        Cmd::Daemon { idle_secs } => {
            eprintln!("kule: daemon for {}", home.display());
            kule_daemon::run(&home, std::time::Duration::from_secs(idle_secs))
                .with_context(|| format!("daemon for {}", home.display()))?;
        }
        Cmd::Rm { task } => {
            Task::open(&home, &task)?.remove()?;
            Task::empty_trash(&home)?;
            println!("removed task {task}");
        }
    }
    Ok(ExitCode::SUCCESS)
}

/// The user's policy from `<home>/policy.cedar` (or the built-in one), plus
/// the forbid rules of the task's real workspace.
fn load_policy(home: &Path, task: &Task) -> Result<Policy> {
    Ok(Policy::load(
        Some(&home.join("policy.cedar")),
        task.workspace(),
    )?)
}

/// Prints an agent's events as they come: its answer as running text, tool
/// calls and what each changed as their own lines.
async fn print_events(mut rx: futures::channel::mpsc::UnboundedReceiver<kule_acp::Event>) {
    use futures::StreamExt;
    use kule_acp::Event;
    let mut at_line_start = true;
    let line = |text: String, at_line_start: &mut bool| {
        if !*at_line_start {
            println!();
        }
        println!("{text}");
        *at_line_start = true;
    };
    while let Some(event) = rx.next().await {
        match event {
            Event::Message { text } => {
                print!("{text}");
                let _ = std::io::stdout().flush();
                at_line_start = text.ends_with('\n');
            }
            Event::Thought { .. } | Event::Done { .. } => {}
            Event::ToolCall { title, .. } => line(format!("· {title}"), &mut at_line_start),
            Event::ToolDone {
                title,
                failed,
                step,
                ..
            } => {
                let mark = if failed { "✗" } else { "✓" };
                let Some(step) = step else {
                    line(format!("{mark} {title}"), &mut at_line_start);
                    continue;
                };
                let changed: Vec<&str> = step.changes.iter().map(|c| c.path.as_str()).collect();
                // A build touches hundreds of files; the first few say enough,
                // and `kule steps` has the rest.
                const SHOWN: usize = 5;
                let what = match changed.len() {
                    0 => "no file changes".to_owned(),
                    n if n <= SHOWN => changed.join(", "),
                    n => format!("{} and {} more", changed[..SHOWN].join(", "), n - SHOWN),
                };
                line(
                    format!("{mark} {title} → step {}: {what}", step.id),
                    &mut at_line_start,
                );
            }
            Event::Permission { title, chosen } => line(
                format!("· permission \"{title}\": {chosen}"),
                &mut at_line_start,
            ),
            Event::Plan { entries } => {
                line(format!("plan: {}", entries.join(" / ")), &mut at_line_start)
            }
        }
    }
}

/// The connections a command made, grouped by host, port and outcome.
fn print_connections(log: &[Connection]) {
    let mut seen: Vec<(&Connection, usize)> = Vec::new();
    for c in log {
        match seen.iter_mut().find(|(s, _)| {
            (&s.host, s.port, s.verdict, &s.outcome) == (&c.host, c.port, c.verdict, &c.outcome)
        }) {
            Some((_, n)) => *n += 1,
            None => seen.push((c, 1)),
        }
    }
    for (c, n) in seen {
        let times = if n > 1 {
            format!(" ×{n}")
        } else {
            String::new()
        };
        let what = match c.verdict {
            Verdict::Allow => c.outcome.clone(),
            Verdict::Ask => "refused, waits for approval".to_owned(),
            Verdict::Deny => "refused, denied".to_owned(),
        };
        println!(
            "net: {}:{}{times} · {what} ({})",
            c.host,
            c.port,
            c.rules.join(", ")
        );
    }
}

fn print_ruling(ruling: &Ruling) {
    let word = |v: Verdict| match v {
        Verdict::Allow => "passes on its own",
        Verdict::Ask => "waits for approval",
        Verdict::Deny => "denied",
    };
    println!("policy: {}", word(ruling.verdict));
    for judgement in ruling.holding() {
        println!(
            "  {} · {} ({})",
            judgement.subject,
            word(judgement.verdict),
            judgement.rules.join(", ")
        );
    }
}

/// Files one per line; `.git` as what it means, not as object file names.
fn warn_if_network_not_isolated() {
    if !kule_engine::isolates_network() {
        eprintln!(
            "kule: this system refuses unprivileged user namespaces, so the sandbox \
             checks only the proxy's port: a command can reach any address on that \
             port and send datagrams anywhere. Allow them (on Ubuntu: sysctl \
             kernel.apparmor_restrict_unprivileged_userns=0) to close that."
        );
    }
}

fn print_set(set: &EffectSet) {
    const SHOWN: usize = 40;
    let files: Vec<&Effect> = set.files().filter(|e| !e.ignored).collect();
    let ignored: Vec<&Effect> = set.ignored().collect();
    if !ignored.is_empty() {
        println!(
            "{} ignored by the workspace, staying in the copy (commit --include-ignored takes them):",
            ignored.len()
        );
        for effect in ignored.iter().take(10) {
            let slash = if effect.kind == Kind::Dir { "/" } else { "" };
            println!("  · {}{slash}", effect.path);
        }
        if ignored.len() > 10 {
            println!("  … {} more", ignored.len() - 10);
        }
    }
    if files.is_empty() && set.git.is_none() {
        println!("no changes");
        return;
    }
    if !files.is_empty() {
        let count = |change| files.iter().filter(|e| e.change == change).count();
        println!(
            "{} file changes: +{} ~{} -{}",
            files.len(),
            count(Change::Created),
            count(Change::Modified),
            count(Change::Deleted)
        );
        for effect in files.iter().take(SHOWN) {
            let mark = match effect.change {
                Change::Created => '+',
                Change::Modified => '~',
                Change::Deleted => '-',
            };
            let slash = if effect.kind == Kind::Dir { "/" } else { "" };
            println!("  {mark} {}{slash}", effect.path);
        }
        if files.len() > SHOWN {
            println!("  … {} more (kule status --json)", files.len() - SHOWN);
        }
    }
    if let Some(git) = &set.git {
        print_git(git);
    }
}

fn print_git(git: &GitSummary) {
    for change in &git.refs {
        println!("git: {}", describe_ref(change));
    }
    let mut store = Vec::new();
    if git.object_files > 0 {
        store.push(format!("{} new object files", git.object_files));
    }
    if git.index {
        store.push("index changed".to_owned());
    }
    if !store.is_empty() {
        println!("git: {}", store.join(" · "));
    }
    if git.config {
        println!("git: config changed (it can make the next git command run code)");
    }
    for hook in &git.hooks {
        println!("git: hook {hook} changed (it runs on the next matching git command)");
    }
    for path in &git.other {
        println!("git: {path} changed");
    }
}

fn describe_ref(change: &RefChange) -> String {
    let name = change
        .name
        .strip_prefix("refs/heads/")
        .unwrap_or(&change.name);
    let short = |value: &str| match value.strip_prefix("ref: ") {
        Some(target) => target
            .strip_prefix("refs/heads/")
            .unwrap_or(target)
            .to_owned(),
        None => value.chars().take(7).collect(),
    };
    match (&change.old, &change.new) {
        (None, Some(new)) => format!("new {name} at {}", short(new)),
        (Some(old), None) => format!("deleted {name} (was {})", short(old)),
        (None, None) => name.to_owned(),
        (Some(old), Some(new)) => {
            let mut line = format!("{name} {} → {}", short(old), short(new));
            match change.fast_forward {
                Some(true) => line.push_str(" · fast-forward"),
                Some(false) => line.push_str(" · history rewritten"),
                None => {}
            }
            if let Some(n) = change.commits {
                line.push_str(&format!(" · {n} commit{}", if n == 1 { "" } else { "s" }));
            }
            line
        }
    }
}
