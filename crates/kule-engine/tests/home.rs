//! The home layer: what a command writes in the home directory is caught
//! for review, in a real sandbox, over a home made for the test.

use std::fs;
use std::path::Path;

use kule_engine::{CacheSources, Change, HomeChange, Kind, Sandbox, Task};
use tempfile::TempDir;

struct Setup {
    _home: TempDir,
    _ws: TempDir,
    person: TempDir,
    task: Task,
}

impl Setup {
    fn new() -> Setup {
        // The person's home outside /tmp, which the sandbox lets commands
        // write to, as no real home is.
        let (home, ws, person) = (
            TempDir::new().unwrap(),
            TempDir::new().unwrap(),
            TempDir::new_in(env!("CARGO_TARGET_TMPDIR")).unwrap(),
        );
        let p = person.path();
        let write = |rel: &str, text: &str| {
            let path = p.join(rel);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, text).unwrap();
        };
        write(".zshrc", "export A=1\n");
        write(".config/tool/settings.toml", "x = 1\n");
        write(".config/gh/hosts.yml", "token: secret\n");
        write(".ssh/id_ed25519", "private\n");
        write("Documents/notes.txt", "notes\n");
        write(".claude/settings.json", "{}\n");
        fs::write(ws.path().join("a.txt"), "a\n").unwrap();
        let task = Task::create(home.path(), ws.path()).unwrap();
        Setup {
            _home: home,
            _ws: ws,
            person,
            task,
        }
    }

    /// Runs `script` in the sandbox with the home layer as `HOME`.
    fn run(&self, script: &str) -> String {
        let p = self.person.path();
        let mut sandbox = Sandbox::for_task_with(
            &self.task,
            Some(CacheSources {
                home: p.to_owned(),
                cargo_home: p.join(".cargo"),
            }),
        )
        .unwrap();
        let layer = self
            .task
            .prepare_home(
                p,
                &[p.join(".claude")],
                sandbox.hidden(),
                &[".claude".to_owned()],
            )
            .unwrap();
        sandbox.use_home(&layer, p).unwrap();
        let out = sandbox
            .command("sh", &["-c".to_owned(), script.to_owned()])
            .unwrap()
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    fn real(&self, rel: &str) -> String {
        fs::read_to_string(self.person.path().join(rel)).unwrap_or_default()
    }
}

fn change(path: &str, change: Change, kind: Kind) -> HomeChange {
    HomeChange {
        path: path.into(),
        change,
        kind,
    }
}

const SCRIPT: &str = r#"
echo 'export B=2' >> "$HOME/.zshrc"
echo 'y = 2' >> "$HOME/.config/tool/settings.toml"
mkdir -p "$HOME/.config/new" && echo n > "$HOME/.config/new/n.toml"
echo hi > "$HOME/.newrc"
echo state > "$HOME/.claude.json.tmp.1"
(echo leak >> "$HOME/Documents/notes.txt") 2>/dev/null || echo refused-documents
cat "$HOME/.config/gh/hosts.yml" 2>/dev/null || echo no-gh
ls "$HOME/.ssh" 2>/dev/null || echo no-ssh
cat "$HOME/.config/tool/settings.toml"
"#;

#[test]
fn writes_in_the_home_are_caught_and_the_real_one_untouched() {
    let s = Setup::new();
    let out = s.run(SCRIPT);
    for expected in ["refused-documents", "no-gh", "no-ssh", "x = 1\ny = 2"] {
        assert!(out.contains(expected), "{expected} in {out}");
    }
    assert_eq!(s.real(".zshrc"), "export A=1\n");
    assert_eq!(s.real(".config/tool/settings.toml"), "x = 1\n");
    assert_eq!(s.real("Documents/notes.txt"), "notes\n");
    assert!(!s.person.path().join(".newrc").exists());
    assert_eq!(
        s.task.home_changes().unwrap(),
        [
            change(".config/new", Change::Created, Kind::Dir),
            change(".config/new/n.toml", Change::Created, Kind::File),
            change(".config/tool/settings.toml", Change::Modified, Kind::File),
            change(".newrc", Change::Created, Kind::File),
            change(".zshrc", Change::Modified, Kind::File),
        ]
    );
}

