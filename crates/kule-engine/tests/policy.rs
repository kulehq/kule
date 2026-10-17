//! M1, step 3: Cedar policy over effect sets, and the gate in front of commit.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use kule_engine::{DEFAULT_POLICY, Error, Policy, Ruling, Task, Verdict};
use tempfile::TempDir;

fn write(path: &Path, text: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, text).unwrap();
}

fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .args(["-c", "user.name=Test", "-c", "user.email=test@example.com"])
        .args([
            "-c",
            "commit.gpgsign=false",
            "-c",
            "init.defaultBranch=main",
        ])
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

struct Setup {
    _home: TempDir,
    _ws: TempDir,
    task: Task,
}

impl Setup {
    fn new(files: &[(&str, &str)], repo: bool) -> Self {
        let home = TempDir::new().unwrap();
        let ws = TempDir::new().unwrap();
        for (path, text) in files {
            write(&ws.path().join(path), text);
        }
        if repo {
            git(ws.path(), &["init", "-q"]);
            git(ws.path(), &["add", "-A"]);
            git(ws.path(), &["commit", "-qm", "initial"]);
        }
        let task = Task::create(home.path(), ws.path()).unwrap();
        Setup {
            _home: home,
            _ws: ws,
            task,
        }
    }

    fn copy(&self) -> PathBuf {
        self.task.copy()
    }

    fn judged(&self) -> Ruling {
        Policy::builtin().judge(&self.task.effects().unwrap())
    }
}

/// The verdict and rules for one subject of a ruling.
fn on(ruling: &Ruling, subject: &str) -> (Verdict, Vec<String>) {
    let j = ruling
        .judgements
        .iter()
        .find(|j| j.subject == subject)
        .unwrap_or_else(|| panic!("no judgement on {subject}: {ruling:#?}"));
    (j.verdict, j.rules.clone())
}

fn rules(names: &[&str]) -> Vec<String> {
    names.iter().map(|n| n.to_string()).collect()
}

const PROJECT: &[(&str, &str)] = &[
    ("src/main.rs", "fn main() {}\n"),
    ("Cargo.toml", "[package]\nname = \"app\"\n"),
    ("README.md", "hello\n"),
];

#[test]
fn source_edits_pass_on_their_own() {
    let s = Setup::new(PROJECT, false);
    write(&s.copy().join("src/main.rs"), "fn main() { run() }\n");
    write(&s.copy().join("src/run.rs"), "pub fn run() {}\n");
    fs::remove_file(s.copy().join("README.md")).unwrap();

    let ruling = s.judged();
    assert_eq!(ruling.verdict, Verdict::Allow);
    assert_eq!(
        on(&ruling, "src/run.rs"),
        (Verdict::Allow, rules(&["files"]))
    );
}

#[test]
fn manifests_and_project_files_wait_for_a_person() {
    let s = Setup::new(PROJECT, false);
    write(
        &s.copy().join("Cargo.toml"),
        "[package]\nname = \"app\"\n[dependencies]\nleft-pad = \"1\"\n",
    );
    write(&s.copy().join("tools/App.csproj"), "<Project />\n");
    write(&s.copy().join("src/main.rs"), "fn main() { run() }\n");

    let ruling = s.judged();
    assert_eq!(ruling.verdict, Verdict::Ask);
    assert_eq!(
        on(&ruling, "Cargo.toml"),
        (Verdict::Ask, rules(&["manifests-and-lockfiles"]))
    );
    assert_eq!(
        on(&ruling, "tools/App.csproj"),
        (Verdict::Ask, rules(&["project-files"]))
    );
    assert_eq!(on(&ruling, "src/main.rs").0, Verdict::Allow);
    let holding: Vec<&str> = ruling.holding().map(|j| j.subject.as_str()).collect();
    assert_eq!(holding, ["Cargo.toml", "tools/App.csproj"]);
}

