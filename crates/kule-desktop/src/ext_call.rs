//! What an extension asks Kule for, decided here and not in the window.
//!
//! The window's host passes each request on with the id of the extension
//! whose frame sent it (the frame's port, never what the frame says). This
//! looks the extension up again, refuses it unless it is on and approved as
//! it is, and refuses any call its approved permissions do not cover. The
//! list of calls is closed: what is not in [`needs`] does not exist.

use std::collections::HashMap;
use std::io::Write;
use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use kule_daemon::{Call, Result, TaskView};
use kule_ext::{Permission, Store};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tauri::{AppHandle, Manager, State};
use tauri_plugin_notification::NotificationExt;

use crate::{K, ext_net};

/// What a call needs: None for one that does not exist, Some(None) for one
/// every running extension may make (its own settings).
pub fn needs(method: &str) -> Option<Option<Permission>> {
    Some(Some(match method {
        "config.get" => return Some(None),
        "tasks.list" | "tasks.get" => Permission::TasksRead,
        "review.get" => Permission::ReviewRead,
        "files.list" | "files.read" => Permission::FilesRead,
        "storage.get" | "storage.set" => Permission::Storage,
        "ui.notify" => Permission::Notify,
        // The notes live in the window; it asks here first all the same.
        "review.annotate" => Permission::ReviewAnnotate,
        "net.fetch" => Permission::Net,
        _ => return None,
    }))
}

/// What a running extension was given.
#[derive(Debug, Clone)]
pub struct Granted {
    pub permissions: Vec<Permission>,
    pub hosts: Vec<String>,
    pub name: String,
}

/// What `ext` was given, if it may run now: on, and approved as it is.
/// Reading this opens the extension's package, so the window keeps it
/// (see [`Grants`]) until the extensions change.
pub fn granted(home: &Path, ext: &str, person: Option<&Path>) -> Result<Granted> {
    let listed = Store::new(home).get(ext).map_err(|e| e.to_string())?;
    if let Some(why) = &listed.held {
        return Err(format!("extension {ext} is held: {why}"));
    }
    // A folder an agent could have rewritten since it was installed.
    if let Some(dir) = &listed.dev
        && let Some(root) =
            kule_ext::under_any(dir, &kule_daemon::extensions::agent_dirs(home, person)?)
    {
        return Err(format!(
            "extension {ext} is held: its folder is under {}, where an agent can write",
            root.display()
        ));
    }
    if !listed.enabled {
        return Err(format!("extension {ext} is turned off"));
    }
    let approval = listed
        .approved
        .ok_or_else(|| format!("extension {ext} is not approved"))?;
    Ok(Granted {
        permissions: approval.permissions,
        hosts: approval.hosts,
        name: listed.manifest.map(|m| m.name).unwrap_or_default(),
    })
}

/// Whether what was given covers the call `method`.
pub fn permits(granted: &Granted, method: &str) -> Result<()> {
    match needs(method) {
        None => Err(format!("there is no call {method}")),
        Some(Some(need)) if !granted.permissions.contains(&need) => Err(format!(
            "{method} needs the {} permission, which {} was not given",
            need.as_str(),
            granted.name
        )),
        Some(_) => Ok(()),
    }
}

/// Whether `ext` may make the call `method` now.
#[cfg(test)]
fn allowed(home: &Path, ext: &str, method: &str) -> Result<Granted> {
    let g = granted(home, ext, None)?;
    permits(&g, method)?;
    Ok(g)
}

/// What each running extension was given, read once rather than on every
/// call (each read opens the package, up to 40 MiB inflated). Dropped
/// whenever the extensions change.
#[derive(Default)]
pub struct Grants(Mutex<HashMap<String, Granted>>);

impl Grants {
    pub fn get(&self, home: &Path, ext: &str) -> Result<Granted> {
        if let Some(g) = self.0.lock().unwrap().get(ext) {
            return Ok(g.clone());
        }
        let person = std::env::var_os("HOME").map(std::path::PathBuf::from);
        let g = granted(home, ext, person.as_deref())?;
        self.0.lock().unwrap().insert(ext.to_owned(), g.clone());
        Ok(g)
    }