#[test]
fn applied_changes_reach_the_real_home_and_what_they_replaced_is_kept() {
    let s = Setup::new();
    s.run(SCRIPT);
    let applied = s
        .task
        .apply_home(&[".zshrc".into(), ".newrc".into()])
        .unwrap();
    assert_eq!(applied.paths, [".newrc", ".zshrc"]);
    assert_eq!(s.real(".zshrc"), "export A=1\nexport B=2\n");
    assert_eq!(s.real(".newrc"), "hi\n");
    let kept = applied.kept.unwrap();
    assert_eq!(
        fs::read_to_string(kept.join(".zshrc")).unwrap(),
        "export A=1\n"
    );
    assert_eq!(s.task.home_changes().unwrap().len(), 3);

    // A file the person changed meanwhile is not overwritten.
    fs::write(
        s.person.path().join(".config/tool/settings.toml"),
        "x = 9\n",
    )
    .unwrap();
    let e = s
        .task
        .apply_home(&[".config/tool/settings.toml".into()])
        .unwrap_err();
    assert!(
        e.to_string().contains("~/.config/tool/settings.toml"),
        "{e}"
    );
    assert_eq!(s.real(".config/tool/settings.toml"), "x = 9\n");
}

#[test]
fn discarded_changes_leave_the_layer_and_a_clean_layer_is_made_again() {
    let s = Setup::new();
    s.run(SCRIPT);
    let all: Vec<String> = s
        .task
        .home_changes()
        .unwrap()
        .into_iter()
        .map(|c| c.path)
        .collect();
    s.task.discard_home(&all).unwrap();
    assert!(s.task.home_changes().unwrap().is_empty());
    assert!(!s.task.home_layer().join(".newrc").exists());
    assert_eq!(
        fs::read_to_string(s.task.home_layer().join(".zshrc")).unwrap(),
        "export A=1\n"
    );

    // Nothing waits, so the next run starts from the home as it is now.
    fs::write(s.person.path().join(".zshrc"), "export A=3\n").unwrap();
    let out = s.run("cat \"$HOME/.zshrc\"");
    assert_eq!(out, "export A=3\n");
}

#[test]
fn a_layer_with_changes_waiting_is_kept_for_the_next_run() {
    let s = Setup::new();
    s.run("echo 'export B=2' >> \"$HOME/.zshrc\"");
    let out = s.run("cat \"$HOME/.zshrc\"");
    assert_eq!(out, "export A=1\nexport B=2\n");
}

#[test]
fn the_agents_own_state_is_its_own() {
    let s = Setup::new();
    let layer = s.task.home_layer();
    s.run("true");
    let link = fs::read_link(layer.join(".claude")).unwrap();
    assert_eq!(
        fs::canonicalize(link).unwrap(),
        fs::canonicalize(s.person.path().join(".claude")).unwrap()
    );
    assert!(!layer.join(".ssh").exists());
    assert!(!layer.join(".config/gh").exists());
    assert!(Path::new(&layer.join("Documents")).is_symlink());
}

#[test]
fn a_directory_the_person_cannot_read_is_linked_and_the_rest_caught() {
    use std::os::unix::fs::PermissionsExt;
    let s = Setup::new();
    let locked = s.person.path().join(".config/locked");
    fs::create_dir(&locked).unwrap();
    fs::write(locked.join("x"), "x\n").unwrap();
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();
    let out = s.run("echo 'y = 2' >> \"$HOME/.config/tool/settings.toml\" && echo done");
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(out.contains("done"), "{out}");
    assert_eq!(
        s.task.home_changes().unwrap(),
        [change(
            ".config/tool/settings.toml",
            Change::Modified,
            Kind::File
        )]
    );
}

#[test]
fn what_shells_rewrite_on_their_own_is_not_offered() {
    let s = Setup::new();
    s.run(r#"echo dump > "$HOME/.zcompdump-host-5.9"; echo ls >> "$HOME/.zsh_history"; mkdir -p "$HOME/.config/tool" && touch "$HOME/.config/tool/.DS_Store""#);
    assert!(s.task.home_changes().unwrap().is_empty());
}
