//! `kule rules` and `kule commit --remember`: asked once, not again.

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use tempfile::TempDir;

fn kule(home: &Path, dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_kule"))
        .args(args)
        .env("KULE_HOME", home)
        .current_dir(dir)
        .output()
        .unwrap()
}

fn text(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

#[test]
fn an_approval_remembered_is_not_asked_again() {
    let (home, ws) = (TempDir::new().unwrap(), TempDir::new().unwrap());
    let (home, ws) = (home.path(), ws.path());
    fs::write(ws.join("Cargo.toml"), "[package]\n").unwrap();
    let out = kule(home, ws, &["new", ws.to_str().unwrap()]);
    let id = text(&out).split_whitespace().nth(1).unwrap().to_owned();
    let copy = home.join("tasks").join(&id).join("copy");

    fs::write(copy.join("Cargo.toml"), "[package]\nv = 1\n").unwrap();
    assert!(!kule(home, ws, &["commit", &id]).status.success());
    let out = kule(home, ws, &["commit", &id, "--approve", "--remember"]);
    assert!(
        text(&out).contains(
            "not asked again on this folder: changes to Cargo.toml (manifests-and-lockfiles)"
        ),
        "{}",
        text(&out)
    );

    // The next change to it passes on its own.
    fs::write(copy.join("Cargo.toml"), "[package]\nv = 2\n").unwrap();
    let out = kule(home, ws, &["commit", &id]);
    assert!(out.status.success(), "{}", text(&out));

    let list = text(&kule(home, ws, &["rules"]));
    assert!(
        list.contains("changes to Cargo.toml (manifests-and-lockfiles)"),
        "{list}"
    );
    let rule = list.split_whitespace().next().unwrap().to_owned();
    assert!(kule(home, ws, &["rules", "forget", &rule]).status.success());
    fs::write(copy.join("Cargo.toml"), "[package]\nv = 3\n").unwrap();
    assert!(!kule(home, ws, &["commit", &id]).status.success());
}

#[test]
fn an_allowance_shows_what_it_would_let_through_before_it_is_kept() {
    let (home, ws) = (TempDir::new().unwrap(), TempDir::new().unwrap());
    let (home, ws) = (home.path(), ws.path());
    let out = kule(home, ws, &["rules", "allow", "api.example.com:443"]);
    assert!(
        text(&out).contains("would have let through so far: 0 connections"),
        "{}",
        text(&out)
    );
    assert!(text(&out).contains("not kept"), "{}", text(&out));
    assert!(text(&kule(home, ws, &["rules"])).contains("nothing remembered"));

    let out = kule(
        home,
        ws,
        &[
            "rules",
            "allow",
            "api.example.com:443",
            "--everywhere",
            "--yes",
        ],
    );
    assert!(text(&out).contains("kept as 1"), "{}", text(&out));
    assert!(
        text(&kule(home, ws, &["rules"]))
            .contains("connections to api.example.com:443  · every folder")
    );
    let out = kule(
        home,
        ws,
        &[
            "rules",
            "allow",
            ".kule/policy.cedar",
            "--rule",
            "automation",
            "--yes",
        ],
    );
    assert!(!out.status.success());
}
