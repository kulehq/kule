//! Kule's desktop app: a window onto the Kule daemon. The daemon owns the
//! tasks and the agents' runs, so closing the window stops nothing; the app
//! starts the daemon when none is running.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod terminal;

use std::path::{Path, PathBuf};
use std::time::Duration;

use kule_daemon::{
    Call, Client, Commit, Notice, Question, Result, Review, RunEvent, TaskView, Timeline,
    default_home, spawn_daemon,
};
use serde::Serialize;
use serde::de::DeserializeOwned;
use tauri::{AppHandle, Emitter, State};
use tauri_plugin_dialog::DialogExt;

/// How the window reaches the daemon.
struct Kule {
    home: PathBuf,
    client: Client,
}

impl Kule {
    /// Makes a call, starting the daemon first if it went away.
    fn call<T: DeserializeOwned>(&self, call: Call) -> Result<T> {
        if !self.client.is_running() {
            self.start()?;
        }
        self.client.call(call)
    }

    fn start(&self) -> Result<()> {
        self.client
            .ensure_running(|| spawn_daemon(&kule_program(), &self.home))
    }
}

/// The `kule` program that runs the daemon: `KULE_BIN`, else the one next
/// to this app, else `kule` on the path.
fn kule_program() -> PathBuf {
    if let Some(bin) = std::env::var_os("KULE_BIN") {
        return PathBuf::from(bin);
    }
    std::env::current_exe()
        .ok()
        .and_then(|exe| Some(exe.parent()?.join("kule")))
        .filter(|beside| beside.is_file())
        .unwrap_or_else(|| PathBuf::from("kule"))
}

type K<'a> = State<'a, Kule>;

// Commands run off the main thread (`async`): a call can wait on a scan of
// a large copy, and the window must not freeze meanwhile.

#[tauri::command(async)]
fn agents(kule: K<'_>) -> Result<Vec<String>> {
    kule.call(Call::Agents)
}

#[tauri::command(async)]
fn tasks(kule: K<'_>) -> Result<Vec<TaskView>> {
    kule.call(Call::Tasks)
}

#[tauri::command(async)]
fn pick_workspace(app: AppHandle) -> Option<PathBuf> {
    app.dialog()
        .file()
        .set_title("Choose the workspace for a new task")
        .blocking_pick_folder()
        .and_then(|path| path.into_path().ok())
}

#[tauri::command(async)]
fn create_task(kule: K<'_>, workspace: PathBuf) -> Result<TaskView> {
    kule.call(Call::CreateTask { workspace })
}

#[tauri::command(async)]
fn review(kule: K<'_>, task: String, include_ignored: bool) -> Result<Review> {
    kule.call(Call::Review {
        task,
        include_ignored,
    })
}

#[tauri::command(async)]
fn commit(
    kule: K<'_>,
    task: String,
    approve: bool,
    include_ignored: bool,
    reviewed: Option<String>,
) -> Result<Commit> {
    kule.call(Call::Commit {
        task,
        approve,
        include_ignored,
        reviewed,
    })
}

#[tauri::command(async)]
fn rollback(kule: K<'_>, task: String) -> Result<usize> {
    kule.call(Call::Rollback { task })
}

#[tauri::command(async)]
fn undo(kule: K<'_>, task: String) -> Result<Option<u32>> {
    kule.call(Call::Undo { task })
}

#[tauri::command(async)]
fn timeline(kule: K<'_>, task: String) -> Result<Timeline> {
    kule.call(Call::Timeline { task })
}

#[tauri::command(async)]
fn remove_task(kule: K<'_>, task: String) -> Result<()> {
    kule.call(Call::RemoveTask { task })
}

#[tauri::command(async)]
fn open_terminal(kule: K<'_>, task: String) -> Result<()> {
    let copy: PathBuf = kule.call(Call::CopyOf { task })?;
    terminal::open(&copy)
}

#[tauri::command(async)]
fn questions(kule: K<'_>) -> Result<Vec<Question>> {
    kule.call(Call::Questions)
}

#[tauri::command(async)]
fn answer(kule: K<'_>, id: u64, approve: bool) -> Result<()> {
    kule.call(Call::Answer { id, approve })
}

#[tauri::command(async)]
fn set_approval_timeout(kule: K<'_>, task: String, minutes: u64) -> Result<TaskView> {
    kule.call(Call::SetApprovalTimeout { task, minutes })
}

#[tauri::command(async)]
fn start_run(kule: K<'_>, task: String, agent: String, prompt: String) -> Result<()> {
    kule.call(Call::StartRun {
        task,
        agent,
        prompt,
    })
}

/// The task's latest run events, for a window that opened after they came.
#[tauri::command(async)]
fn stream(kule: K<'_>, task: String) -> Result<Vec<RunEvent>> {
    kule.call(Call::Stream { task })
}

/// One event of a run, for the window's `run` listener.
#[derive(Clone, Serialize)]
struct RunMessage {
    task: String,
    event: RunEvent,
}

/// Passes the daemon's notices on to the window for as long as the app
/// runs, starting the daemon again if it goes away.
fn relay(app: AppHandle, home: &Path) {
    let kule = Kule {
        home: home.to_owned(),
        client: Client::new(home),
    };
    loop {
        if kule.start().is_ok() {
            let _ = kule.client.subscribe(|notice| match notice {
                Notice::Run { task, event } => {
                    let _ = app.emit("run", RunMessage { task, event });
                }
                Notice::Questions { waiting } => {
                    let _ = app.emit("questions", waiting);
                }
            });
        }
        std::thread::sleep(Duration::from_secs(1));
    }
}

fn main() {
    let home = match default_home() {
        Ok(home) => home,
        Err(e) => {
            eprintln!("kule-desktop: {e}");
            std::process::exit(2);
        }
    };
    let kule = Kule {
        home: home.clone(),
        client: Client::new(&home),
    };
    if let Err(e) = kule.start() {
        eprintln!("kule-desktop: {e}");
    }
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(move |app| {
            let handle = app.handle().clone();
            std::thread::spawn(move || relay(handle, &home));
            Ok(())
        })
        .manage(kule)
        .invoke_handler(tauri::generate_handler![
            agents,
            tasks,
            pick_workspace,
            create_task,
            review,
            commit,
            rollback,
            undo,
            timeline,
            remove_task,
            open_terminal,
            questions,
            answer,
            set_approval_timeout,
            start_run,
            stream,
        ])
        .run(tauri::generate_context!())
        .expect("the window could not start");
}
