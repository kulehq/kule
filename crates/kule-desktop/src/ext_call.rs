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
use tauri::{AppHandle, State};
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
        "net.fetch" => Permission::Net,
        _ => return None,
    }))
}

/// What a running extension was given.
#[derive(Debug)]
pub struct Granted {
    pub hosts: Vec<String>,
    pub name: String,
}

/// Whether `ext` may make the call `method` now: it is on, approved as it
/// is, and its approved permissions cover the call.
pub fn allowed(home: &Path, ext: &str, method: &str) -> Result<Granted> {
    let listed = Store::new(home).get(ext).map_err(|e| e.to_string())?;
    if let Some(why) = &listed.held {
        return Err(format!("extension {ext} is held: {why}"));
    }
    if !listed.enabled {
        return Err(format!("extension {ext} is turned off"));
    }
    let approval = listed
        .approved
        .ok_or_else(|| format!("extension {ext} is not approved"))?;
    let name = listed.manifest.map(|m| m.name).unwrap_or_default();
    match needs(method) {
        None => Err(format!("there is no call {method}")),
        Some(Some(need)) if !approval.permissions.contains(&need) => Err(format!(
            "{method} needs the {} permission, which {name} was not given",
            need.as_str()
        )),
        Some(_) => Ok(Granted {
            hosts: approval.hosts,
            name,
        }),
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
    ext: String,
    method: String,
    params: Value,
) -> Result<Value> {
    let Granted { hosts, name } = allowed(&kule.home, &ext, &method)?;
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
        "storage.get" => kule.call(Call::ExtensionStorage { id: ext }),
        "storage.set" => {
            let StorageParams { value } = self::params(&method, p)?;
            kule.call::<()>(Call::SetExtensionStorage { id: ext, value })?;
            Ok(Value::Null)
        }
        "ui.notify" => {
            let NotifyParams { title, body } = self::params(&method, p)?;
            let title = format!("{name}: {}", shown(&title, 80));
            let _ = app
                .notification()
                .builder()
                .title(title)
                .body(shown(&body, 300))
                .show();
            Ok(Value::Null)
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
    fn shown_text_hides_nothing() {
        assert_eq!(shown("Pay\u{202E}lacol\nx", 80), "Paylacolx");
        assert_eq!(shown(&"a".repeat(100), 10).len(), 10);
    }
}