    pub fn forget(&self) {
        self.0.lock().unwrap().clear();
    }
}

/// How often an extension may do something that reaches outside.
const LIMITS: [(&str, usize, Duration); 2] = [
    ("ui.notify", 5, Duration::from_secs(60)),
    ("net.fetch", 60, Duration::from_secs(60)),
];

#[derive(Default)]
pub struct Limits(Mutex<HashMap<(String, &'static str), Vec<Instant>>>);

impl Limits {
    fn take(&self, ext: &str, method: &str) -> Result<()> {
        let Some((name, most, within)) = LIMITS.iter().find(|(m, _, _)| *m == method) else {
            return Ok(());
        };
        let mut all = self.0.lock().unwrap();
        let times = all.entry((ext.to_owned(), *name)).or_default();
        let now = Instant::now();
        times.retain(|t| now.duration_since(*t) < *within);
        if times.len() >= *most {
            return Err(format!(
                "{method} is limited to {most} a minute; try again shortly"
            ));
        }
        times.push(now);
        Ok(())
    }
}

#[derive(Deserialize)]
struct TaskParam {
    task: String,
}

#[derive(Deserialize)]
struct FileParams {
    task: String,
    #[serde(default)]
    path: String,
}

#[derive(Deserialize)]
struct StorageParams {
    value: Value,
}

#[derive(Deserialize)]
struct NotifyParams {
    title: String,
    #[serde(default)]
    body: String,
}

#[derive(Deserialize)]
struct FetchParams {
    url: String,
    #[serde(default)]
    init: Option<ext_net::Init>,
}

/// A task as an extension sees it: less than the window does (no budget,
/// spending, overlaps or home changes).
#[derive(Serialize)]
struct ExtTask {
    id: String,
    label: Option<String>,
    workspace: String,
    created_unix: u64,
    changes: usize,
    verdict: Option<Value>,
    running: bool,
    waiting: usize,
}

impl From<TaskView> for ExtTask {
    fn from(t: TaskView) -> ExtTask {
        ExtTask {
            id: t.id,
            label: t.label,
            workspace: t.workspace.display().to_string(),
            created_unix: t.created_unix,
            changes: t.changes,
            verdict: t.verdict.map(|v| json!(v)),
            running: t.running,
            waiting: t.waiting,
        }
    }
}

fn params<T: for<'de> Deserialize<'de>>(method: &str, value: Value) -> Result<T> {
    serde_json::from_value(value).map_err(|e| format!("{method}: {e}"))
}

#[tauri::command(async)]
pub fn ext_call(
    app: AppHandle,
    kule: K<'_>,
    limits: State<'_, Limits>,
    grants: State<'_, Grants>,
    ext: String,
    method: String,
    params: Value,
) -> Result<Value> {
    let g = grants.get(&kule.home, &ext)?;
    permits(&g, &method)?;
    let Granted { hosts, name, .. } = g;
    limits.take(&ext, &method)?;
    let p = params;
    match method.as_str() {
        "config.get" => {
            let config: HashMap<String, Value> = kule.call(Call::ExtensionConfig { id: ext })?;
            Ok(json(&config))
        }
        "tasks.list" => {
            let tasks: Vec<TaskView> = kule.call(Call::Tasks)?;
            let tasks: Vec<ExtTask> = tasks.into_iter().map(ExtTask::from).collect();
            Ok(json(&tasks))
        }
        "tasks.get" => {
            let TaskParam { task } = self::params(&method, p)?;
            let tasks: Vec<TaskView> = kule.call(Call::Tasks)?;
            let found = tasks
                .into_iter()
                .find(|t| t.id == task)
                .map(ExtTask::from)
                .ok_or_else(|| format!("no task {task}"))?;
            Ok(json(&found))
        }
        "review.get" => {
            let TaskParam { task } = self::params(&method, p)?;
            let mut review: Value = kule.call(Call::Review {
                task,
                include_ignored: false,
            })?;
            // What waits in the person's home is theirs, not the task's.
            if let Some(map) = review.as_object_mut() {
                map.retain(|k, _| {
                    matches!(k.as_str(), "effects" | "ruling" | "digest" | "outline")
                });
            }
            Ok(review)
        }
        "files.list" => {
            let FileParams { task, path } = self::params(&method, p)?;
            kule.call(Call::ListDir { task, path })
        }
        "files.read" => {
            let FileParams { task, path } = self::params(&method, p)?;
            kule.call(Call::ReadFile { task, path })
        }
        "review.annotate" => Ok(Value::Null),
        "storage.get" => kule.call(Call::ExtensionStorage { id: ext }),
        "storage.set" => {
            let StorageParams { value } = self::params(&method, p)?;
            kule.call::<()>(Call::SetExtensionStorage { id: ext, value })?;
            Ok(Value::Null)
        }
        "ui.notify" => {
            let NotifyParams { title, body } = self::params(&method, p)?;
            let in_front = app
                .get_webview_window("main")
                .and_then(|w| w.is_focused().ok())
                .unwrap_or(false);
            let (answer, system) = notice(&name, &title, &body, in_front);
            if let Some((title, body)) = system {
                let _ = app.notification().builder().title(title).body(body).show();
            }
            Ok(answer)
        }
        "net.fetch" => {
            let FetchParams { url, init } = self::params(&method, p)?;
            let checked = ext_net::check(&url, init.unwrap_or_default(), &hosts)?;
            let (verb, host) = (checked.method.clone(), checked.host.clone());
            let result = ext_net::send(checked);
            log_fetch(
                &kule.home,
                &ext,
                &verb,
                &host,
                result.as_ref().ok().map(|f| (f.status, f.body.len())),
            );
            Ok(json(&result?))
        }
        _ => Err(format!("there is no call {method}")),
    }
}

/// Where an extension's notification goes: back to the window when it is in
/// front (the system shows no banner for the app in front, as for Kule's
/// own), else to the system, as (title, body). The title carries the
/// extension's name.
fn notice(
    name: &str,
    title: &str,
    body: &str,
    in_front: bool,
) -> (Value, Option<(String, String)>) {
    let title = format!("{name}: {}", shown(title, 80));
    let body = shown(body, 300);
    if in_front {
        (
            json!({ "shown": "window", "title": title, "body": body }),
            None,
        )
    } else {
        (json!({ "shown": "system" }), Some((title, body)))
    }
}

/// Text from an extension made fit to show: no control or direction
/// characters, at most `max` characters.
fn shown(text: &str, max: usize) -> String {
    text.chars()
        .filter(|c| {
            !c.is_control()
                && !matches!(c, '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}' | '\u{200E}' | '\u{200F}' | '\u{061C}')
        })
        .take(max)
        .collect()
}

/// One line per fetch in the extension's own `network.jsonl`: method, host,
/// status and size, never the path or query (a webhook's secret is there).
fn log_fetch(home: &Path, ext: &str, method: &str, host: &str, outcome: Option<(u16, usize)>) {
    let dir = home.join("extensions").join("data").join(ext);
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let unix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let line = json!({
        "unix": unix,
        "method": method,
        "host": host,
        "status": outcome.map(|o| o.0),
        "bytes": outcome.map(|o| o.1),
    });
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("network.jsonl"))
    {
        let _ = writeln!(file, "{line}");
    }
}

fn json<T: Serialize>(value: &T) -> Value {
    serde_json::to_value(value).unwrap_or(Value::Null)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    /// Installs acme.notes asking for `permissions`; approves it when asked.
    fn installed(home: &Path, permissions: &str, approve: bool) {
        let src = TempDir::new().unwrap();
        let hosts = if permissions.contains("net") {
            r#""hosts": ["hooks.slack.com"],"#
        } else {
            ""
        };
        fs::write(
            src.path().join("kule-extension.json"),
            format!(
                r#"{{ "kule_extension": 1, "id": "acme.notes", "name": "Notes", "version": "1.0.0",
                     "publisher": "acme", {hosts} "permissions": [{permissions}] }}"#
            ),
        )
        .unwrap();
        let package = kule_ext::Package::open(kule_ext::pack(src.path()).unwrap()).unwrap();
        let store = Store::new(home);
        store.install(&package).unwrap();
        if approve {
            store.approve("acme.notes", package.digest()).unwrap();
        }
    }

    #[test]
    fn a_call_needs_its_permission_and_nothing_else_exists() {
        let home = TempDir::new().unwrap();
        installed(home.path(), r#""tasks.read", "net""#, true);
        assert!(allowed(home.path(), "acme.notes", "tasks.list").is_ok());
        assert!(allowed(home.path(), "acme.notes", "config.get").is_ok());
        let net = allowed(home.path(), "acme.notes", "net.fetch").unwrap();
        assert_eq!(net.hosts, ["hooks.slack.com"]);
        for (method, says) in [
            ("review.get", "needs the review.read permission"),
            ("review.annotate", "needs the review.annotate permission"),
            ("files.read", "needs the files.read permission"),
            ("storage.set", "needs the storage permission"),
            ("ui.notify", "needs the notify permission"),
            ("commit", "there is no call commit"),
            ("secrets", "there is no call secrets"),
            ("answer", "there is no call answer"),
            ("set_layout", "there is no call set_layout"),
        ] {
            let e = allowed(home.path(), "acme.notes", method).unwrap_err();
            assert!(e.contains(says), "{method}: {e}");
        }
    }

    #[test]
    fn an_extension_held_turned_off_or_unknown_calls_nothing() {
        let home = TempDir::new().unwrap();
        installed(home.path(), r#""tasks.read""#, false);
        let e = allowed(home.path(), "acme.notes", "tasks.list").unwrap_err();
        assert!(e.contains("held: not approved yet"), "{e}");
        assert!(allowed(home.path(), "other.ext", "tasks.list").is_err());

        let home = TempDir::new().unwrap();
        installed(home.path(), r#""tasks.read""#, true);
        Store::new(home.path())
            .set_enabled("acme.notes", false)
            .unwrap();
        let e = allowed(home.path(), "acme.notes", "tasks.list").unwrap_err();
        assert!(e.contains("turned off"), "{e}");
    }

    /// A call does not open the package again (an extension could make it
    /// inflate up to 40 MiB on every call); what changes the extensions
    /// drops what was kept.
    #[test]
    fn what_an_extension_was_given_is_read_once_until_the_extensions_change() {
        let home = TempDir::new().unwrap();
        installed(home.path(), r#""tasks.read""#, true);
        let grants = Grants::default();
        assert!(grants.get(home.path(), "acme.notes").is_ok());
        let packages = home.path().join("extensions/packages");
        for f in fs::read_dir(&packages).unwrap() {
            fs::remove_file(f.unwrap().path()).unwrap();
        }
        let kept = grants.get(home.path(), "acme.notes").unwrap();
        assert!(permits(&kept, "tasks.list").is_ok());
        assert!(permits(&kept, "files.read").is_err());
        grants.forget();
        assert!(grants.get(home.path(), "acme.notes").is_err());
    }

    #[test]
    fn notifications_and_fetches_are_limited_per_extension() {
        let limits = Limits::default();
        for _ in 0..5 {
            limits.take("acme.notes", "ui.notify").unwrap();
        }
        assert!(limits.take("acme.notes", "ui.notify").is_err());
        assert!(limits.take("other.ext", "ui.notify").is_ok());
        assert!(limits.take("acme.notes", "tasks.list").is_ok());
    }

    #[test]
    fn a_notification_shows_in_the_window_while_it_is_in_front() {
        let (answer, system) = notice("Hello", "Hi\u{202E}", "1 task", true);
        assert_eq!(
            answer,
            json!({ "shown": "window", "title": "Hello: Hi", "body": "1 task" })
        );
        assert!(system.is_none());
        let (answer, system) = notice("Hello", "Hi", "1 task", false);
        assert_eq!(answer, json!({ "shown": "system" }));
        assert_eq!(system, Some(("Hello: Hi".to_owned(), "1 task".to_owned())));
    }

    #[test]
    fn shown_text_hides_nothing() {
        assert_eq!(shown("Pay\u{202E}lacol\nx", 80), "Paylacolx");
        assert_eq!(shown(&"a".repeat(100), 10).len(), 10);
    }
}
