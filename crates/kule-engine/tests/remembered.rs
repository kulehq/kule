//! Approvals a person told Kule to remember: asked once, not again.

use std::fs;
use std::path::Path;

use kule_engine::{
    Allowance, Judgement, Policy, Task, Verdict, allowances, forget, impact, remember,
};
use tempfile::TempDir;

fn manifest(path: &str) -> Allowance {
    Allowance::File {
        path: path.into(),
        rule: "manifests-and-lockfiles".into(),
    }
}

/// The policy as Kule loads it for `workspace`, with `home`'s remembered
/// allowances.
fn policy(home: &Path, workspace: &Path) -> Policy {
    Policy::load(Some(home), workspace).unwrap()
}

struct Setup {
    home: TempDir,
    ws: TempDir,
    task: Task,
}

impl Setup {
    fn new() -> Setup {
        let (home, ws) = (TempDir::new().unwrap(), TempDir::new().unwrap());
        fs::write(ws.path().join("Cargo.toml"), "[package]\n").unwrap();
        fs::write(ws.path().join("package.json"), "{}\n").unwrap();
        let task = Task::create(home.path(), ws.path()).unwrap();
        Setup { home, ws, task }
    }

    fn verdicts(&self) -> Vec<(String, Verdict, Vec<String>)> {
        policy(self.home.path(), self.ws.path())
            .judge(&self.task.effects().unwrap())
            .judgements
            .into_iter()
            .map(|j| (j.subject, j.verdict, j.rules))
            .collect()
    }
}

#[test]
fn a_remembered_file_passes_in_its_folder_only() {
    let s = Setup::new();
    fs::write(
        s.task.copy().join("Cargo.toml"),
        "[package]\nname = \"a\"\n",
    )
    .unwrap();
    fs::write(s.task.copy().join("package.json"), "{\"a\": 1}\n").unwrap();
    assert!(s.verdicts().iter().all(|(_, v, _)| *v == Verdict::Ask));

    remember(s.home.path(), manifest("Cargo.toml"), Some(s.ws.path())).unwrap();
    assert_eq!(
        s.verdicts(),
        [
            (
                "Cargo.toml".into(),
                Verdict::Allow,
                vec!["remembered".into()]
            ),
            (
                "package.json".into(),
                Verdict::Ask,
                vec!["manifests-and-lockfiles".into()]
            ),
        ]
    );
    // Another folder still asks about Cargo.toml.
    let elsewhere = TempDir::new().unwrap();
    let other = policy(s.home.path(), elsewhere.path()).judge(&s.task.effects().unwrap());
    let cargo = other
        .judgements
        .iter()
        .find(|j| j.subject == "Cargo.toml")
        .unwrap();
    assert_eq!(cargo.verdict, Verdict::Ask);
}

#[test]
fn a_remembered_connection_passes_on_its_port_only() {
    let s = Setup::new();
    let ask = |p: &Policy, port| p.judge_connection("api.example.com", port).verdict;
    assert_eq!(ask(&policy(s.home.path(), s.ws.path()), 443), Verdict::Ask);
    let kept = remember(
        s.home.path(),
        Allowance::Connect {
            host: "api.example.com".into(),
            port: 443,
        },
        None,
    )
    .unwrap();
    let p = policy(s.home.path(), s.ws.path());
    assert_eq!(ask(&p, 443), Verdict::Allow);
    assert_eq!(ask(&p, 8443), Verdict::Ask);
    assert_eq!(
        p.judge_connection("api.example.com", 443).rules,
        [format!("remembered-{}", kept.id)]
    );

    assert!(forget(s.home.path(), kept.id).unwrap());
    assert_eq!(ask(&policy(s.home.path(), s.ws.path()), 443), Verdict::Ask);
}

