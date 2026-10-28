//! The home layer: what a command writes in the home directory is caught
//! for review, in a real sandbox, over a home made for the test.
#![cfg(unix)]

use std::fs;
use std::path::Path;

use kule_engine::{
    CacheSources, Change, HomeChange, Kind, LineKind, Sandbox, Task, TextDiff, add_secret,
};
use tempfile::TempDir;

/// [`Task::apply_home`] by a person who reviewed the home changes as they
/// are now.
trait Reviewed {
    fn apply_reviewed(&self, paths: &[String]) -> kule_engine::Result<kule_engine::HomeApplied>;
}

impl Reviewed for Task {
    fn apply_reviewed(&self, paths: &[String]) -> kule_engine::Result<kule_engine::HomeApplied> {
        let digest = kule_engine::home_digest(&self.home_changes()?);
        self.apply_home(paths, &digest)
    }
}

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
        write(".config/git/settings.toml", "x = 1\n");
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

    /// Runs `script` in the sandbox with the home layer as `HOME`, for an
    /// agent whose state is `.claude`. How it ended goes to the test's
    /// output, shown when the test fails.
    fn run(&self, script: &str) -> String {
        self.run_as(&[".claude"], script)
    }

    /// [`Setup::run`], for an agent whose state is `state`.
    fn run_as(&self, state: &[&str], script: &str) -> String {
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
                &state.iter().map(|s| p.join(s)).collect::<Vec<_>>(),
                sandbox.hidden(),
                &sandbox.shown(),
                &state.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>(),
            )
            .unwrap();
        sandbox.use_home(&self.task, p).unwrap();
        let out = sandbox
            .command("sh", &["-c".to_owned(), script.to_owned()])
            .unwrap()
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
        eprintln!(
            "--- run ({}):\n{script}\n--- stdout:\n{stdout}--- stderr:\n{}",
            out.status,
            String::from_utf8_lossy(&out.stderr)
        );
        stdout
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
        hash: String::new(),
    }
}

/// `changes` as [`change`] makes them: what they hold left out.
fn unhashed(changes: Vec<HomeChange>) -> Vec<HomeChange> {
    changes
        .into_iter()
        .map(|c| HomeChange {
            hash: String::new(),
            ..c
        })
        .collect()
}

const SCRIPT: &str = r#"
echo 'export B=2' >> "$HOME/.zshrc"
echo 'y = 2' >> "$HOME/.config/git/settings.toml"
mkdir -p "$HOME/.config/new" && echo n > "$HOME/.config/new/n.toml"
echo hi > "$HOME/.newrc"
echo state > "$HOME/.claude.json.tmp.1"
(echo leak >> "$HOME/Documents/notes.txt") 2>/dev/null || echo refused-documents
cat "$HOME/.config/gh/hosts.yml" 2>/dev/null || echo no-gh
ls "$HOME/.ssh" 2>/dev/null || echo no-ssh
cat "$HOME/.config/git/settings.toml"
"#;

