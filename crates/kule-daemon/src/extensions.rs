//! The person's extensions, as the daemon and the CLI handle them: the
//! package checks of `kule_ext`, plus the checks of what an extension adds
//! that only Kule's own modules know (themes).

use std::collections::BTreeMap;
use std::path::Path;

use kule_ext::{Listed, Package, Source, Store};
use serde::Serialize;
use serde_json::Value;

use crate::Result;
use crate::themes::{self, ThemeFile};

/// Installs or updates the `.kulext` at `path`. It waits for approval.
pub fn install(home: &Path, path: &Path) -> Result<Listed> {
    let package = Package::read_file(path).map_err(|e| e.to_string())?;
    contributions_ok(&Source::Packed(package.clone()))?;
    Store::new(home)
        .install(&package)
        .map_err(|e| e.to_string())
}

/// Installs a folder under development. It waits for approval, and again
/// whenever its manifest changes.
pub fn install_folder(home: &Path, dir: &Path) -> Result<Listed> {
    contributions_ok(&Source::folder(dir).map_err(|e| e.to_string())?)?;
    Store::new(home)
        .install_folder(dir)
        .map_err(|e| e.to_string())
}

/// What an extension adds, checked as Kule checks its own.
fn contributions_ok(source: &Source) -> Result<()> {
    let manifest = source.manifest();
    for theme in &manifest.contributes.themes {
        let bytes = source.read(theme).map_err(|e| e.to_string())?;
        themes::read_file(&bytes).map_err(|e| format!("{theme}: {e}"))?;
    }
    if manifest.contributes.agents.is_some() || !manifest.contributes.mcp_servers.is_empty() {
        return Err("this Kule does not yet take agents or MCP servers from extensions".to_owned());
    }
    Ok(())
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
    Store::new(home).remove(id).map_err(|e| e.to_string())
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