#[test]
fn what_is_denied_or_the_repositorys_own_is_never_remembered() {
    let home = TempDir::new().unwrap();
    let refused = [
        Allowance::File {
            path: ".git/hooks/pre-commit".into(),
            rule: "git-hooks".into(),
        },
        Allowance::File {
            path: "data".into(),
            rule: "mass-delete".into(),
        },
        Allowance::File {
            path: ".kule/policy.cedar".into(),
            rule: "automation".into(),
        },
        manifest("../Cargo.toml"),
        manifest("/etc/passwd"),
        Allowance::Connect {
            host: "*.example.com".into(),
            port: 443,
        },
        Allowance::Connect {
            host: "example.com\" || true || \"".into(),
            port: 443,
        },
        // What runs code on its own is always asked.
        Allowance::File {
            path: ".github/workflows/ci.yml".into(),
            rule: "automation".into(),
        },
        Allowance::Kind { kind: "".into() },
        Allowance::Kind {
            kind: ".envrc".into(),
        },
        Allowance::Kind {
            kind: "tar.gz".into(),
        },
        Allowance::Kind { kind: "VUE".into() },
        Allowance::Kind { kind: "a/b".into() },
    ];
    for allowance in refused {
        assert!(
            remember(home.path(), allowance.clone(), None).is_err(),
            "{allowance:?}"
        );
    }
}

#[test]
fn before_it_is_kept_an_allowance_tells_what_it_would_have_let_through() {
    let s = Setup::new();
    fs::write(
        s.task.copy().join("Cargo.toml"),
        "[package]\nname = \"a\"\n",
    )
    .unwrap();
    s.task.commit().unwrap();
    fs::write(
        s.task.copy().join("Cargo.toml"),
        "[package]\nname = \"b\"\n",
    )
    .unwrap();
    let found = impact(s.home.path(), &manifest("Cargo.toml"), Some(s.ws.path())).unwrap();
    assert_eq!((found.changes, found.tasks), (2, 1));
    assert_eq!(
        impact(s.home.path(), &manifest("package.json"), Some(s.ws.path()))
            .unwrap()
            .changes,
        0
    );
}

/// A repository's own rule is told as the repository's, and no answer a
/// person remembered cancels it, even one named as a rule Kule's policy has.
#[test]
fn a_remembered_answer_never_cancels_a_repository_s_rule() {
    let (home, ws) = (TempDir::new().unwrap(), TempDir::new().unwrap());
    fs::write(ws.path().join("Cargo.toml"), "[package]\n").unwrap();
    fs::create_dir(ws.path().join(".kule")).unwrap();
    fs::write(
        ws.path().join(".kule/policy.cedar"),
        r#"@id("manifests-and-lockfiles") forbid (principal, action == Kule::Action::"auto", resource) when { resource.path == "Cargo.toml" };"#,
    )
    .unwrap();
    let task = Task::create(home.path(), ws.path()).unwrap();
    fs::write(task.copy().join("Cargo.toml"), "[package]\nname = \"a\"\n").unwrap();
    remember(home.path(), manifest("Cargo.toml"), Some(ws.path())).unwrap();
    let ruling = policy(home.path(), ws.path()).judge(&task.effects().unwrap());
    let cargo = ruling
        .judgements
        .iter()
        .find(|j| j.subject == "Cargo.toml")
        .unwrap();
    assert_eq!(cargo.verdict, Verdict::Ask, "{cargo:?}");
    assert!(
        cargo
            .rules
            .contains(&"repo:manifests-and-lockfiles".to_owned()),
        "{cargo:?}"
    );
}

/// A kind answered for good is the project's from then on: files of it
/// pass in that folder, and nowhere else; one in a hidden folder still
/// waits for its place.
#[test]
fn a_kind_answered_for_good_passes_in_its_folder_only() {
    let s = Setup::new();
    let other = Setup::new();
    for setup in [&s, &other] {
        fs::write(setup.task.copy().join("a.weird"), "x\n").unwrap();
        fs::create_dir(setup.task.copy().join(".cache")).unwrap();
        fs::write(setup.task.copy().join(".cache/b.weird"), "y\n").unwrap();
        fs::write(setup.task.copy().join("c.other"), "z\n").unwrap();
    }
    let weird = Allowance::Kind {
        kind: "weird".into(),
    };
    assert_eq!(
        impact(s.home.path(), &weird, Some(s.ws.path()))
            .unwrap()
            .changes,
        2
    );
    remember(s.home.path(), weird.clone(), Some(s.ws.path())).unwrap();
    remember(
        other.home.path(),
        weird,
        Some(TempDir::new().unwrap().path()),
    )
    .unwrap();
    let verdict = |setup: &Setup, path: &str| {
        let found = setup.verdicts().into_iter().find(|(p, _, _)| p == path);
        found.unwrap()
    };
    assert_eq!(
        verdict(&s, "a.weird"),
        ("a.weird".into(), Verdict::Allow, vec!["remembered".into()])
    );
    assert_eq!(
        verdict(&s, ".cache/b.weird"),
        (
            ".cache/b.weird".into(),
            Verdict::Ask,
            vec!["dot-paths".into(), "unfamiliar-kinds".into()]
        )
    );
    assert_eq!(verdict(&s, "c.other").1, Verdict::Ask);
    assert_eq!(verdict(&other, "a.weird").1, Verdict::Ask);
}

