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
    /// Packages read once, shared rather than copied per request.
    packed: Mutex<HashMap<String, Arc<Source>>>,
    /// Where agents may write (their own directories), read once.
    agent_dirs: Mutex<Option<Vec<PathBuf>>>,
    /// The person's home, which agents' directories are under.
    person: Option<PathBuf>,
}

impl StoreFiles {
    pub fn new(home: &Path) -> StoreFiles {
        StoreFiles::with_person(home, std::env::var_os("HOME").map(PathBuf::from))
    }

    fn with_person(home: &Path, person: Option<PathBuf>) -> StoreFiles {
        StoreFiles {
            home: home.to_path_buf(),
            packed: Mutex::new(HashMap::new()),
            agent_dirs: Mutex::new(None),
            person,
        }
    }

    /// Drops what was read, after any change to the extensions.
    pub fn forget(&self) {
        self.packed.lock().unwrap().clear();
        *self.agent_dirs.lock().unwrap() = None;
    }

    fn agent_dirs(&self) -> Vec<PathBuf> {
        let mut kept = self.agent_dirs.lock().unwrap();
        kept.get_or_insert_with(|| {
            kule_daemon::extensions::agent_dirs(&self.home, self.person.as_deref())
                .unwrap_or_default()
        })
        .clone()
    }

    fn source(&self, ext: &str) -> Option<Arc<Source>> {
        if let Some(source) = self.packed.lock().unwrap().get(ext) {
            return Some(source.clone());
        }
        let source = Arc::new(Store::new(&self.home).runnable(ext).ok()?);
        match source.as_ref() {
            Source::Packed(_) => {
                self.packed
                    .lock()
                    .unwrap()
                    .insert(ext.to_owned(), source.clone());
            }
            // A folder an agent could rewrite since it was installed (an
            // agent's directory named after) is not served.
            Source::Folder { dir, .. } => {
                if kule_ext::under_any(dir, &self.agent_dirs()).is_some() {
                    return None;
                }
            }
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
    /// Whether the window has no WebRTC, which the content policy cannot
    /// close (see [`no_webrtc`]); no extension runs until it is so.
    webrtc_off: std::sync::atomic::AtomicBool,
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
        Host {
            server,
            files,
            // WebKitGTK ships with WebRTC off; macOS turns it off at start.
            webrtc_off: std::sync::atomic::AtomicBool::new(!cfg!(target_os = "macos")),
        }
    }

    pub fn forget(&self) {
        self.files.forget();
    }

    /// Records that the window's WebRTC is off.
    pub fn webrtc_is_off(&self) {
        self.webrtc_off
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }

    /// Where extension files are, or None when no extension may run.
    fn prefix(&self) -> Option<String> {
        let off = self.webrtc_off.load(std::sync::atomic::Ordering::SeqCst);
        self.server.as_ref().filter(|_| off).map(Server::prefix)
    }

    /// The origin extension frames come from, when they are served.
    pub fn origin(&self) -> Option<String> {
        self.server.as_ref().map(Server::origin)
    }
}

/// Turns WebRTC off in the window, so an extension's frame has no
/// `RTCPeerConnection`: the content policy does not govern it, and a frame
/// with no `net` permission reached a public STUN server through it (data
/// can ride in the server's name or the request). Kule itself uses none.
/// WebKitGTK ships with WebRTC off; on macOS it is a WebKit preference.
/// Returns whether it is off.
#[cfg(target_os = "macos")]
pub fn no_webrtc(webview: *mut std::ffi::c_void) -> bool {
    use objc2::runtime::AnyObject;
    use objc2::{msg_send, sel};
    if webview.is_null() {
        return false;
    }
    // SAFETY: `webview` is the window's WKWebView, alive for this call;
    // each message is sent only after the object says it answers it.
    unsafe {
        let web: &AnyObject = &*(webview as *const AnyObject);
        let config: *mut AnyObject = msg_send![web, configuration];
        let Some(config) = config.as_ref() else {
            return false;
        };
        let prefs: *mut AnyObject = msg_send![config, preferences];
        let Some(prefs) = prefs.as_ref() else {
            return false;
        };
        let setter = sel!(_setPeerConnectionEnabled:);
        let answers: bool = msg_send![prefs, respondsToSelector: setter];
        if !answers {
            return false;
        }
        let _: () = msg_send![prefs, _setPeerConnectionEnabled: false];
        let on: bool = msg_send![prefs, _peerConnectionEnabled];
        !on
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
        prefix: host.prefix(),
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

    /// No extension runs while the window could still have WebRTC.
    #[test]
    fn extensions_wait_for_webrtc_to_be_off() {
        let home = TempDir::new().unwrap();
        let host = Host::start(home.path());
        if cfg!(target_os = "macos") {
            assert!(host.prefix().is_none());
        }
        host.webrtc_is_off();
        assert!(
            host.prefix()
                .is_some_and(|p| p.starts_with("http://127.0.0.1:"))
        );
    }

    /// A folder that became an agent's after it was installed (a profile
    /// added later names it) is no longer served, nor its calls answered.
    #[test]
    fn a_folder_that_became_an_agents_is_no_longer_served() {
        let base = concat!(env!("CARGO_MANIFEST_DIR"), "/../../target");
        let (home, person) = (TempDir::new().unwrap(), TempDir::new_in(base).unwrap());
        let dir = person.path().join(".acme/ext");
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("kule-extension.json"),
            r#"{ "kule_extension": 1, "id": "acme.notes", "name": "Notes", "version": "1.0.0",
                 "publisher": "acme", "permissions": ["tasks.read"],
                 "contributes": { "panels": [{ "id": "notes", "title": "Notes", "entry": "panel.html" }] } }"#,
        )
        .unwrap();
        fs::write(dir.join("panel.html"), "<p>notes</p>").unwrap();
        let store = Store::new(home.path());
        let listed = store.install_folder(&dir, &[]).unwrap();
        store.approve("acme.notes", &listed.digest).unwrap();
        let files = StoreFiles::with_person(home.path(), Some(person.path().to_path_buf()));
        assert!(files.get("acme.notes", "panel.html").is_some());

        fs::write(
            home.path().join("agents.toml"),
            "[[agent]]\nname = \"acme-agent\"\ncommand = [\"acme-agent\"]\nstate = [\".acme\"]\n",
        )
        .unwrap();
        files.forget();
        assert!(files.get("acme.notes", "panel.html").is_none());
        let e =
            crate::ext_call::granted(home.path(), "acme.notes", Some(person.path())).unwrap_err();
        assert!(e.contains("where an agent can write"), "{e}");
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
