//! The home layer: what a command writes in the home directory is caught
//! for review, in a real sandbox, over a home made for the test.

use std::fs;
use std::path::Path;

use kule_engine::{
    CacheSources, Change, HomeChange, Kind, LineKind, Sandbox, Task, TextDiff, add_secret,
};
use tempfile::TempDir;

struct Setup {
    home: TempDir,
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
        write("work/plan.txt", "plan\n");
        write(".claude/settings.json", "{}\n");
        fs::write(ws.path().join("a.txt"), "a\n").unwrap();
        let task = Task::create(home.path(), ws.path()).unwrap();
        Setup {
            home,
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
        self.task
            .prepare_home(
                p,
                &[p.join(".claude")],
                sandbox.hidden(),
                &[".claude".to_owned()],
            )
            .unwrap();
        sandbox.use_home(&self.task, p).unwrap();
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
        startup: path == ".zshrc",
        target: None,
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
    // The person's own files are hidden, and not in the layer either.
    assert!(!layer.join("Documents").exists());
    assert!(Path::new(&layer.join("work")).is_symlink());
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

fn lines(d: TextDiff) -> Vec<(LineKind, String)> {
    let TextDiff::Lines { hunks, .. } = d else {
        panic!("{d:?}");
    };
    hunks
        .into_iter()
        .flat_map(|h| h.lines)
        .map(|l| (l.kind, l.text))
        .collect()
}

#[test]
fn a_change_shows_what_it_does_to_the_real_file_line_by_line() {
    let s = Setup::new();
    s.run(SCRIPT);
    assert_eq!(
        lines(s.task.home_diff(".zshrc").unwrap()),
        [
            (LineKind::Same, "export A=1".into()),
            (LineKind::Added, "export B=2".into()),
        ]
    );
    assert_eq!(
        lines(s.task.home_diff(".newrc").unwrap()),
        [(LineKind::Added, "hi".into())]
    );
    // Only a change waiting, and only a file: never any path in the home.
    for path in [
        ".ssh/id_ed25519",
        "Documents/notes.txt",
        "../etc/passwd",
        ".config/new",
    ] {
        assert!(s.task.home_diff(path).is_err(), "{path}");
    }
}

#[test]
fn an_apply_is_undone_and_its_changes_wait_again() {
    let s = Setup::new();
    s.run(SCRIPT);
    let before = s.task.home_changes().unwrap();
    s.task
        .apply_home(&[
            ".zshrc".into(),
            ".config/new".into(),
            ".config/new/n.toml".into(),
        ])
        .unwrap();
    s.task.apply_home(&[".newrc".into()]).unwrap();
    assert_eq!(
        s.task.home_last_applied().unwrap().unwrap().paths,
        [".newrc"]
    );

    // The last apply first.
    assert_eq!(s.task.undo_home().unwrap().paths, [".newrc"]);
    assert!(!s.person.path().join(".newrc").exists());
    assert_eq!(s.real(".zshrc"), "export A=1\nexport B=2\n");

    let undone = s.task.undo_home().unwrap();
    assert_eq!(
        undone.paths,
        [".config/new", ".config/new/n.toml", ".zshrc"]
    );
    assert_eq!(s.real(".zshrc"), "export A=1\n");
    assert!(!s.person.path().join(".config/new").exists());
    assert_eq!(s.task.home_changes().unwrap(), before);
    assert!(s.task.home_last_applied().unwrap().is_none());
    assert!(s.task.undo_home().is_err());

    // And they apply again.
    s.task.apply_home(&[".zshrc".into()]).unwrap();
    assert_eq!(s.real(".zshrc"), "export A=1\nexport B=2\n");
}

#[test]
fn an_undo_never_takes_what_the_person_changed_since() {
    let s = Setup::new();
    s.run(SCRIPT);
    s.task
        .apply_home(&[
            ".zshrc".into(),
            ".config/new".into(),
            ".config/new/n.toml".into(),
        ])
        .unwrap();
    fs::write(s.person.path().join(".zshrc"), "mine\n").unwrap();
    let e = s.task.undo_home().unwrap_err();
    assert!(e.to_string().contains("~/.zshrc"), "{e}");
    assert_eq!(s.real(".zshrc"), "mine\n");
    assert_eq!(s.real(".config/new/n.toml"), "n\n");

    // A directory the apply made, with the person's file in it now.
    let s = Setup::new();
    s.run(SCRIPT);
    s.task.apply_home(&[".config/new".into()]).unwrap();
    fs::write(s.person.path().join(".config/new/theirs"), "x").unwrap();
    let e = s.task.undo_home().unwrap_err();
    assert!(e.to_string().contains("~/.config/new"), "{e}");
    assert_eq!(s.real(".config/new/theirs"), "x");
}

#[test]
fn a_replaced_link_comes_back_a_link() {
    let s = Setup::new();
    let p = s.person.path();
    fs::write(p.join(".config/tool/real.toml"), "r\n").unwrap();
    fs::remove_file(p.join(".config/tool/settings.toml")).unwrap();
    std::os::unix::fs::symlink("real.toml", p.join(".config/tool/settings.toml")).unwrap();
    s.run("rm \"$HOME/.config/tool/settings.toml\" && echo own > \"$HOME/.config/tool/settings.toml\"");
    s.task
        .apply_home(&[".config/tool/settings.toml".into()])
        .unwrap();
    assert!(!p.join(".config/tool/settings.toml").is_symlink());
    s.task.undo_home().unwrap();
    assert_eq!(
        fs::read_link(p.join(".config/tool/settings.toml")).unwrap(),
        Path::new("real.toml")
    );
}

/// A layer holding changes one agent made waits for them to be reviewed:
/// another agent's run does not take it over, the same agent's does.
#[test]
fn a_layer_with_changes_waiting_is_never_given_to_another_agent() {
    let s = Setup::new();
    s.run(SCRIPT);
    assert!(!s.task.home_changes().unwrap().is_empty());
    // The same agent again: its layer, with what it holds.
    s.run("true");
    assert!(!s.task.home_changes().unwrap().is_empty());
    let p = s.person.path();
    let other = s
        .task
        .prepare_home(p, &[p.join(".codex")], &[], &[".codex".to_owned()]);
    assert!(other.unwrap_err().to_string().contains("apply or discard"));
}

/// What still runs in the home layer, and nowhere else of the task's, is
/// ended before the layer is applied. Where a command's processes are its
/// own, none outlives it to be ended (see the sandbox's tests).
#[test]
fn a_process_left_in_the_home_layer_ends_before_it_is_applied() {
    if kule_engine::own_processes() {
        eprintln!("skipped: what a command leaves running ends with it here");
        return;
    }
    let s = Setup::new();
    let pid: i32 = s
        .run("cd \"$HOME\"; nohup sleep 60 >/dev/null 2>&1 </dev/null & echo $!")
        .trim()
        .parse()
        .unwrap();
    // SAFETY: signal 0 only asks whether the process is there.
    let alive = |pid: i32| unsafe { libc::kill(pid, 0) } == 0;
    assert!(alive(pid));
    s.task.apply_home(&[]).unwrap();
    let started = std::time::Instant::now();
    while alive(pid) && started.elapsed() < std::time::Duration::from_secs(3) {
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    let left = alive(pid);
    // SAFETY: as above; one the test could not end is ended here.
    unsafe { libc::kill(pid, libc::SIGKILL) };
    assert!(!left, "a process left in the home layer outlived the apply");
}

/// A link the agent made in the home shows where it leads before it is
/// applied.
#[test]
fn a_link_in_the_layer_shows_where_it_leads() {
    let s = Setup::new();
    s.run("ln -s \"$HOME/Documents/notes.txt\" \"$HOME/.toolrc\"");
    let changes = s.task.home_changes().unwrap();
    let link = changes
        .iter()
        .find(|c| c.path == ".toolrc")
        .unwrap_or_else(|| panic!("{changes:?}"));
    assert_eq!(link.kind, Kind::Symlink);
    assert!(
        link.target
            .as_deref()
            .is_some_and(|t| t.ends_with("Documents/notes.txt")),
        "{link:?}"
    );
}

/// What is applied lands where the review showed it: a folder on the way
/// in the real home that is a link is not written through.
#[test]
fn nothing_is_applied_through_a_link_in_the_real_home() {
    let s = Setup::new();
    s.run("echo y = 2 > \"$HOME/.config/tool/new.toml\"");
    assert!(
        s.task
            .home_changes()
            .unwrap()
            .iter()
            .any(|c| c.path == ".config/tool/new.toml")
    );
    // The folder becomes a link to somewhere else meanwhile.
    let elsewhere = TempDir::new().unwrap();
    let tool = s.person.path().join(".config/tool");
    fs::rename(&tool, s.person.path().join(".config/tool.old")).unwrap();
    std::os::unix::fs::symlink(elsewhere.path(), &tool).unwrap();

    let refused = s
        .task
        .apply_home(&[".config/tool/new.toml".to_owned()])
        .unwrap_err();
    assert!(refused.to_string().contains("is a link"), "{refused}");
    assert!(!elsewhere.path().join("new.toml").exists());
}

/// A token in a file shells read as they start is masked in the layer, as
/// in the copy's `.env`: the command sees a placeholder, and that is no
/// change. What the command adds there is applied with the token as it was,
/// the diff shows only what it added, and a discarded change comes back
/// masked.
#[test]
fn a_token_in_a_shell_startup_file_is_masked_in_the_layer() {
    let s = Setup::new();
    let p = s.person.path();
    let token = "ghp_k3yL1ke0nlyF0rTest1ng9Abc";
    let zshrc =
        format!("export A=1\nexport GITHUB_TOKEN={token}\nexport PATH=\"$HOME/bin:$PATH\"\n");
    fs::write(p.join(".zshrc"), &zshrc).unwrap();
    fs::write(p.join(".env"), format!("API_KEY={token}\nPORT=3000\n")).unwrap();

    let seen = s.run("cat \"$HOME/.zshrc\" \"$HOME/.env\"");
    assert!(!seen.contains(token), "{seen}");
    // Nor where they are in the real home, by its path.
    let real = s.run(&format!(
        "cat '{}' '{}' 2>/dev/null; true",
        p.join(".zshrc").display(),
        p.join(".env").display()
    ));
    assert!(!real.contains(token), "{real}");
    assert!(seen.contains("export GITHUB_TOKEN=kule_secret_"), "{seen}");
    assert!(
        seen.contains("export A=1") && seen.contains("PORT=3000"),
        "{seen}"
    );
    assert!(s.task.home_changes().unwrap().is_empty());

    s.run("echo \"alias ll='ls -l'\" >> \"$HOME/.zshrc\"");
    let diff = s.task.home_diff(".zshrc").unwrap();
    // The token's line is the same on both sides: masking is no change.
    let shown = format!("{diff:?}");
    assert!(shown.contains("kind: Added, text: \"alias ll"), "{shown}");
    assert!(!shown.contains("kule_secret_"), "{shown}");
    assert!(!shown.contains("Removed"), "{shown}");
    // Nor the value itself: the diff is shown in a terminal or a window,
    // so what looks secret is hidden on both sides.
    assert!(!shown.contains(token), "{shown}");
    assert!(shown.contains("export GITHUB_TOKEN=‹hidden "), "{shown}");
    s.task.apply_home(&[".zshrc".to_owned()]).unwrap();
    assert_eq!(
        fs::read_to_string(p.join(".zshrc")).unwrap(),
        format!("{zshrc}alias ll='ls -l'\n")
    );

    s.run("echo 'export B=2' >> \"$HOME/.zshrc\"");
    s.task.discard_home(&[".zshrc".to_owned()]).unwrap();
    let layer = fs::read_to_string(s.task.home_layer().join(".zshrc")).unwrap();
    assert!(!layer.contains(token), "{layer}");
    assert!(!layer.contains("export B=2"), "{layer}");
}

/// A kept secret inside a line of a startup file (an alias, a command) is
/// masked in the layer, and so is one kept for another folder: the home is
/// the same for every folder. An apply puts the lines back as they were.
#[test]
fn a_kept_secret_inside_a_startup_line_is_masked_and_applied_back() {
    let s = Setup::new();
    let other = TempDir::new().unwrap();
    add_secret(
        s.home.path(),
        "SHOP_KEY",
        "shop-Kept-4821",
        &["api.shop.example".into()],
        None,
    )
    .unwrap();
    add_secret(
        s.home.path(),
        "OTHER_KEY",
        "other-Folder-9",
        &["api.other.example".into()],
        Some(other.path()),
    )
    .unwrap();
    let p = s.person.path();
    let zshrc = "export A=1\n\
                 alias shop='curl -H \"x-key: shop-Kept-4821\" https://api.shop.example'\n\
                 echo other-Folder-9 >/dev/null\n";
    fs::write(p.join(".zshrc"), zshrc).unwrap();

    let seen = s.run("cat \"$HOME/.zshrc\"");
    assert!(
        !seen.contains("shop-Kept-4821") && !seen.contains("other-Folder-9"),
        "{seen}"
    );
    assert!(
        seen.contains("alias shop='curl -H \"x-key: kule_secret_"),
        "{seen}"
    );
    assert!(s.task.home_changes().unwrap().is_empty());

    s.run("echo 'export B=2' >> \"$HOME/.zshrc\"");
    let diff = format!("{:?}", s.task.home_diff(".zshrc").unwrap());
    assert!(!diff.contains("Removed"), "{diff}");
    assert!(
        !diff.contains("shop-Kept-4821") && !diff.contains("other-Folder-9"),
        "{diff}"
    );
    s.task.apply_home(&[".zshrc".to_owned()]).unwrap();
    assert_eq!(s.real(".zshrc"), format!("{zshrc}export B=2\n"));
}

/// A search path in a startup file is a setting, not a credential: it stays
/// as it is in the layer, so a shell that reads the file finds its tools,
/// and the diff shows it. A path under a key that names a secret stays
/// masked, and so does a token that starts with `/` under a key that names
/// no path.
#[test]
fn a_search_path_in_a_startup_file_is_kept_as_it_is() {
    let s = Setup::new();
    let p = s.person.path();
    let path = "$HOME/.cargo/bin:/opt/homebrew/bin:${HOME}/go/bin:$PATH";
    let secret_dir = "/run/secrets/kule/Test1ngOnly";
    let token = "/K7MDENG/bPxRfiCYEXAMPLEKEY";
    let zshrc =
        format!("export PATH=\"{path}\"\nexport SECRET_DIR={secret_dir}\nexport UPLOAD={token}\n");
    fs::write(p.join(".zshrc"), &zshrc).unwrap();

    let seen = s.run(". \"$HOME/.zshrc\"; cat \"$HOME/.zshrc\"; echo \"path=$PATH\"");
    assert!(seen.contains(&format!("export PATH=\"{path}\"")), "{seen}");
    assert!(
        seen.contains("/.cargo/bin:/opt/homebrew/bin:") && !seen.contains("path=kule_secret_"),
        "{seen}"
    );
    assert!(
        !seen.contains(secret_dir) && !seen.contains(token),
        "{seen}"
    );
    assert!(
        seen.contains("export SECRET_DIR=kule_secret_")
            && seen.contains("export UPLOAD=kule_secret_"),
        "{seen}"
    );
    assert!(s.task.home_changes().unwrap().is_empty());

    s.run("echo 'export B=2' >> \"$HOME/.zshrc\"");
    let diff = format!("{:?}", s.task.home_diff(".zshrc").unwrap());
    assert!(diff.contains(path), "{diff}");
    assert!(
        !diff.contains(secret_dir) && !diff.contains(token),
        "{diff}"
    );
    s.task.apply_home(&[".zshrc".to_owned()]).unwrap();
    assert_eq!(s.real(".zshrc"), format!("{zshrc}export B=2\n"));
}

/// What the home layer holds is not applied while a terminal Kule opened
/// is open on the copy: applying would end what runs there.
#[test]
fn home_changes_wait_while_a_terminal_is_open() {
    let s = Setup::new();
    s.run(SCRIPT);
    let open = s.task.terminal_opened().unwrap();
    let e = s.task.apply_home(&[".newrc".into()]).unwrap_err();
    assert!(e.to_string().contains("terminal is open"), "{e}");
    assert_eq!(s.real(".newrc"), "");
    drop(open);
    s.task.apply_home(&[".newrc".into()]).unwrap();
    assert_eq!(s.real(".newrc"), "hi\n");
}