/// An answer an earlier version kept for what now runs code on its own is
/// listed as no longer applying, and the file is asked about again.
#[test]
fn an_earlier_answer_for_what_runs_code_no_longer_applies() {
    let s = Setup::new();
    fs::create_dir_all(s.task.copy().join(".vscode")).unwrap();
    fs::write(s.task.copy().join(".vscode/tasks.json"), "{}\n").unwrap();
    let workspace = fs::canonicalize(s.ws.path()).unwrap();
    fs::write(
        s.home.path().join(kule_engine::REMEMBERED_FILE),
        serde_json::json!({
            "next": 2,
            "remembered": [
                { "id": 1, "kind": "file", "path": ".vscode/tasks.json", "rule": "automation",
                  "workspace": workspace, "added_unix": 1 },
                { "id": 2, "kind": "file", "path": ".vscode/tasks.json", "rule": "dot-paths",
                  "workspace": workspace, "added_unix": 1 },
            ]
        })
        .to_string(),
    )
    .unwrap();
    let kept = kule_engine::remembered::load(s.home.path()).unwrap();
    assert_eq!(
        kept.iter().map(|r| r.applies()).collect::<Vec<_>>(),
        [false, true]
    );
    let tasks = s
        .verdicts()
        .into_iter()
        .find(|(p, _, _)| p == ".vscode/tasks.json")
        .unwrap();
    assert_eq!(tasks.1, Verdict::Ask, "{tasks:?}");
}

/// What answering a question for good keeps: a kind for the rule that asks
/// about kinds, the path for the others and for a hidden file; nothing for
/// what runs code, what does not wait, or what is not a file.
#[test]
fn an_answer_for_good_keeps_a_kind_or_a_path() {
    let judgement = |subject: &str, verdict: Verdict, rules: &[&str]| Judgement {
        subject: subject.into(),
        verdict,
        rules: rules.iter().map(|r| r.to_string()).collect(),
    };
    let file = |path: &str, rule: &str| Allowance::File {
        path: path.into(),
        rule: rule.into(),
    };
    assert_eq!(
        allowances(&judgement(
            "ui/a.Weird",
            Verdict::Ask,
            &["unfamiliar-kinds"]
        )),
        [Allowance::Kind {
            kind: "weird".into()
        }]
    );
    assert_eq!(
        allowances(&judgement(
            ".gitignore",
            Verdict::Ask,
            &["dot-paths", "unfamiliar-kinds"]
        )),
        [
            file(".gitignore", "dot-paths"),
            file(".gitignore", "unfamiliar-kinds")
        ]
    );
    assert_eq!(
        allowances(&judgement(
            "Cargo.toml",
            Verdict::Ask,
            &["manifests-and-lockfiles"]
        )),
        [file("Cargo.toml", "manifests-and-lockfiles")]
    );
    for nothing in [
        judgement(".envrc", Verdict::Ask, &["automation", "dot-paths"]),
        judgement("a.weird", Verdict::Allow, &["files"]),
        judgement("a.weird", Verdict::Deny, &["unfamiliar-kinds"]),
        judgement(
            "git ref refs/heads/main: a.weird",
            Verdict::Ask,
            &["unfamiliar-kinds"],
        ),
    ] {
        assert!(allowances(&nothing).is_empty(), "{nothing:?}");
    }
    // Each one is an answer Kule keeps.
    let home = TempDir::new().unwrap();
    for allowance in [
        Allowance::Kind {
            kind: "weird".into(),
        },
        file(".gitignore", "dot-paths"),
        file(".gitignore", "unfamiliar-kinds"),
    ] {
        remember(home.path(), allowance, None).unwrap();
    }
}
