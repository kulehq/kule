//! Kule's desktop app: a window onto the Kule daemon. The daemon owns the
//! tasks and the agents' runs, so closing the window stops nothing; the app
//! starts the daemon when none is running.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod terminal;

use std::path::{Path, PathBuf};
use std::time::Duration;

use kule_daemon::{
    Call, Client, Commit, Hello, Notice, PROTOCOL, Question, Result, Review, RunEvent, Sequenced,
    TaskView, Timeline, default_home,
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

    /// Starts the daemon unless one of this protocol answers. The app runs
    /// its own binary as the daemon (`kule-desktop daemon`), so the two never
    /// come from different builds.
    fn start(&self) -> Result<()> {
        self.client.ensure_running(|| spawn_self(&self.home))?;
        match self.client.call::<Hello>(Call::Hello) {
            Ok(hello) if hello.protocol == PROTOCOL => Ok(()),
            Ok(hello) => Err(format!(
                "the Kule daemon speaks protocol {} ({}), this app {PROTOCOL}",
                hello.protocol, hello.version
            )),
            Err(e) => Err(e),
        }
    }
}

/// Starts this program as the daemon for `home`, detached from the app so it
/// outlives it; its output goes to `kuled.log` in `home`.
fn spawn_self(home: &Path) -> std::io::Result<()> {
    use std::os::unix::process::CommandExt;
    std::fs::create_dir_all(home)?;
    let log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(home.join("kuled.log"))?;
    let mut command = std::process::Command::new(std::env::current_exe()?);
    command
        .arg("daemon")
        .env("KULE_HOME", home)
        .stdin(std::process::Stdio::null())
        .stdout(log.try_clone()?)
        .stderr(log);
    // A session of its own: no terminal to be reached through, and no signal
    // meant for the app's.
    // SAFETY: setsid is async-signal-safe and touches only the child.
    unsafe {
        command.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
    command.spawn().map(|_| ())
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
    let copy: PathBuf = kule.call(Call::TerminalDir { task })?;
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
fn stream(kule: K<'_>, task: String) -> Result<Vec<Sequenced>> {
    kule.call(Call::Stream { task })
}

/// One event of a run, for the window's `run` listener.
#[derive(Clone, Serialize)]
struct RunMessage {
    task: String,
    seq: u64,
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
            // Connected (again): the window reloads what it shows, since it
            // may have missed notices while the daemon was away.
            let _ = app.emit("daemon", ());
            let _ = kule.client.subscribe(|notice| match notice {
                Notice::Run { task, seq, event } => {
                    let _ = app.emit("run", RunMessage { task, seq, event });
                }
                Notice::Questions { waiting } => {
                    let _ = app.emit("questions", waiting);
                }
                Notice::Tasks => {
                    let _ = app.emit("tasks", ());
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
    if std::env::args().nth(1).as_deref() == Some("daemon") {
        eprintln!("kule-desktop: daemon for {}", home.display());
        if let Err(e) = kule_daemon::run(&home, Duration::from_secs(600)) {
            eprintln!("kule-desktop: daemon: {e}");
            std::process::exit(2);
        }
        return;
    }
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