#[test]
fn ci_editor_and_kule_files_wait_for_a_person() {
    let s = Setup::new(PROJECT, false);
    write(&s.copy().join(".github/workflows/ci.yml"), "on: push\n");
    write(&s.copy().join(".vscode/tasks.json"), "{}\n");
    write(&s.copy().join(".kule/policy.cedar"), "\n");
    write(&s.copy().join(".envrc"), "export X=1\n");

    let ruling = s.judged();
    for path in [
        ".github/workflows/ci.yml",
        ".vscode/tasks.json",
        ".kule/policy.cedar",
        ".envrc",
    ] {
        assert_eq!(
            on(&ruling, path),
            (Verdict::Ask, rules(&["automation"])),
            "{path}"
        );
    }
}

/// `n` files under `data/`, in a git repository or a plain folder.
fn data(n: usize, repo: bool) -> Setup {
    let files: Vec<(String, String)> = (0..n)
        .map(|i| (format!("data/{i}.txt"), format!("{i}\n")))
        .collect();
    let refs: Vec<(&str, &str)> = files
        .iter()
        .map(|(p, t)| (p.as_str(), t.as_str()))
        .collect();
    Setup::new(&refs, repo)
}

#[test]
fn a_mass_delete_waits_for_a_person() {
    let s = data(201, true);
    fs::remove_dir_all(s.copy().join("data")).unwrap();

    let ruling = s.judged();
    assert_eq!(ruling.verdict, Verdict::Ask);
    assert_eq!(
        on(&ruling, "data/7.txt"),
        (Verdict::Ask, rules(&["mass-delete"]))
    );
}

#[test]
fn outside_git_a_few_deletions_already_wait() {
    // 20 files and their directory: 21 deletions.
    let s = data(20, false);
    fs::remove_dir_all(s.copy().join("data")).unwrap();
    let ruling = s.judged();
    assert_eq!(
        on(&ruling, "data/7.txt"),
        (Verdict::Ask, rules(&["deletes-outside-git"]))
    );

    // Twenty pass, in a plain folder as in a repository.
    let s = data(20, false);
    for i in 0..20 {
        fs::remove_file(s.copy().join(format!("data/{i}.txt"))).unwrap();
    }
    assert_eq!(s.judged().verdict, Verdict::Allow);
    let s = data(30, true);
    fs::remove_dir_all(s.copy().join("data")).unwrap();
    assert_eq!(s.judged().verdict, Verdict::Allow);
}

#[test]
fn moving_files_is_not_deleting_them() {
    // Sorting 250 photos into folders: no deletion, nothing waits.
    let s = data(250, false);
    let copy = s.copy();
    fs::create_dir(copy.join("sorted")).unwrap();
    for i in 0..250 {
        let to = copy.join(format!("sorted/{}", i % 2));
        fs::create_dir_all(&to).unwrap();
        fs::rename(
            copy.join(format!("data/{i}.txt")),
            to.join(format!("{i}.txt")),
        )
        .unwrap();
    }
    fs::remove_dir(copy.join("data")).unwrap();

    let set = s.task.effects().unwrap();
    assert_eq!(set.moves.len(), 250);
    assert!(
        set.moves
            .iter()
            .any(|m| m.from == "data/7.txt" && m.to == "sorted/1/7.txt")
    );
    assert_eq!(
        set.deleted().map(|e| e.path.as_str()).collect::<Vec<_>>(),
        ["data"]
    );
    assert_eq!(s.judged().verdict, Verdict::Allow);
}

#[test]
fn a_file_rewritten_under_a_new_name_is_not_a_move() {
    let s = data(1, false);
    let copy = s.copy();
    fs::rename(copy.join("data/0.txt"), copy.join("data/new.txt")).unwrap();
    write(&copy.join("data/new.txt"), "something else\n");
    assert!(s.task.effects().unwrap().moves.is_empty());
}

