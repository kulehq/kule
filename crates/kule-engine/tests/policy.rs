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

/// What runs code, or steers an agent, without anyone running the project
/// waits: at any depth, however its name is cased, and a link never passes
/// on its own.
#[test]
fn what_runs_on_its_own_waits_wherever_and_however_it_is_named() {
    let s = Setup::new(PROJECT, false);
    let waiting = [
        ".claude/settings.json",
        ".claude/settings.local.json",
        ".mcp.json",
        "CLAUDE.md",
        "docs/AGENTS.md",
        ".husky/pre-commit",
        ".pre-commit-config.yaml",
        "build.rs",
        ".cargo/config.toml",
        ".yarnrc.yml",
        ".pnpmfile.cjs",
        ".npmrc",
        "gradle/wrapper/gradle-wrapper.properties",
        "gradlew",
        "mise.toml",
        ".tool-versions",
        "lefthook.yml",
        "packages/app/.vscode/tasks.json",
        "packages/app/package.json",
        ".VSCode/settings.json",
        "web/.GitHub/workflows/x.yml",
        "tools/CARGO.TOML",
        "sub/.git/config",
        "lib/.git",
        ".devcontainer.json",
        "rust-toolchain",
        "crates/x/rust-toolchain.toml",
        ".yarn/releases/yarn-4.cjs",
        "mvnw",
        "mvnw.cmd",
        "bunfig.toml",
        ".config/dotnet-tools.json",
        "azure-pipelines.yml",
        ".travis.yml",
        "Jenkinsfile",
        "bitbucket-pipelines.yml",
        ".buildkite/pipeline.yml",
        ".gitea/workflows/ci.yml",
        ".forgejo/workflows/ci.yml",
        ".woodpecker/ci.yml",
        ".windsurf/rules/x.md",
        ".continue/config.yaml",
        ".roo/rules/x.md",
        ".clinerules",
        "opencode.json",
        ".aider.conf.yml",
        // Names a case-insensitive disk reads as listed ones: a long s, the
        // Kelvin sign, a ligature, an accent.
        "x/agent\u{17F}.md",
        "y/.hu\u{17F}ky/pre-commit",
        "w/lefthoo\u{212A}.yml",
        "v/bun\u{FB01}g.toml",
        "u/clau\u{0301}de.md",
    ];
    for path in waiting {
        write(&s.copy().join(path), "x\n");
    }
    write(&s.copy().join("sub/.git/hooks/pre-commit"), "#!/bin/sh\n");
    std::os::unix::fs::symlink("../src", s.copy().join("docs/source")).unwrap();
    std::os::unix::fs::symlink("tools/ide", s.copy().join(".idea")).unwrap();

    let ruling = s.judged();
    for path in waiting.iter().chain(&["docs/source", ".idea"]) {
        assert_eq!(on(&ruling, path).0, Verdict::Ask, "{path}");
    }
    assert_eq!(on(&ruling, "sub/.git/hooks/pre-commit").0, Verdict::Deny);
    // Plain work still passes.
    write(&s.copy().join("src/lib.rs"), "pub fn f() {}\n");
    assert_eq!(on(&s.judged(), "src/lib.rs").0, Verdict::Allow);
}

