//! What a task's commands may read beyond the copy: canaries planted where
//! a project, a sign-in or an app's data lies, each found by a read outside
//! the sandbox first, then refused inside it.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Output;

use kule_engine::{CacheSources, Sandbox, Task};
use tempfile::TempDir;

fn sh(sandbox: &Sandbox, script: &str) -> Output {
    sandbox
        .command("sh", &["-c".to_owned(), script.to_owned()])
        .unwrap()
        .output()
        .unwrap()
}

/// A person's home outside /tmp (the sandbox lets commands write there on
/// Linux), resolved, holding `paths`, each a canary.
fn home_with(paths: &[&str]) -> (TempDir, PathBuf) {
    let dir = TempDir::new_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let home = fs::canonicalize(dir.path()).unwrap();
    for path in paths {
        plant(&home.join(path));
    }
    (dir, home)
}

fn plant(path: &Path) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, "canary\n").unwrap();
}

fn sources(home: &Path) -> CacheSources {
    CacheSources {
        home: home.to_owned(),
        cargo_home: home.join(".cargo"),
    }
}

/// Whether a sandboxed `cat` reads `path`; a read outside finds it first.
fn reads(sandbox: &Sandbox, path: &Path) -> bool {
    assert_eq!(
        fs::read_to_string(path).unwrap(),
        "canary\n",
        "{} is not planted",
        path.display()
    );
    sh(sandbox, &format!("cat '{}'", path.display()))
        .status
        .success()
}

/// Every project Kule opened is hidden from every other task, with the
/// projects beside it, wherever it lies: in the home under another folder
/// than the workspace's, or outside the home.
#[test]
fn projects_kule_opened_are_hidden_from_every_other_task() {
    let (_home, home) = home_with(&[
        "work/app/a.txt",
        "projects/shop/.env",
        "projects/beside-shop/.env",
    ]);
    let outside_dir = TempDir::new_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let outside = fs::canonicalize(outside_dir.path()).unwrap();
    plant(&outside.join(".env"));
    let kule = TempDir::new_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let task = Task::create(kule.path(), &home.join("work/app")).unwrap();
    Task::create(kule.path(), &home.join("projects/shop")).unwrap();
    Task::create(kule.path(), &outside).unwrap();
    // A repository's settings, which a task's copy would not have.
    plant(&home.join("projects/shop/.git/config"));
    plant(&outside.join(".git/config"));
    let sandbox = Sandbox::for_task_with(&task, Some(sources(&home))).unwrap();
    for path in [
        home.join("projects/shop/.env"),
        home.join("projects/shop/.git/config"),
        home.join("projects/beside-shop/.env"),
        outside.join(".env"),
        outside.join(".git/config"),
    ] {
        assert!(!reads(&sandbox, &path), "{} was read", path.display());
    }
    // The task's own copy as ever.
    assert!(sh(&sandbox, "cat a.txt").status.success());
}

