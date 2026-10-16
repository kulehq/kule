//! Kule's desktop app: the window around [`app::App`].

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;

use std::path::PathBuf;
use std::sync::Arc;

use serde::Serialize;
use tauri::{AppHandle, Emitter, State};
use tauri_plugin_dialog::DialogExt;

use app::{App, Commit, Result, Review, RunEvent, TaskView, Timeline};

type Kule<'a> = State<'a, Arc<App>>;

// Commands run off the main thread (`async`): scanning a large copy takes a
// while, and the window must not freeze meanwhile.

#[tauri::command(async)]
fn agents(kule: Kule<'_>) -> Vec<&'static str> {
    kule.agents()
}

#[tauri::command(async)]
fn tasks(kule: Kule<'_>) -> Result<Vec<TaskView>> {
    kule.tasks()
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
fn create_task(kule: Kule<'_>, workspace: PathBuf) -> Result<TaskView> {
    kule.create(&workspace)
}

#[tauri::command(async)]
fn review(kule: Kule<'_>, task: String) -> Result<Review> {
    kule.review(&task)
}

#[tauri::command(async)]
fn commit(kule: Kule<'_>, task: String, approve: bool) -> Result<Commit> {
    kule.commit(&task, approve)
}

#[tauri::command(async)]
fn rollback(kule: Kule<'_>, task: String) -> Result<usize> {
    kule.rollback(&task)
}

#[tauri::command(async)]
fn undo(kule: Kule<'_>, task: String) -> Result<Option<u32>> {
    kule.undo(&task)
}

#[tauri::command(async)]
fn timeline(kule: Kule<'_>, task: String) -> Result<Timeline> {
    kule.timeline(&task)
}

#[tauri::command(async)]
fn remove_task(kule: Kule<'_>, task: String) -> Result<()> {
    kule.remove(&task)
}

/// One event of a run, for the window's `run` listener.
#[derive(Clone, Serialize)]
struct RunMessage {
    task: String,
    event: RunEvent,
}

#[tauri::command(async)]
fn start_run(
    app: AppHandle,
    kule: Kule<'_>,
    task: String,
    agent: String,
    prompt: String,
) -> Result<()> {
    let id = task.clone();
    kule.start_run(&task, &agent, &prompt, move |event| {
        let _ = app.emit(
            "run",
            RunMessage {
                task: id.clone(),
                event,
            },
        );
    })
}

fn main() {
    let home = match App::default_home() {
        Ok(home) => home,
        Err(e) => {
            eprintln!("kule-desktop: {e}");
            std::process::exit(2);
        }
    };
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(Arc::new(App::new(home)))
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
            start_run,
        ])
        .run(tauri::generate_context!())
        .expect("the window could not start");
}