#[test]
fn git_moves_are_judged_by_what_they_mean() {
    let s = Setup::new(PROJECT, true);
    let copy = s.copy();
    write(&copy.join("src/main.rs"), "fn main() { run() }\n");
    git(&copy, &["commit", "-qam", "agent"]);
    git(&copy, &["branch", "feature"]);
    write(
        &copy.join(".git/hooks/pre-commit"),
        "#!/bin/sh\necho pwned\n",
    );
    git(&copy, &["config", "core.fsmonitor", "echo pwned"]);

    let ruling = s.judged();
    assert_eq!(
        on(&ruling, "git ref refs/heads/main"),
        (Verdict::Allow, rules(&["branch-forward"]))
    );
    assert_eq!(
        on(&ruling, "git ref refs/heads/feature"),
        (Verdict::Allow, rules(&["branch-created"]))
    );
    assert_eq!(
        on(&ruling, "git config"),
        (Verdict::Ask, rules(&["no-rule-lets-it-pass"]))
    );
    assert_eq!(
        on(&ruling, "git hook pre-commit"),
        (Verdict::Deny, rules(&["git-hooks"]))
    );
    assert_eq!(ruling.verdict, Verdict::Deny);
}

#[test]
fn rewriting_history_and_switching_head_wait_for_a_person() {
    let s = Setup::new(PROJECT, true);
    let copy = s.copy();
    write(&copy.join("README.md"), "changed\n");
    git(&copy, &["commit", "-qam", "one"]);
    s.task.commit().unwrap();
    git(&copy, &["reset", "-q", "--hard", "HEAD~1"]);
    git(&copy, &["switch", "-q", "-c", "elsewhere"]);

    let ruling = s.judged();
    assert_eq!(on(&ruling, "git ref refs/heads/main").0, Verdict::Ask);
    assert_eq!(on(&ruling, "git ref HEAD").0, Verdict::Ask);
}

#[test]
fn a_repository_policy_can_only_forbid() {
    let permit = "permit (principal, action, resource);";
    assert!(matches!(
        Policy::from_texts(DEFAULT_POLICY, Some(permit)),
        Err(Error::Invalid(_))
    ));

    let s = Setup::new(PROJECT, false);
    write(&s.copy().join("docs/guide.md"), "# Guide\n");
    let tighter = r#"@id("docs-reviewed") forbid (principal, action == Kule::Action::"auto", resource) when { resource.path like "docs/*" };"#;
    let policy = Policy::from_texts(DEFAULT_POLICY, Some(tighter)).unwrap();
    let ruling = policy.judge(&s.task.effects().unwrap());
    assert_eq!(
        on(&ruling, "docs/guide.md"),
        (Verdict::Ask, rules(&["docs-reviewed"]))
    );
}

/// The agent edits the copy, so the repository's rules come from the real
/// workspace. A rule the agent writes into its copy does not apply.
#[test]
fn the_repository_policy_is_read_from_the_workspace() {
    let forbid_docs = r#"@id("docs-reviewed") forbid (principal, action == Kule::Action::"auto", resource) when { resource.path like "docs/*" };"#;
    let s = Setup::new(
        &[
            ("README.md", "hello\n"),
            (".kule/policy.cedar", forbid_docs),
        ],
        false,
    );
    write(&s.copy().join(".kule/policy.cedar"), "\n");
    write(&s.copy().join("docs/guide.md"), "# Guide\n");

    let policy = Policy::load(None, s.task.workspace()).unwrap();
    let ruling = policy.judge(&s.task.effects().unwrap());
    assert_eq!(
        on(&ruling, "docs/guide.md"),
        (Verdict::Ask, rules(&["docs-reviewed"]))
    );
}

#[test]
fn a_user_policy_replaces_the_default() {
    let s = Setup::new(PROJECT, false);
    write(
        &s.copy().join("Cargo.toml"),
        "[package]\nname = \"renamed\"\n",
    );
    let user_dir = TempDir::new().unwrap();
    let user = user_dir.path().join("policy.cedar");
    write(
        &user,
        r#"@id("trust-everything") permit (principal, action, resource);"#,
    );

    let ruling = Policy::load(Some(&user), s.task.workspace())
        .unwrap()
        .judge(&s.task.effects().unwrap());
    assert_eq!(
        on(&ruling, "Cargo.toml"),
        (Verdict::Allow, rules(&["trust-everything"]))
    );
}

