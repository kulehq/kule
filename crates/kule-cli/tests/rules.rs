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

/// The digest `kule status` gives for task `id`'s changes.
fn digest(home: &Path, dir: &Path, id: &str) -> String {
    let out = kule(home, dir, &["status", id, "--json"]);
    let status: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    status["digest"].as_str().unwrap().to_owned()
}

#[test]
fn an_approval_remembered_is_not_asked_again() {
    let (home, ws) = (TempDir::new().unwrap(), TempDir::new().unwrap());
    let (home, ws) = (home.path(), ws.path());
    fs::write(ws.join("Cargo.toml"), "[package]\n").unwrap();
    let out = kule(home, ws, &["new", ws.to_str().unwrap()]);
    let id = text(&out).split_whitespace().nth(1).unwrap().to_owned();
    let copy = std::path::PathBuf::from(
        text(&out)
            .split(" · copy at ")
            .nth(1)
            .unwrap()
            .lines()
            .next()
            .unwrap(),
    );

    fs::write(copy.join("Cargo.toml"), "[package]\nv = 1\n").unwrap();
    assert!(!kule(home, ws, &["commit", &id]).status.success());
    let digest = digest(home, ws, &id);
    let out = kule(
        home,
        ws,
        &[
            "commit",
            &id,
            "--approve",
            "--remember",
            "--reviewed",
            &digest,
        ],
    );
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

/// `kule rules allow --kind` makes a kind the project's: shown with what it
/// would have let through, kept with --yes, and files of it pass from then
/// on. What runs code on its own cannot be answered for good, and an answer
/// an earlier version kept for it is listed as no longer applying.
#[test]
fn a_kind_answered_for_good_passes_from_then_on() {
    let (home, ws) = (TempDir::new().unwrap(), TempDir::new().unwrap());
    let (home, ws) = (home.path(), ws.path());
    fs::write(ws.join("main.rs"), "fn main() {}\n").unwrap();
    let out = kule(home, ws, &["new", ws.to_str().unwrap()]);
    let id = text(&out).split_whitespace().nth(1).unwrap().to_owned();
    let copy = std::path::PathBuf::from(
        text(&out)
            .split(" · copy at ")
            .nth(1)
            .unwrap()
            .lines()
            .next()
            .unwrap(),
    );
    fs::write(copy.join("a.weird"), "x\n").unwrap();
    let status = text(&kule(home, ws, &["status", &id]));
    assert!(
        status.contains(
            "  a kind of file this project has none of: weird (unfamiliar-kinds) · waits for approval\n    a.weird\n"
        ),
        "{status}"
    );

    let out = kule(home, ws, &["rules", "allow", "--kind", "weird"]);
    let said = text(&out);
    assert!(
        said.contains("files of kind weird (unfamiliar-kinds)"),
        "{said}"
    );
    assert!(
        said.contains("0 connections, 1 changes, in 1 tasks"),
        "{said}"
    );
    assert!(said.contains("not kept"), "{said}");
    let out = kule(home, ws, &["rules", "allow", "--kind", "weird", "--yes"]);
    assert!(out.status.success(), "{}", text(&out));
    assert!(kule(home, ws, &["commit", &id]).status.success());
    assert!(ws.join("a.weird").exists());

    for refused in [
        vec!["rules", "allow", "--kind", "tar.gz", "--yes"],
        vec!["rules", "allow", ".envrc", "--rule", "automation", "--yes"],
    ] {
        let out = kule(home, ws, &refused);
        assert!(!out.status.success(), "{refused:?}: {}", text(&out));
    }
    let said = text(&kule(
        home,
        ws,
        &["rules", "allow", ".envrc", "--rule", "automation", "--yes"],
    ));
    assert!(
        said.contains("what runs code on its own is always asked"),
        "{said}"
    );

    // An answer an earlier version kept for what now always asks.
    let path = home.join("remembered.json");
    let mut kept: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    kept["remembered"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({
            "id": 9, "kind": "file", "path": ".envrc", "rule": "automation", "added_unix": 1
        }));
    fs::write(&path, kept.to_string()).unwrap();
    let listed = text(&kule(home, ws, &["rules"]));
    assert!(
        listed.contains("changes to .envrc (automation)  · every folder · no longer applies"),
        "{listed}"
    );
    assert!(
        listed.contains("files of kind weird (unfamiliar-kinds)"),
        "{listed}"
    );
}
