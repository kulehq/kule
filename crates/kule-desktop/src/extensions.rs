//! Extensions in the window: their files, served to sandboxed frames by
//! [`crate::ext_serve`], and the person's handling of them (install,
//! approve, turn on or off, remove), carried to the daemon.
//!
//! Only extensions that may run (on, and approved as they are) are served.
//! A package is kept in memory once read, its digest checked against the
//! approval; a folder under development is read live.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use kule_daemon::{Call, Result};
use kule_ext::{Listed, Source, Store};
use serde::Serialize;
use serde_json::Value;
use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;

use crate::K;
use crate::ext_serve::{self, Files, Server};

/// Kule's own files every extension's frames load, at `_kule/` beside any
/// of the extension's pages.
const SDK: &str = include_str!("../ext/sdk.js");

pub struct StoreFiles {
    home: PathBuf,
    packed: Mutex<HashMap<String, Source>>,
}

impl StoreFiles {
    pub fn new(home: &Path) -> StoreFiles {
        StoreFiles {
            home: home.to_path_buf(),
            packed: Mutex::new(HashMap::new()),
        }
    }

    /// Drops what was read, after any change to the extensions.
    pub fn forget(&self) {
        self.packed.lock().unwrap().clear();
    }

    fn source(&self, ext: &str) -> Option<Source> {
        if let Some(source) = self.packed.lock().unwrap().get(ext) {
            return Some(source.clone());
        }
        let source = Store::new(&self.home).runnable(ext).ok()?;
        if matches!(source, Source::Packed(_)) {
            self.packed
                .lock()
                .unwrap()
                .insert(ext.to_owned(), source.clone());
        }
        Some(source)
    }
}

impl Files for StoreFiles {
    fn get(&self, ext: &str, path: &str) -> Option<(Vec<u8>, &'static str)> {
        let source = self.source(ext)?;
        let segments: Vec<&str> = path.split('/').collect();
        match segments.as_slice() {
            // The hidden page `main` runs in.
            ["_kule", "main.html"] => {
                let main = source.manifest().main.as_ref()?;
                // `main` is a plain path (checked with the manifest): no
                // quote or angle bracket can be in it.
                let page = format!(
                    "<!doctype html><meta charset=\"utf-8\">\
                     <script src=\"sdk.js\"></script><script src=\"../{main}\"></script>"
                );
                Some((page.into_bytes(), "text/html; charset=utf-8"))
            }
            [.., "_kule", "sdk.js"] => {
                Some((SDK.as_bytes().to_vec(), "text/javascript; charset=utf-8"))
            }
            _ => {
                let media = kule_ext::media_type(path)?;
                Some((source.read(path).ok()?, media))
            }
        }
    }
}

/// The window's extension host: where the files are served.
pub struct Host {
    server: Option<Server>,
    files: Arc<StoreFiles>,
}

impl Host {
    pub fn start(home: &Path) -> Host {
        let files = Arc::new(StoreFiles::new(home));
        let server = match ext_serve::start(files.clone()) {
            Ok(server) => Some(server),
            Err(e) => {
                eprintln!("kule-desktop: extensions cannot be served: {e}");
                None
            }
        };
        Host { server, files }
    }

    pub fn forget(&self) {
        self.files.forget();
    }

    /// The origin extension frames come from, when they are served.
    pub fn origin(&self) -> Option<String> {
        self.server.as_ref().map(Server::origin)
    }
}

/// What the window needs to place an extension's frames.
#[derive(Serialize)]
pub struct HostInfo {
    /// Where extension files are: `<prefix><id>/<path>`. None when they
    /// cannot be served (the window then runs no extension).
    prefix: Option<String>,
}

#[tauri::command]
pub fn extension_host(host: State<'_, Host>) -> HostInfo {
    HostInfo {
        prefix: host.server.as_ref().map(Server::prefix),
    }
}

#[tauri::command(async)]
pub fn extensions(kule: K<'_>) -> Result<Vec<Listed>> {
    kule.call(Call::Extensions)
}

/// Installs a `.kulext` the person picks; None when they picked nothing.
#[tauri::command(async)]
pub fn install_extension(app: AppHandle, kule: K<'_>) -> Result<Option<Listed>> {
    let Some(path) = app
        .dialog()
        .file()
        .set_title("Choose a Kule extension")
        .add_filter("Kule extension", &["kulext"])
        .blocking_pick_file()
        .and_then(|p| p.into_path().ok())
    else {
        return Ok(None);
    };
    kule.call(Call::InstallExtension { path }).map(Some)
}