/// A repository without `.git` in its path (a bare one) is judged as a
/// `.git` is: its config waits, its hooks never pass.
#[test]
fn a_bare_repository_in_the_folder_is_judged_as_a_git_folder() {
    let s = Setup::new(PROJECT, false);
    let copy = s.copy();
    git(&copy, &["init", "-q", "--bare", "vendor/mirror"]);
    write(
        &copy.join("vendor/mirror/hooks/post-checkout"),
        "#!/bin/sh\n",
    );
    let ruling = s.judged();
    assert_eq!(on(&ruling, "vendor/mirror/config").0, Verdict::Ask);
    assert_eq!(on(&ruling, "vendor/mirror/HEAD").0, Verdict::Ask);
    assert_eq!(
        on(&ruling, "vendor/mirror/hooks/post-checkout").0,
        Verdict::Deny
    );
    // Files that only look alike are plain work.
    write(&copy.join("docs/HEAD"), "x\n");
    assert_eq!(on(&s.judged(), "docs/HEAD").0, Verdict::Allow);
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

/// What new commits carry is judged as files are, though the files are
/// gone from the folder: a workflow committed and then deleted still waits
/// with the branch that brings it.
#[test]
fn files_in_new_commits_are_judged_like_files() {
    let s = Setup::new(PROJECT, true);
    let copy = s.copy();
    write(&copy.join(".github/workflows/x.yml"), "on: push\n");
    git(&copy, &["add", "-A"]);
    git(&copy, &["commit", "-qm", "ci"]);
    git(&copy, &["rm", "-q", "--cached", ".github/workflows/x.yml"]);
    fs::remove_file(copy.join(".github/workflows/x.yml")).unwrap();
    git(&copy, &["commit", "-qm", "gone"]);
    write(&copy.join("src/main.rs"), "fn main() { run() }\n");
    git(&copy, &["commit", "-qam", "edit"]);
    git(&copy, &["branch", "side"]);

    let ruling = s.judged();
    for subject in [
        "git ref refs/heads/main: .github/workflows/x.yml",
        "git ref refs/heads/side: .github/workflows/x.yml",
    ] {
        assert_eq!(
            on(&ruling, subject),
            (Verdict::Ask, rules(&["automation"])),
            "{subject}"
        );
    }
    assert_eq!(
        on(&ruling, "git ref refs/heads/main: src/main.rs").0,
        Verdict::Allow
    );
}

/// What a commit does is judged as it is: a link it adds as a link, a file
/// it only makes executable as a change, another repository's commit (a
/// submodule) by a person.
#[test]
fn what_new_commits_carry_is_judged_as_what_it_is() {
    let s = Setup::new(PROJECT, true);
    let copy = s.copy();
    std::os::unix::fs::symlink("/etc/passwd", copy.join("notes.txt")).unwrap();
    git(&copy, &["update-index", "--chmod=+x", "README.md"]);
    let head = String::from_utf8(
        Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(&copy)
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap();
    git(
        &copy,
        &[
            "update-index",
            "--add",
            "--cacheinfo",
            &format!("160000,{},vendor/lib", head.trim()),
        ],
    );
    git(&copy, &["add", "notes.txt"]);
    git(&copy, &["commit", "-qm", "carry"]);
    // The folder then looks as before: only the commit carries them.
    fs::remove_file(copy.join("notes.txt")).unwrap();
    git(&copy, &["update-index", "--chmod=-x", "README.md"]);

    let ruling = s.judged();
    assert_eq!(
        on(&ruling, "git ref refs/heads/main: notes.txt"),
        (Verdict::Ask, rules(&["links"]))
    );
    assert_eq!(
        on(&ruling, "git ref refs/heads/main: README.md").0,
        Verdict::Allow
    );
    assert_eq!(
        on(&ruling, "git ref refs/heads/main: vendor/lib").0,
        Verdict::Ask
    );
}

/// A new branch passes; a tag, a note or a replacement (which makes git
/// show other content in place of a commit or a file) waits.
#[test]
fn only_a_new_branch_passes_among_new_references() {
    let s = Setup::new(PROJECT, true);
    let copy = s.copy();
    git(&copy, &["branch", "feature"]);
    git(&copy, &["tag", "v9"]);
    git(&copy, &["notes", "add", "-m", "note"]);
    let head = String::from_utf8(
        Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(&copy)
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap();
    git(&copy, &["replace", "-f", head.trim(), head.trim()]);

    let ruling = s.judged();
    assert_eq!(on(&ruling, "git ref refs/heads/feature").0, Verdict::Allow);
    for r in ["refs/tags/v9", "refs/notes/commits"] {
        assert_eq!(on(&ruling, &format!("git ref {r}")).0, Verdict::Ask, "{r}");
    }
    let replaced = ruling
        .judgements
        .iter()
        .find(|j| j.subject.starts_with("git ref refs/replace/"))
        .unwrap_or_else(|| panic!("{ruling:#?}"));
    assert_eq!(replaced.verdict, Verdict::Ask);
}

/// Moving forward passes for a branch alone: a tag, a note or a
/// replacement that exists already and moves on waits.
#[test]
fn only_a_branch_moves_forward_on_its_own() {
    let s = Setup::new(PROJECT, true);
    let copy = s.copy();
    git(&copy, &["tag", "v9"]);
    git(&copy, &["notes", "add", "-m", "note"]);
    s.task.commit().unwrap();
    write(&copy.join("README.md"), "changed\n");
    git(&copy, &["commit", "-qam", "one"]);
    git(&copy, &["tag", "-f", "v9"]);
    git(&copy, &["notes", "add", "-f", "-m", "other note"]);

    let ruling = s.judged();
    assert_eq!(
        on(&ruling, "git ref refs/heads/main: README.md").0,
        Verdict::Allow
    );
    for r in ["refs/tags/v9", "refs/notes/commits"] {
        let j = ruling
            .judgements
            .iter()
            .find(|j| j.subject.starts_with(&format!("git ref {r}")))
            .unwrap_or_else(|| panic!("{r}: {ruling:#?}"));
        assert_eq!(j.verdict, Verdict::Ask, "{r}");
    }
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

#[test]
fn a_tasks_group_chooses_the_policy_it_is_judged_by() {
    use kule_engine::{
        GROUP_POLICY_DIR, GroupPolicy, PolicyChoice, choose_policy, create_group, delete_group,
        set_group_policy,
    };
    let forbid_docs = r#"@id("docs-reviewed") forbid (principal, action == Kule::Action::"auto", resource) when { resource.path like "docs/*" };"#;
    let mut files = PROJECT.to_vec();
    files.push((".kule/policy.cedar", forbid_docs));
    let s = Setup::new(&files, false);
    let home = s._home.path();
    write(&s.copy().join("README.md"), "hello again\n");
    write(&s.copy().join("docs/guide.md"), "# Guide\n");
    let judged = |subject: &str| {
        let policy = Policy::for_task(&s.task).unwrap();
        on(&policy.judge(&s.task.effects().unwrap()), subject)
    };
    let allowed = (Verdict::Allow, rules(&["files"]));
    let careful = (Verdict::Ask, rules(&["careful"]));

    // In no group, the policy for every task holds.
    assert_eq!(judged("README.md"), allowed);
    choose_policy(home, PolicyChoice::Careful).unwrap();
    assert_eq!(judged("README.md"), careful);
    choose_policy(home, PolicyChoice::Balanced).unwrap();

    let group = create_group(home, "clients", None).unwrap();
    assert!(s.task.set_group(Some(group.id + 1)).is_err());
    s.task.set_group(Some(group.id)).unwrap();
    assert_eq!(s.task.group().unwrap(), Some(group.id));
    // A new group keeps the policy for every task.
    assert_eq!(judged("README.md"), allowed);
    set_group_policy(home, group.id, GroupPolicy::Careful, None).unwrap();
    assert_eq!(judged("README.md"), careful);
    // The group's choice holds whatever holds for the rest.
    choose_policy(home, PolicyChoice::Careful).unwrap();
    set_group_policy(home, group.id, GroupPolicy::Balanced, None).unwrap();
    assert_eq!(judged("README.md"), allowed);
    choose_policy(home, PolicyChoice::Balanced).unwrap();

    // A policy of the group's own; the repository's forbids still hold.
    let trusting = r#"@id("trust-everything") permit (principal, action, resource);"#;
    set_group_policy(home, group.id, GroupPolicy::Own, Some(trusting)).unwrap();
    assert_eq!(
        judged("README.md"),
        (Verdict::Allow, rules(&["trust-everything"]))
    );
    assert_eq!(judged("docs/guide.md").0, Verdict::Ask);
    // Gone, it refuses rather than fall back to another.
    fs::remove_file(
        home.join(GROUP_POLICY_DIR)
            .join(format!("{}.cedar", group.id)),
    )
    .unwrap();
    assert!(Policy::for_task(&s.task).is_err());

    // A deleted group lets go of its tasks.
    set_group_policy(home, group.id, GroupPolicy::Careful, None).unwrap();
    delete_group(home, group.id).unwrap();
    assert_eq!(s.task.group().unwrap(), None);
    assert_eq!(judged("README.md"), allowed);
}
