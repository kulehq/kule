//! The sandbox on Windows: real commands in a real process container, and
//! what reached the disk, the network and the process list. The container
//! needs a desktop session and a measured build (see
//! `windows::preconditions`): where it is not available the tests say so
//! and return, unless `KULE_TEST_REQUIRE_SANDBOX` is set, which turns every
//! such skip into a failure (the Windows test machine sets it).
#![cfg(windows)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Output;

use kule_engine::{CacheSources, Io, Network, Sandbox, Task};
use tempfile::TempDir;

struct Setup {
    home: TempDir,
    _ws: TempDir,
    /// A directory outside the task, standing for the rest of the machine.
    outside: TempDir,
    task: Task,
}

impl Setup {
    fn new() -> Self {
        let home = TempDir::new().unwrap();
        let ws = TempDir::new().unwrap();
        fs::write(ws.path().join("README.md"), "hello\n").unwrap();
        let task = Task::create(home.path(), ws.path()).unwrap();
        Setup {
            home,
            _ws: ws,
            outside: TempDir::new().unwrap(),
            task,
        }
    }

    fn outside(&self) -> PathBuf {
        kule_engine::osfs::plain(&fs::canonicalize(self.outside.path()).unwrap())
    }

    fn sandbox(&self) -> Sandbox {
        Sandbox::new(&self.task).unwrap()
    }
}

/// Whether a container can be made here; says why not otherwise.
fn available() -> bool {
    let refused = kule_engine::sandbox_refusals();
    if refused.is_empty() {
        return true;
    }
    assert!(
        std::env::var_os("KULE_TEST_REQUIRE_SANDBOX").is_none(),
        "the sandbox is required here, but: {}",
        refused.join("; ")
    );
    eprintln!("skipped: {}", refused.join("; "));
    false
}

fn cmd(sandbox: &Sandbox, script: &str) -> Output {
    sandbox
        .start(
            "cmd.exe",
            &[
                "/d".to_owned(),
                "/s".to_owned(),
                "/c".to_owned(),
                script.to_owned(),
            ],
        )
        .unwrap()
        .output()
        .unwrap()
}

fn ok(out: &Output) -> bool {
    out.status.success()
}

