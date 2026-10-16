use std::path::PathBuf;
use std::process::{Command, ExitCode};

use anyhow::{Context, Result, anyhow};
use clap::{Parser, Subcommand};
use kule_engine::{Change, EffectSet, Error, Kind, Task};

/// Kule engine (M0): run commands in a copy-on-write copy of a workspace,
/// then commit or roll back what they did.
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
    /// Print the path of a task's copy.
    Path { task: String },
    /// Run a command inside the task's copy, then show what it changed.
    /// M0 has no sandbox: the command can still write outside the copy.
    Exec {
        task: String,
        #[arg(last = true, required = true)]
        command: Vec<String>,
    },
    /// Show the task's uncommitted changes.
    Status {
        task: String,
        #[arg(long)]
        json: bool,
    },
    /// Apply the task's changes to the workspace, keeping a checkpoint.
    Commit { task: String },
    /// Throw the task's uncommitted changes away.
    Rollback { task: String },
    /// Revert the task's newest commit in the workspace and the copy.
    Undo { task: String },
    /// Delete a task and its copy. The workspace is not touched.
    Rm { task: String },
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
        Cmd::Path { task } => println!("{}", Task::open(&home, &task)?.copy().display()),
        Cmd::Exec { task, command } => {
            let task = Task::open(&home, &task)?;
            let status = Command::new(&command[0])
                .args(&command[1..])
                .current_dir(task.copy())
                .env("KULE_TASK", task.id())
                .status()
                .with_context(|| format!("could not run {}", command[0]))?;
            print_set(&task.effects()?);
            return Ok(match status.code() {
                Some(0) => ExitCode::SUCCESS,
                Some(code) => ExitCode::from(code.clamp(1, 255) as u8),
                None => ExitCode::FAILURE,
            });
        }
        Cmd::Status { task, json } => {
            let set = Task::open(&home, &task)?.effects()?;
            if json {
                println!("{}", serde_json::to_string_pretty(&set)?);
            } else {
                print_set(&set);
            }
        }
        Cmd::Commit { task } => match Task::open(&home, &task)?.commit()? {
            Some(cp) => println!(
                "committed {} changes · checkpoint {}",
                cp.effects.len(),
                cp.id
            ),
            None => println!("nothing to commit"),
        },
        Cmd::Rollback { task } => {
            let set = Task::open(&home, &task)?.rollback()?;
            println!("rolled back {} changes", set.effects.len());
        }
        Cmd::Undo { task } => match Task::open(&home, &task)?.undo()? {
            Some(cp) => println!("undid checkpoint {} ({} changes)", cp.id, cp.effects.len()),
            None => println!("nothing to undo"),
        },
        Cmd::Rm { task } => {
            Task::open(&home, &task)?.remove()?;
            println!("removed task {task}");
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn print_set(set: &EffectSet) {
    const SHOWN: usize = 40;
    if set.is_empty() {
        println!("no changes");
        return;
    }
    println!(
        "{} changes: +{} ~{} -{}",
        set.effects.len(),
        set.count(Change::Created),
        set.count(Change::Modified),
        set.count(Change::Deleted)
    );
    for effect in set.effects.iter().take(SHOWN) {
        let mark = match effect.change {
            Change::Created => '+',
            Change::Modified => '~',
            Change::Deleted => '-',
        };
        let slash = if effect.kind == Kind::Dir { "/" } else { "" };
        println!("  {mark} {}{slash}", effect.path);
    }
    if set.effects.len() > SHOWN {
        println!(
            "  … {} more (kule status --json)",
            set.effects.len() - SHOWN
        );
    }
}
