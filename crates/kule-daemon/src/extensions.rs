//! The person's extensions, as the daemon and the CLI handle them: the
//! package checks of `kule_ext`, plus the checks of what an extension adds
//! that only Kule's own modules know (themes).

use std::collections::BTreeMap;
use std::path::Path;

use kule_ext::{Listed, OfferedMcp, Package, Source, Store};
use serde::Serialize;
use serde_json::Value;

use crate::Result;
use crate::themes::{self, ThemeFile};

/// Installs or updates the `.kulext` at `path`. It waits for approval.
pub fn install(home: &Path, path: &Path) -> Result<Listed> {
    let package = Package::read_file(path).map_err(|e| e.to_string())?;
    contributions_ok(home, &Source::Packed(package.clone()))?;
    Store::new(home)
        .install(&package)
        .map_err(|e| e.to_string())
}

/// Installs a folder under development. It waits for approval, and again
/// whenever its manifest changes. Its code runs unapproved, so it may not
/// sit where an agent can write (see [`agent_dirs`]).
pub fn install_folder(home: &Path, dir: &Path) -> Result<Listed> {
    let person = std::env::var_os("HOME").map(std::path::PathBuf::from);
    let refused = agent_dirs(home, person.as_deref())?;
    contributions_ok(home, &Source::folder(dir).map_err(|e| e.to_string())?)?;
    Store::new(home)
        .install_folder(dir, &refused)
        .map_err(|e| e.to_string())
}

/// What every agent may change besides the task's copy and `/tmp`: its
/// state, logins and home of its own, under `person` (the person's home);
/// and the folders tasks work on, which an agent's work reaches once a
/// commit takes it there.
pub fn agent_dirs(home: &Path, person: Option<&Path>) -> Result<Vec<std::path::PathBuf>> {
    let mut dirs: Vec<std::path::PathBuf> = kule_engine::Task::list(home)
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|t| t.workspace)
        .collect();
    let Some(person) = person else {
        return Ok(dirs);
    };
    let agents = kule_acp::Agents::load(home).map_err(|e| e.to_string())?;
    dirs.extend(agents.listed().iter().flat_map(|l| {
        let p = &l.profile;
        p.state
            .iter()
            .chain(&p.login)
            .chain(&p.outside)
            .cloned()
            .chain(p.own_home.as_ref().map(|_| p.own_home_path()))
            .map(|rel| person.join(rel))
            .collect::<Vec<_>>()
    }));
    Ok(dirs)
}

/// What an extension adds, checked as Kule checks its own.
fn contributions_ok(home: &Path, source: &Source) -> Result<()> {
    let manifest = source.manifest();
    for theme in &manifest.contributes.themes {
        let bytes = source.read(theme).map_err(|e| e.to_string())?;
        themes::read_file(&bytes).map_err(|e| format!("{theme}: {e}"))?;
    }
    if let Some(path) = &manifest.contributes.agents {
        let text = String::from_utf8(source.read(path).map_err(|e| e.to_string())?)
            .map_err(|_| format!("{path} is not UTF-8"))?;
        let profiles = kule_acp::parse_profiles(&text).map_err(|e| format!("{path}: {e}"))?;
        if profiles.is_empty() {
            return Err(format!("{path} holds no [[agent]]"));
        }
        let agents = kule_acp::Agents::load(home).map_err(|e| e.to_string())?;
        for profile in &profiles {
            agents
                .check_draft(home, profile)
                .map_err(|why| format!("{path}: agent {}: {why}", profile.name))?;
        }
    }
    for server in &manifest.contributes.mcp_servers {
        kule_acp::mcp::pinned(&server.command, &server.args)
            .map_err(|why| format!("MCP server {}: {why}", server.name))?;
    }
    Ok(())
}

/// The MCP servers extensions that may run offer, approved or not.
pub fn mcp_servers(home: &Path) -> Result<Vec<OfferedMcp>> {
    Store::new(home).mcp_servers().map_err(|e| e.to_string())
}

pub fn approve_mcp(home: &Path, extension: &str, name: &str, digest: &str) -> Result<()> {
    Store::new(home)
        .approve_mcp(extension, name, digest)
        .map_err(|e| e.to_string())
}

pub fn revoke_mcp(home: &Path, extension: &str, name: &str) -> Result<()> {
    Store::new(home)
        .revoke_mcp(extension, name)
        .map_err(|e| e.to_string())
}

/// The approved MCP servers, as an agent is told to start them; npx
/// packages are installed once under `person` (the person's home), when
/// given.
pub fn mcp_for_runs(home: &Path, person: Option<&Path>) -> Vec<kule_acp::McpCommand> {
    Store::new(home)
        .mcp_servers()
        .unwrap_or_default()
        .into_iter()
        .filter(|o| o.approved)
        .map(|o| {
            kule_acp::McpCommand::prepared(
                person,
                &o.server.name,
                &o.server.command,
                &o.server.args,
                &o.server.env,
            )
        })
        .collect()
}

pub fn list(home: &Path) -> Result<Vec<Listed>> {
    Store::new(home).list().map_err(|e| e.to_string())
}

