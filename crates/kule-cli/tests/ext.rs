//! `kule ext`: extensions, packed, installed and approved before they run.

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use tempfile::TempDir;

fn kule(home: &Path, cwd: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_kule"))
        .args(args)
        .current_dir(cwd)
        .env("KULE_HOME", home)
        .output()
        .unwrap()
}

fn stdout(out: &Output) -> String {
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr(out: &Output) -> String {
    assert!(
        !out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stdout)
    );
    String::from_utf8_lossy(&out.stderr).into_owned()
}

fn extension(dir: &Path, version: &str, permissions: &str) {
    fs::write(
        dir.join("kule-extension.json"),
        format!(
            r#"{{ "kule_extension": 1, "id": "acme.notes", "name": "Notes", "version": "{version}",
                 "publisher": "acme", "main": "main.js", "permissions": [{permissions}],
                 "contributes": {{ "commands": [{{ "id": "scan", "title": "Scan" }}],
                                   "themes": ["dusk.kule-theme.json"] }} }}"#
        ),
    )
    .unwrap();
    fs::write(dir.join("main.js"), "// main").unwrap();
    fs::write(
        dir.join("dusk.kule-theme.json"),
        r##"{ "kule_theme": 1, "name": "Dusk", "scheme": "dark", "colors": { "bg": "#101020" } }"##,
    )
    .unwrap();
}

#[test]
fn a_packed_extension_runs_once_approved_and_waits_again_when_updated() {
    let (home, work, src) = (
        TempDir::new_in(env!("CARGO_TARGET_TMPDIR")).unwrap(),
        TempDir::new_in(env!("CARGO_TARGET_TMPDIR")).unwrap(),
        TempDir::new_in(env!("CARGO_TARGET_TMPDIR")).unwrap(),
    );
    extension(src.path(), "1.0.0", r#""tasks.read""#);

    let packed = stdout(&kule(
        home.path(),
        work.path(),
        &["ext", "pack", src.path().to_str().unwrap()],
    ));
    assert!(
        packed.contains("packed acme.notes 1.0.0 into acme.notes-1.0.0.kulext"),
        "{packed}"
    );
    let file = work.path().join("acme.notes-1.0.0.kulext");

    let info = stdout(&kule(
        home.path(),
        work.path(),
        &["ext", "info", file.to_str().unwrap()],
    ));
    assert!(
        info.contains("publisher:   acme (not verified by anyone)"),
        "{info}"
    );
    assert!(
        info.contains("may:         tasks.read — See your tasks"),
        "{info}"
    );
    assert!(
        info.contains("command:     Scan (ext.acme.notes.scan)"),
        "{info}"
    );

    let installed = stdout(&kule(
        home.path(),
        work.path(),
        &["ext", "install", file.to_str().unwrap()],
    ));
    assert!(
        installed.contains("held:        not approved yet"),
        "{installed}"
    );
    let list = stdout(&kule(home.path(), work.path(), &["ext"]));
    assert!(list.contains("held: not approved yet"), "{list}");

    stdout(&kule(
        home.path(),
        work.path(),
        &["ext", "approve", "acme.notes", "--yes"],
    ));
    let list = stdout(&kule(home.path(), work.path(), &["ext"]));
    assert!(list.contains("Notes — runs"), "{list}");

    // An update that asks for more waits, and says what is new.
    extension(src.path(), "1.1.0", r#""tasks.read", "events.read", "net""#);
    let manifest = fs::read_to_string(src.path().join("kule-extension.json")).unwrap();
    fs::write(
        src.path().join("kule-extension.json"),
        manifest.replace(
            r#""main": "main.js","#,
            r#""main": "main.js", "hosts": ["hooks.slack.com"],"#,
        ),
    )
    .unwrap();
    stdout(&kule(
        home.path(),
        work.path(),
        &["ext", "pack", src.path().to_str().unwrap()],
    ));
    let updated = stdout(&kule(
        home.path(),
        work.path(),
        &[
            "ext",
            "install",
            work.path()
                .join("acme.notes-1.1.0.kulext")
                .to_str()
                .unwrap(),
        ],
    ));
    assert!(
        updated.contains("events.read — Read what agents say and do"),
        "{updated}"
    );
    assert!(updated.contains("(new)"), "{updated}");
    assert!(
        updated.contains("reaches:     hooks.slack.com  (new)"),
        "{updated}"
    );
    assert!(
        updated.contains("warning:     it can read your work and send data"),
        "{updated}"
    );
    assert!(
        updated.contains("held:        it changed since you approved it"),
        "{updated}"
    );

    stdout(&kule(
        home.path(),
        work.path(),
        &["ext", "disable", "acme.notes"],
    ));
    stdout(&kule(
        home.path(),
        work.path(),
        &["ext", "remove", "acme.notes"],
    ));
    assert!(stdout(&kule(home.path(), work.path(), &["ext"])).contains("no extensions"));
}

#[test]
fn a_theme_that_is_not_a_theme_stops_the_install() {
    let (home, work, src) = (
        TempDir::new_in(env!("CARGO_TARGET_TMPDIR")).unwrap(),
        TempDir::new_in(env!("CARGO_TARGET_TMPDIR")).unwrap(),
        TempDir::new_in(env!("CARGO_TARGET_TMPDIR")).unwrap(),
    );
    extension(src.path(), "1.0.0", "");
    fs::write(
        src.path().join("dusk.kule-theme.json"),
        r##"{ "kule_theme": 1, "name": "Dusk", "scheme": "dark", "colors": { "bg": "url(x)" } }"##,
    )
    .unwrap();
    let err = stderr(&kule(
        home.path(),
        work.path(),
        &["ext", "dev", src.path().to_str().unwrap()],
    ));
    assert!(err.contains("dusk.kule-theme.json"), "{err}");
    assert!(stdout(&kule(home.path(), work.path(), &["ext"])).contains("no extensions"));
}

fn tools(dir: &Path, mcp_args: &str) {
    fs::write(
        dir.join("kule-extension.json"),
        format!(
            r#"{{ "kule_extension": 1, "id": "acme.tools", "name": "Tools", "version": "1.0.0",
                 "publisher": "acme",
                 "contributes": {{ "agents": "agents.toml",
                   "mcpServers": [{{ "name": "db", "command": "npx", "args": [{mcp_args}] }}] }} }}"#
        ),
    )
    .unwrap();
    fs::write(
        dir.join("agents.toml"),
        "[[agent]]\nname = \"acme-agent\"\ncommand = [\"acme-agent\", \"acp\"]\nstate = [\".acme\"]\n",
    )
    .unwrap();
}

