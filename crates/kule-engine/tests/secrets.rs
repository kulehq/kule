//! Secrets in the folder's settings files: masked in the copy, real in the
//! folder, through every way the copy and the folder sync.

use std::fs;

use kule_engine::{Task, add_secret};
use tempfile::TempDir;

const ENV: &str = "PORT=3000\nSTRIPE_KEY=sk_live_51Hreal\nDB_PASSWORD='hunter2'\n";

struct Setup {
    home: TempDir,
    ws: TempDir,
    task: Task,
}

impl Setup {
    fn new() -> Setup {
        let (home, ws) = (TempDir::new().unwrap(), TempDir::new().unwrap());
        fs::write(ws.path().join(".env"), ENV).unwrap();
        fs::write(ws.path().join("app.txt"), "app\n").unwrap();
        let task = Task::create(home.path(), ws.path()).unwrap();
        Setup { home, ws, task }
    }

    fn copy_env(&self) -> String {
        fs::read_to_string(self.task.copy().join(".env")).unwrap()
    }

    fn folder_env(&self) -> String {
        fs::read_to_string(self.ws.path().join(".env")).unwrap()
    }
}

/// The copy's `.env` with each placeholder in it, in order.
fn placeholders(text: &str) -> Vec<String> {
    text.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .filter(|w| w.starts_with("kule_secret_"))
        .map(str::to_owned)
        .collect()
}

#[test]
fn the_copy_holds_placeholders_and_shows_no_change() {
    let s = Setup::new();
    let copy = s.copy_env();
    let p = placeholders(&copy);
    assert_eq!(p.len(), 2, "{copy}");
    assert_eq!(
        copy,
        format!("PORT=3000\nSTRIPE_KEY={}\nDB_PASSWORD='{}'\n", p[0], p[1])
    );
    assert!(!copy.contains("sk_live") && !copy.contains("hunter2"));
    assert_eq!(s.folder_env(), ENV);
    assert!(s.task.effects().unwrap().is_empty());
}

#[test]
fn a_commit_takes_the_real_values_to_the_folder_and_masks_the_copy_again() {
    let s = Setup::new();
    let masked = s.copy_env();
    fs::write(s.task.copy().join(".env"), format!("{masked}NEW_FLAG=1\n")).unwrap();
    fs::write(s.task.copy().join("app.txt"), "app 2\n").unwrap();
    let set = s.task.effects().unwrap();
    assert_eq!(set.paths(), [".env", "app.txt"]);

    s.task.commit().unwrap().unwrap();
    assert_eq!(s.folder_env(), format!("{ENV}NEW_FLAG=1\n"));
    assert_eq!(s.copy_env(), format!("{masked}NEW_FLAG=1\n"));
    assert!(s.task.effects().unwrap().is_empty());

    // Undo brings the folder's file back, and the copy stays masked.
    s.task.undo().unwrap().unwrap();
    assert_eq!(s.folder_env(), ENV);
    assert_eq!(s.copy_env(), masked);
    assert!(s.task.effects().unwrap().is_empty());
}

#[test]
fn a_rollback_masks_what_it_brings_back() {
    let s = Setup::new();
    let masked = s.copy_env();
    fs::write(
        s.task.copy().join(".env"),
        "STRIPE_KEY=typed-by-the-agent\n",
    )
    .unwrap();
    s.task.rollback().unwrap();
    assert_eq!(s.copy_env(), masked);
    assert!(s.task.effects().unwrap().is_empty());
}

#[test]
fn a_change_in_the_folder_merges_with_the_real_values() {
    let s = Setup::new();
    let masked = s.copy_env();
    // The agent adds a line at the end; the person rotates the key.
    fs::write(s.task.copy().join(".env"), format!("{masked}NEW_FLAG=1\n")).unwrap();
    let rotated = ENV.replace("sk_live_51Hreal", "sk_live_51Hnew");
    fs::write(s.ws.path().join(".env"), &rotated).unwrap();
    s.task.commit().unwrap().unwrap();
    assert_eq!(s.folder_env(), format!("{rotated}NEW_FLAG=1\n"));
    assert!(!s.copy_env().contains("sk_live"));
}