fn text(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

fn q(path: &Path) -> String {
    format!("\"{}\"", path.display())
}

/// Writes land in the copy and the task's temporary folder, and nowhere
/// else; a write outside fails and leaves nothing.
#[test]
fn writes_land_only_in_the_copy_and_the_tmp() {
    if !available() {
        return;
    }
    let s = Setup::new();
    let sandbox = s.sandbox();
    let copy = s.task.copy();
    let out = cmd(&sandbox, "echo a> a.txt && type a.txt");
    assert!(ok(&out), "{}", text(&out));
    assert_eq!(fs::read_to_string(copy.join("a.txt")).unwrap().trim(), "a");
    let out = cmd(
        &sandbox,
        "echo t> \"%TMPDIR%\\t.txt\" && type \"%TMPDIR%\\t.txt\"",
    );
    assert!(ok(&out), "{}", text(&out));
    let elsewhere = s.outside().join("x.txt");
    let out = cmd(&sandbox, &format!("echo x> {}", q(&elsewhere)));
    assert!(!ok(&out), "wrote outside the task: {}", text(&out));
    assert!(
        !elsewhere.exists(),
        "the file reached {}",
        elsewhere.display()
    );
}

/// A folder opened to writing is written; a path hidden inside it cannot
/// be read, while its neighbours can. Breaks when the hidden paths inside
/// a grant are not denied.
#[test]
fn hidden_paths_inside_a_grant_cannot_be_read() {
    if !available() {
        return;
    }
    let s = Setup::new();
    let outside = s.outside();
    fs::create_dir_all(outside.join("secret")).unwrap();
    fs::write(outside.join("secret").join("key"), "s3cret\n").unwrap();
    fs::write(outside.join("plain.txt"), "plain\n").unwrap();
    let mut sandbox = s.sandbox();
    sandbox.allow_write(&outside).unwrap();
    sandbox.hide(&outside.join("secret")).unwrap();
    let out = cmd(&sandbox, &format!("type {}", q(&outside.join("plain.txt"))));
    assert!(ok(&out) && text(&out).contains("plain"), "{}", text(&out));
    let out = cmd(
        &sandbox,
        &format!("type {}", q(&outside.join("secret").join("key"))),
    );
    assert!(!ok(&out), "read the hidden file: {}", text(&out));
    assert!(!text(&out).contains("s3cret"), "{}", text(&out));
    let out = cmd(&sandbox, &format!("echo w> {}", q(&outside.join("w.txt"))));
    assert!(ok(&out), "{}", text(&out));
}

/// From the copy, the way up leads to Kule's home, where the daemon's
/// token and the other tasks lie: listing the copy works, reading
/// anything of Kule's own does not.
#[test]
fn kules_home_above_the_copy_cannot_be_read() {
    if !available() {
        return;
    }
    let s = Setup::new();
    let token = s.home.path().join("kuled.token");
    fs::write(&token, "t0ken\n").unwrap();
    let sandbox = s.sandbox();
    let out = cmd(&sandbox, "dir /b");
    assert!(
        ok(&out) && text(&out).contains("README.md"),
        "{}",
        text(&out)
    );
    let out = cmd(&sandbox, &format!("type {}", q(&token)));
    assert!(!ok(&out), "read the daemon's token: {}", text(&out));
    assert!(!text(&out).contains("t0ken"));
    let home = kule_engine::osfs::plain(&fs::canonicalize(s.home.path()).unwrap());
    let out = cmd(
        &sandbox,
        &format!("type {}", q(&home.join("tasks").join("x"))),
    );
    assert!(!ok(&out));
}

/// With the network shut, nothing is reached: not this computer's own
/// listeners either.
#[test]
fn a_shut_network_reaches_no_listener_of_this_computer() {
    if !available() {
        return;
    }
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let s = Setup::new();
    let mut sandbox = s.sandbox();
    sandbox.set_network(Network::Deny);
    let out = cmd(
        &sandbox,
        &format!("curl.exe -s -S -m 5 -o NUL -w %%{{http_code}} http://127.0.0.1:{port}/"),
    );
    assert!(!ok(&out), "reached the host's listener: {}", text(&out));
    let out = cmd(&sandbox, "curl.exe -s -S -m 5 -o NUL http://example.com/");
    assert!(!ok(&out), "reached the network: {}", text(&out));
}

/// What was set for Kule itself stays out of the command's environment;
/// what tools need, and the task's folders, are there.
#[test]
fn kules_own_environment_stays_out() {
    if !available() {
        return;
    }
    // SAFETY: set before any command starts in this test; other tests read
    // no such name.
    unsafe {
        std::env::set_var("KULE_TEST_PLANTED_TOKEN", "ghp_planted");
    }
    let s = Setup::new();
    let sandbox = s.sandbox();
    let out = cmd(
        &sandbox,
        "echo [%KULE_TEST_PLANTED_TOKEN%] [%SystemRoot%] [%TMPDIR%] [%PATH%]",
    );
    assert!(ok(&out), "{}", text(&out));
    let printed = text(&out);
    assert!(printed.contains("[%KULE_TEST_PLANTED_TOKEN%]"), "{printed}");
    assert!(!printed.contains("ghp_planted"), "{printed}");
    assert!(!printed.contains("[%SystemRoot%]"), "{printed}");
    let tmp = kule_engine::osfs::plain(&s.task.tmp().unwrap())
        .display()
        .to_string();
    assert!(
        printed.to_lowercase().contains(&tmp.to_lowercase()),
        "{printed} does not name {tmp}"
    );
}

/// A process the command left in the background is gone with the command:
/// the container's job ends it.
#[test]
fn what_a_command_leaves_running_ends_with_it() {
    if !available() {
        return;
    }
    let s = Setup::new();
    let sandbox = s.sandbox();
    // A wait value of its own, to tell this ping from any other.
    let mark = 4000 + (std::process::id() % 1000);
    let out = cmd(
        &sandbox,
        &format!("start /b ping.exe -t -w {mark} 127.0.0.1 >nul & echo started"),
    );
    assert!(text(&out).contains("started"), "{}", text(&out));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    let left = loop {
        let list = std::process::Command::new("powershell.exe")
            .args([
                "-NoProfile",
                "-Command",
                "(Get-CimInstance Win32_Process -Filter \"name='ping.exe'\").CommandLine",
            ])
            .output()
            .unwrap();
        let left = String::from_utf8_lossy(&list.stdout).contains(&format!("-w {mark}"));
        if !left || std::time::Instant::now() > deadline {
            break left;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    };
    assert!(!left, "a process the command started outlived it");
}

/// A program named with its folder is run from there (a batch file in the
/// copy, under cmd.exe); a bare name is looked for on the person's PATH,
/// never in the copy, where a repository could plant it.
#[test]
fn programs_come_from_the_path_or_an_explicit_folder() {
    if !available() {
        return;
    }
    let s = Setup::new();
    let copy = s.task.copy();
    fs::write(
        copy.join("hello.cmd"),
        "@echo off\r\necho hello-from-batch %1\r\n",
    )
    .unwrap();
    fs::write(copy.join("where.exe"), b"").unwrap();
    let sandbox = s.sandbox();
    let out = sandbox
        .start(".\\hello.cmd", &["arg".to_owned()])
        .unwrap()
        .output()
        .unwrap();
    assert!(
        ok(&out) && text(&out).contains("hello-from-batch arg"),
        "{}",
        text(&out)
    );
    let refused = sandbox
        .start(".\\hello.cmd", &["a&b".to_owned()])
        .unwrap()
        .output();
    assert!(
        refused.is_err(),
        "cmd metacharacters were passed to a batch file"
    );
    // `where` is the system's, not the copy's empty file.
    let out = sandbox
        .start("where", &["cmd.exe".to_owned()])
        .unwrap()
        .output()
        .unwrap();
    assert!(
        ok(&out) && text(&out).to_lowercase().contains("system32"),
        "{}",
        text(&out)
    );
    assert_eq!(
        sandbox
            .start("no-such-program-kule", &[])
            .unwrap()
            .output()
            .unwrap_err()
            .kind(),
        std::io::ErrorKind::NotFound
    );
}

/// Piped streams reach the caller as the command writes them, and the
/// process ends when waited for.
#[test]
fn piped_streams_reach_the_caller() {
    if !available() {
        return;
    }
    let s = Setup::new();
    let sandbox = s.sandbox();
    let mut start = sandbox
        .start(
            "cmd.exe",
            &[
                "/d".into(),
                "/v:on".into(),
                "/c".into(),
                "set /p x= & echo got:!x! & echo err 1>&2".into(),
            ],
        )
        .unwrap();
    start.stdin(Io::Piped).stdout(Io::Piped).stderr(Io::Piped);
    let mut process = start.spawn().unwrap();
    assert!(process.id() > 0);
    {
        let mut stdin = process.stdin.take().unwrap();
        std::io::Write::write_all(&mut stdin, b"typed\r\n").unwrap();
    }
    let mut out = String::new();
    std::io::Read::read_to_string(&mut process.stdout.take().unwrap(), &mut out).unwrap();
    let mut err = String::new();
    std::io::Read::read_to_string(&mut process.stderr.take().unwrap(), &mut err).unwrap();
    assert!(process.wait().unwrap().success());
    assert!(out.contains("got:typed"), "{out}");
    assert!(err.contains("err"), "{err}");
}

/// Where this computer gives no sandbox at all, no command runs, and
/// accepting weaker isolation changes nothing: that acceptance is for
/// what a sandbox lacks, not for having none. Runs everywhere.
#[test]
fn no_sandbox_is_refused_whatever_the_person_accepted() {
    let s = Setup::new();
    kule_engine::set_isolation(
        s.home.path(),
        &kule_engine::Isolation {
            accept_weaker: true,
        },
    )
    .unwrap();
    let mut sandbox = s.sandbox();
    sandbox.refuse_for_test("this test's computer gives no container");
    let error = sandbox
        .start("cmd.exe", &["/c".to_owned(), "exit 0".to_owned()])
        .expect_err("ran with no sandbox");
    let text = error.to_string();
    assert!(text.contains("refused"), "{text}");
    assert!(
        text.contains("this test's computer gives no container"),
        "{text}"
    );
}

/// The credentials Windows tools keep in the home, and Windows's own
/// stores of the person's secrets, are hidden by the default sandbox.
#[test]
fn the_windows_credential_stores_are_hidden() {
    let s = Setup::new();
    let person = TempDir::new().unwrap();
    let sandbox = Sandbox::for_task_with(
        &s.task,
        Some(CacheSources {
            home: person.path().to_owned(),
            cargo_home: person.path().join(".cargo"),
        }),
    )
    .unwrap();
    let hidden: Vec<String> = sandbox
        .hidden()
        .iter()
        .map(|p| p.display().to_string().replace('/', "\\"))
        .collect();
    for name in [
        ".ssh",
        "AppData\\Roaming\\Microsoft\\Credentials",
        "AppData\\Roaming\\Microsoft\\Protect",
        "AppData\\Local\\Microsoft\\Vault",
        "AppData\\Roaming\\Microsoft\\UserSecrets",
    ] {
        assert!(
            hidden.iter().any(|h| h.ends_with(name)),
            "{name} is not hidden: {hidden:?}"
        );
    }
}

/// The folders on the way to what the sandbox grants are listed, so that
/// paths resolve, and nothing beside them: a folder next to the way, in
/// the temporary folder or the person's profile, cannot be listed, while
/// the copy can. Breaks when what is beside the way is not denied.
#[test]
fn nothing_beside_the_way_to_a_grant_can_be_listed() {
    if !available() {
        return;
    }
    let s = Setup::new();
    fs::write(s.outside().join("beside-in-temp.txt"), "x\n").unwrap();
    let profile = kule_engine::osfs::person_home().unwrap();
    let mine = TempDir::new_in(&profile).unwrap();
    let mine = kule_engine::osfs::plain(&fs::canonicalize(mine.path()).unwrap());
    fs::write(mine.join("beside-in-profile.txt"), "x\n").unwrap();
    let sandbox = s.sandbox();
    let out = cmd(&sandbox, "dir /b");
    assert!(
        ok(&out) && text(&out).contains("README.md"),
        "{}",
        text(&out)
    );
    for (dir, name) in [
        (s.outside(), "beside-in-temp.txt"),
        (mine.clone(), "beside-in-profile.txt"),
    ] {
        let out = cmd(&sandbox, &format!("dir /b {}", q(&dir)));
        assert!(
            !text(&out).contains(name),
            "listed {}: {}",
            dir.display(),
            text(&out)
        );
    }
}

/// A folder revealed to a command (`kule exec --reveal`) is read, not
/// written, as on every platform; one not revealed is not read.
#[test]
fn a_revealed_folder_is_read_and_not_written() {
    if !available() {
        return;
    }
    let s = Setup::new();
    let outside = s.outside();
    fs::write(outside.join("shown.txt"), "shown\n").unwrap();
    let out = cmd(
        &s.sandbox(),
        &format!("type {}", q(&outside.join("shown.txt"))),
    );
    assert!(!ok(&out), "read without a grant: {}", text(&out));
    let mut sandbox = s.sandbox();
    sandbox.reveal(&outside).unwrap();
    let out = cmd(&sandbox, &format!("type {}", q(&outside.join("shown.txt"))));
    assert!(ok(&out) && text(&out).contains("shown"), "{}", text(&out));
    let out = cmd(
        &sandbox,
        &format!("echo x> {}", q(&outside.join("put.txt"))),
    );
    assert!(
        !ok(&out) && !outside.join("put.txt").exists(),
        "{}",
        text(&out)
    );
}

/// A folder a command may read that holds Kule's home would open the home
/// whole: with the task's copy inside, the home above the copy is not
/// denied, only not granted. The command is refused instead, whether the
/// home itself or a folder above it is the one revealed. Breaks when a
/// grant holding a hidden folder that holds a grant is not refused.
#[test]
fn a_grant_that_holds_kules_home_is_refused() {
    if !available() {
        return;
    }
    let s = Setup::new();
    let home = fs::canonicalize(s.home.path()).unwrap();
    for granted in [home.clone(), home.parent().unwrap().to_owned()] {
        let mut sandbox = s.sandbox();
        sandbox.reveal(&granted).unwrap();
        let said = match sandbox.start(
            "cmd.exe",
            &["/d".to_owned(), "/c".to_owned(), "dir".to_owned()],
        ) {
            Err(e) => e.to_string(),
            Ok(_) => String::from("started"),
        };
        assert!(
            said.contains("starts none"),
            "a command reading {} was not refused: {said}",
            granted.display()
        );
    }
}

/// A command gets a home of its own, in the task's folders, and nothing
/// of the person's profile: `USERPROFILE` and `APPDATA` are the task's and
/// writable, `HOMEDRIVE` and `HOMEPATH` are not passed, and a file in the
/// person's own profile cannot be read. Preparing the task's home layer,
/// which `kule exec` asks for, changes none of that and needs no link
/// privilege.
#[test]
fn a_command_gets_a_home_of_its_own_and_none_of_the_persons() {
    if !available() {
        return;
    }
    let s = Setup::new();
    let person = kule_engine::osfs::person_home().unwrap();
    let mut sandbox = s.sandbox();
    s.task
        .prepare_home(&person, &[], sandbox.hidden(), sandbox.shown(), &[])
        .unwrap();
    sandbox.use_home(&s.task, &person).unwrap();
    let out = cmd(
        &sandbox,
        "echo h> \"%USERPROFILE%\\own.txt\" && echo a> \"%APPDATA%\\own.txt\" && echo P=%USERPROFILE% A=%APPDATA% D=%HOMEDRIVE%%HOMEPATH%",
    );
    assert!(ok(&out), "{}", text(&out));
    let shown = text(&out);
    let task_dir = kule_engine::osfs::plain(&fs::canonicalize(s.home.path()).unwrap());
    for name in ["P=", "A="] {
        let value = shown
            .split_whitespace()
            .find_map(|w| w.strip_prefix(name))
            .unwrap_or_default();
        assert!(
            Path::new(value).starts_with(&task_dir),
            "{name}{value} is not the task's own ({shown})"
        );
    }
    assert!(shown.contains("D=%HOMEDRIVE%%HOMEPATH%"), "{shown}");
    let mine = TempDir::new_in(&person).unwrap();
    let theirs =
        kule_engine::osfs::plain(&fs::canonicalize(mine.path()).unwrap()).join("theirs.txt");
    fs::write(&theirs, "theirs\n").unwrap();
    let out = cmd(&sandbox, &format!("type {}", q(&theirs)));
    assert!(
        !ok(&out) && !text(&out).contains("theirs"),
        "read the person's file: {}",
        text(&out)
    );
}
