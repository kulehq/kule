use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::time::Instant;

use anyhow::{Context, Result, anyhow};
use clap::{Parser, Subcommand};
use kule_engine::entities::{Change as EntityChange, Edit, Outline, Skip};
use kule_engine::{
    Change, Connection, DEFAULT_POLICY, Denial, Effect, EffectSet, Error, GitSummary, Kind,
    Network, Policy, Proxy, RefChange, Ruling, Sandbox, Task, Verdict, connections, meet,
    record_denials,
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
enum AgentsCmd {
    /// List them, and why the ones that cannot run cannot (the default).
    List,
    /// Show what an agent's profile opens in the sandbox.
    Show { name: String },
    /// Let one of your profiles run, as it stands now. Changed later, it
    /// waits for approval again.
    Approve { name: String },
    /// Take an approval back.
    Revoke { name: String },
    /// The agents of the ACP registry, which `add` takes a draft from.
    Registry,
    /// Add an agent of the ACP registry to your profiles, opening nothing
    /// yet: `try` it to see what it needs.
    Add { id: String },
    /// Run one of your profiles once, in an empty folder, and show what the
    /// sandbox refused it: what its profile should open. With --apply, add
    /// that to the profile (which then waits for approval again).
    Try {
        name: String,
        #[arg(long)]
        apply: bool,
    },
    /// Take one of your profiles out of agents.toml.
    Remove { name: String },
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
    /// Give an agent a prompt: it runs over ACP inside the sandbox,
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
    /// The agents Kule can run: its own, and yours from
    /// <KULE_HOME>/agents.toml, each of which runs only once approved.
    Agents {
        #[command(subcommand)]
        action: Option<AgentsCmd>,
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
            print_set(&task, &task.effects()?);
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
                if let Some(person) = std::env::var_os("HOME") {
                    kule_acp::Agents::load(&home)?.hide_logins(&mut sandbox, Path::new(&person))?;
                }
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
            print_set(&task, &task.effects()?);
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
            let agents = kule_acp::Agents::load(&home)?;
            let profile = agents
                .get(&agent)
                .ok_or_else(|| anyhow!("{}", agents.refusal(&agent)))?;
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
            let outcome =
                runtime.block_on(kule_acp::run(&task, command, &prompt, mode.as_deref(), tx));
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
            print_set(&task, &task.effects()?);
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
            let put_back: Vec<Denial> = put_back.iter().map(Denial::put_back).collect();
            record_denials(&task, &put_back)?;
            for d in &put_back {
                match &d.saved {
                    Some(saved) => println!(
                        "sandbox: {} changed during the run; Kule put it back and moved what was there to {saved}",
                        d.path
                    ),
                    None => println!(
                        "sandbox: {} was removed during the run; Kule put it back",
                        d.path
                    ),
                }
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
                let outline = task.outline(&committed).unwrap_or_default();
                let both =
                    serde_json::json!({ "effects": set, "ruling": ruling, "outline": outline });
                println!("{}", serde_json::to_string_pretty(&both)?);
            } else {
                if !task.workspace_exists() {
                    println!(
                        "workspace {} is gone (moved or deleted); commit and undo are refused",
                        task.workspace().display()
                    );
                }
                print_set(&task, &set);
                if !committed.is_empty() {
                    print_ruling(&ruling);
                    print_meetings(&home, &task)?;
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
                Ok(Some(cp)) => {
                    println!(
                        "committed {} changes · checkpoint {}",
                        cp.effects.len(),
                        cp.id
                    );
                    if !cp.merged.is_empty() {
                        println!(
                            "merged with what the folder changed meanwhile: {}",
                            cp.merged.join(", ")
                        );
                    }
                }
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
        Cmd::Agents { action } => agents_cmd(&home, action.unwrap_or(AgentsCmd::List))?,
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

fn warn_if_network_not_isolated() {
    if !kule_engine::isolates_network() {
        eprintln!(
            "kule: this system refuses unprivileged user namespaces, so the sandbox \
             checks only the proxy's port: a command can reach any address on that \
             port and send datagrams anywhere. The agent's settings are put back \
             after the run instead of kept read-only. Allow them (on Ubuntu: sysctl \
             kernel.apparmor_restrict_unprivileged_userns=0) to close that."
        );
    }
}

/// Files one per line, each with what changed inside it when Kule can tell;
/// `.git` as what it means, not as object file names.
fn print_set(task: &Task, set: &EffectSet) {
    const SHOWN: usize = 40;
    // A move is one line, under its new name.
    let moved_to: HashMap<&str, &str> = set
        .moves
        .iter()
        .map(|m| (m.to.as_str(), m.from.as_str()))
        .collect();
    let files: Vec<&Effect> = set
        .files()
        .filter(|e| !e.ignored && !set.moves.iter().any(|m| m.from == e.path))
        .collect();
    let outlines = task.outline(&set.to_commit(false)).unwrap_or_default();
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
        let moved = |e: &&&Effect| moved_to.contains_key(e.path.as_str());
        let count = |change| {
            files
                .iter()
                .filter(|e| e.change == change && !moved(e))
                .count()
        };
        let moves = files.iter().filter(moved).count();
        println!(
            "{} file changes: +{} ~{} -{}{}",
            files.len(),
            count(Change::Created),
            count(Change::Modified),
            count(Change::Deleted),
            if moves > 0 {
                format!(" »{moves}")
            } else {
                String::new()
            }
        );
        for effect in files.iter().take(SHOWN) {
            let mark = match effect.change {
                Change::Created => '+',
                Change::Modified => '~',
                Change::Deleted => '-',
            };
            let slash = if effect.kind == Kind::Dir { "/" } else { "" };
            if let Some(from) = moved_to.get(effect.path.as_str()) {
                // What moved with its directory goes without saying.
                let with_parent = set.moves.iter().any(|d| {
                    effect.path.strip_prefix(d.to.as_str()).is_some_and(|rest| {
                        rest.starts_with('/') && from.strip_prefix(d.from.as_str()) == Some(rest)
                    })
                });
                if !with_parent {
                    println!("  » {from}{slash} → {}{slash}", effect.path);
                }
                continue;
            }
            let outline = outlines.iter().find(|o| o.path == effect.path);
            match outline.map(|o| &o.outline) {
                Some(Outline::Skipped { reason }) if *reason != Skip::Binary => {
                    println!(
                        "  {mark} {}{slash} ({})",
                        effect.path,
                        why_not_outlined(*reason)
                    );
                }
                _ => println!("  {mark} {}{slash}", effect.path),
            }
            if let Some(Outline::Changes { changes }) = outline.map(|o| &o.outline) {
                print_entity_changes(changes);
            }
        }
        if files.len() > SHOWN {
            println!("  … {} more (kule status --json)", files.len() - SHOWN);
        }
    }
    if let Some(git) = &set.git {
        print_git(git);
    }
}

/// Where the task's work meets that of other tasks on its workspace: the
/// files both change and, inside them, what both change.
fn agents_cmd(home: &Path, action: AgentsCmd) -> Result<()> {
    use kule_acp::{Agents, Origin};
    match action {
        AgentsCmd::List => {
            let agents = Agents::load(home)?;
            for listed in agents.listed() {
                let origin = match listed.origin {
                    Origin::BuiltIn => "built in",
                    Origin::Person => "yours",
                };
                match &listed.held {
                    None => println!("{:<16} {origin}", listed.profile.name),
                    Some(why) => println!("{:<16} {origin}, held: {why}", listed.profile.name),
                }
            }
            println!(
                "\nAdd your own in {} (format: kule agents show claude-code).",
                home.join(kule_acp::AGENTS_FILE).display()
            );
        }
        AgentsCmd::Show { name } => {
            let agents = Agents::load(home)?;
            let listed = agents
                .listed()
                .iter()
                .find(|l| l.profile.name == name)
                .ok_or_else(|| anyhow!("no agent {name}"))?;
            print_profile(&listed.profile);
            if let Some(dir) = &listed.own_home_dir {
                println!("its own home      {dir}");
            }
            for warning in &listed.warnings {
                println!("\nwarning: {warning}");
            }
            if let Some(why) = &listed.held {
                println!("\nheld: {why}");
            }
        }
        AgentsCmd::Approve { name } => {
            let profile = Agents::approve(home, &name)?;
            print_profile(&profile);
            let agents = Agents::load(home)?;
            let listed = agents.listed().iter().find(|l| l.profile.name == name);
            for warning in listed.map(|l| l.warnings.as_slice()).unwrap_or_default() {
                println!("\nwarning: {warning}");
            }
            println!("\napproved {name} as it stands; a change to it needs approval again");
        }
        AgentsCmd::Revoke { name } => {
            if Agents::revoke(home, &name)? {
                println!("{name} is no longer approved");
            } else {
                println!("{name} was not approved");
            }
        }
        AgentsCmd::Registry => {
            let (entries, fresh) = kule_acp::registry::load(home)?;
            if !fresh {
                println!("(the registry could not be reached; as last read)");
            }
            for entry in &entries {
                let how = match &entry.install {
                    kule_acp::registry::Install::Package { command, .. } => command.join(" "),
                    kule_acp::registry::Install::Download { cmd, .. } => {
                        format!("download, then {cmd}")
                    }
                    kule_acp::registry::Install::Unavailable => "not for this system".to_owned(),
                };
                println!("{:<22} {:<10} {how}", entry.id, entry.version);
            }
        }
        AgentsCmd::Add { id } => {
            let (entries, _) = kule_acp::registry::load(home)?;
            let entry = entries
                .iter()
                .find(|e| e.id == id)
                .ok_or_else(|| anyhow!("no agent {id} in the ACP registry"))?;
            let draft = entry
                .draft()
                .ok_or_else(|| anyhow!("{id} has nothing to run on this system"))?;
            if Agents::load(home)?
                .listed()
                .iter()
                .any(|l| l.profile.name == id)
            {
                return Err(anyhow!("there is a profile named {id} already"));
            }
            Agents::save(home, &draft)?;
            if let kule_acp::registry::Install::Download {
                archive,
                sha256,
                cmd,
                ..
            } = &entry.install
            {
                println!(
                    "{id} is a download: get {archive} (sha256 {sha256}) and put {cmd} on your PATH"
                );
            }
            println!(
                "added {id}, opening nothing yet; next: kule agents try {id} --apply, then kule agents approve {id}"
            );
        }
        AgentsCmd::Try { name, apply } => {
            let agents = Agents::load(home)?;
            let listed = agents
                .listed()
                .iter()
                .find(|l| l.profile.name == name && l.origin == Origin::Person)
                .ok_or_else(|| anyhow!("no profile {name} of yours"))?;
            println!("trying {name} in an empty folder (up to two minutes)…");
            let trial = kule_acp::trial::trial(
                home,
                &agents,
                &listed.profile,
                kule_engine::CacheSources::from_env(),
            )?;
            match &trial.error {
                None => println!("it answered: {}", trial.reply),
                Some(error) => println!("it did not answer: {error}"),
            }
            for line in &trial.log {
                println!("log      {line}");
            }
            for refused in &trial.refused {
                println!("refused  {refused}");
            }
            println!("suggested state: {}", trial.state.join(", "));
            println!("suggested hosts: {}", trial.hosts.join(", "));
            println!("suggested read-only: {}", trial.protect.join(", "));
            if cfg!(target_os = "linux") {
                println!("(Linux does not report refused files: only hosts are suggested)");
            }
            let nothing =
                trial.state.is_empty() && trial.hosts.is_empty() && trial.protect.is_empty();
            if apply && !nothing {
                let mut profile = listed.profile.clone();
                for path in trial.protect {
                    if !profile.protected.contains(&path) {
                        profile.protected.push(path);
                    }
                }
                for state in trial.state {
                    if !profile.state.contains(&state) {
                        profile.state.push(state);
                    }
                }
                for host in trial.hosts {
                    if !profile.hosts.contains(&host) {
                        profile.hosts.push(host);
                    }
                }
                Agents::save(home, &profile)?;
                println!(
                    "added to {name}; review it with kule agents show {name}, then approve it"
                );
            }
        }
        AgentsCmd::Remove { name } => {
            if Agents::remove(home, &name)? {
                println!("removed {name}");
            } else {
                println!("no profile {name} of yours");
            }
        }
    }
    Ok(())
}

/// A profile as the person approves it: what it runs and what it opens.
fn print_profile(profile: &kule_acp::Profile) {
    let list = |items: &[String]| {
        if items.is_empty() {
            "nothing".to_owned()
        } else {
            items.join(", ")
        }
    };
    println!("[[agent]]  {}", profile.name);
    println!("runs              {}", profile.command.join(" "));
    println!("writes in ~/      {}", list(&profile.state));
    println!("its login         {}", list(&profile.login));
    println!("read-only         {}", list(&profile.protected));
    if !profile.reveal_on_macos.is_empty() {
        println!("reveals (macOS)   {}", list(&profile.reveal_on_macos));
    }
    println!("sees your env     {}", list(&profile.pass_env));
    let env: Vec<String> = profile
        .env
        .iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect();
    if !env.is_empty() {
        println!("sets env          {}", env.join(" "));
    }
    let hosts: Vec<String> = profile
        .hosts
        .iter()
        .map(|h| format!("https://{h}"))
        .collect();
    println!("also connects to  {}", list(&hosts));
    if let Some(mode) = &profile.mode {
        println!("session mode      {mode}");
    }
    if let Some(var) = &profile.own_home {
        println!("home of its own   ${var} points at Kule's folder for it; sign it in there once");
    }
}

fn print_meetings(home: &Path, task: &Task) -> Result<()> {
    let ours = task.touched()?;
    for info in Task::list(home)? {
        if info.id == task.id() || info.workspace != task.workspace() {
            continue;
        }
        // A task being removed meanwhile meets nobody.
        let Ok(theirs) = Task::open(home, &info.id).and_then(|t| t.touched()) else {
            continue;
        };
        let meetings = meet(&ours, &theirs);
        if meetings.is_empty() {
            continue;
        }
        println!("task {} changes them too:", info.id);
        for m in meetings {
            match m.entities.as_slice() {
                [] => println!("  {}", m.path),
                entities => println!("  {} · {}", m.path, entities.join(", ")),
            }
        }
    }
    Ok(())
}

/// What changed inside one file, under its line.
fn print_entity_changes(changes: &[EntityChange]) {
    const SHOWN: usize = 8;
    for change in changes.iter().take(SHOWN) {
        let mark = match change.edit {
            Edit::Added => '+',
            Edit::Modified => '~',
            Edit::Removed => '-',
        };
        let kind = change.kind.label();
        match change.name.as_str() {
            "" => println!("      {mark} {kind}"),
            name => println!("      {mark} {kind} {name}"),
        }
    }
    if changes.len() > SHOWN {
        println!("      … {} more", changes.len() - SHOWN);
    }
}

fn why_not_outlined(reason: Skip) -> &'static str {
    match reason {
        Skip::BeforeUnknown => "changed in the workspace too; not outlined",
        Skip::TooLarge => "too large to outline",
        Skip::TooMany => "too much in it to outline",
        Skip::Unparsed => "could not be read to outline",
        Skip::Binary | Skip::Unsupported => "not outlined",
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
    for path in &git.staged {
        println!(
            "git: {path} is staged unlike the file (the next git commit records what you did not review)"
        );
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