pub fn approve(home: &Path, id: &str, digest: &str) -> Result<Listed> {
    Store::new(home)
        .approve(id, digest)
        .map_err(|e| e.to_string())
}

pub fn set_enabled(home: &Path, id: &str, enabled: bool) -> Result<Listed> {
    Store::new(home)
        .set_enabled(id, enabled)
        .map_err(|e| e.to_string())
}

pub fn remove(home: &Path, id: &str) -> Result<()> {
    Store::new(home).remove(id).map_err(|e| e.to_string())?;
    // Its agents went with it; their approvals go too, so a later
    // extension of the same id starts unapproved.
    kule_acp::Agents::forget_extension(home, id).map_err(|e| e.to_string())
}

pub fn storage(home: &Path, id: &str) -> Result<Value> {
    Store::new(home).storage(id).map_err(|e| e.to_string())
}

pub fn set_storage(home: &Path, id: &str, value: &Value) -> Result<()> {
    Store::new(home)
        .set_storage(id, value)
        .map_err(|e| e.to_string())
}

pub fn config(home: &Path, id: &str) -> Result<BTreeMap<String, Value>> {
    Store::new(home).config(id).map_err(|e| e.to_string())
}

pub fn set_config(home: &Path, id: &str, key: &str, value: &Value) -> Result<()> {
    Store::new(home)
        .set_config(id, key, value)
        .map_err(|e| e.to_string())
}

/// A theme an extension that may run brings.
#[derive(Debug, Clone, Serialize)]
pub struct ExtensionTheme {
    pub extension: String,
    pub extension_name: String,
    pub theme: ThemeFile,
}

/// The themes of every extension that may run. One that cannot be read
/// is left out rather than failing the rest.
pub fn themes(home: &Path) -> Result<Vec<ExtensionTheme>> {
    let store = Store::new(home);
    let mut out = Vec::new();
    for listed in list(home)?.into_iter().filter(Listed::runs) {
        let Ok(source) = store.runnable(&listed.id) else {
            continue;
        };
        let manifest = source.manifest();
        for path in &manifest.contributes.themes {
            let Ok(theme) = source
                .read(path)
                .map_err(|e| e.to_string())
                .and_then(|b| themes::read_file(&b))
            else {
                continue;
            };
            out.push(ExtensionTheme {
                extension: manifest.id.clone(),
                extension_name: manifest.name.clone(),
                theme,
            });
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn runs_are_given_only_the_mcp_servers_the_person_approved() {
        let home = TempDir::new().unwrap();
        // Not under /tmp, where a folder under development is refused.
        let src = TempDir::new_in(concat!(env!("CARGO_MANIFEST_DIR"), "/../../target")).unwrap();
        fs::write(
            src.path().join("kule-extension.json"),
            r#"{ "kule_extension": 1, "id": "acme.tools", "name": "Tools", "version": "1.0.0",
                 "publisher": "acme", "contributes": { "mcpServers": [
                   { "name": "db", "command": "npx", "args": ["-y", "@acme/db@1.2.3"] },
                   { "name": "web", "command": "uvx", "args": ["acme-web==2.0.0"] } ] } }"#,
        )
        .unwrap();
        let listed = install_folder(home.path(), src.path()).unwrap();
        approve(home.path(), "acme.tools", &listed.digest).unwrap();
        assert!(mcp_for_runs(home.path(), None).is_empty());

        let offered = mcp_servers(home.path()).unwrap();
        let web = offered.iter().find(|o| o.server.name == "web").unwrap();
        let stale = approve_mcp(home.path(), "acme.tools", "web", "not-what-you-saw").unwrap_err();
        assert!(stale.contains("changed since you looked"), "{stale}");
        approve_mcp(home.path(), "acme.tools", "web", &web.digest).unwrap();
        let given = mcp_for_runs(home.path(), None);
        assert_eq!(given.len(), 1);
        assert_eq!(
            (
                given[0].name.as_str(),
                given[0].program.as_str(),
                given[0].args.as_slice()
            ),
            ("web", "uvx", &["acme-web==2.0.0".to_owned()][..])
        );

        // An extension turned off offers nothing, approved or not.
        set_enabled(home.path(), "acme.tools", false).unwrap();
        assert!(mcp_for_runs(home.path(), None).is_empty());
    }

    /// A folder under development inside a folder a task works on: an
    /// agent's work, once committed, would change what runs.
    #[test]
    fn an_extension_in_a_tasks_folder_is_refused() {
        let home = TempDir::new().unwrap();
        let ws = TempDir::new_in(concat!(env!("CARGO_MANIFEST_DIR"), "/../../target")).unwrap();
        let src = ws.path().join("tools");
        fs::create_dir(&src).unwrap();
        fs::write(
            src.join("kule-extension.json"),
            r#"{ "kule_extension": 1, "id": "acme.tools", "name": "Tools", "version": "1.0.0", "publisher": "acme" }"#,
        )
        .unwrap();
        kule_engine::Task::create(home.path(), ws.path()).unwrap();
        let refused = install_folder(home.path(), &src).unwrap_err();
        assert!(refused.contains("agents can write"), "{refused}");
    }
}