/// Where apps and tools keep settings and data, an app's sign-in or a
/// new tool's included, nothing is read but what toolchains, shells and
/// prompts read; editors' stores and saved shell sessions neither.
#[test]
fn settings_and_app_data_are_hidden_but_for_what_toolchains_read() {
    let hidden = [
        ".config/github-copilot/apps.json",
        ".config/configstore/firebase-tools.json",
        ".config/containers/auth.json",
        ".config/fly/config.yml",
        ".fly/config.yml",
        ".config/doppler/.doppler.yaml",
        ".config/sops/age/keys.txt",
        ".config/.wrangler/config/default.toml",
        ".config/pypoetry/auth.toml",
        ".config/some-new-tool/token",
        ".config/Cursor/User/globalStorage/state.vscdb",
        ".config/Windsurf/User/globalStorage/state.vscdb",
        ".config/VSCodium/User/globalStorage/state.vscdb",
        ".local/share/some-app/session",
        ".local/share/fish/fish_history",
        "Library/Preferences/.wrangler/config/default.toml",
        "Library/Preferences/com.example.app.plist",
        "Library/Preferences/ByHost/com.example.app.0A1B.plist",
        "Library/Application Support/pypoetry/auth.toml",
        "Library/Application Support/Cursor/User/globalStorage/state.vscdb",
        "Library/Application Support/Windsurf/User/globalStorage/state.vscdb",
        "Library/Application Support/Some App/token",
        ".zsh_sessions/A1.session",
        ".bash_sessions/A1.session",
        ".yarnrc.yml",
        ".bunfig.toml",
        ".nuget/NuGet/NuGet.Config",
    ];
    let read = [
        ".config/git/config",
        ".config/uv/uv.toml",
        ".config/pypoetry/config.toml",
        ".config/fish/config.fish",
        ".config/starship.toml",
        ".config/mise/config.toml",
        ".local/share/uv/python/marker",
        ".local/share/fish/vendor_completions.d/x.fish",
        ".local/share/man/man1/x.1",
        "Library/Preferences/.GlobalPreferences.plist",
        "Library/Preferences/.GlobalPreferences_m.plist",
        "Library/Preferences/ByHost/.GlobalPreferences.0A1B.plist",
        "Library/Application Support/go/env",
        "Library/Application Support/pypoetry/config.toml",
    ];
    let (_home, home) = home_with(&[hidden.as_slice(), read.as_slice()].concat());
    let s = Setup::new();
    let sandbox = Sandbox::for_task_with(&s.task, Some(sources(&home))).unwrap();
    for path in hidden {
        assert!(!reads(&sandbox, &home.join(path)), "{path} was read");
    }
    for path in read {
        assert!(reads(&sandbox, &home.join(path)), "{path} was not read");
    }
    // Revealed whole for a command, a store keeps the sign-ins in it hidden.
    let mut sandbox = Sandbox::for_task_with(&s.task, Some(sources(&home))).unwrap();
    sandbox.reveal(&home.join(".config")).unwrap();
    assert!(reads(&sandbox, &home.join(".config/some-new-tool/token")));
    for path in hidden.iter().filter(|p| p.starts_with(".config/") && **p != ".config/some-new-tool/token") {
        assert!(!reads(&sandbox, &home.join(path)), "{path} was read once revealed");
    }
}

/// A tool on `PATH` whose program is a link into where apps keep their
/// data (Claude Code's native install, uv's tools) still runs: what the
/// link leads to is read, and nothing beside it.
#[test]
fn a_tool_linked_from_path_into_app_data_runs() {
    let (_home, home) = home_with(&[".local/share/other-app/session"]);
    let tool = home.join(".local/share/a-tool/versions/1.0/a-tool");
    fs::create_dir_all(tool.parent().unwrap()).unwrap();
    fs::write(&tool, "#!/bin/sh\necho ran\n").unwrap();
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(&tool, fs::Permissions::from_mode(0o755)).unwrap();
    let bin = home.join(".local/bin");
    fs::create_dir_all(&bin).unwrap();
    std::os::unix::fs::symlink(&tool, bin.join("a-tool")).unwrap();
    // A link to a file at the top of the home makes no tool of the home.
    plant(&home.join("notes.txt"));
    std::os::unix::fs::symlink(home.join("notes.txt"), bin.join("notes")).unwrap();
    let s = Setup::new();
    let mut sandbox = Sandbox::new(&s.task).unwrap();
    sandbox.hide_settings(&home, std::slice::from_ref(&bin)).unwrap();
    let out = sh(&sandbox, &format!("'{}'", bin.join("a-tool").display()));
    assert!(out.status.success(), "{out:?}");
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "ran");
    assert!(!reads(
        &sandbox,
        &home.join(".local/share/other-app/session")
    ));
}

/// A writable folder where apps keep their data, such as an agent's home
/// of its own in Kule's folder there, stays writable; what is hidden inside
/// it stays hidden, and what lies beside it too.
#[test]
fn a_writable_folder_inside_app_data_stays_writable() {
    let (_home, home) = home_with(&[
        ".local/share/kule/agents/mine/settings.json",
        ".local/share/kule/agents/mine/login.json",
        ".local/share/kule/agents/other/settings.json",
    ]);
    let own = home.join(".local/share/kule/agents/mine");
    let s = Setup::new();
    let mut sandbox = Sandbox::new(&s.task).unwrap();
    sandbox.hide_settings(&home, &[]).unwrap();
    sandbox.allow_write(&own).unwrap();
    sandbox.hide(&own.join("login.json")).unwrap();
    let out = sh(
        &sandbox,
        &format!(
            "cd '{}' && cat settings.json && echo new > new.txt && cat new.txt",
            own.display()
        ),
    );
    assert!(out.status.success(), "{out:?}");
    assert!(!reads(&sandbox, &own.join("login.json")));
    assert!(!reads(
        &sandbox,
        &home.join(".local/share/kule/agents/other/settings.json")
    ));
}