#[test]
fn an_extension_brings_agents_and_mcp_servers_that_wait_for_the_person() {
    let (home, work, src) = (
        TempDir::new_in(env!("CARGO_TARGET_TMPDIR")).unwrap(),
        TempDir::new_in(env!("CARGO_TARGET_TMPDIR")).unwrap(),
        TempDir::new_in(env!("CARGO_TARGET_TMPDIR")).unwrap(),
    );
    let dir = src.path().to_str().unwrap();

    tools(src.path(), r#""-y", "@acme/db@latest""#);
    let err = stderr(&kule(home.path(), work.path(), &["ext", "dev", dir]));
    assert!(
        err.contains("MCP server db") && err.contains("exact version"),
        "{err}"
    );

    tools(src.path(), r#""-y", "@acme/db@1.2.3""#);
    stdout(&kule(home.path(), work.path(), &["ext", "dev", dir]));
    // Held with the extension: nothing is offered yet.
    let mcp = stdout(&kule(home.path(), work.path(), &["ext", "mcp"]));
    assert!(mcp.contains("no extension that runs offers"), "{mcp}");

    stdout(&kule(
        home.path(),
        work.path(),
        &["ext", "approve", "acme.tools", "--yes"],
    ));
    let mcp = stdout(&kule(home.path(), work.path(), &["ext", "mcp"]));
    assert!(mcp.contains("acme.tools/db — waits for you"), "{mcp}");
    assert!(mcp.contains("starts: npx -y @acme/db@1.2.3"), "{mcp}");
    let agents = stdout(&kule(home.path(), work.path(), &["agents"]));
    assert!(
        agents.contains("acme-agent       from extension acme.tools, held: not approved"),
        "{agents}"
    );

    stdout(&kule(
        home.path(),
        work.path(),
        &["ext", "mcp", "approve", "acme.tools/db", "--yes"],
    ));
    let mcp = stdout(&kule(home.path(), work.path(), &["ext", "mcp"]));
    assert!(mcp.contains("acme.tools/db — agents get it"), "{mcp}");

    // A changed server waits again.
    tools(src.path(), r#""-y", "@acme/db@1.2.4""#);
    stdout(&kule(
        home.path(),
        work.path(),
        &["ext", "approve", "acme.tools", "--yes"],
    ));
    let mcp = stdout(&kule(home.path(), work.path(), &["ext", "mcp"]));
    assert!(mcp.contains("acme.tools/db — waits for you"), "{mcp}");

    stdout(&kule(
        home.path(),
        work.path(),
        &["ext", "mcp", "approve", "acme.tools/db", "--yes"],
    ));
    stdout(&kule(
        home.path(),
        work.path(),
        &["ext", "mcp", "revoke", "acme.tools/db"],
    ));
    let mcp = stdout(&kule(home.path(), work.path(), &["ext", "mcp"]));
    assert!(mcp.contains("waits for you"), "{mcp}");
}

#[test]
fn a_new_extension_installs_as_it_is() {
    let (home, work) = (
        TempDir::new_in(env!("CARGO_TARGET_TMPDIR")).unwrap(),
        TempDir::new_in(env!("CARGO_TARGET_TMPDIR")).unwrap(),
    );
    let made = stdout(&kule(
        home.path(),
        work.path(),
        &["ext", "new", "me.first-try"],
    ));
    assert!(made.contains("wrote me.first-try into first-try"), "{made}");
    assert!(work.path().join("first-try/kule-ext.d.ts").is_file());
    stdout(&kule(
        home.path(),
        work.path(),
        &["ext", "dev", "first-try"],
    ));
    stdout(&kule(
        home.path(),
        work.path(),
        &["ext", "approve", "me.first-try", "--yes"],
    ));
    assert!(stdout(&kule(home.path(), work.path(), &["ext"])).contains("First Try (dev) — runs"));
    // Never over a folder with something in it.
    let err = stderr(&kule(
        home.path(),
        work.path(),
        &["ext", "new", "me.first-try"],
    ));
    assert!(err.contains("is not empty"), "{err}");
}

/// The examples in the repository stay extensions Kule takes.
#[test]
fn every_example_extension_reads() {
    let (home, work) = (
        TempDir::new_in(env!("CARGO_TARGET_TMPDIR")).unwrap(),
        TempDir::new_in(env!("CARGO_TARGET_TMPDIR")).unwrap(),
    );
    let examples = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/extensions");
    let mut seen = 0;
    for entry in fs::read_dir(&examples).unwrap() {
        let dir = entry.unwrap().path();
        let info = stdout(&kule(
            home.path(),
            work.path(),
            &["ext", "info", dir.to_str().unwrap()],
        ));
        assert!(info.contains("publisher:"), "{}: {info}", dir.display());
        stdout(&kule(
            home.path(),
            work.path(),
            &["ext", "dev", dir.to_str().unwrap()],
        ));
        seen += 1;
    }
    assert!(seen >= 3, "{seen}");
}

#[test]
fn an_approval_is_asked_for_and_never_given_without_a_yes() {
    let (home, work, src) = (
        TempDir::new_in(env!("CARGO_TARGET_TMPDIR")).unwrap(),
        TempDir::new_in(env!("CARGO_TARGET_TMPDIR")).unwrap(),
        TempDir::new_in(env!("CARGO_TARGET_TMPDIR")).unwrap(),
    );
    extension(src.path(), "1.0.0", r#""tasks.read""#);
    stdout(&kule(
        home.path(),
        work.path(),
        &["ext", "dev", src.path().to_str().unwrap()],
    ));
    // No terminal to ask on and no --yes: shown, not approved.
    let out = kule(home.path(), work.path(), &["ext", "approve", "acme.notes"]);
    let err = stderr(&out);
    assert!(err.contains("add --yes"), "{err}");
    assert!(String::from_utf8_lossy(&out.stdout).contains("may:         tasks.read"));
    assert!(stdout(&kule(home.path(), work.path(), &["ext"])).contains("held: not approved yet"));
}

/// The CLI goes through a running daemon, so the app hears of a change at
/// once: an extension turned off stops making calls.
#[test]
fn a_change_from_the_cli_reaches_a_running_app() {
    let (home, work, src) = (
        TempDir::new_in(env!("CARGO_TARGET_TMPDIR")).unwrap(),
        TempDir::new_in(env!("CARGO_TARGET_TMPDIR")).unwrap(),
        TempDir::new_in(env!("CARGO_TARGET_TMPDIR")).unwrap(),
    );
    extension(src.path(), "1.0.0", r#""tasks.read""#);
    stdout(&kule(
        home.path(),
        work.path(),
        &["ext", "dev", src.path().to_str().unwrap()],
    ));
    stdout(&kule(
        home.path(),
        work.path(),
        &["ext", "approve", "acme.notes", "--yes"],
    ));
    let mut daemon = Command::new(env!("CARGO_BIN_EXE_kule"))
        .args(["daemon", "--idle-secs", "60"])
        .env("KULE_HOME", home.path())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let client = kule_daemon::Client::new(home.path());
    let started = std::time::Instant::now();
    while !client.is_running() {
        assert!(
            started.elapsed() < std::time::Duration::from_secs(20),
            "no daemon"
        );
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    let (tx, rx) = std::sync::mpsc::channel();
    let at = home.path().to_owned();
    std::thread::spawn(move || {
        let _ = kule_daemon::Client::new(&at).subscribe(|notice| {
            let _ = tx.send(notice);
        });
    });
    std::thread::sleep(std::time::Duration::from_millis(500));
    stdout(&kule(
        home.path(),
        work.path(),
        &["ext", "disable", "acme.notes"],
    ));
    let heard = std::iter::from_fn(|| rx.recv_timeout(std::time::Duration::from_secs(5)).ok())
        .any(|n| matches!(n, kule_daemon::Notice::Extensions));
    let _ = daemon.kill();
    let _ = daemon.wait();
    assert!(heard, "the app heard nothing of the change");
}

/// `kule run` gives the agent the MCP servers approved from extensions, and
/// only those, as the app's runs do.
#[cfg(unix)]
#[test]
fn kule_run_gives_the_agent_only_the_mcp_servers_approved() {
    let target = env!("CARGO_TARGET_TMPDIR");
    let (home, work, src, ws) = (
        TempDir::new_in(target).unwrap(),
        TempDir::new_in(target).unwrap(),
        TempDir::new_in(target).unwrap(),
        TempDir::new().unwrap(),
    );
    fs::write(
        src.path().join("kule-extension.json"),
        r#"{ "kule_extension": 1, "id": "acme.tools", "name": "Tools", "version": "1.0.0",
             "publisher": "acme", "contributes": { "mcpServers": [
               { "name": "web", "command": "uvx", "args": ["acme-web==2.0.0"] },
               { "name": "db", "command": "uvx", "args": ["acme-db==1.0.0"] } ] } }"#,
    )
    .unwrap();
    let run = |args: &[&str]| kule(home.path(), work.path(), args);
    stdout(&run(&["ext", "dev", src.path().to_str().unwrap()]));
    stdout(&run(&["ext", "approve", "acme.tools", "--yes"]));
    stdout(&run(&["ext", "mcp", "approve", "acme.tools/web", "--yes"]));

    let created = stdout(&run(&["new", ws.path().to_str().unwrap()]));
    let id = created.split_whitespace().nth(1).unwrap().to_owned();
    let copy = fs::canonicalize(stdout(&run(&["path", &id])).trim()).unwrap();
    // An agent that writes down the MCP servers it is given, then stops; it
    // lives in the copy, which its sandbox lets it read.
    let given = copy.join("given.json");
    let probe = copy.join("probe.py");
    fs::write(
        &probe,
        format!(
            "import json, sys\nfor line in sys.stdin:\n    m = json.loads(line)\n    if m.get('method') == 'initialize':\n        print(json.dumps({{'jsonrpc': '2.0', 'id': m['id'], 'result': {{'protocolVersion': 1, 'agentCapabilities': {{}}, 'authMethods': []}}}}), flush=True)\n    elif m.get('method') == 'session/new':\n        open({:?}, 'w').write(json.dumps(m['params'].get('mcpServers', [])))\n        sys.exit(1)\n",
            given.display().to_string()
        ),
    )
    .unwrap();
    fs::write(
        home.path().join("agents.toml"),
        format!(
            "[[agent]]\nname = \"probe\"\ncommand = [\"python3\", \"-I\", {:?}]\n",
            probe.display().to_string()
        ),
    )
    .unwrap();
    stdout(&run(&["agents", "approve", "probe"]));
    run(&["run", &id, "--agent", "probe", "hello"]);

    let given: Vec<serde_json::Value> =
        serde_json::from_str(&fs::read_to_string(&given).expect("the agent was never started"))
            .unwrap();
    let names: Vec<_> = given.iter().map(|s| s["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["web"], "{given:?}");

    // What was approved cannot be read: the run goes without, and says so.
    fs::write(
        home.path().join("extensions/mcp-approved.json"),
        "{ not json",
    )
    .unwrap();
    let out = run(&["run", &id, "--agent", "probe", "hello"]);
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("kule: Kule could not read the extensions' MCP servers"),
        "{err}"
    );
    assert_eq!(
        fs::read_to_string(copy.join("given.json")).unwrap(),
        "[]",
        "{err}"
    );
}