#[test]
fn a_kept_secret_in_the_folder_gets_its_own_placeholder() {
    let (home, ws) = (TempDir::new().unwrap(), TempDir::new().unwrap());
    add_secret(
        home.path(),
        "STRIPE_KEY",
        "sk_live_51Hreal",
        &["api.stripe.com".into()],
        None,
    )
    .unwrap();
    // Not a name or a value Kule would take for a secret on its own.
    fs::write(ws.path().join(".env"), "PAYMENTS=sk_live_51Hreal\n").unwrap();
    let task = Task::create(home.path(), ws.path()).unwrap();
    let env = task.vault().unwrap().env();
    assert_eq!(env.len(), 1);
    let copy = fs::read_to_string(task.copy().join(".env")).unwrap();
    assert_eq!(copy, format!("PAYMENTS={}\n", env[0].1));
}

#[test]
fn the_values_are_kept_where_no_sandbox_reads_them() {
    use std::os::unix::fs::PermissionsExt;
    let s = Setup::new();
    let dir = s.home.path().join("tasks").join(s.task.id());
    let kept = dir.join("secrets.json");
    assert_eq!(
        fs::metadata(&kept).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert!(!kept.starts_with(s.task.copy()));
}

/// The values in Kule's home are sealed: the file says what is kept and
/// for which hosts, never a value; a file from before sealing is sealed at
/// the next read, and every value still reaches its hosts.
#[test]
fn kept_values_are_sealed_on_disk() {
    let home = TempDir::new().unwrap();
    add_secret(
        home.path(),
        "API_KEY",
        "sk-real-1",
        &["api.example.com".into()],
        None,
    )
    .unwrap();
    let file = fs::read_to_string(home.path().join(kule_engine::SECRETS_FILE)).unwrap();
    assert!(!file.contains("sk-real-1"), "{file}");
    assert!(
        file.contains("API_KEY") && file.contains("api.example.com"),
        "{file}"
    );
    assert_eq!(
        kule_engine::secrets_key_home(home.path()).unwrap(),
        Some(kule_engine::KeyHome::File)
    );

    // As an older Kule left it: the value as it is.
    let old = TempDir::new().unwrap();
    fs::write(
        old.path().join(kule_engine::SECRETS_FILE),
        r#"{ "secrets": [{ "name": "OLD_KEY", "hosts": ["api.example.com"], "added_unix": 1, "value": "sk-old-2" }] }"#,
    )
    .unwrap();
    let ws = TempDir::new().unwrap();
    let task = Task::create(old.path(), ws.path()).unwrap();
    let vault = task.vault().unwrap();
    let p = vault.env()[0].1.clone();
    let (filled, _) = vault.fill(
        "api.example.com",
        &format!("GET / HTTP/1.1\r\nAuthorization: Bearer {p}\r\n\r\n"),
    );
    assert!(filled.contains("sk-old-2"), "{filled}");
    let file = fs::read_to_string(old.path().join(kule_engine::SECRETS_FILE)).unwrap();
    assert!(!file.contains("sk-old-2"), "{file}");
}

/// A sealed value opens only for the entry it was sealed for: moved to
/// another entry, or with its entry's hosts changed, or marked as from
/// before binding, it does not open, and no value goes anywhere.
#[test]
fn a_sealed_value_opens_only_for_what_it_was_kept_for() {
    let home = TempDir::new().unwrap();
    add_secret(
        home.path(),
        "API_KEY",
        "sk-real-1",
        &["api.example.com".into()],
        None,
    )
    .unwrap();
    add_secret(
        home.path(),
        "OTHER",
        "plain-2",
        &["evil.example".into()],
        None,
    )
    .unwrap();
    let path = home.path().join(kule_engine::SECRETS_FILE);
    let file: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    let opens = |edit: &dyn Fn(&mut serde_json::Value)| {
        let mut changed = file.clone();
        edit(&mut changed);
        fs::write(&path, changed.to_string()).unwrap();
        let ws = TempDir::new().unwrap();
        let task = Task::create(home.path(), ws.path());
        task.and_then(|t| t.vault()).is_ok()
    };
    assert!(opens(&|_| {}));
    let entry = |f: &mut serde_json::Value, name: &str| -> usize {
        f["secrets"]
            .as_array()
            .unwrap()
            .iter()
            .position(|s| s["name"] == name)
            .unwrap()
    };
    // The key's sealed value moved into the entry for the other host.
    assert!(!opens(&|f| {
        let (k, o) = (entry(f, "API_KEY"), entry(f, "OTHER"));
        let sealed = f["secrets"][k]["sealed"].clone();
        f["secrets"][o]["sealed"] = sealed;
    }));
    // The key's entry given another host.
    assert!(!opens(&|f| {
        let k = entry(f, "API_KEY");
        f["secrets"][k]["hosts"] = serde_json::json!(["evil.example"]);
    }));
    // Passed off as sealed before values were bound.
    assert!(!opens(&|f| {
        let k = entry(f, "API_KEY");
        f["secrets"][k]["bound"] = serde_json::json!(false);
    }));
}

#[test]
fn a_secret_typed_into_the_copy_is_masked_before_an_agent_starts() {
    let s = Setup::new();
    let masked = s.copy_env();
    // A person adds a key in the copy, from a terminal outside the sandbox.
    fs::write(
        s.task.copy().join(".env"),
        format!("{masked}GITHUB_TOKEN=ghp_typedByThePerson12345\n"),
    )
    .unwrap();
    let at_work = s.task.agent_at_work().unwrap();
    let copy = s.copy_env();
    assert!(!copy.contains("ghp_"), "{copy}");
    assert_eq!(placeholders(&copy).len(), 3);
    drop(at_work);
    // Committed, the folder gets the key the person typed.
    s.task.commit().unwrap().unwrap();
    assert_eq!(
        s.folder_env(),
        format!("{ENV}GITHUB_TOKEN=ghp_typedByThePerson12345\n")
    );
}

/// Switches and short numbers stay as they are, whatever their keys say:
/// a tool reads them as settings, and a response that holds the same word
/// would be blanked.
#[test]
fn switches_and_short_numbers_are_not_masked() {
    let (home, ws) = (TempDir::new().unwrap(), TempDir::new().unwrap());
    let settings =
        "ALLOW_PRIVATE_NETWORKS=false\nAUTH_ENABLED=true\nMAX_TOKENS=4096\nDB_PASSWORD=123456\n";
    fs::write(ws.path().join(".env"), settings).unwrap();
    let task = Task::create(home.path(), ws.path()).unwrap();
    let copy = fs::read_to_string(task.copy().join(".env")).unwrap();
    let p = placeholders(&copy);
    assert_eq!(p.len(), 1, "{copy}");
    assert_eq!(
        copy,
        format!(
            "ALLOW_PRIVATE_NETWORKS=false\nAUTH_ENABLED=true\nMAX_TOKENS=4096\nDB_PASSWORD={}\n",
            p[0]
        )
    );
}

/// A kept secret is masked wherever it is in a settings file, not only as
/// a whole value, and so is one kept for another folder, under a
/// placeholder no host fills. A commit puts a line back only as Kule masked
/// it, and no more times than the file held it: a copy of it the agent
/// made stays masked.
#[test]
fn a_kept_secret_inside_a_value_is_masked_and_put_back_where_it_was() {
    let (home, ws, other) = (
        TempDir::new().unwrap(),
        TempDir::new().unwrap(),
        TempDir::new().unwrap(),
    );
    add_secret(
        home.path(),
        "STRIPE_KEY",
        "sk_live_51Hreal",
        &["api.stripe.com".into()],
        None,
    )
    .unwrap();
    add_secret(
        home.path(),
        "OTHER_KEY",
        "other-Folder-9",
        &["api.other.example".into()],
        Some(other.path()),
    )
    .unwrap();
    let env = "CHECKOUT=\"curl -u sk_live_51Hreal: https://api.stripe.com\"\n\
               NOTE=other-Folder-9 # kept for another folder\n";
    fs::write(ws.path().join(".env"), env).unwrap();
    let task = Task::create(home.path(), ws.path()).unwrap();
    let copy = fs::read_to_string(task.copy().join(".env")).unwrap();
    assert!(
        !copy.contains("sk_live_51Hreal") && !copy.contains("other-Folder-9"),
        "{copy}"
    );
    // The proxy fills the kept secret's placeholder for its host.
    let vars = task.vault().unwrap().env();
    assert_eq!(vars.len(), 1, "{vars:?}");
    assert!(
        copy.contains(&format!("curl -u {}: https://api.stripe.com", vars[0].1)),
        "{copy}"
    );
    assert!(task.effects().unwrap().is_empty());

    let first = copy.lines().next().unwrap().to_owned();
    fs::write(
        task.copy().join(".env"),
        format!("{copy}NEW_FLAG=1\n{first}\n"),
    )
    .unwrap();
    task.commit().unwrap().unwrap();
    let folder = fs::read_to_string(ws.path().join(".env")).unwrap();
    assert_eq!(folder, format!("{env}NEW_FLAG=1\n{first}\n"));
}