/// The commands' home is the task's layer over the person's: what it
/// holds of their settings is what the sandbox lets them read there, and
/// nothing hidden is cloned into it.
#[test]
fn the_home_layer_holds_only_settings_the_sandbox_shows() {
    let (_home, home) = home_with(&[
        ".config/git/config",
        ".config/git/credentials",
        ".config/github-copilot/apps.json",
        ".config/some-new-tool/token",
        ".zshrc",
    ]);
    let s = Setup::new();
    let mut sandbox = Sandbox::for_task_with(&s.task, Some(sources(&home))).unwrap();
    s.task
        .prepare_home(&home, &[], sandbox.hidden(), sandbox.shown(), &[])
        .unwrap();
    sandbox.use_home(&s.task, &home).unwrap();
    let out = sh(
        &sandbox,
        "cat \"$HOME/.config/git/config\" && cat \"$HOME/.zshrc\"",
    );
    assert!(out.status.success(), "{out:?}");
    let layer = s.task.home_layer();
    for gone in [
        ".config/git/credentials",
        ".config/github-copilot",
        ".config/some-new-tool",
    ] {
        assert!(
            fs::symlink_metadata(layer.join(gone)).is_err(),
            "{gone} is in the layer"
        );
        let out = sh(&sandbox, &format!("cat \"$HOME/{gone}\"*"));
        assert!(!out.status.success(), "{gone} was read through the layer");
    }
    // What a command writes in its place is its own, offered for review,
    // and never applied over the person's file it could not see.
    let out = sh(
        &sandbox,
        "mkdir -p \"$HOME/.config/some-new-tool\" && echo mine > \"$HOME/.config/some-new-tool/token\"",
    );
    assert!(out.status.success(), "{out:?}");
    assert!(
        s.task
            .home_changes()
            .unwrap()
            .iter()
            .any(|c| c.path == ".config/some-new-tool/token")
    );
    assert!(
        s.task
            .apply_home(&[".config/some-new-tool/token".into()])
            .is_err()
    );
    assert_eq!(
        fs::read_to_string(home.join(".config/some-new-tool/token")).unwrap(),
        "canary\n"
    );
}

/// On macOS a program reads preferences through the system's preferences
/// service, outside the sandbox: an app's hidden preferences stay out of
/// reach that way too.
#[cfg(target_os = "macos")]
#[test]
fn hidden_preferences_are_not_read_through_the_preferences_service() {
    let domain = format!("dev.kule.canary-{}", std::process::id());
    let defaults = |args: &[&str]| {
        std::process::Command::new("defaults")
            .args(args)
            .output()
            .unwrap()
    };
    assert!(
        defaults(&["write", &domain, "token", "canary"])
            .status
            .success()
    );
    let person = fs::canonicalize(std::env::var("HOME").unwrap()).unwrap();
    let s = Setup::new();
    let mut sandbox = Sandbox::new(&s.task).unwrap();
    sandbox.hide_settings(&person, &[]).unwrap();
    let out = sandbox
        .command("defaults", &["read".into(), domain.clone(), "token".into()])
        .unwrap()
        .output()
        .unwrap();
    let _ = defaults(&["delete", &domain]);
    assert!(
        !String::from_utf8_lossy(&out.stdout).contains("canary"),
        "{out:?}"
    );
}

struct Setup {
    _kule: TempDir,
    _ws: TempDir,
    task: Task,
}

impl Setup {
    fn new() -> Setup {
        let kule = TempDir::new_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
        let ws = TempDir::new().unwrap();
        fs::write(ws.path().join("a.txt"), "a\n").unwrap();
        let task = Task::create(kule.path(), ws.path()).unwrap();
        Setup {
            _kule: kule,
            _ws: ws,
            task,
        }
    }
}
