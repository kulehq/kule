//! Agent profiles an extension brings: listed only while the extension may
//! run, each held until the person approves it as it stands.

use std::fs;
use std::path::Path;

use kule_acp::{Agents, Origin};
use kule_ext::{Package, Store};
use tempfile::TempDir;

fn extension(home: &Path, agents: &str) {
    let src = TempDir::new().unwrap();
    fs::write(
        src.path().join("kule-extension.json"),
        r#"{ "kule_extension": 1, "id": "acme.tools", "name": "Tools", "version": "1.0.0",
             "publisher": "acme", "contributes": { "agents": "agents.toml" } }"#,
    )
    .unwrap();
    fs::write(src.path().join("agents.toml"), agents).unwrap();
    let package = Package::open(kule_ext::pack(src.path()).unwrap()).unwrap();
    let store = Store::new(home);
    store.install(&package).unwrap();
    store.approve("acme.tools", package.digest()).unwrap();
}

const ACME: &str = r#"
[[agent]]
name = "acme-agent"
command = ["acme-agent", "acp"]
state = [".acme"]
"#;

fn listed<'a>(agents: &'a Agents, name: &str) -> Option<&'a kule_acp::Listed> {
    agents.listed().iter().find(|l| l.profile.name == name)
}

#[test]
fn an_extensions_agent_waits_for_its_own_approval() {
    let home = TempDir::new().unwrap();
    extension(home.path(), ACME);
    let agents = Agents::load(home.path()).unwrap();
    let acme = listed(&agents, "acme-agent").unwrap();
    assert_eq!(acme.origin, Origin::Extension);
    assert_eq!(acme.extension.as_deref(), Some("acme.tools"));
    assert!(
        acme.held.as_deref().unwrap().contains("not approved"),
        "{:?}",
        acme.held
    );
    assert!(agents.get("acme-agent").is_none());

    Agents::approve_seen(home.path(), "acme-agent", &acme.digest).unwrap();
    let agents = Agents::load(home.path()).unwrap();
    assert!(agents.get("acme-agent").is_some());
    let approved = fs::read_to_string(home.path().join("agents-approved.json")).unwrap();
    assert!(approved.contains("ext/acme.tools/acme-agent"), "{approved}");

    // Turned off, the extension brings nothing; turned on, the approval holds.
    Store::new(home.path())
        .set_enabled("acme.tools", false)
        .unwrap();
    assert!(listed(&Agents::load(home.path()).unwrap(), "acme-agent").is_none());
    Store::new(home.path())
        .set_enabled("acme.tools", true)
        .unwrap();
    assert!(
        Agents::load(home.path())
            .unwrap()
            .get("acme-agent")
            .is_some()
    );

    // Gone with its extension, the approval goes too.
    Agents::forget_extension(home.path(), "acme.tools").unwrap();
    assert!(
        Agents::load(home.path())
            .unwrap()
            .get("acme-agent")
            .is_none()
    );
}

#[test]
fn an_extensions_agent_cannot_take_a_name_or_a_path_already_held() {
    let home = TempDir::new().unwrap();
    extension(
        home.path(),
        &ACME.replace("acme-agent\"\ncommand", "claude-code\"\ncommand"),
    );
    let agents = Agents::load(home.path()).unwrap();
    let theirs = agents
        .listed()
        .iter()
        .find(|l| l.origin == Origin::Extension)
        .unwrap();
    assert!(
        theirs
            .held
            .as_deref()
            .unwrap()
            .contains("another profile is named claude-code")
    );

    let home = TempDir::new().unwrap();
    extension(home.path(), &ACME.replace("[\".acme\"]", "[\".claude\"]"));
    let agents = Agents::load(home.path()).unwrap();
    let acme = listed(&agents, "acme-agent").unwrap();
    assert!(acme.held.is_some(), "it reaches another agent's state");
    assert!(Agents::approve(home.path(), "acme-agent").is_err());
}
