//! Kule's desktop app: a window onto the Kule daemon. The daemon owns the
//! tasks and the agents' runs, so closing the window stops nothing; the app
//! starts the daemon when none is running.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod menu;
mod pty;
mod shell_path;
mod terminal;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use kule_daemon::layout::Layout;
use kule_daemon::settings_profile::{Part, ProfileFile};
use kule_daemon::shortcuts::Shortcuts;
use kule_daemon::themes::{ThemeFile, Themes};
use kule_daemon::{
    AgentProfile, Alert, Alerts, Call, Client, Commit, DailyBudget, Hello, Money, Notice, PROTOCOL,
    PolicyChoice, Question, Result, Review, RunEvent, Sequenced, TaskColor, TaskView, Timeline,
    default_home,
};
use serde::Serialize;
use serde::de::DeserializeOwned;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_notification::NotificationExt;

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

// Agent profiles: passed through as the daemon says them.

#[tauri::command(async)]
fn agent_list(kule: K<'_>) -> Result<serde_json::Value> {
    kule.call(Call::AgentList)
}

#[tauri::command(async)]
fn agent_registry(kule: K<'_>) -> Result<serde_json::Value> {
    kule.call(Call::AgentRegistry)
}

#[tauri::command(async)]
fn save_agent(kule: K<'_>, profile: AgentProfile) -> Result<()> {
    kule.call(Call::SaveAgent { profile })
}

#[tauri::command(async)]
fn remove_agent(kule: K<'_>, name: String) -> Result<bool> {
    kule.call(Call::RemoveAgent { name })
}

#[tauri::command(async)]
fn approve_agent(kule: K<'_>, name: String, digest: String) -> Result<()> {
    kule.call(Call::ApproveAgent { name, digest })
}

#[tauri::command(async)]
fn revoke_agent(kule: K<'_>, name: String) -> Result<bool> {
    kule.call(Call::RevokeAgent { name })
}

/// Runs a draft once in an empty folder; takes up to two minutes.
#[tauri::command(async)]
fn try_agent(kule: K<'_>, profile: AgentProfile) -> Result<serde_json::Value> {
    kule.call(Call::TryAgent { profile })
}

#[tauri::command(async)]
fn tasks(kule: K<'_>) -> Result<Vec<TaskView>> {
    kule.call(Call::Tasks)
}

