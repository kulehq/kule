//! Shell completion: the scripts `kule completions` prints load in their
//! shells and complete, and what `kule __complete` offers comes from the
//! command line's definitions and from a real home, read and never made.

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use tempfile::TempDir;

fn kule() -> Command {
    Command::new(env!("CARGO_BIN_EXE_kule"))
}

fn text(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// What `kule __complete <shell>` offers for `words`, in `home`.
fn offered(home: &Path, shell: &str, words: &[&str]) -> Vec<String> {
    let mut cmd = kule();
    cmd.env("KULE_HOME", home);
    offered_by(cmd, shell, words)
}

/// What `kule __complete <shell>` offers for `words`, run as `cmd` is set.
fn offered_by(mut cmd: Command, shell: &str, words: &[&str]) -> Vec<String> {
    let out = cmd
        .args(["__complete", shell, "--"])
        .args(words)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        out.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    text(&out).lines().map(str::to_owned).collect()
}

/// A task made in `home` on a fresh folder, and its id.
fn a_task(home: &Path, name: Option<&str>) -> (TempDir, String) {
    let ws = TempDir::new().unwrap();
    let out = kule()
        .args(["new", ws.path().to_str().unwrap()])
        .env("KULE_HOME", home)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let id = text(&out).split_whitespace().nth(1).unwrap().to_owned();
    if let Some(name) = name {
        let named = kule()
            .args(["name", &id, name])
            .env("KULE_HOME", home)
            .output()
            .unwrap();
        assert!(named.status.success());
    }
    (ws, id)
}

#[test]
fn subcommands_options_and_their_values_come_from_the_definitions() {
    let home = TempDir::new().unwrap();
    let subcommands = offered(home.path(), "zsh", &["kule", "ro"]);
    assert_eq!(subcommands[0], "values");
    assert!(
        subcommands.iter().any(|l| l.starts_with("rollback\t")),
        "{subcommands:?}"
    );
    assert!(
        subcommands
            .iter()
            .all(|l| l == "values" || l.starts_with("ro"))
    );
    // The hidden one is never offered.
    let all = offered(home.path(), "bash", &["kule", ""]);
    assert!(all.contains(&"completions".to_owned()), "{all:?}");
    assert!(!all.iter().any(|l| l.starts_with("__")), "{all:?}");

    let options = offered(home.path(), "bash", &["kule", "limits", "--"]);
    assert!(options.contains(&"--group".to_owned()), "{options:?}");
    assert!(options.contains(&"--task".to_owned()), "{options:?}");
    let shells = offered(home.path(), "bash", &["kule", "completions", ""]);
    assert_eq!(shells, ["values", "bash", "zsh", "fish"]);
    let fields = offered(home.path(), "fish", &["kule", "limits", "me"]);
    assert_eq!(fields[1].split('\t').next(), Some("memory="), "{fields:?}");
    // A path is the shell's own to complete.
    assert_eq!(
        offered(home.path(), "bash", &["kule", "new", ""]),
        ["files"]
    );
}

#[test]
fn task_ids_complete_from_the_home_with_their_names() {
    let home = TempDir::new().unwrap();
    let (_ws, id) = a_task(home.path(), Some("fix the login"));
    let (_other, other) = a_task(home.path(), None);
    for words in [
        &["kule", "status", ""][..],
        &["kule", "exec", ""],
        &["kule", "limits", "--task", ""],
        &[
            "kule",
            "--home",
            home.path().to_str().unwrap(),
            "commit",
            "",
        ],
    ] {
        let ids = offered(home.path(), "bash", words);
        assert!(
            ids.contains(&id) && ids.contains(&other),
            "{words:?}: {ids:?}"
        );
    }
    let described = offered(home.path(), "zsh", &["kule", "status", &id[..3]]);
    assert!(
        described
            .iter()
            .any(|l| l.starts_with(&format!("{id}\t")) && l.contains("fix the login")),
        "{described:?}"
    );
    // Bash breaks the word at `=`: it gets what follows.
    let bash = offered(
        home.path(),
        "bash",
        &["kule", "limits", &format!("--task={}", &id[..3])],
    );
    assert!(bash.contains(&id), "{bash:?}");
    // The value given after `=` is completed after it.
    let after = offered(
        home.path(),
        "zsh",
        &["kule", "limits", &format!("--task={}", &id[..3])],
    );
    assert!(
        after
            .iter()
            .any(|l| l.starts_with(&format!("--task={id}\t"))),
        "{after:?}"
    );
}

/// What a word is completed with depends on what stands before it: an
/// option's value after the option, the next argument after a word, only
/// arguments after `--`.
#[test]
fn each_word_is_completed_by_what_stands_before_it() {
    let home = TempDir::new().unwrap();
    let (_ws, id) = a_task(home.path(), None);
    let at = |words: &[&str]| offered(home.path(), "bash", words);
    // After `--`, a dash starts no option.
    assert_eq!(at(&["kule", "status", "--", "-"]), ["values"]);
    // A short option's value, and an option that is not there.
    assert_eq!(at(&["kule", "ext", "pack", "dir", "-o", ""]), ["files"]);
    assert_eq!(at(&["kule", "ext", "pack", "dir", "-x", ""]), ["values"]);
    // A lone dash is an argument, as is a word given: neither is
    // completed again.
    assert_eq!(at(&["kule", "status", "-", ""]), ["values"]);
    assert_eq!(at(&["kule", "status", &id, ""]), ["values"]);
    // A flag takes no value: the argument after it is completed.
    assert!(at(&["kule", "status", "--json", ""]).contains(&id));
    // A word that is no subcommand is not followed by subcommands.
    assert_eq!(at(&["kule", "nosuch", ""]), ["values"]);
    // The last argument, when it takes many, takes each further word.
    assert!(
        at(&["kule", "limits", "memory=4", ""]).contains(&"cpu=".to_owned()),
        "{:?}",
        at(&["kule", "limits", "memory=4", ""])
    );
    // A name is no path: nothing to offer, and no file names.
    assert_eq!(at(&["kule", "name", &id, ""]), ["values"]);
    // A subcommand is described by the first line of its help.
    assert!(
        offered(home.path(), "zsh", &["kule", "rollb"])
            .contains(&"rollback\tThrow the task's uncommitted changes away".to_owned())
    );
}

/// Agent names, extension ids and group ids come from the home, each with
/// what it is.
#[test]
fn agents_extensions_and_groups_complete_from_the_home() {
    // Extensions refuse folders under the temp folder agents write to.
    let home = TempDir::new_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let work = TempDir::new_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let (_ws, id) = a_task(home.path(), None);
    let made = |args: &[&str]| {
        let out = kule()
            .args(args)
            .current_dir(work.path())
            .env("KULE_HOME", home.path())
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    };
    made(&["group", "new", "clients"]);
    made(&["ext", "new", "me.first-try"]);
    made(&["ext", "dev", "first-try"]);
    let at = |words: &[&str]| offered(home.path(), "zsh", words);

    for words in [
        &["kule", "run", &id, "--agent", ""][..],
        &["kule", "agents", "show", ""],
    ] {
        let agents = at(words);
        assert!(
            agents.contains(&"claude-code\tagent".to_owned()),
            "{words:?}: {agents:?}"
        );
    }
    assert_eq!(
        at(&["kule", "ext", "enable", ""]),
        ["values", "me.first-try\tFirst Try"]
    );
    for words in [
        &["kule", "limits", "--group", ""][..],
        &["kule", "group", "put", &id, ""],
    ] {
        assert_eq!(at(words), ["values", "1\tclients"], "{words:?}");
    }
}

/// The home is the one `--home` names when `KULE_HOME` names none, and
/// `~/.kule` when neither does.
#[test]
fn the_home_is_the_one_named_on_the_line_else_the_default() {
    let home = TempDir::new().unwrap();
    let (_ws, id) = a_task(home.path(), None);
    let named = home.path().to_str().unwrap();
    // As for the command itself, `--home` wins over `KULE_HOME`.
    let other = TempDir::new().unwrap();
    let (_ws, elsewhere) = a_task(other.path(), None);
    for env in ["", other.path().to_str().unwrap()] {
        for words in [
            &["kule", "--home", named, "status", ""][..],
            &["kule", &format!("--home={named}"), "status", ""],
        ] {
            let mut cmd = kule();
            cmd.env("KULE_HOME", env);
            let ids = offered_by(cmd, "bash", words);
            assert!(ids.contains(&id), "{words:?}: {ids:?}");
            assert!(!ids.contains(&elsewhere), "{words:?}: {ids:?}");
        }
    }
    let ids = offered(other.path(), "bash", &["kule", "status", ""]);
    assert!(ids.contains(&elsewhere) && !ids.contains(&id), "{ids:?}");

    let person = TempDir::new().unwrap();
    let (_ws, theirs) = a_task(&person.path().join(".kule"), None);
    let mut cmd = kule();
    // The person's home: HOME, and USERPROFILE on Windows.
    cmd.env_remove("KULE_HOME")
        .env("HOME", person.path())
        .env("USERPROFILE", person.path());
    let ids = offered_by(cmd, "bash", &["kule", "status", ""]);
    assert!(ids.contains(&theirs), "{ids:?}");
}

/// A name or label in the home holding what a terminal or shell would
/// read as an instruction is never printed as it is.
#[test]
fn what_the_home_holds_reaches_the_shell_plain() {
    let home = TempDir::new().unwrap();
    let (_ws, id) = a_task(home.path(), Some("named"));
    // Kule refuses such a name; one written into the home all the same is
    // still shown plain.
    let settings = home.path().join("tasks").join(&id).join("settings.json");
    let text = fs::read_to_string(&settings).unwrap();
    fs::write(
        &settings,
        text.replace("named", "ok\\u202ename\\u001b]0;x\\u0007"),
    )
    .unwrap();
    let lines = offered(home.path(), "fish", &["kule", "status", ""]);
    let line = lines.iter().find(|l| l.starts_with(&id)).unwrap();
    assert!(line.contains("ok") && line.contains("name"), "{line:?}");
    // A record whose id a shell would run is never offered.
    let evil = home.path().join("tasks").join("evil");
    fs::create_dir_all(&evil).unwrap();
    fs::write(
        evil.join("task.json"),
        r#"{"id":"$(touch pwned)","workspace":"/tmp","created_unix":1}"#,
    )
    .unwrap();
    let lines = offered(home.path(), "bash", &["kule", "status", ""]);
    assert!(lines.contains(&id), "{lines:?}");
    assert!(!lines.iter().any(|l| l.contains("pwned")), "{lines:?}");
    assert!(
        !line.contains('\u{202e}') && !line.contains('\u{1b}') && !line.contains('\u{7}'),
        "{line:?}"
    );
}

/// A home that does not exist is not made, and one that cannot be read
/// gives nothing: no error into the shell, exit 0.
#[cfg(unix)]
#[test]
fn a_home_that_cannot_be_read_gives_nothing_quietly() {
    use std::os::unix::fs::PermissionsExt;
    let dir = TempDir::new().unwrap();
    let missing = dir.path().join("none");
    assert_eq!(
        offered(&missing, "bash", &["kule", "status", ""]),
        ["values"]
    );
    assert!(!missing.exists(), "completion made a home");

    let home = dir.path().join("home");
    fs::create_dir(&home).unwrap();
    let (_ws, id) = a_task(&home, None);
    assert!(offered(&home, "bash", &["kule", "status", ""]).contains(&id));
    fs::set_permissions(&home, fs::Permissions::from_mode(0o000)).unwrap();
    let locked = offered(&home, "bash", &["kule", "status", ""]);
    fs::set_permissions(&home, fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(locked, ["values"]);

    // A link in the home's place is not Kule's home.
    let link = dir.path().join("link");
    std::os::unix::fs::symlink(&home, &link).unwrap();
    assert_eq!(offered(&link, "bash", &["kule", "status", ""]), ["values"]);

    // Words it cannot place give nothing either.
    let out = kule()
        .args(["__complete", "tcsh", "--", "kule"])
        .output()
        .unwrap();
    assert!(out.status.success() && out.stdout.is_empty() && out.stderr.is_empty());
}

#[test]
fn the_script_holds_nothing_of_the_home_and_makes_none() {
    let dir = TempDir::new().unwrap();
    let missing = dir.path().join("none");
    for shell in ["bash", "zsh", "fish"] {
        let out = kule()
            .args(["completions", shell])
            .env("KULE_HOME", &missing)
            .output()
            .unwrap();
        assert!(out.status.success());
        let script = text(&out);
        assert!(script.contains("kule __complete"), "{shell}");
        assert!(!script.contains(dir.path().to_str().unwrap()), "{shell}");
    }
    assert!(!missing.exists(), "printing a script made a home");
}

// ------------------------------------------------- each shell, for real

/// Each shell loads its script and completes with it.
#[cfg(unix)]
mod each_shell {
    use super::*;

    /// `PATH` with the built `kule` first.
    fn path_with_kule() -> String {
        let bin = Path::new(env!("CARGO_BIN_EXE_kule")).parent().unwrap();
        format!(
            "{}:{}",
            bin.display(),
            std::env::var("PATH").unwrap_or_default()
        )
    }

    fn script(shell: &str, dir: &Path) -> std::path::PathBuf {
        let out = kule().args(["completions", shell]).output().unwrap();
        let path = dir.join(format!("kule.{shell}"));
        fs::write(&path, out.stdout).unwrap();
        path
    }

    fn shell(program: &str, code: &str, home: &Path) -> String {
        let out = Command::new(program)
            .args(["-c", code])
            .env("PATH", path_with_kule())
            .env("KULE_HOME", home)
            .output()
            .unwrap_or_else(|e| panic!("{program} is needed for this test: {e}"));
        assert!(
            out.status.success(),
            "{program}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        text(&out)
    }

    #[test]
    fn bash_loads_the_script_and_completes() {
        let (dir, home) = (TempDir::new().unwrap(), TempDir::new().unwrap());
        let (_ws, id) = a_task(home.path(), None);
        let file = script("bash", dir.path());
        // The bash on PATH, and the system's own: macOS keeps bash 3.2 there.
        for bash in ["bash", "/bin/bash"] {
            let complete = |words: &str| {
                shell(
                    bash,
                    &format!(
                        "source '{}'; COMP_WORDS=({words}); COMP_CWORD=$((${{#COMP_WORDS[@]}}-1)); _kule; printf '%s\\n' \"${{COMPREPLY[@]}}\"",
                        file.display()
                    ),
                    home.path(),
                )
            };
            assert_eq!(complete("kule rollb"), "rollback\n", "{bash}");
            assert_eq!(complete("kule limits memo"), "memory=\n", "{bash}");
            assert_eq!(
                complete(&format!("kule status {}", &id[..4])),
                format!("{id}\n"),
                "{bash}"
            );
        }
    }

    #[test]
    fn zsh_loads_the_script_and_completes() {
        let (dir, home) = (TempDir::new().unwrap(), TempDir::new().unwrap());
        let (_ws, id) = a_task(home.path(), Some("named"));
        let file = script("zsh", dir.path());
        // zsh's own completion functions, standing in: what the script hands
        // them is what a person sees.
        let complete = |words: &str| {
            shell(
                "zsh",
                &format!(
                    "compdef() {{ :; }}; _files() {{ print files; }}; \
                     _describe() {{ print -l -- \"${{(@P)4}}\"; }}; \
                     source '{}'; words=({words}); CURRENT=${{#words}}; _kule",
                    file.display()
                ),
                home.path(),
            )
        };
        let rollback = complete("kule rollb");
        assert!(rollback.starts_with("rollback:"), "{rollback}");
        let ids = complete(&format!("kule status {}", &id[..4]));
        assert!(ids.starts_with(&format!("{id}:named")), "{ids}");
        assert_eq!(complete("kule new ''"), "files\n");
    }

    #[test]
    fn fish_loads_the_script_and_completes() {
        let (dir, home) = (TempDir::new().unwrap(), TempDir::new().unwrap());
        let (_ws, id) = a_task(home.path(), Some("named"));
        let file = script("fish", dir.path());
        let complete = |line: &str| {
            shell(
                "fish",
                &format!("source '{}'; complete -C '{line}'", file.display()),
                home.path(),
            )
        };
        let rollback = complete("kule rollb");
        assert!(rollback.starts_with("rollback\t"), "{rollback}");
        let ids = complete(&format!("kule status {}", &id[..4]));
        assert!(ids.starts_with(&format!("{id}\tnamed")), "{ids}");
    }
}
