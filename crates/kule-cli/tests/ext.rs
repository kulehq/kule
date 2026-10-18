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
        TempDir::new().unwrap(),
        TempDir::new().unwrap(),
        TempDir::new().unwrap(),
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
        &["ext", "approve", "acme.notes"],
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
        TempDir::new().unwrap(),
        TempDir::new().unwrap(),
        TempDir::new().unwrap(),
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