/// Installs a folder under development the person picks.
#[tauri::command(async)]
pub fn install_dev_extension(app: AppHandle, kule: K<'_>) -> Result<Option<Listed>> {
    let Some(path) = app
        .dialog()
        .file()
        .set_title("Choose an extension's folder")
        .blocking_pick_folder()
        .and_then(|p| p.into_path().ok())
    else {
        return Ok(None);
    };
    kule.call(Call::InstallDevExtension { path }).map(Some)
}

#[tauri::command(async)]
pub fn approve_extension(kule: K<'_>, id: String, digest: String) -> Result<Listed> {
    kule.call(Call::ApproveExtension { id, digest })
}

#[tauri::command(async)]
pub fn set_extension_enabled(kule: K<'_>, id: String, enabled: bool) -> Result<Listed> {
    kule.call(Call::SetExtensionEnabled { id, enabled })
}

#[tauri::command(async)]
pub fn remove_extension(kule: K<'_>, id: String) -> Result<()> {
    kule.call(Call::RemoveExtension { id })
}

#[tauri::command(async)]
pub fn extension_config(kule: K<'_>, id: String) -> Result<HashMap<String, Value>> {
    kule.call(Call::ExtensionConfig { id })
}

#[tauri::command(async)]
pub fn set_extension_config(kule: K<'_>, id: String, key: String, value: Value) -> Result<()> {
    kule.call(Call::SetExtensionConfig { id, key, value })
}

#[tauri::command(async)]
pub fn extension_themes(kule: K<'_>) -> Result<Vec<Value>> {
    kule.call(Call::ExtensionThemes)
}

#[tauri::command(async)]
pub fn mcp_servers(kule: K<'_>) -> Result<Vec<Value>> {
    kule.call(Call::McpServers)
}

#[tauri::command(async)]
pub fn approve_mcp_server(
    kule: K<'_>,
    extension: String,
    name: String,
    digest: String,
) -> Result<()> {
    kule.call(Call::ApproveMcpServer {
        extension,
        name,
        digest,
    })
}

#[tauri::command(async)]
pub fn revoke_mcp_server(kule: K<'_>, extension: String, name: String) -> Result<()> {
    kule.call(Call::RevokeMcpServer { extension, name })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn installed(home: &Path) -> String {
        let src = TempDir::new().unwrap();
        fs::write(
            src.path().join("kule-extension.json"),
            r#"{ "kule_extension": 1, "id": "acme.notes", "name": "Notes", "version": "1.0.0",
                 "publisher": "acme", "main": "src/main.js",
                 "contributes": { "panels": [{ "id": "notes", "title": "Notes", "entry": "pages/panel.html" }] } }"#,
        )
        .unwrap();
        fs::create_dir(src.path().join("src")).unwrap();
        fs::write(src.path().join("src/main.js"), "// main").unwrap();
        fs::create_dir(src.path().join("pages")).unwrap();
        fs::write(src.path().join("pages/panel.html"), "<p>notes</p>").unwrap();
        let package = kule_ext::Package::open(kule_ext::pack(src.path()).unwrap()).unwrap();
        Store::new(home).install(&package).unwrap();
        package.digest().to_owned()
    }

    #[test]
    fn only_an_approved_extension_that_is_on_is_served() {
        let home = TempDir::new().unwrap();
        let digest = installed(home.path());
        let files = StoreFiles::new(home.path());
        assert!(
            files.get("acme.notes", "pages/panel.html").is_none(),
            "not approved yet"
        );

        Store::new(home.path())
            .approve("acme.notes", &digest)
            .unwrap();
        let (page, media) = files.get("acme.notes", "pages/panel.html").unwrap();
        assert_eq!(
            (page.as_slice(), media),
            (&b"<p>notes</p>"[..], "text/html; charset=utf-8")
        );
        assert!(files.get("other.ext", "pages/panel.html").is_none());

        Store::new(home.path())
            .set_enabled("acme.notes", false)
            .unwrap();
        files.forget();
        assert!(
            files.get("acme.notes", "pages/panel.html").is_none(),
            "turned off"
        );
    }

    #[test]
    fn the_sdk_is_beside_every_page_and_main_gets_a_page_of_its_own() {
        let home = TempDir::new().unwrap();
        let digest = installed(home.path());
        Store::new(home.path())
            .approve("acme.notes", &digest)
            .unwrap();
        let files = StoreFiles::new(home.path());
        for at in ["_kule/sdk.js", "pages/_kule/sdk.js"] {
            let (sdk, media) = files.get("acme.notes", at).unwrap();
            assert_eq!(sdk, SDK.as_bytes(), "{at}");
            assert_eq!(media, "text/javascript; charset=utf-8");
        }
        let (page, _) = files.get("acme.notes", "_kule/main.html").unwrap();
        let page = String::from_utf8(page).unwrap();
        assert!(
            page.contains(
                r#"<script src="sdk.js"></script><script src="../src/main.js"></script>"#
            ),
            "{page}"
        );
    }
}
