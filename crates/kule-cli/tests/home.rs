//! `kule home`: a path in the home layer named as the person types it,
//! quoted (`'~/.zshrc'`), relative (`.zshrc`) or as the shell expands an
//! unquoted `~/.zshrc`, and nothing outside the home.

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use tempfile::TempDir;

fn kule(home: &Path, person: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_kule"))
        .args(args)
        .env("KULE_HOME", home)
        .env("HOME", person)
        .output()
        .unwrap()
}

fn ok(home: &Path, person: &Path, args: &[&str]) -> String {
    let out = kule(home, person, args);
    assert!(
        out.status.success(),
        "kule {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}

fn refused(home: &Path, person: &Path, args: &[&str]) -> String {
    let out = kule(home, person, args);
    assert!(!out.status.success(), "kule {args:?} went through");
    String::from_utf8(out.stderr).unwrap()
}

#[test]
fn a_home_path_is_found_however_it_is_typed_and_only_inside_the_home() {
    let (home, person, ws) = (
        TempDir::new().unwrap(),
        TempDir::new().unwrap(),
        TempDir::new().unwrap(),
    );
    fs::write(person.path().join(".zshrc"), "export EDITOR=vi\n").unwrap();
    fs::write(ws.path().join("a.txt"), "a\n").unwrap();
    let created = ok(
        home.path(),
        person.path(),
        &["new", ws.path().to_str().unwrap()],
    );
    let id = created.split_whitespace().nth(1).unwrap().to_owned();
    ok(
        home.path(),
        person.path(),
        &[
            "exec",
            &id,
            "--",
            "sh",
            "-c",
            "echo 'alias ll=\"ls -l\"' >> \"$HOME/.zshrc\"",
        ],
    );
    assert!(
        ok(home.path(), person.path(), &["home", &id]).contains("~/.zshrc"),
        "the write was not caught in the home layer"
    );

    let expanded = person.path().join(".zshrc");
    for typed in ["~/.zshrc", ".zshrc", expanded.to_str().unwrap()] {
        let diff = ok(home.path(), person.path(), &["home", &id, "diff", typed]);
        assert!(diff.contains("+alias ll=\"ls -l\""), "{typed}: {diff}");
    }

    let beyond = person.path().join("../elsewhere/.zshrc");
    for typed in ["/etc/hosts", beyond.to_str().unwrap(), "../.zshrc", "~/"] {
        let said = refused(home.path(), person.path(), &["home", &id, "diff", typed]);
        assert!(
            said.contains("not in your home") || said.contains("inside your home"),
            "{typed}: {said}"
        );
        refused(home.path(), person.path(), &["home", &id, "discard", typed]);
    }
    assert!(
        ok(home.path(), person.path(), &["home", &id]).contains("~/.zshrc"),
        "a refused path still threw the change away"
    );

    ok(
        home.path(),
        person.path(),
        &["home", &id, "apply", expanded.to_str().unwrap()],
    );
    assert!(fs::read_to_string(&expanded).unwrap().contains("alias ll="));
}