#[tauri::command(async)]
fn pick_workspace(app: AppHandle) -> Option<PathBuf> {
    app.dialog()
        .file()
        .set_title("Choose the folder for a new task")
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

/// What a changed picture was and is, passed through as the daemon says it.
#[tauri::command(async)]
fn preview(kule: K<'_>, task: String, path: String) -> Result<serde_json::Value> {
    kule.call(Call::Preview { task, path })
}

#[tauri::command(async)]
fn commit(
    kule: K<'_>,
    task: String,
    approve: bool,
    include_ignored: bool,
    reviewed: Option<String>,
    remember: Option<Vec<String>>,
) -> Result<Commit> {
    kule.call(Call::Commit {
        task,
        approve,
        include_ignored,
        reviewed,
        remember: remember.unwrap_or_default(),
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

/// A shell in the task's copy, inside the window; its id. Refused when
/// what the task ran changed what git would run there (see `TerminalDir`).
#[tauri::command(async)]
fn terminal_open(
    app: AppHandle,
    kule: K<'_>,
    terms: State<'_, pty::Terminals>,
    task: String,
    cols: u16,
    rows: u16,
) -> Result<pty::Opened> {
    let copy: PathBuf = kule.call(Call::TerminalDir { task: task.clone() })?;
    let id = terms.open(app, &copy, &task, cols, rows)?;
    Ok(pty::Opened {
        id,
        shell: pty::shell_name(),
    })
}

/// Opens an http or https address in the person's browser: a link clicked
/// in the terminal. Nothing else, and never through a shell.
#[tauri::command]
fn open_url(url: String) -> Result<()> {
    let url = web_url(&url)?;
    let opener = if cfg!(target_os = "macos") {
        "open"
    } else if cfg!(windows) {
        "explorer"
    } else {
        "xdg-open"
    };
    std::process::Command::new(opener)
        .arg(url)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("{opener}: {e}"))
}

/// `url` when it is an http or https address a browser can take as it is.
fn web_url(url: &str) -> Result<&str> {
    let lower = url.to_ascii_lowercase();
    let scheme_ok = lower.starts_with("http://") || lower.starts_with("https://");
    let rest = url.split_once("://").map_or("", |(_, r)| r);
    let clean = !url.chars().any(|c| c.is_control() || c.is_whitespace());
    if scheme_ok && clean && url.len() <= 2048 && !rest.is_empty() && !rest.starts_with('/') {
        Ok(url)
    } else {
        Err("only an http or https address is opened".to_owned())
    }
}

#[tauri::command]
fn terminal_write(terms: State<'_, pty::Terminals>, id: u64, data: String) -> Result<()> {
    terms.write(id, &data)
}

#[tauri::command]
fn terminal_resize(terms: State<'_, pty::Terminals>, id: u64, cols: u16, rows: u16) -> Result<()> {
    terms.resize(id, cols, rows)
}

#[tauri::command(async)]
fn terminal_close(terms: State<'_, pty::Terminals>, id: u64) {
    terms.close(id);
}

#[tauri::command(async)]
fn questions(kule: K<'_>) -> Result<Vec<Question>> {
    kule.call(Call::Questions)
}

#[tauri::command(async)]
fn answer(kule: K<'_>, id: u64, approve: bool, remember: Option<bool>) -> Result<()> {
    kule.call(Call::Answer {
        id,
        approve,
        remember: remember.unwrap_or(false),
    })
}

// Remembered approvals: passed through as the daemon says them.

#[tauri::command(async)]
fn rules(kule: K<'_>) -> Result<serde_json::Value> {
    kule.call(Call::Rules)
}

#[tauri::command(async)]
fn forget_rule(kule: K<'_>, id: u64) -> Result<bool> {
    kule.call(Call::ForgetRule { id })
}

// Secrets: the window sends a value once, to keep; it never gets one back.

#[tauri::command(async)]
fn secrets(kule: K<'_>) -> Result<serde_json::Value> {
    kule.call(Call::Secrets)
}

#[tauri::command(async)]
fn add_secret(
    kule: K<'_>,
    name: String,
    value: String,
    hosts: Vec<String>,
    workspace: Option<PathBuf>,
) -> Result<serde_json::Value> {
    kule.call(Call::AddSecret {
        name,
        value: kule_daemon::Redacted(value),
        hosts,
        workspace,
    })
}

#[tauri::command(async)]
fn remove_secret(kule: K<'_>, name: String, workspace: Option<PathBuf>) -> Result<bool> {
    kule.call(Call::RemoveSecret { name, workspace })
}

#[tauri::command(async)]
fn apply_home(kule: K<'_>, task: String, paths: Vec<String>) -> Result<serde_json::Value> {
    kule.call(Call::ApplyHome { task, paths })
}

#[tauri::command(async)]
fn discard_home(kule: K<'_>, task: String, paths: Vec<String>) -> Result<Vec<String>> {
    kule.call(Call::DiscardHome { task, paths })
}

#[tauri::command(async)]
fn home_diff(kule: K<'_>, task: String, path: String) -> Result<serde_json::Value> {
    kule.call(Call::HomeDiff { task, path })
}

#[tauri::command(async)]
fn undo_home(kule: K<'_>, task: String) -> Result<serde_json::Value> {
    kule.call(Call::UndoHome { task })
}

#[tauri::command(async)]
fn rule_impact(
    kule: K<'_>,
    allowance: serde_json::Value,
    workspace: Option<PathBuf>,
) -> Result<serde_json::Value> {
    let allowance = serde_json::from_value(allowance).map_err(|e| e.to_string())?;
    kule.call(Call::RuleImpact {
        allowance,
        workspace,
    })
}

#[tauri::command(async)]
fn set_approval_timeout(kule: K<'_>, task: String, minutes: u64) -> Result<TaskView> {
    kule.call(Call::SetApprovalTimeout { task, minutes })
}

#[tauri::command(async)]
fn set_budget(kule: K<'_>, task: String, budget: Option<Money>) -> Result<TaskView> {
    kule.call(Call::SetBudget { task, budget })
}

#[tauri::command(async)]
fn set_label(kule: K<'_>, task: String, label: Option<String>) -> Result<TaskView> {
    kule.call(Call::SetLabel { task, label })
}

#[tauri::command(async)]
fn set_color(kule: K<'_>, task: String, color: Option<TaskColor>) -> Result<TaskView> {
    kule.call(Call::SetColor { task, color })
}

#[tauri::command(async)]
fn set_order(kule: K<'_>, tasks: Vec<String>) -> Result<()> {
    kule.call(Call::SetOrder { tasks })
}

#[tauri::command(async)]
fn policy(kule: K<'_>) -> Result<PolicyChoice> {
    kule.call(Call::Policy)
}

#[tauri::command(async)]
fn choose_policy(kule: K<'_>, choice: PolicyChoice) -> Result<PolicyChoice> {
    kule.call(Call::ChoosePolicy { choice })
}

/// Writes a profile with `parts` where the person says; the path, or `None`
/// when they said nowhere.
#[tauri::command(async)]
fn export_profile(app: AppHandle, kule: K<'_>, parts: Vec<Part>) -> Result<Option<String>> {
    let text: String = kule.call(Call::ExportProfile { parts })?;
    let Some(path) = app
        .dialog()
        .file()
        .set_title("Save your Kule profile")
        .set_file_name("kule.kule-profile.json")
        .add_filter("Kule profile", &["json"])
        .blocking_save_file()
        .and_then(|p| p.into_path().ok())
    else {
        return Ok(None);
    };
    std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(Some(path.display().to_string()))
}

/// A profile file the person picks: its text, to apply later, and what is
/// in it, checked; `None` when they picked none.
#[derive(Serialize)]
struct PickedProfile {
    text: String,
    file: ProfileFile,
}

#[tauri::command(async)]
fn import_profile(app: AppHandle, kule: K<'_>) -> Result<Option<PickedProfile>> {
    let Some(path) = app
        .dialog()
        .file()
        .set_title("Choose a Kule profile")
        .add_filter("Kule profile", &["json"])
        .blocking_pick_file()
        .and_then(|p| p.into_path().ok())
    else {
        return Ok(None);
    };
    let len = std::fs::metadata(&path)
        .map_err(|e| format!("{}: {e}", path.display()))?
        .len();
    if len > kule_daemon::settings_profile::MAX_FILE {
        return Err(format!("{}: a profile is at most 256 KiB", path.display()));
    }
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let file = kule.call(Call::ReadProfile { text: text.clone() })?;
    Ok(Some(PickedProfile { text, file }))
}

#[tauri::command(async)]
fn apply_profile(kule: K<'_>, text: String, parts: Vec<Part>) -> Result<Vec<String>> {
    kule.call(Call::ApplyProfile { text, parts })
}

#[tauri::command(async)]
fn layout(kule: K<'_>) -> Result<Option<Layout>> {
    kule.call(Call::Layout)
}

#[tauri::command(async)]
fn set_layout(kule: K<'_>, layout: Option<Layout>) -> Result<()> {
    kule.call(Call::SetLayout { layout })
}

#[tauri::command(async)]
fn list_dir(kule: K<'_>, task: String, path: String) -> Result<serde_json::Value> {
    kule.call(Call::ListDir { task, path })
}

#[tauri::command(async)]
fn read_file(kule: K<'_>, task: String, path: String) -> Result<serde_json::Value> {
    kule.call(Call::ReadFile { task, path })
}

/// Puts the person's shortcuts next to the menu's items: command id →
/// accelerator (`CmdOrCtrl+Shift+K`).
#[tauri::command]
fn set_menu_keys(app: AppHandle, keys: HashMap<String, String>) -> Result<()> {
    if keys.len() > 256 || keys.iter().any(|(k, v)| k.len() > 64 || v.len() > 64) {
        return Err("too many or too long shortcuts for the menu".to_owned());
    }
    let bar = menu::build(&app, &keys).map_err(|e| e.to_string())?;
    app.set_menu(bar).map(|_| ()).map_err(|e| e.to_string())
}

#[tauri::command(async)]
fn themes(kule: K<'_>) -> Result<Themes> {
    kule.call(Call::Themes)
}

#[tauri::command(async)]
fn set_themes(kule: K<'_>, themes: Themes) -> Result<Themes> {
    kule.call(Call::SetThemes { themes })
}

/// A theme file the person picks, read and checked; `None` when they
/// picked none.
#[tauri::command(async)]
fn import_theme(app: AppHandle) -> Result<Option<ThemeFile>> {
    let Some(path) = app
        .dialog()
        .file()
        .set_title("Choose a Kule theme")
        .add_filter("Kule theme", &["json"])
        .blocking_pick_file()
        .and_then(|p| p.into_path().ok())
    else {
        return Ok(None);
    };
    let len = std::fs::metadata(&path)
        .map_err(|e| format!("{}: {e}", path.display()))?
        .len();
    if len > kule_daemon::themes::MAX_FILE {
        return Err(format!(
            "{}: a theme file is at most {} KiB",
            path.display(),
            kule_daemon::themes::MAX_FILE / 1024
        ));
    }
    let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    kule_daemon::themes::read_file(&bytes).map(Some)
}

/// Writes `theme` where the person says; the path, or `None` when they
/// said nowhere.
#[tauri::command(async)]
fn export_theme(app: AppHandle, theme: ThemeFile) -> Result<Option<String>> {
    let text = kule_daemon::themes::to_file(&theme)?;
    let slug: String = theme
        .name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let Some(path) = app
        .dialog()
        .file()
        .set_title("Save the theme")
        .set_file_name(format!("{}.kule-theme.json", slug.trim_matches('-')))
        .add_filter("Kule theme", &["json"])
        .blocking_save_file()
        .and_then(|p| p.into_path().ok())
    else {
        return Ok(None);
    };
    std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(Some(path.display().to_string()))
}

#[tauri::command(async)]
fn shortcuts(kule: K<'_>) -> Result<Shortcuts> {
    kule.call(Call::Shortcuts)
}

#[tauri::command(async)]
fn set_shortcuts(kule: K<'_>, shortcuts: Shortcuts) -> Result<Shortcuts> {
    kule.call(Call::SetShortcuts { shortcuts })
}

#[tauri::command(async)]
fn daily_budget(kule: K<'_>) -> Result<DailyBudget> {
    kule.call(Call::DailyBudget)
}

#[tauri::command(async)]
fn set_daily_budget(kule: K<'_>, budget: Option<Money>) -> Result<DailyBudget> {
    kule.call(Call::SetDailyBudget { budget })
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
/// runs, starting the daemon again if it goes away. While the window is
/// not in front, what needs the person also goes to the system's
/// notifications.
fn relay(app: AppHandle, home: &Path) {
    let kule = Kule {
        home: home.to_owned(),
        client: Client::new(home),
    };
    let mut alerts = Alerts::new();
    let tell = |app: &AppHandle, told: Vec<Alert>| {
        let in_front = app
            .get_webview_window("main")
            .and_then(|w| w.is_focused().ok())
            .unwrap_or(false);
        if in_front {
            return;
        }
        for alert in told {
            let _ = app
                .notification()
                .builder()
                .title(alert.title)
                .body(alert.body)
                .show();
        }
    };
    loop {
        if kule.start().is_ok() {
            // Connected (again): the window reloads what it shows, since it
            // may have missed notices while the daemon was away.
            let _ = app.emit("daemon", ());
            if let Ok(views) = kule.client.call::<Vec<TaskView>>(Call::Tasks) {
                tell(&app, alerts.tasks(&views));
            }
            let _ = kule.client.subscribe(|notice| match notice {
                Notice::Run { task, seq, event } => {
                    if let RunEvent::Finished { error, .. } = &event {
                        tell(&app, vec![alerts.finished(&task, error.as_deref())]);
                    }
                    let _ = app.emit("run", RunMessage { task, seq, event });
                }
                Notice::Questions { waiting } => {
                    tell(&app, alerts.questions(&waiting));
                    let _ = app.emit("questions", waiting);
                }
                Notice::Tasks => {
                    if let Ok(views) = kule.client.call::<Vec<TaskView>>(Call::Tasks) {
                        tell(&app, alerts.tasks(&views));
                    }
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
    // Before any thread: the daemon this starts, its agents and the
    // terminals it opens find what the person's shell finds.
    if let Some(path) = shell_path::for_app() {
        // SAFETY: nothing else runs yet that could read the environment.
        unsafe { std::env::set_var("PATH", path) };
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
        .plugin(tauri_plugin_notification::init())
        .setup(move |app| {
            let handle = app.handle().clone();
            // Without shortcuts until the window sends the person's.
            app.set_menu(menu::build(&handle, &HashMap::new())?)?;
            std::thread::spawn(move || relay(handle, &home));
            Ok(())
        })
        .on_menu_event(|app, event| {
            if let Some(command) = event.id().as_ref().strip_prefix(menu::PREFIX) {
                let _ = app.emit("menu", command.to_owned());
            }
        })
        .manage(kule)
        .manage(pty::Terminals::default())
        .invoke_handler(tauri::generate_handler![
            agents,
            agent_list,
            agent_registry,
            save_agent,
            remove_agent,
            approve_agent,
            revoke_agent,
            try_agent,
            tasks,
            pick_workspace,
            create_task,
            review,
            preview,
            commit,
            rollback,
            undo,
            timeline,
            remove_task,
            open_terminal,
            questions,
            answer,
            rules,
            forget_rule,
            secrets,
            add_secret,
            remove_secret,
            apply_home,
            discard_home,
            home_diff,
            undo_home,
            rule_impact,
            set_approval_timeout,
            set_budget,
            set_label,
            set_color,
            set_order,
            policy,
            choose_policy,
            set_menu_keys,
            terminal_open,
            open_url,
            terminal_write,
            terminal_resize,
            terminal_close,
            layout,
            set_layout,
            list_dir,
            read_file,
            export_profile,
            import_profile,
            apply_profile,
            themes,
            set_themes,
            import_theme,
            export_theme,
            shortcuts,
            set_shortcuts,
            daily_budget,
            set_daily_budget,
            start_run,
            stream,
        ])
        .run(tauri::generate_context!())
        .expect("the window could not start");
}

#[cfg(test)]
mod tests {
    use super::web_url;

    #[test]
    fn only_web_addresses_are_opened() {
        for ok in [
            "https://kule.dev",
            "http://localhost:3000/a?b=c",
            "HTTPS://Example.com/x",
        ] {
            assert!(web_url(ok).is_ok(), "{ok}");
        }
        for bad in [
            "file:///etc/passwd",
            "javascript:alert(1)",
            "ssh://host",
            "https://",
            "https:///etc",
            "https://a b",
            "https://a\nb",
            "-a https://x",
            "",
        ] {
            assert!(web_url(bad).is_err(), "{bad}");
        }
        assert!(web_url(&format!("https://{}", "a".repeat(2050))).is_err());
    }
}
