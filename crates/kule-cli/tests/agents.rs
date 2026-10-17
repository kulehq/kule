//! `kule agents`: the person's own agent profiles, approved before they run.

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use tempfile::TempDir;

fn kule(home: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_kule"))
        .args(args)
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

const OPENCODE: &str = r#"
[[agent]]
name = "opencode"
command = ["npx", "-y", "opencode-ai@1.2.3", "acp"]
state = [".local/share/opencode"]
login = [".local/share/opencode/auth.json"]
pass_env = ["OPENROUTER_API_KEY"]
hosts = ["openrouter.ai"]
"#;

#[test]
fn a_profile_of_your_own_runs_once_approved() {
    let (home, ws) = (TempDir::new().unwrap(), TempDir::new().unwrap());
    fs::write(home.path().join("agents.toml"), OPENCODE).unwrap();

    let list = stdout(&kule(home.path(), &["agents"]));
    assert!(list.contains("claude-code      built in\n"), "{list}");
    assert!(
        list.contains("opencode         yours, held: not approved"),
        "{list}"
    );

    // Held: a run is refused before anything starts.
    let created = stdout(&kule(home.path(), &["new", ws.path().to_str().unwrap()]));
    let id = created.split_whitespace().nth(1).unwrap();
    let run = kule(home.path(), &["run", id, "--agent", "opencode", "hello"]);
    assert!(!run.status.success());
    assert!(
        String::from_utf8_lossy(&run.stderr).contains("agent opencode cannot run: not approved"),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );

    let approved = stdout(&kule(home.path(), &["agents", "approve", "opencode"]));
    assert!(
        approved.contains("also connects to  https://openrouter.ai"),
        "{approved}"
    );
    assert!(
        approved.contains("sees your env     OPENROUTER_API_KEY"),
        "{approved}"
    );
    let list = stdout(&kule(home.path(), &["agents"]));
    assert!(list.contains("opencode         yours\n"), "{list}");

    stdout(&kule(home.path(), &["agents", "revoke", "opencode"]));
    let list = stdout(&kule(home.path(), &["agents"]));
    assert!(list.contains("opencode         yours, held"), "{list}");
}

#[test]
fn a_profile_that_opens_too_much_cannot_be_approved() {
    let home = TempDir::new().unwrap();
    let wide = OPENCODE.replace(
        r#"state = [".local/share/opencode"]"#,
        r#"state = [".ssh"]"#,
    );
    fs::write(home.path().join("agents.toml"), wide).unwrap();
    let out = kule(home.path(), &["agents", "approve", "opencode"]);
    assert!(!out.status.success());
    assert!(
        String::from_utf8_lossy(&out.stderr).contains(".ssh"),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}