#[test]
fn writes_in_the_home_are_caught_and_the_real_one_untouched() {
    let s = Setup::new();
    let out = s.run(SCRIPT);
    for expected in ["refused-documents", "no-gh", "no-ssh", "x = 1\ny = 2"] {
        assert!(out.contains(expected), "{expected} in {out}");
    }
    assert_eq!(s.real(".zshrc"), "export A=1\n");
    assert_eq!(s.real(".config/git/settings.toml"), "x = 1\n");
    assert_eq!(s.real("Documents/notes.txt"), "notes\n");
    assert!(!s.person.path().join(".newrc").exists());
    assert_eq!(
        unhashed(s.task.home_changes().unwrap()),
        [
            change(".config/git/settings.toml", Change::Modified, Kind::File),
            change(".config/new", Change::Created, Kind::Dir),
            change(".config/new/n.toml", Change::Created, Kind::File),
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
        .apply_reviewed(&[".zshrc".into(), ".newrc".into()])
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
    fs::write(s.person.path().join(".config/git/settings.toml"), "x = 9\n").unwrap();
    let e = s
        .task
        .apply_reviewed(&[".config/git/settings.toml".into()])
        .unwrap_err();
    assert!(e.to_string().contains("~/.config/git/settings.toml"), "{e}");
    assert_eq!(s.real(".config/git/settings.toml"), "x = 9\n");
}

/// An apply takes the home changes the person reviewed, and only those: a
/// change written after the review refuses it, and nothing reaches the
/// home.
#[test]
fn an_apply_takes_only_the_home_changes_reviewed() {
    let s = Setup::new();
    s.run(SCRIPT);
    let reviewed = kule_engine::home_digest(&s.task.home_changes().unwrap());
    s.run("echo 'export C=3' >> \"$HOME/.zshrc\"");
    let e = s
        .task
        .apply_home(&[".zshrc".into()], &reviewed)
        .unwrap_err();
    assert!(e.to_string().contains("review them again"), "{e}");
    assert_eq!(s.real(".zshrc"), "export A=1\n");
    // Reviewed again, it goes.
    let again = kule_engine::home_digest(&s.task.home_changes().unwrap());
    s.task.apply_home(&[".zshrc".into()], &again).unwrap();
    assert_eq!(s.real(".zshrc"), "export A=1\nexport B=2\nexport C=3\n");
}

/// As a commit: nothing comes to run as its owner or group in the home
/// where what it replaces did not, and nothing at all is applied then.
#[test]
fn an_apply_never_brings_setuid_into_the_home() {
    use std::os::unix::fs::PermissionsExt;
    let s = Setup::new();
    s.run("echo tool > \"$HOME/.newrc\"; echo 'export B=2' >> \"$HOME/.zshrc\"");
    // The sandbox refuses the bit itself; set as anything outside it could.
    let layer = s.task.home_layer().join(".newrc");
    fs::set_permissions(&layer, fs::Permissions::from_mode(0o4755)).unwrap();
    let changes = s.task.home_changes().unwrap();
    let e = s
        .task
        .apply_home(
            &[".zshrc".into(), ".newrc".into()],
            &kule_engine::home_digest(&changes),
        )
        .unwrap_err();
    assert!(e.to_string().contains("setuid"), "{e}");
    assert_eq!(s.real(".zshrc"), "export A=1\n");
    assert!(!s.person.path().join(".newrc").exists());
}

/// An undo puts nothing back through a link on the way: a folder swapped
/// for a link to a look-alike after the apply would lead the undo there.
#[test]
fn an_undo_refuses_a_link_on_the_way() {
    let s = Setup::new();
    s.run(SCRIPT);
    s.task
        .apply_reviewed(&[
            ".config/new".into(),
            ".config/new/n.toml".into(),
            ".newrc".into(),
        ])
        .unwrap();
    let p = s.person.path();
    let elsewhere = TempDir::new_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    fs::rename(p.join(".config"), elsewhere.path().join(".config")).unwrap();
    std::os::unix::fs::symlink(elsewhere.path().join(".config"), p.join(".config")).unwrap();
    let e = s.task.undo_home().unwrap_err();
    assert!(
        e.to_string().contains("~/.config is a link in your home"),
        "{e}"
    );
    assert!(elsewhere.path().join(".config/new/n.toml").exists());
    // Refused before anything was put back, not halfway.
    assert_eq!(s.real(".newrc"), "hi\n");
}

/// Kule writes in the home through the folders on the way, opened without
/// following a link: one found on the way is refused, never written
/// through.
#[test]
fn an_apply_writes_through_no_link_on_the_way() {
    let s = Setup::new();
    s.run(SCRIPT);
    let p = s.person.path();
    // The same folder, moved and linked to: every file in it is as it was.
    let elsewhere = TempDir::new_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    fs::rename(p.join(".config"), elsewhere.path().join(".config")).unwrap();
    std::os::unix::fs::symlink(elsewhere.path().join(".config"), p.join(".config")).unwrap();
    let e = s
        .task
        .apply_reviewed(&[".config/git/settings.toml".into()])
        .unwrap_err();
    assert!(
        e.to_string().contains("~/.config is a link in your home"),
        "{e}"
    );
    assert_eq!(
        fs::read_to_string(elsewhere.path().join(".config/git/settings.toml")).unwrap(),
        "x = 1\n"
    );
}

// Filesystems keep time in ticks (milliseconds on Linux ext4, two seconds on
// FAT), so a same-size save right after Kule reads a stamp can leave the
// stamp as it was. The test below repeats a race that a coarse clock loses
// often; on a nanosecond clock (APFS) it passes either way.
const SAME_TICK_TRIES: usize = 20;

#[test]
fn a_same_size_save_while_the_layer_is_made_is_not_overwritten() {
    for _ in 0..SAME_TICK_TRIES {
        let s = Setup::new();
        let p = s.person.path();
        // Many settings to clone before the one saved again: in `.config`,
        // which is cloned entry by entry (a dot file at the top that no
        // toolchain reads is not in the layer at all).
        for i in 0..500 {
            fs::write(p.join(format!(".config/b{i}")), "b\n").unwrap();
        }
        let a = p.join(".zshrc");
        fs::write(&a, "1\n").unwrap();
        // Saves .a again as soon as its clone appears, while the rest of the
        // home is still being cloned. Gives up after a while, so a layer
        // made elsewhere fails the test instead of hanging.
        let writer = {
            let (layer, a) = (s.task.home_layer(), a.clone());
            std::thread::spawn(move || {
                let started = std::time::Instant::now();
                while started.elapsed() < std::time::Duration::from_secs(30) {
                    if layer.join(".zshrc").exists() {
                        fs::write(&a, "2\n").unwrap();
                        return;
                    }
                    std::thread::yield_now();
                }
                panic!("the layer never appeared");
            })
        };
        // Right after the first save, so the clone reads its stamp within
        // the same tick where the clock is coarse.
        s.task
            .prepare_home(p, &[p.join(".claude")], &[], &[], &[".claude".to_owned()])
            .unwrap();
        writer.join().unwrap();
        let layer = s.task.home_layer();
        let cloned = fs::read_to_string(layer.join(".zshrc")).unwrap();
        fs::write(layer.join(".zshrc"), "agent\n").unwrap();
        match s.task.apply_reviewed(&[".zshrc".to_owned()]) {
            Err(e) => {
                assert!(e.to_string().contains("~/.zshrc"), "{e}");
                assert_eq!(s.real(".zshrc"), "2\n");
            }
            Ok(_) => assert_eq!(cloned, "2\n", "the person's save was overwritten unseen"),
        }
    }
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

/// The agent's state is a copy in the layer: it reads what the person has
/// there and writes to it, and nothing of that reaches the person's home or
/// is offered to be applied.
#[test]
fn the_agents_own_state_is_its_own() {
    let s = Setup::new();
    let layer = s.task.home_layer();
    let out = s.run(
        r#"cat "$HOME/.claude/settings.json"; echo evil > "$HOME/.claude/settings.json"; mkdir -p "$HOME/.claude/shell-snapshots" && echo evil > "$HOME/.claude/shell-snapshots/s.sh" && echo wrote"#,
    );
    assert_eq!(out, "{}\nwrote\n");
    assert!(
        !fs::symlink_metadata(layer.join(".claude"))
            .unwrap()
            .is_symlink()
    );
    assert_eq!(s.real(".claude/settings.json"), "{}\n");
    assert!(!s.person.path().join(".claude/shell-snapshots").exists());
    assert!(s.task.home_changes().unwrap().is_empty());
    assert!(!layer.join(".ssh").exists());
    assert!(!layer.join(".config/gh").exists());
    // The person's own files are hidden, and not in the layer either: what
    // lies at the top of the home but its settings too.
    assert!(!layer.join("Documents").exists());
    assert!(fs::symlink_metadata(layer.join("work")).is_err());
}

#[test]
fn a_directory_the_person_cannot_read_is_linked_and_the_rest_caught() {
    use std::os::unix::fs::PermissionsExt;
    let s = Setup::new();
    let locked = s.person.path().join(".config/fish");
    fs::create_dir(&locked).unwrap();
    fs::write(locked.join("x"), "x\n").unwrap();
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();
    let out = s.run("echo 'y = 2' >> \"$HOME/.config/git/settings.toml\" && echo done");
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(out.contains("done"), "{out}");
    assert_eq!(
        unhashed(s.task.home_changes().unwrap()),
        [change(
            ".config/git/settings.toml",
            Change::Modified,
            Kind::File
        )]
    );
}

#[test]
fn what_shells_rewrite_on_their_own_is_not_offered() {
    let s = Setup::new();
    s.run(r#"echo dump > "$HOME/.zcompdump-host-5.9"; echo ls >> "$HOME/.zsh_history"; mkdir -p "$HOME/.config/git" && touch "$HOME/.config/git/.DS_Store""#);
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
        .apply_reviewed(&[
            ".zshrc".into(),
            ".config/new".into(),
            ".config/new/n.toml".into(),
        ])
        .unwrap();
    s.task.apply_reviewed(&[".newrc".into()]).unwrap();
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
    s.task.apply_reviewed(&[".zshrc".into()]).unwrap();
    assert_eq!(s.real(".zshrc"), "export A=1\nexport B=2\n");
}

#[test]
fn an_undo_never_takes_what_the_person_changed_since() {
    let s = Setup::new();
    s.run(SCRIPT);
    s.task
        .apply_reviewed(&[
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
    s.task.apply_reviewed(&[".config/new".into()]).unwrap();
    fs::write(s.person.path().join(".config/new/theirs"), "x").unwrap();
    let e = s.task.undo_home().unwrap_err();
    assert!(e.to_string().contains("~/.config/new"), "{e}");
    assert_eq!(s.real(".config/new/theirs"), "x");
}

#[test]
fn a_replaced_link_comes_back_a_link() {
    let s = Setup::new();
    let p = s.person.path();
    fs::write(p.join(".config/git/real.toml"), "r\n").unwrap();
    fs::remove_file(p.join(".config/git/settings.toml")).unwrap();
    std::os::unix::fs::symlink("real.toml", p.join(".config/git/settings.toml")).unwrap();
    s.run(
        "rm \"$HOME/.config/git/settings.toml\" && echo own > \"$HOME/.config/git/settings.toml\"",
    );
    s.task
        .apply_reviewed(&[".config/git/settings.toml".into()])
        .unwrap();
    assert!(!p.join(".config/git/settings.toml").is_symlink());
    s.task.undo_home().unwrap();
    assert_eq!(
        fs::read_link(p.join(".config/git/settings.toml")).unwrap(),
        Path::new("real.toml")
    );
}

/// State deeper in the home is a copy too: each folder on the way becomes
/// the layer's own, with the rest in it still the person's, read-only. State
/// the person has none of yet is made in the layer, unoffered.
#[test]
fn state_deep_in_the_home_is_a_copy_and_the_rest_beside_it_the_persons() {
    let s = Setup::new();
    let p = s.person.path();
    fs::create_dir_all(p.join(".local/state/tool")).unwrap();
    fs::write(p.join(".local/state/tool/log"), "old\n").unwrap();
    fs::create_dir_all(p.join(".local/share/uv")).unwrap();
    fs::write(p.join(".local/share/uv/data"), "data\n").unwrap();
    let out = s.run_as(
        &[".local/state/tool", "Library/Caches/tool", ".claude"],
        r#"cat "$HOME/.local/state/tool/log"
echo new >> "$HOME/.local/state/tool/log" && echo state-written
cat "$HOME/.local/share/uv/data"
(echo leak >> "$HOME/.local/share/uv/data") 2>/dev/null || echo refused-beside
mkdir -p "$HOME/Library/Caches/tool" && echo c > "$HOME/Library/Caches/tool/c" && echo cache-written"#,
    );
    assert_eq!(
        out,
        "old\nstate-written\ndata\nrefused-beside\ncache-written\n"
    );
    assert_eq!(s.real(".local/state/tool/log"), "old\n");
    assert_eq!(s.real(".local/share/uv/data"), "data\n");
    assert!(!p.join("Library").exists());
    assert!(s.task.home_changes().unwrap().is_empty());
}

/// A layer made for another agent holds none of what the first one wrote
/// in its state, its login included.
#[test]
fn another_agents_layer_holds_nothing_of_the_first_ones_state() {
    let s = Setup::new();
    s.run(r#"echo token > "$HOME/.claude/.credentials.json""#);
    let layer = s.task.home_layer();
    assert!(layer.join(".claude/.credentials.json").exists());
    s.run_as(&[".codex"], "true");
    // Another agent's folder is no toolchain's: not in this layer at all.
    assert!(fs::symlink_metadata(layer.join(".claude")).is_err());
    assert!(!s.person.path().join(".claude/.credentials.json").exists());
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
        .prepare_home(p, &[p.join(".codex")], &[], &[], &[".codex".to_owned()]);
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
    s.task.apply_reviewed(&[]).unwrap();
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
    s.run("echo y = 2 > \"$HOME/.config/git/new.toml\"");
    assert!(
        s.task
            .home_changes()
            .unwrap()
            .iter()
            .any(|c| c.path == ".config/git/new.toml")
    );
    // The folder becomes a link to somewhere else meanwhile.
    let elsewhere = TempDir::new().unwrap();
    let tool = s.person.path().join(".config/git");
    fs::rename(&tool, s.person.path().join(".config/git.old")).unwrap();
    std::os::unix::fs::symlink(elsewhere.path(), &tool).unwrap();

    let refused = s
        .task
        .apply_reviewed(&[".config/git/new.toml".to_owned()])
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
    s.task.apply_reviewed(&[".zshrc".to_owned()]).unwrap();
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

/// A startup file Kule cannot mask (not UTF-8) is withheld from the
/// commands, there and at its real path, rather than shown with its token:
/// Kule says which file and why, offers no change for it, and the real file
/// stays as it was.
#[test]
fn a_startup_file_kule_cannot_mask_is_withheld_and_told() {
    let s = Setup::new();
    let p = s.person.path();
    let token = "ghp_k3yL1ke0nlyF0rTest1ng9Latin";
    let mut bashrc = b"# caf\xe9\n".to_vec();
    bashrc.extend_from_slice(format!("export GITHUB_TOKEN={token}\n").as_bytes());
    fs::write(p.join(".bashrc"), &bashrc).unwrap();
    // Too large to mask, at the top and in a captured folder: neither
    // cloned nor linked to the real one.
    let mut big = format!("export NPM_TOKEN={token}\n").into_bytes();
    big.extend(std::iter::repeat_n(b'#', 1024 * 1024 + 10));
    fs::write(p.join(".profile"), &big).unwrap();
    fs::create_dir_all(p.join(".config/fish")).unwrap();
    fs::write(p.join(".config/fish/.env"), &big).unwrap();

    let seen = s.run(&format!(
        "head -c 200 \"$HOME/.bashrc\" \"$HOME/.profile\" \"$HOME/.config/fish/.env\" '{}' '{}' 2>/dev/null; echo done",
        p.join(".bashrc").display(),
        p.join(".profile").display()
    ));
    assert!(!seen.contains(token), "{seen}");
    assert!(seen.contains("done"), "{seen}");
    assert_eq!(
        s.task.home_withheld().unwrap(),
        [
            (".bashrc".to_owned(), "not UTF-8".to_owned()),
            (
                ".config/fish/.env".to_owned(),
                "larger than 1 MB".to_owned()
            ),
            (".profile".to_owned(), "larger than 1 MB".to_owned()),
        ]
    );
    assert!(s.task.home_changes().unwrap().is_empty());
    assert_eq!(fs::read(p.join(".bashrc")).unwrap(), bashrc);
    // The masked startup file beside it is shown as ever.
    assert!(s.run("cat \"$HOME/.zshrc\"").contains("export A=1"));
}

/// Made with nothing hidden, the layer still never links a settings file
/// it cannot mask: one too large, right in `.config`, is withheld.
#[test]
fn a_settings_file_too_large_to_mask_is_never_linked() {
    let s = Setup::new();
    let p = s.person.path();
    let mut big = b"API_KEY=ghp_k3yL1ke0nlyF0rTest1ng9Large\n".to_vec();
    big.extend(std::iter::repeat_n(b'#', 1024 * 1024 + 10));
    fs::write(p.join(".config/app.env"), &big).unwrap();
    let layer = s.task.prepare_home(p, &[], &[], &[], &[]).unwrap();
    assert!(fs::symlink_metadata(layer.join(".config/app.env")).is_err());
    assert_eq!(
        s.task.home_withheld().unwrap(),
        [(".config/app.env".to_owned(), "larger than 1 MB".to_owned())]
    );
    assert!(
        s.task
            .home_masked()
            .unwrap()
            .contains(&p.join(".config/app.env"))
    );
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
    s.task.apply_reviewed(&[".zshrc".to_owned()]).unwrap();
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
    s.task.apply_reviewed(&[".zshrc".to_owned()]).unwrap();
    assert_eq!(s.real(".zshrc"), format!("{zshrc}export B=2\n"));
}

/// What the home layer holds is not applied while a terminal Kule opened
/// is open on the copy: applying would end what runs there.
#[test]
fn home_changes_wait_while_a_terminal_is_open() {
    let s = Setup::new();
    s.run(SCRIPT);
    let open = s.task.terminal_opened().unwrap();
    let e = s.task.apply_reviewed(&[".newrc".into()]).unwrap_err();
    assert!(e.to_string().contains("terminal is open"), "{e}");
    assert_eq!(s.real(".newrc"), "");
    drop(open);
    let at_work = s.task.agent_at_work().unwrap();
    let e = s.task.apply_reviewed(&[".newrc".into()]).unwrap_err();
    assert!(e.to_string().contains("agent is at work"), "{e}");
    assert_eq!(s.real(".newrc"), "");
    drop(at_work);
    s.task.apply_reviewed(&[".newrc".into()]).unwrap();
    assert_eq!(s.real(".newrc"), "hi\n");

    // Nor is it undone while an agent is at work, whose commands write in
    // the layer.
    let at_work = s.task.agent_at_work().unwrap();
    let e = s.task.undo_home().unwrap_err();
    assert!(e.to_string().contains("agent is at work"), "{e}");
    drop(at_work);

    // Undoing it waits the same way.
    let open = s.task.terminal_opened().unwrap();
    let e = s.task.undo_home().unwrap_err();
    assert!(e.to_string().contains("terminal is open"), "{e}");
    assert_eq!(s.real(".newrc"), "hi\n");
    drop(open);
    s.task.undo_home().unwrap();
    assert!(!s.person.path().join(".newrc").exists());
}

/// NuGet skips its one-time migration, whose mutex lives in /tmp where a
/// task does not write on macOS: the layer carries the marker that says it
/// ran, Kule's own, never offered as a change of the home.
#[test]
fn the_layer_carries_nugets_migration_marker_as_kules_own() {
    let s = Setup::new();
    let out = s.run("[ -f \"$HOME/.local/share/NuGet/Migrations/1\" ] && echo there");
    assert_eq!(out.trim(), "there");
    let changes = s.task.home_changes().unwrap();
    assert!(
        !changes
            .iter()
            .any(|c| c.path.starts_with(".local/share/NuGet")),
        "{changes:?}"
    );
    assert!(!s.person.path().join(".local/share/NuGet").exists());
}