#[test]
fn a_policy_that_fails_to_evaluate_denies() {
    let s = Setup::new(PROJECT, false);
    write(&s.copy().join("src/main.rs"), "fn main() { run() }\n");
    let broken =
        r#"permit (principal, action, resource) when { resource.no_such_attribute == 1 };"#;
    let ruling = Policy::from_texts(broken, None)
        .unwrap()
        .judge(&s.task.effects().unwrap());
    let (verdict, why) = on(&ruling, "src/main.rs");
    assert_eq!(verdict, Verdict::Deny);
    assert!(why[0].starts_with("policy-error"), "{why:?}");
}

#[test]
fn the_gate_holds_what_waits_and_what_is_denied() {
    let s = Setup::new(PROJECT, true);
    let readme = s.task.workspace().join("README.md");
    let policy = Policy::builtin();

    write(
        &s.copy().join("Cargo.toml"),
        "[package]\nname = \"renamed\"\n",
    );
    match s.task.commit_if(policy.gate(false)) {
        Err(Error::Held(ruling)) => assert_eq!(ruling.verdict, Verdict::Ask),
        other => panic!("expected the gate to hold, got {other:?}"),
    }
    assert!(
        fs::read_to_string(s.task.workspace().join("Cargo.toml"))
            .unwrap()
            .contains("app")
    );
    assert!(s.task.commit_if(policy.gate(true)).unwrap().is_some());

    write(&s.copy().join(".git/hooks/pre-commit"), "#!/bin/sh\n");
    write(&s.copy().join("README.md"), "changed\n");
    assert!(matches!(
        s.task.commit_if(policy.gate(true)),
        Err(Error::Held(_))
    ));
    assert_eq!(fs::read_to_string(&readme).unwrap(), "hello\n");
    assert!(!s.task.workspace().join(".git/hooks/pre-commit").exists());
}

#[test]
fn a_person_chooses_between_kules_policies_but_keeps_their_own() {
    use kule_engine::{PolicyChoice, USER_POLICY, choose_policy, policy_choice};
    let s = Setup::new(PROJECT, false);
    write(&s.copy().join("README.md"), "hello again\n");
    let home = TempDir::new().unwrap();
    let user = home.path().join(USER_POLICY);
    let readme = |home: &Path| {
        let policy = Policy::load(Some(&home.join(USER_POLICY)), s.task.workspace()).unwrap();
        on(&policy.judge(&s.task.effects().unwrap()), "README.md")
    };
    assert_eq!(policy_choice(home.path()).unwrap(), PolicyChoice::Balanced);
    assert_eq!(readme(home.path()), (Verdict::Allow, rules(&["files"])));

    choose_policy(home.path(), PolicyChoice::Careful).unwrap();
    assert_eq!(policy_choice(home.path()).unwrap(), PolicyChoice::Careful);
    assert_eq!(readme(home.path()), (Verdict::Ask, rules(&["careful"])));

    choose_policy(home.path(), PolicyChoice::Balanced).unwrap();
    assert!(!user.exists());
    assert_eq!(readme(home.path()), (Verdict::Allow, rules(&["files"])));
    // Twice is no error.
    choose_policy(home.path(), PolicyChoice::Balanced).unwrap();

    let own = r#"@id("mine") permit (principal, action, resource);"#;
    write(&user, own);
    assert_eq!(policy_choice(home.path()).unwrap(), PolicyChoice::Own);
    for choice in [
        PolicyChoice::Balanced,
        PolicyChoice::Careful,
        PolicyChoice::Own,
    ] {
        assert!(choose_policy(home.path(), choice).is_err());
    }
    assert_eq!(fs::read_to_string(&user).unwrap(), own);
}
