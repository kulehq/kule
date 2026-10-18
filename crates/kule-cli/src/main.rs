mod paint;

use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::time::Instant;

use anyhow::{Context, Result, anyhow, bail};
use clap::{Parser, Subcommand};
use kule_engine::entities::{Change as EntityChange, Edit, Outline, Skip};
use kule_engine::{CAREFUL_POLICY, PolicyChoice, USER_POLICY, choose_policy, policy_choice};
use kule_engine::{
    Change, Connection, DEFAULT_POLICY, Denial, DiffSkip, Effect, EffectSet, Error, GitSummary,
    Kind, LineKind, Money, Network, Policy, Proxy, RefChange, Ruling, RunUsage, Sandbox, Task,
    TaskColor, TextDiff, Verdict, connections, meet, record_denials,
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
enum RulesCmd {
    /// List them (the default).
    List,
    /// Ask again about what rule ID let through.
    Forget { id: u64 },
    /// Let a connection (HOST:PORT) or the changes to a file (PATH, with
    /// --rule) through without asking. Shows what it would have let
    /// through so far; --yes keeps it.
    Allow {
        what: String,
        /// For a file: the rule that asks about it.
        #[arg(long, default_value = "manifests-and-lockfiles")]
        rule: String,
        /// The folder it holds for (default: the current one).
        #[arg(long, conflicts_with = "everywhere")]
        folder: Option<PathBuf>,
        /// Every folder.
        #[arg(long)]
        everywhere: bool,
        #[arg(long)]
        yes: bool,
    },
}

#[derive(Subcommand)]
enum GroupCmd {
    /// List the groups, their policies and tasks (the default).
    List,
    /// Make a group; it keeps the policy for every task until you choose
    /// another.
    New {
        name: String,
        #[arg(long)]
        color: Option<String>,
    },
    /// Rename group ID.
    Rename { id: u64, name: String },
    /// Mark group ID with a colour; without COLOR, take it away.
    Color { id: u64, color: Option<String> },
    /// The policy group ID's tasks are judged by: default (the one for
    /// every task), balanced, careful, or own. Without a choice, print the
    /// rules that hold for the group.
    Policy {
        id: u64,
        choice: Option<String>,
        /// With `own`: the group's policy, in Cedar.
        #[arg(long, value_name = "PATH")]
        file: Option<PathBuf>,
    },
    /// Put TASK in group GROUP; without GROUP, take it out of its group.
    Put { task: String, group: Option<u64> },
    /// Delete group ID; its tasks stay, in no group.
    Delete { id: u64 },
}

#[derive(Subcommand)]
enum HomeCmd {
    /// List them (the default).
    List,
    /// Take changes to your real home, after checking each path there is
    /// still as it was; what they replace is kept in the task.
    Apply {
        /// As listed, like ~/.zshrc.
        paths: Vec<String>,
        #[arg(long, conflicts_with = "paths")]
        all: bool,
    },
    /// Throw changes away; the next run starts from your home as it is.
    Discard {
        paths: Vec<String>,
        #[arg(long, conflicts_with = "paths")]
        all: bool,
    },
    /// Show what a file's change would do to yours, line by line.
    Diff {
        /// As listed, like ~/.zshrc.
        path: String,
    },
    /// Take the last apply back, if nothing changed at those paths since;
    /// the changes wait for review again.
    Undo,
}

#[derive(Subcommand)]
enum SecretsCmd {
    /// List them, without their values (the default).
    List,
    /// Keep a secret: its value is read from the terminal without echo, or
    /// from standard input, never from the command line. Commands in a
    /// task see a placeholder in variable NAME; the proxy puts the value
    /// into requests to the hosts named, over HTTPS, and nowhere else.
    Add {
        /// The environment variable it goes in, like STRIPE_KEY.
        name: String,
        /// A host it may go to (repeatable), like api.stripe.com.
        #[arg(long = "host", required = true)]
        hosts: Vec<String>,
        /// The folder it is for (default: the current one).
        #[arg(long, conflicts_with = "everywhere")]
        folder: Option<PathBuf>,
        /// Every folder.
        #[arg(long)]
        everywhere: bool,
    },
    /// Forget a secret.
    Remove {
        name: String,
        #[arg(long, conflicts_with = "everywhere")]
        folder: Option<PathBuf>,
        #[arg(long)]
        everywhere: bool,
    },
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
    /// List tasks, in the order the app shows them.
    List,
    /// Give the task a name, shown with its folder's; without NAME, take
    /// it away.
    Name { task: String, name: Option<String> },
    /// Mark the task with a colour (red, orange, yellow, green, teal, blue,
    /// purple, pink); without COLOR, take it away.
    Color { task: String, color: Option<String> },
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
    /// Set what the task's agent runs may cost together, as the agents
    /// report it: none starts once it is spent, and a run whose agent
    /// reports its cost as it works is stopped when it gets there. Claude
    /// Code reports it as each turn ends, so a turn may go over and the next
    /// run is refused. `off` takes it away. Agents that report no cost (one
    /// signed in with a subscription, say) are not held by it.
    Budget {
        task: String,
        /// An amount like 5 or 2.50, or `off`.
        amount: String,
        /// ISO 4217 [default: USD]
        currency: Option<String>,
    },
    /// Show or set what every task may cost together in any 24 hours: no
    /// run starts anywhere once the runs started in the last 24 hours
    /// spent it, and a run is held to what is left, as a task's budget
    /// holds it. `off` takes it away.
    DailyBudget {
        /// An amount like 20 or 7.50, or `off`; shows it without one.
        amount: Option<String>,
        /// ISO 4217 [default: USD]
        currency: Option<String>,
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
        /// With --approve: do not ask again, on this folder, about the files
        /// approved now (manifests, CI and project files).
        #[arg(long, requires = "approve")]
        remember: bool,
    },
    /// Groups of tasks, each with a name, a colour and the policy its tasks
    /// are judged by.
    Group {
        #[command(subcommand)]
        action: Option<GroupCmd>,
    },
    /// What you told Kule not to ask about again.
    Rules {
        #[command(subcommand)]
        action: Option<RulesCmd>,
    },
    /// What the task's commands wrote in your home directory (~/.zshrc,
    /// ~/.config, new files at the top): caught in the task's home layer
    /// for you to apply or discard, never written there on their own.
    Home {
        task: String,
        #[command(subcommand)]
        action: Option<HomeCmd>,
    },
    /// Secrets a task's commands use without seeing them: API keys that
    /// go only to their hosts. Values in the copy's .env files that look
    /// like secrets are masked the same way.
    Secrets {
        #[command(subcommand)]
        action: Option<SecretsCmd>,
    },
    /// The agents Kule can run: its own, and yours from
    /// <KULE_HOME>/agents.toml, each of which runs only once approved.
    Agents {
        #[command(subcommand)]
        action: Option<AgentsCmd>,
    },
    /// Print the policy that holds, or put one of Kule's in place:
    /// `balanced` (the built-in one) or `careful` (every change to a file
    /// or to git waits for you). A policy of your own is written to
    /// <KULE_HOME>/policy.cedar, and Kule never replaces it; a repository
    /// adds forbid rules in .kule/policy.cedar.
    Policy {
        /// balanced or careful
        choice: Option<String>,
    },
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
            for info in kule_engine::arranged(&home)? {
                let task = Task::open(&home, &info.id)?;
                let mut line = format!("{}  {}", info.id, info.workspace.display());
                if let Some(label) = task.label()? {
                    line += &format!(" · {label}");
                }
                if let Some(color) = task.color()? {
                    line += &format!(" ({})", color.name());
                }
                if let Some(id) = task.group()?
                    && let Some(group) =
                        kule_engine::groups(&home)?.into_iter().find(|g| g.id == id)
                {
                    line += &format!(" [{}]", group.name);
                }
                println!("{line}");
            }
        }
        Cmd::Name { task, name } => {
            Task::open(&home, &task)?.set_label(name.as_deref())?;
        }
        Cmd::Color { task, color } => {
            let color = color.as_deref().map(TaskColor::parse).transpose()?;
            Task::open(&home, &task)?.set_color(color)?;
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
                    let person = Path::new(&person);
                    kule_acp::Agents::load(&home)?.hide_logins(&mut sandbox, person)?;
                    // What the command writes in the home is caught for
                    // review (`kule home`), not refused.
                    let layer = task.prepare_home(person, &[], sandbox.hidden(), &[])?;
                    sandbox.use_home(&layer, person)?;
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
                match &proxy {
                    Some(proxy) => sandbox.use_proxy(proxy),
                    None => sandbox.set_network(network),
                }
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
            print_home(&task)?;
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
            let limit = task.budget_left()?;
            if let Some(left) = &limit
                && left.amount <= 0.0
            {
                bail!(
                    "the task has spent its budget; raise it with `kule budget {} AMOUNT CURRENCY` or turn it off",
                    task.id()
                );
            }
            let daily = kule_engine::daily_left(&home)?;
            if let Some(left) = &daily
                && left.amount <= 0.0
            {
                bail!(
                    "the daily budget is spent: every task together, in the last 24 hours; raise it with `kule daily-budget AMOUNT CURRENCY` or turn it off"
                );
            }
            let limit = Money::tighter(limit, daily);
            let _at_work = task.agent_at_work()?;
            let started = Instant::now();
            let usage = RunUsage {
                agent: profile.name.clone(),
                started_unix: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |d| d.as_secs()),
                ..RunUsage::default()
            };
            let logged_before = connections(&task)?.len();
            warn_if_network_not_isolated();
            let policy = profile.open_hosts(load_policy(&home, &task)?)?;
            let proxy = Proxy::start_with_keys(&task, policy, &profile.agent_keys())?;
            let kule_acp::Launch {
                command,
                sandbox,
                kept,
                mode,
            } = profile.launch(&task, &proxy)?;
            let (tx, rx) = futures::channel::mpsc::unbounded();
            let runtime = tokio::runtime::Runtime::new()?;
            let printer = runtime.spawn(print_events(rx, usage, profile.name.clone()));
            let outcome = runtime.block_on(kule_acp::run_limited(
                &task,
                command,
                &prompt,
                mode.as_deref(),
                limit,
                tx,
            ));
            if let Ok(usage) = runtime.block_on(printer) {
                // Kept whether the run went well or not: it was spent either way.
                task.record_usage(&usage)?;
                if usage.cost.is_none() && task.budget()?.is_some() {
                    eprintln!(
                        "kule: {} reported no cost, so the task's budget does not count this run",
                        profile.name
                    );
                }
            }
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
            print_home(&task)?;
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
            print_spent(&task)?;
            println!("{} {}", paint::dim("took:"), paint::took(started.elapsed()));
        }
        Cmd::Budget {
            task,
            amount,
            currency,
        } => {
            let task = Task::open(&home, &task)?;
            let budget = match amount.as_str() {
                "off" => None,
                amount => Some(Money {
                    amount: amount
                        .parse()
                        .map_err(|_| anyhow!("{amount}: an amount like 5 or 2.50, or off"))?,
                    currency: currency.unwrap_or_else(|| "USD".to_owned()),
                }),
            };
            task.set_budget(budget)?;
            print_spent(&task)?;
        }
        Cmd::DailyBudget { amount, currency } => {
            match amount.as_deref() {
                None => {}
                Some("off") => kule_engine::set_daily_budget(&home, None)?,
                Some(amount) => kule_engine::set_daily_budget(
                    &home,
                    Some(Money {
                        amount: amount
                            .parse()
                            .map_err(|_| anyhow!("{amount}: an amount like 20 or 7.50, or off"))?,
                        currency: currency.unwrap_or_else(|| "USD".to_owned()),
                    }),
                )?,
            }
            let spent = kule_engine::spent_today(&home)?;
            let cost = if spent.cost.is_empty() {
                "no cost reported".to_owned()
            } else {
                spent.cost.iter().map(money).collect::<Vec<_>>().join(" + ")
            };
            let mut line = format!("last 24 hours, every task: {cost} in {} runs", spent.runs);
            match kule_engine::daily_budget(&home)? {
                Some(budget) => {
                    let left = budget.amount - spent.in_currency(&budget.currency);
                    line += &format!(
                        "; daily budget {}, {} left",
                        money(&budget),
                        money(&Money {
                            amount: left.max(0.0),
                            currency: budget.currency.clone()
                        })
                    );
                }
                None => line += "; no daily budget",
            }
            println!("{line}");
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
                let both = serde_json::json!({
                    "effects": set,
                    "ruling": ruling,
                    "outline": outline,
                    "spent": task.spent()?,
                    "budget": task.budget()?,
                    "home": task.home_changes()?,
                });
                println!("{}", serde_json::to_string_pretty(&both)?);
            } else {
                if !task.workspace_exists() {
                    println!(
                        "workspace {} is gone (moved or deleted); commit and undo are refused",
                        task.workspace().display()
                    );
                }
                print_set(&task, &set);
                print_home(&task)?;
                if !committed.is_empty() {
                    print_ruling(&ruling);
                    print_meetings(&home, &task)?;
                }
                print_spent(&task)?;
            }
        }
        Cmd::Commit {
            task,
            approve,
            include_ignored,
            remember,
        } => {
            let task = Task::open(&home, &task)?;
            let policy = load_policy(&home, &task)?;
            // What to remember is read before the commit takes the changes.
            let keep: Vec<kule_engine::Allowance> = if remember {
                policy
                    .judge(&task.pending(include_ignored)?)
                    .judgements
                    .into_iter()
                    .filter(|j| {
                        j.verdict == Verdict::Ask
                            && j.rules
                                .iter()
                                .all(|r| kule_engine::REMEMBERABLE.contains(&r.as_str()))
                    })
                    .flat_map(|j| {
                        let path = j.subject;
                        j.rules
                            .into_iter()
                            .map(move |rule| kule_engine::Allowance::File {
                                path: path.clone(),
                                rule,
                            })
                    })
                    .collect()
            } else {
                Vec::new()
            };
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
                    for allowance in keep {
                        let kept = kule_engine::remember(&home, allowance, Some(task.workspace()))?;
                        println!(
                            "not asked again on this folder: {}",
                            describe_allowance(&kept.allowance)
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
        Cmd::Group { action } => group_cmd(&home, action.unwrap_or(GroupCmd::List))?,
        Cmd::Rules { action } => rules_cmd(&home, action.unwrap_or(RulesCmd::List))?,
        Cmd::Secrets { action } => secrets_cmd(&home, action.unwrap_or(SecretsCmd::List))?,
        Cmd::Home { task, action } => home_cmd(&home, &task, action.unwrap_or(HomeCmd::List))?,
        Cmd::Policy { choice } => {
            let path = home.join(USER_POLICY);
            match choice.as_deref() {
                None => match policy_choice(&home)? {
                    PolicyChoice::Balanced => print!("{DEFAULT_POLICY}"),
                    PolicyChoice::Careful => print!("{CAREFUL_POLICY}"),
                    PolicyChoice::Own => print!("{}", std::fs::read_to_string(&path)?),
                },
                Some(name) => {
                    let choice = match name {
                        "balanced" => PolicyChoice::Balanced,
                        "careful" => PolicyChoice::Careful,
                        _ => bail!("{name}: the policies to choose are balanced and careful"),
                    };
                    choose_policy(&home, choice)?;
                    println!("policy: {name}");
                }
            }
        }
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

/// The task's group's policy, or the one for every task from
/// `<home>/policy.cedar` (or the built-in one), plus the forbid rules of the
/// task's real workspace.
fn load_policy(_home: &Path, task: &Task) -> Result<Policy> {
    Ok(Policy::for_task(task)?)
}

/// Prints an agent's events as they come: its answer as running text, tool
/// calls and what each changed as their own lines. Returns what the run
/// used, as the agent reported it.
///
/// At a terminal, a busy line says the agent is at work and what on, tool
/// calls are one coloured line each once done, set in under the answer,
/// and a permission shows only when it was not granted. Elsewhere every
/// event is a plain line of its own.
async fn print_events(
    mut rx: futures::channel::mpsc::UnboundedReceiver<kule_acp::Event>,
    mut usage: RunUsage,
    agent: String,
) -> RunUsage {
    use futures::StreamExt;
    use kule_acp::Event;
    let tty = paint::on();
    let spinner = paint::Spinner::new(&agent);
    // What was printed last: the answer's text, or a line of Kule's.
    #[derive(PartialEq)]
    enum Last {
        Nothing,
        Answer { line_open: bool },
        Line,
    }
    let mut last = Last::Nothing;
    let line = |text: String, last: &mut Last| {
        match last {
            Last::Answer { line_open } => {
                // The answer's paragraph ends, and a blank line sets it apart.
                let end = if *line_open { "\n\n" } else { "\n" };
                spinner.print(if tty { end } else { &end[1..] }, true);
            }
            Last::Nothing | Last::Line => {}
        }
        spinner.print(&format!("{text}\n"), true);
        *last = Last::Line;
    };
    while let Some(event) = rx.next().await {
        event.tally(&mut usage);
        match event {
            Event::Usage { .. } | Event::Tokens { .. } | Event::Done { .. } => {}
            Event::Message { text } => {
                if tty && last == Last::Line {
                    spinner.print("\n", true);
                }
                let ends = text.ends_with('\n');
                spinner.print(&text, ends);
                last = Last::Answer { line_open: !ends };
            }
            Event::Thought { .. } => spinner.doing("thinking"),
            Event::ToolCall { title, .. } => {
                spinner.doing(&title);
                if !tty {
                    line(format!("· {title}"), &mut last);
                }
            }
            Event::ToolDone {
                title,
                failed,
                step,
                ..
            } => {
                spinner.doing("thinking");
                let mark = if failed {
                    paint::red("✗")
                } else {
                    paint::green("✓")
                };
                let indent = if tty { "  " } else { "" };
                let Some(step) = step else {
                    line(format!("{indent}{mark} {title}"), &mut last);
                    continue;
                };
                let changed: Vec<&str> = step.changes.iter().map(|c| c.path.as_str()).collect();
                // A build touches hundreds of files; the first few say enough,
                // and `kule steps` has the rest.
                const SHOWN: usize = 5;
                let what = match changed.len() {
                    0 => paint::dim("no file changes"),
                    n if n <= SHOWN => paint::yellow(&changed.join(", ")),
                    n => paint::yellow(&format!(
                        "{} and {} more",
                        changed[..SHOWN].join(", "),
                        n - SHOWN
                    )),
                };
                let step = paint::dim(&format!("→ step {}:", step.id));
                line(format!("{indent}{mark} {title} {step} {what}"), &mut last);
            }
            Event::Permission {
                title,
                chosen,
                granted,
            } => {
                if !tty {
                    line(format!("· permission \"{title}\": {chosen}"), &mut last);
                } else if !granted {
                    line(
                        format!("  {} {title}: {chosen}", paint::red("✗ permission")),
                        &mut last,
                    );
                }
            }
            Event::Plan { entries } => line(
                format!("{} {}", paint::cyan("plan:"), entries.join(" / ")),
                &mut last,
            ),
        }
    }
    spinner.stop();
    if matches!(last, Last::Answer { line_open: true }) {
        println!();
    }
    usage
}

fn money(m: &Money) -> String {
    format!("{:.2} {}", m.amount, m.currency)
}

/// What the task's runs used, and what is left of its budget; nothing
/// before a run reported anything.
fn print_spent(task: &Task) -> Result<()> {
    let spent = task.spent()?;
    let budget = task.budget()?;
    if spent.runs == 0 && budget.is_none() {
        return Ok(());
    }
    let cost = if spent.cost.is_empty() {
        "no cost reported".to_owned()
    } else {
        spent.cost.iter().map(money).collect::<Vec<_>>().join(" + ")
    };
    let runs = if spent.runs == 1 { "run" } else { "runs" };
    let mut line = if spent.cost.is_empty() && spent.tokens == 0 {
        format!("spent: nothing reported in {} {runs}", spent.runs)
    } else {
        format!(
            "spent: {cost}, {} tokens in {} {runs}",
            spent.tokens, spent.runs
        )
    };
    if spent.unpriced_runs > 0 && !spent.cost.is_empty() {
        line += &format!(" ({} without a cost)", spent.unpriced_runs);
    }
    if let Some(budget) = budget {
        let left = budget.amount - spent.in_currency(&budget.currency);
        line += &format!(
            "; budget {}, {} left",
            money(&budget),
            money(&Money {
                amount: left.max(0.0),
                currency: budget.currency.clone()
            })
        );
    }
    println!("{}{}", paint::dim("spent:"), &line["spent:".len()..]);
    Ok(())
}

/// The connections a command made, grouped by host, port and outcome.
fn print_connections(log: &[Connection]) {
    let mut seen: Vec<(&Connection, usize)> = Vec::new();
    for c in log {
        match seen.iter_mut().find(|(s, _)| {
            (&s.host, s.port, s.verdict, &s.outcome, &s.secrets)
                == (&c.host, c.port, c.verdict, &c.outcome, &c.secrets)
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
        // The command line has nobody to ask: what would wait for a person
        // is refused at once.
        let what = match c.verdict {
            Verdict::Allow if c.outcome == "tunneled" => paint::green(&c.outcome),
            Verdict::Allow => paint::yellow(&c.outcome),
            Verdict::Ask => {
                paint::red("refused, would wait for approval; the command line asks no one")
            }
            Verdict::Deny => paint::red("refused, denied"),
        };
        let secrets = if c.secrets.is_empty() {
            String::new()
        } else {
            format!(" · with {}", paint::cyan(&c.secrets.join(", ")))
        };
        println!(
            "{} {}:{}{times} · {what}{secrets} {}",
            paint::dim("net:"),
            c.host,
            c.port,
            paint::dim(&format!("({})", c.rules.join(", ")))
        );
    }
}

fn print_ruling(ruling: &Ruling) {
    let word = |v: Verdict| match v {
        Verdict::Allow => paint::green("passes on its own"),
        Verdict::Ask => paint::yellow("waits for approval"),
        Verdict::Deny => paint::red("denied"),
    };
    println!("{} {}", paint::dim("policy:"), word(ruling.verdict));
    for judgement in ruling.holding() {
        println!(
            "  {} · {} {}",
            judgement.subject,
            word(judgement.verdict),
            paint::dim(&format!("({})", judgement.rules.join(", ")))
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

/// What the task's commands wrote in the home directory, waiting for a
/// person; nothing when there is none.
fn print_home(task: &Task) -> Result<()> {
    let changes = task.home_changes()?;
    if changes.is_empty() {
        return Ok(());
    }
    println!(
        "in your home, caught for review ({}; `kule home {} apply PATH` takes one there):",
        changes.len(),
        task.id()
    );
    for c in &changes {
        let mark = match c.change {
            Change::Created => "+",
            Change::Modified => "~",
            Change::Deleted => "-",
        };
        let slash = if c.kind == Kind::Dir { "/" } else { "" };
        let runs = if c.startup {
            "  (runs every time a shell or tool starts)"
        } else {
            ""
        };
        println!("  {mark} ~/{}{slash}{runs}", c.path);
    }
    Ok(())
}

fn home_cmd(home: &Path, task: &str, action: HomeCmd) -> Result<()> {
    let task = Task::open(home, task)?;
    let pick = |paths: Vec<String>, all: bool| -> Result<Vec<String>> {
        let paths: Vec<String> = paths
            .into_iter()
            .map(|p| p.trim_start_matches("~/").trim_end_matches('/').to_owned())
            .collect();
        if all {
            return Ok(task.home_changes()?.into_iter().map(|c| c.path).collect());
        }
        if paths.is_empty() {
            bail!("name the paths (as listed, like ~/.zshrc), or --all");
        }
        Ok(paths)
    };
    match action {
        HomeCmd::List => {
            if task.home_changes()?.is_empty() {
                println!("nothing written in your home");
            }
            print_home(&task)?;
        }
        HomeCmd::Apply { paths, all } => {
            let applied = task.apply_home(&pick(paths, all)?)?;
            if applied.paths.is_empty() {
                println!("nothing to apply at those paths");
            }
            for path in &applied.paths {
                println!("applied ~/{path}");
            }
            if let Some(kept) = applied.kept {
                println!("what they replaced is kept in {}", kept.display());
            }
        }
        HomeCmd::Discard { paths, all } => {
            let gone = task.discard_home(&pick(paths, all)?)?;
            println!("discarded {} changes", gone.len());
        }
        HomeCmd::Diff { path } => {
            let path = path.trim_start_matches("~/").trim_end_matches('/');
            match task.home_diff(path)? {
                TextDiff::Skipped { reason } => println!(
                    "~/{path}: {}",
                    match reason {
                        DiffSkip::Binary => "not text, so not shown",
                        DiffSkip::TooLarge => "too large to show",
                    }
                ),
                TextDiff::Lines { hunks, cut } => {
                    println!("--- ~/{path} (yours)\n+++ ~/{path} (the task's)");
                    for hunk in hunks {
                        println!("@@ line {} @@", hunk.before);
                        for line in hunk.lines {
                            let mark = match line.kind {
                                LineKind::Same => ' ',
                                LineKind::Added => '+',
                                LineKind::Removed => '-',
                            };
                            println!("{mark}{}", line.text);
                        }
                    }
                    if cut {
                        println!("(cut here: the change is longer)");
                    }
                }
            }
        }
        HomeCmd::Undo => {
            let undone = task.undo_home()?;
            for path in &undone.paths {
                println!("undid ~/{path}");
            }
            println!("the changes wait for review again");
        }
    }
    Ok(())
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
            "{} {}{} {}{} {}{}{}",
            paint::bold(&format!("{} file changes:", files.len())),
            coloured('+'),
            count(Change::Created),
            coloured('~'),
            count(Change::Modified),
            coloured('-'),
            count(Change::Deleted),
            if moves > 0 {
                format!(" {}{moves}", coloured('»'))
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
                    println!("  {} {from}{slash} → {}{slash}", coloured('»'), effect.path);
                }
                continue;
            }
            let outline = outlines.iter().find(|o| o.path == effect.path);
            match outline.map(|o| &o.outline) {
                Some(Outline::Skipped { reason }) if *reason != Skip::Binary => {
                    println!(
                        "  {} {}{slash} ({})",
                        coloured(mark),
                        effect.path,
                        why_not_outlined(*reason)
                    );
                }
                _ => println!("  {} {}{slash}", coloured(mark), effect.path),
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
fn describe_allowance(allowance: &kule_engine::Allowance) -> String {
    match allowance {
        kule_engine::Allowance::Connect { host, port } => format!("connections to {host}:{port}"),
        kule_engine::Allowance::File { path, rule } => format!("changes to {path} ({rule})"),
    }
}

fn group_cmd(home: &Path, action: GroupCmd) -> Result<()> {
    use kule_engine::GroupPolicy;
    let color = |name: Option<&str>| name.map(TaskColor::parse).transpose();
    match action {
        GroupCmd::List => {
            let groups = kule_engine::groups(home)?;
            if groups.is_empty() {
                println!("no groups: every task is judged by the policy for every task");
            }
            let tasks = kule_engine::arranged(home)?;
            for group in groups {
                let policy = match group.policy {
                    GroupPolicy::Default => "the policy for every task",
                    GroupPolicy::Balanced => "balanced",
                    GroupPolicy::Careful => "careful",
                    GroupPolicy::Own => "its own policy",
                };
                let mut line = format!("{}  {} · {policy}", group.id, group.name);
                if let Some(color) = group.color {
                    line += &format!(" ({})", color.name());
                }
                println!("{line}");
                for info in &tasks {
                    if Task::open(home, &info.id)?.group()? == Some(group.id) {
                        println!("    {}  {}", info.id, info.workspace.display());
                    }
                }
            }
        }
        GroupCmd::New { name, color: c } => {
            let group = kule_engine::create_group(home, &name, color(c.as_deref())?)?;
            println!("group {} made: {}", group.id, group.name);
        }
        GroupCmd::Rename { id, name } => {
            let group = find_group(home, id)?;
            kule_engine::update_group(home, id, &name, group.color, group.collapsed)?;
        }
        GroupCmd::Color { id, color: c } => {
            let group = find_group(home, id)?;
            kule_engine::update_group(
                home,
                id,
                &group.name,
                color(c.as_deref())?,
                group.collapsed,
            )?;
        }
        GroupCmd::Policy { id, choice, file } => {
            let Some(choice) = choice else {
                let group = find_group(home, id)?;
                let text = match group.policy {
                    GroupPolicy::Default => match policy_choice(home)? {
                        PolicyChoice::Balanced => DEFAULT_POLICY.to_owned(),
                        PolicyChoice::Careful => CAREFUL_POLICY.to_owned(),
                        PolicyChoice::Own => std::fs::read_to_string(home.join(USER_POLICY))?,
                    },
                    GroupPolicy::Balanced => DEFAULT_POLICY.to_owned(),
                    GroupPolicy::Careful => CAREFUL_POLICY.to_owned(),
                    GroupPolicy::Own => {
                        kule_engine::group_policy_text(home, id)?.unwrap_or_default()
                    }
                };
                print!("{text}");
                return Ok(());
            };
            let policy = match choice.as_str() {
                "default" => GroupPolicy::Default,
                "balanced" => GroupPolicy::Balanced,
                "careful" => GroupPolicy::Careful,
                "own" => GroupPolicy::Own,
                _ => bail!("{choice}: a group's policy is default, balanced, careful or own"),
            };
            let text = file
                .map(|path| {
                    std::fs::read_to_string(&path).with_context(|| path.display().to_string())
                })
                .transpose()?;
            kule_engine::set_group_policy(home, id, policy, text.as_deref())?;
            println!("group {id}'s tasks are judged by it from their next commit and run");
        }
        GroupCmd::Put { task, group } => Task::open(home, &task)?.set_group(group)?,
        GroupCmd::Delete { id } => {
            kule_engine::delete_group(home, id)?;
            println!("group {id} deleted; its tasks are in no group");
        }
    }
    Ok(())
}

fn find_group(home: &Path, id: u64) -> Result<kule_engine::Group> {
    kule_engine::groups(home)?
        .into_iter()
        .find(|g| g.id == id)
        .ok_or_else(|| anyhow!("no group {id}"))
}

fn rules_cmd(home: &Path, action: RulesCmd) -> Result<()> {
    match action {
        RulesCmd::List => {
            let rules = kule_engine::remembered::load(home)?;
            if rules.is_empty() {
                println!("nothing remembered: every question is asked");
            }
            for r in rules {
                let place = r
                    .workspace
                    .as_ref()
                    .map_or("every folder".to_owned(), |w| w.display().to_string());
                println!(
                    "{:>3}  {}  · {place}",
                    r.id,
                    describe_allowance(&r.allowance)
                );
            }
        }
        RulesCmd::Forget { id } => {
            if kule_engine::forget(home, id)? {
                println!("forgot {id}: asked again from now on");
            } else {
                println!("nothing remembered as {id}");
            }
        }
        RulesCmd::Allow {
            what,
            rule,
            folder,
            everywhere,
            yes,
        } => {
            let allowance = match what.rsplit_once(':') {
                Some((host, port)) if port.parse::<u16>().is_ok() && !host.contains('/') => {
                    kule_engine::Allowance::Connect {
                        host: host.to_owned(),
                        port: port.parse()?,
                    }
                }
                _ => kule_engine::Allowance::File { path: what, rule },
            };
            let folder = if everywhere {
                None
            } else {
                Some(match folder {
                    Some(f) => f,
                    None => std::env::current_dir()?,
                })
            };
            let impact = kule_engine::impact(home, &allowance, folder.as_deref())?;
            let place = folder
                .as_ref()
                .map_or("every folder".to_owned(), |f| f.display().to_string());
            println!("{} · {place}", describe_allowance(&allowance));
            println!(
                "would have let through so far: {} connections, {} changes, in {} tasks",
                impact.connections, impact.changes, impact.tasks
            );
            if yes {
                let kept = kule_engine::remember(home, allowance, folder.as_deref())?;
                println!("kept as {}", kept.id);
            } else {
                println!("not kept: rerun with --yes to keep it");
            }
        }
    }
    Ok(())
}

/// The folder a rule or secret holds for: the one named, the current one,
/// or every folder.
fn scope(folder: Option<PathBuf>, everywhere: bool) -> Result<Option<PathBuf>> {
    Ok(if everywhere {
        None
    } else {
        Some(match folder {
            Some(f) => f,
            None => std::env::current_dir()?,
        })
    })
}

fn secrets_cmd(home: &Path, action: SecretsCmd) -> Result<()> {
    match action {
        SecretsCmd::List => {
            let kept = kule_engine::secrets(home)?;
            if kept.is_empty() {
                println!("no secrets kept");
            }
            for s in kept {
                let place = s
                    .workspace
                    .as_ref()
                    .map_or("every folder".to_owned(), |w| w.display().to_string());
                println!("{}  → {}  · {place}", s.name, s.hosts.join(", "));
            }
        }
        SecretsCmd::Add {
            name,
            hosts,
            folder,
            everywhere,
        } => {
            let folder = scope(folder, everywhere)?;
            let value = read_secret(&name)?;
            let kept = kule_engine::add_secret(home, &name, &value, &hosts, folder.as_deref())?;
            println!(
                "kept {}: commands see a placeholder; the value goes only to {}",
                kept.name,
                kept.hosts.join(", ")
            );
        }
        SecretsCmd::Remove {
            name,
            folder,
            everywhere,
        } => {
            let folder = scope(folder, everywhere)?;
            if kule_engine::remove_secret(home, &name, folder.as_deref())? {
                println!("forgot {name}");
            } else {
                println!("no secret {name} kept there");
            }
        }
    }
    Ok(())
}

/// A secret's value: typed without echo at a terminal, or the first line
/// of standard input.
fn read_secret(name: &str) -> Result<String> {
    use std::io::{BufRead, IsTerminal};
    let stdin = std::io::stdin();
    let terminal = stdin.is_terminal();
    let echo_off = terminal.then(|| {
        eprint!("value of {name} (not shown): ");
        let _ = std::io::stderr().flush();
        Echo::off()
    });
    let mut line = String::new();
    stdin.lock().read_line(&mut line)?;
    drop(echo_off);
    if terminal {
        eprintln!();
    }
    let value = line.trim_end_matches(['\n', '\r']).to_owned();
    if value.is_empty() {
        bail!("no value given");
    }
    Ok(value)
}

/// The terminal's echo, turned off until dropped.
struct Echo(Option<libc::termios>);

impl Echo {
    fn off() -> Echo {
        // SAFETY: tcgetattr/tcsetattr on stdin with a zeroed struct they fill.
        unsafe {
            let mut term: libc::termios = std::mem::zeroed();
            if libc::tcgetattr(0, &mut term) != 0 {
                return Echo(None);
            }
            let saved = term;
            term.c_lflag &= !libc::ECHO;
            if libc::tcsetattr(0, libc::TCSANOW, &term) != 0 {
                return Echo(None);
            }
            Echo(Some(saved))
        }
    }
}

impl Drop for Echo {
    fn drop(&mut self) {
        if let Some(saved) = &self.0 {
            // SAFETY: restores what tcgetattr returned.
            unsafe {
                libc::tcsetattr(0, libc::TCSANOW, saved);
            }
        }
    }
}

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
/// A change's mark in its colour: added green, changed yellow, removed
/// red, moved cyan.
fn coloured(mark: char) -> String {
    let text = mark.to_string();
    match mark {
        '+' => paint::green(&text),
        '~' => paint::yellow(&text),
        '-' => paint::red(&text),
        '»' => paint::cyan(&text),
        _ => text,
    }
}

fn print_entity_changes(changes: &[EntityChange]) {
    const SHOWN: usize = 8;
    for change in changes.iter().take(SHOWN) {
        let mark = match change.edit {
            Edit::Added => '+',
            Edit::Modified => '~',
            Edit::Removed => '-',
        };
        let kind = change.kind.label();
        let mark = coloured(mark);
        match change.name.as_str() {
            "" => println!("      {mark} {kind}"),
            name => println!("      {mark} {kind} {}", paint::bold(name)),
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
