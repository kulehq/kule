//! The person's extensions, under `<kule home>/extensions/`:
//!
//! - `installed.json`: id → version, digest, on or off, and the folder of
//!   one under development;
//! - `approved.json`: id → the digest, permissions and hosts the person
//!   approved;
//! - `mcp-approved.json`: `<id>/<server>` → the digest of each MCP server
//!   the person let agents use;
//! - `packages/<id>-<digest>.kulext`: the packages, never unpacked;
//! - `data/<id>/`: what the extension keeps (`storage.json`) and its
//!   settings (`config.json`).
//!
//! An extension runs only when it is on and its approved digest is the one
//! it has now. Anything else holds it, and says why.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{At, Error, Result, invalid};
use crate::manifest::{Manifest, McpServer, Permission};
use crate::package::{Package, Source};

const INSTALLED: &str = "installed.json";
const APPROVED: &str = "approved.json";
/// `<extension>/<server>` → the digest of the MCP server the person approved.
const MCP_APPROVED: &str = "mcp-approved.json";

/// An MCP server an extension offers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OfferedMcp {
    pub extension: String,
    pub extension_name: String,
    pub server: McpServer,
    pub digest: String,
    pub approved: bool,
}

fn mcp_digest(server: &McpServer) -> String {
    blake3::hash(&serde_json::to_vec(server).expect("a server serializes"))
        .to_hex()
        .to_string()
}
/// The most an extension keeps in `storage`.
const MAX_STORAGE: usize = 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Installed {
    version: String,
    digest: String,
    enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    dev: Option<PathBuf>,
}

/// What the person approved, kept after a change so the next approval can
/// show what is new.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Approval {
    pub digest: String,
    pub permissions: Vec<Permission>,
    pub hosts: Vec<String>,
    /// The other extensions the person approved it beside, knowing that
    /// together they can read their work and send data (see
    /// [`Listed::together`]).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub together: Vec<String>,
}

/// One installed extension, as the person sees it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Listed {
    pub id: String,
    pub version: String,
    /// None when its package or folder can no longer be read.
    pub manifest: Option<Manifest>,
    pub digest: String,
    pub enabled: bool,
    /// The folder of an extension under development.
    pub dev: Option<PathBuf>,
    /// Why it may not run; None when it may.
    pub held: Option<String>,
    pub approved: Option<Approval>,
    /// Permissions it asks for that the last approval did not give.
    pub new_permissions: Vec<Permission>,
    /// Hosts it asks for that the last approval did not give.
    pub new_hosts: Vec<String>,
    /// The other extensions that would run beside it with which it can
    /// read the person's work and send data, where it cannot do both
    /// alone: one reads, another sends. A combination the person did not
    /// approve holds it.
    #[serde(default)]
    pub together: Vec<String>,
}

impl Listed {
    /// Whether it may run now.
    pub fn runs(&self) -> bool {
        self.enabled && self.held.is_none()
    }
}

pub struct Store {
    dir: PathBuf,
    /// More folders agents can write, where a folder under development is
    /// refused (see [`Store::refusing`]).
    refused: Vec<PathBuf>,
}

/// Fills each extension's [`Listed::together`] and holds the combinations
/// the person did not approve: of two extensions that would run (on, and
/// approved as they are), one reading their work and the other sending
/// data, approving either one beside the other approves the pair.
fn together(listed: &mut [Listed]) {
    /// What one extension can do, and the others it was approved beside.
    struct Facts {
        id: String,
        would_run: bool,
        reads: bool,
        sends: bool,
        approved_beside: Vec<String>,
    }
    let can = |l: &Listed| {
        let perms = l
            .manifest
            .as_ref()
            .map(|m| m.permissions.as_slice())
            .unwrap_or_default();
        (
            perms.iter().any(|p| p.reads_work()),
            perms.contains(&Permission::Net),
        )
    };
    let facts: Vec<Facts> = listed
        .iter()
        .map(|l| {
            let (reads, sends) = can(l);
            Facts {
                id: l.id.clone(),
                would_run: l.enabled && l.held.is_none(),
                reads,
                sends,
                approved_beside: l
                    .approved
                    .as_ref()
                    .map(|a| a.together.clone())
                    .unwrap_or_default(),
            }
        })
        .collect();
    for l in listed.iter_mut() {
        let (reads, sends) = can(l);
        if reads && sends {
            // Its own card says so already.
            continue;
        }
        l.together = facts
            .iter()
            .filter(|f| f.would_run && f.id != l.id && ((reads && f.sends) || (sends && f.reads)))
            .map(|f| f.id.clone())
            .collect();
        if l.held.is_some() || !l.enabled {
            continue;
        }
        let mine = l
            .approved
            .as_ref()
            .map(|a| a.together.clone())
            .unwrap_or_default();
        let unapproved: Vec<&str> = l
            .together
            .iter()
            .filter(|other| {
                !mine.contains(other)
                    && !facts
                        .iter()
                        .any(|f| f.id == **other && f.approved_beside.contains(&l.id))
            })
            .map(String::as_str)
            .collect();
        if !unapproved.is_empty() {
            l.held = Some(format!(
                "together with {} it can read your work and send data; approve it to let them run together",
                unapproved.join(", ")
            ));
        }
    }
}

impl Store {
    pub fn new(kule_home: &Path) -> Store {
        Store {
            dir: kule_home.join("extensions"),
            refused: Vec::new(),
        }
    }

    /// The store, holding every folder under development that sits under
    /// one of `dirs` (the folders tasks work on, the agents' own, which the
    /// caller knows), as it holds one under a temporary directory: told
    /// each time it is listed or run, not only when it was installed.
    pub fn refusing(mut self, dirs: Vec<PathBuf>) -> Store {
        self.refused = dirs;
        self
    }

    /// Installs or updates a package. An update is held until approved.
    pub fn install(&self, package: &Package) -> Result<Listed> {
        let manifest = package.manifest();
        let id = manifest.id.clone();
        let mut installed = self.installed()?;
        let packages = self.dir.join("packages");
        fs::create_dir_all(&packages).at(&packages)?;
        let file = self.package_path(&id, package.digest());
        write_atomic(&file, package.bytes())?;
        let old = installed.insert(
            id.clone(),
            Installed {
                version: manifest.version.clone(),
                digest: package.digest().to_owned(),
                enabled: installed.get(&id).is_none_or(|i| i.enabled),
                dev: None,
            },
        );
        self.write(INSTALLED, &installed)?;
        if let Some(old) = old
            && old.dev.is_none()
            && old.digest != package.digest()
        {
            let _ = fs::remove_file(self.package_path(&id, &old.digest));
        }
        self.get(&id)
    }

    /// Installs a folder under development, read live from where it is.
    ///
    /// A folder under development runs its code as it is, unapproved, so it
    /// must sit where no agent can write: not under a temporary directory,
    /// Kule's home (the tasks' copies) or any of `also_refused` (the agents'
    /// own directories, which the caller knows).
    pub fn install_folder(&self, dir: &Path, also_refused: &[PathBuf]) -> Result<Listed> {
        let dir = dir.canonicalize().at(dir)?;
        if let Some(root) = self
            .agent_writable(&dir)
            .or_else(|| under_any(&dir, also_refused))
        {
            return Err(invalid(format!(
                "{} is under {}, where agents can write; an extension under development must sit where only you can change it",
                dir.display(),
                root.display()
            )));
        }
        let source = Source::folder(&dir)?;
        let id = source.manifest().id.clone();
        let mut installed = self.installed()?;
        let old = installed.insert(
            id.clone(),
            Installed {
                version: source.manifest().version.clone(),
                digest: source.digest().to_owned(),
                enabled: installed.get(&id).is_none_or(|i| i.enabled),
                dev: Some(dir),
            },
        );
        self.write(INSTALLED, &installed)?;
        if let Some(old) = old
            && old.dev.is_none()
        {
            let _ = fs::remove_file(self.package_path(&id, &old.digest));
        }
        self.get(&id)
    }

    pub fn list(&self) -> Result<Vec<Listed>> {
        let installed = self.installed()?;
        let approved = self.approved()?;
        let mut listed: Vec<Listed> = installed
            .iter()
            .map(|(id, i)| self.listed(id, i, approved.get(id)))
            .collect();
        together(&mut listed);
        Ok(listed)
    }

    pub fn get(&self, id: &str) -> Result<Listed> {
        self.list()?
            .into_iter()
            .find(|l| l.id == id)
            .ok_or_else(|| Error::NotFound(id.to_owned()))
    }

    fn listed(&self, id: &str, i: &Installed, approval: Option<&Approval>) -> Listed {
        let source = self.source(id, i);
        let (manifest, digest, mut held) = match &source {
            Ok(s) => (Some(s.manifest().clone()), s.digest().to_owned(), None),
            Err(e) => (
                None,
                i.digest.clone(),
                Some(format!("it cannot be read: {e}")),
            ),
        };
        if held.is_none() {
            held = match approval {
                None => Some("not approved yet".to_owned()),
                Some(a) if a.digest != digest => {
                    Some("it changed since you approved it".to_owned())
                }
                Some(_) => None,
            };
        }
        let (new_permissions, new_hosts) = match &manifest {
            Some(m) => (
                m.permissions
                    .iter()
                    .filter(|p| approval.is_none_or(|a| !a.permissions.contains(p)))
                    .copied()
                    .collect(),
                m.hosts
                    .iter()
                    .filter(|h| approval.is_none_or(|a| !a.hosts.contains(h)))
                    .cloned()
                    .collect(),
            ),
            None => (Vec::new(), Vec::new()),
        };
        Listed {
            id: id.to_owned(),
            version: i.version.clone(),
            manifest,
            digest,
            enabled: i.enabled,
            dev: i.dev.clone(),
            held,
            approved: approval.cloned(),
            new_permissions,
            new_hosts,
            together: Vec::new(),
        }
    }

    /// The temporary directory or Kule home `dir` is under, if any: places
    /// a sandboxed agent can write.
    fn agent_writable(&self, dir: &Path) -> Option<PathBuf> {
        let kule_home = self.dir.parent().map(Path::to_path_buf);
        let roots: Vec<PathBuf> = AGENT_WRITABLE
            .iter()
            .map(PathBuf::from)
            .chain(kule_home)
            .collect();
        under_any(dir, &roots)
    }

    fn source(&self, id: &str, i: &Installed) -> Result<Source> {
        if let Some(dir) = &i.dev
            && let Some(root) = self
                .agent_writable(dir)
                .or_else(|| under_any(dir, &self.refused))
        {
            return Err(invalid(format!(
                "its folder is under {}, where agents can write",
                root.display()
            )));
        }
        let source = match &i.dev {
            Some(dir) => Source::folder(dir)?,
            None => Source::Packed(Package::read_file(&self.package_path(id, &i.digest))?),
        };
        // A package renamed into place under another id, or a folder whose
        // manifest now says something else, is not this extension.
        if source.manifest().id != id {
            return Err(invalid(format!(
                "it now calls itself {}",
                source.manifest().id
            )));
        }
        if i.dev.is_none() && source.digest() != i.digest {
            return Err(invalid("its package changed on disk"));
        }
        Ok(source)
    }

    /// Approves the extension as the person saw it: `digest` is what they
    /// looked at, and if the extension changed since, nothing is approved.
    pub fn approve(&self, id: &str, digest: &str) -> Result<Listed> {
        let installed = self.installed()?;
        let i = installed
            .get(id)
            .ok_or_else(|| Error::NotFound(id.to_owned()))?;
        let source = self.source(id, i)?;
        if source.digest() != digest {
            return Err(invalid(format!(
                "extension {id} changed since you looked at it; look again"
            )));
        }
        // What it can do beside the others, as the person saw it.
        let beside = self.get(id)?.together;
        let mut approved = self.approved()?;
        approved.insert(
            id.to_owned(),
            Approval {
                digest: digest.to_owned(),
                permissions: source.manifest().permissions.clone(),
                hosts: source.manifest().hosts.clone(),
                together: beside,
            },
        );
        self.write(APPROVED, &approved)?;
        self.get(id)
    }

    pub fn set_enabled(&self, id: &str, enabled: bool) -> Result<Listed> {
        let mut installed = self.installed()?;
        installed
            .get_mut(id)
            .ok_or_else(|| Error::NotFound(id.to_owned()))?
            .enabled = enabled;
        self.write(INSTALLED, &installed)?;
        self.get(id)
    }

    /// Removes the extension, its approval and everything it kept.
    pub fn remove(&self, id: &str) -> Result<()> {
        let mut installed = self.installed()?;
        let i = installed
            .remove(id)
            .ok_or_else(|| Error::NotFound(id.to_owned()))?;
        self.write(INSTALLED, &installed)?;
        let mut approved = self.approved()?;
        if approved.remove(id).is_some() {
            self.write(APPROVED, &approved)?;
        }
        let mut mcp: BTreeMap<String, String> = self.read(MCP_APPROVED)?;
        let before = mcp.len();
        mcp.retain(|key, _| !key.starts_with(&format!("{id}/")));
        if mcp.len() != before {
            self.write(MCP_APPROVED, &mcp)?;
        }
        if i.dev.is_none() {
            let _ = fs::remove_file(self.package_path(id, &i.digest));
        }
        let data = self.data_dir(id)?;
        if data.exists() {
            fs::remove_dir_all(&data).at(&data)?;
        }
        Ok(())
    }

    /// The MCP servers extensions that may run offer, each with whether the
    /// person approved it as it stands.
    pub fn mcp_servers(&self) -> Result<Vec<OfferedMcp>> {
        let approved: BTreeMap<String, String> = self.read(MCP_APPROVED)?;
        let mut out = Vec::new();
        for listed in self.list()?.into_iter().filter(Listed::runs) {
            let Some(manifest) = listed.manifest else {
                continue;
            };
            for server in manifest.contributes.mcp_servers {
                let digest = mcp_digest(&server);
                let key = format!("{}/{}", manifest.id, server.name);
                out.push(OfferedMcp {
                    extension: manifest.id.clone(),
                    extension_name: manifest.name.clone(),
                    approved: approved.get(&key) == Some(&digest),
                    server,
                    digest,
                });
            }
        }
        Ok(out)
    }

    /// Approves an offered MCP server as the person saw it.
    pub fn approve_mcp(&self, ext: &str, name: &str, digest: &str) -> Result<()> {
        let offered = self
            .mcp_servers()?
            .into_iter()
            .find(|o| o.extension == ext && o.server.name == name)
            .ok_or_else(|| invalid(format!("{ext} offers no MCP server {name} now")))?;
        if offered.digest != digest {
            return Err(invalid(format!(
                "MCP server {name} changed since you looked at it; look again"
            )));
        }
        let mut approved: BTreeMap<String, String> = self.read(MCP_APPROVED)?;
        approved.insert(format!("{ext}/{name}"), digest.to_owned());
        self.write(MCP_APPROVED, &approved)
    }

    /// Takes an MCP server's approval back.
    pub fn revoke_mcp(&self, ext: &str, name: &str) -> Result<()> {
        let mut approved: BTreeMap<String, String> = self.read(MCP_APPROVED)?;
        if approved.remove(&format!("{ext}/{name}")).is_some() {
            self.write(MCP_APPROVED, &approved)?;
        }
        Ok(())
    }

    /// The files of an extension that may run, or why it may not.
    pub fn runnable(&self, id: &str) -> Result<Source> {
        let installed = self.installed()?;
        let i = installed
            .get(id)
            .ok_or_else(|| Error::NotFound(id.to_owned()))?;
        let held = |why: &str| Error::Held {
            id: id.to_owned(),
            why: why.to_owned(),
        };
        if !i.enabled {
            return Err(held("it is turned off"));
        }
        let source = self.source(id, i).map_err(|e| held(&e.to_string()))?;
        match self.approved()?.get(id) {
            // Approved as it is, and beside the others that run with it.
            Some(a) if a.digest == source.digest() => match self.get(id)?.held {
                Some(why) => Err(held(&why)),
                None => Ok(source),
            },
            Some(_) => Err(held("it changed since you approved it")),
            None => Err(held("not approved yet")),
        }
    }

    /// What the extension kept; null when nothing.
    pub fn storage(&self, id: &str) -> Result<Value> {
        let path = self.data_dir(id)?.join("storage.json");
        if !path.exists() {
            return Ok(Value::Null);
        }
        read_json(&path)
    }

    pub fn set_storage(&self, id: &str, value: &Value) -> Result<()> {
        self.must_exist(id)?;
        let bytes = serde_json::to_vec(value).map_err(|e| invalid(e.to_string()))?;
        if bytes.len() > MAX_STORAGE {
            return Err(invalid(format!(
                "an extension keeps at most {} KiB",
                MAX_STORAGE / 1024
            )));
        }
        let dir = self.data_dir(id)?;
        fs::create_dir_all(&dir).at(&dir)?;
        write_atomic(&dir.join("storage.json"), &bytes)
    }

    /// The extension's settings: what the person chose, else the defaults.
    /// Settings the manifest no longer declares are left out.
    pub fn config(&self, id: &str) -> Result<BTreeMap<String, Value>> {
        let manifest = self
            .get(id)?
            .manifest
            .ok_or_else(|| invalid(format!("extension {id} cannot be read")))?;
        let path = self.data_dir(id)?.join("config.json");
        let chosen: BTreeMap<String, Value> = if path.exists() {
            read_json(&path)?
        } else {
            BTreeMap::new()
        };
        Ok(manifest
            .contributes
            .configuration
            .iter()
            .filter_map(|f| {
                let value = chosen
                    .get(&f.key)
                    .filter(|v| f.accepts(v))
                    .or(f.default.as_ref())?;
                Some((f.key.clone(), value.clone()))
            })
            .collect())
    }

    /// Sets one setting, or puts it back to its default with null.
    pub fn set_config(&self, id: &str, key: &str, value: &Value) -> Result<()> {
        let manifest = self
            .get(id)?
            .manifest
            .ok_or_else(|| invalid(format!("extension {id} cannot be read")))?;
        let field = manifest
            .field(key)
            .ok_or_else(|| invalid(format!("extension {id} has no setting {key}")))?;
        if !value.is_null() && !field.accepts(value) {
            return Err(invalid(format!("{value} does not fit setting {key}")));
        }
        let dir = self.data_dir(id)?;
        let path = dir.join("config.json");
        let mut chosen: BTreeMap<String, Value> = if path.exists() {
            read_json(&path)?
        } else {
            BTreeMap::new()
        };
        if value.is_null() {
            chosen.remove(key);
        } else {
            chosen.insert(key.to_owned(), value.clone());
        }
        fs::create_dir_all(&dir).at(&dir)?;
        write_atomic(
            &path,
            &serde_json::to_vec_pretty(&chosen).expect("a map serializes"),
        )
    }

    fn must_exist(&self, id: &str) -> Result<()> {
        if self.installed()?.contains_key(id) {
            Ok(())
        } else {
            Err(Error::NotFound(id.to_owned()))
        }
    }

    /// `data/<id>`, for an id that cannot climb out of it.
    fn data_dir(&self, id: &str) -> Result<PathBuf> {
        let plain = !id.is_empty()
            && id.len() <= 41
            && !id.starts_with('.')
            && !id.contains("..")
            && id
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'.');
        if !plain {
            return Err(invalid(format!("{id:?} is not an extension id")));
        }
        Ok(self.dir.join("data").join(id))
    }

    fn package_path(&self, id: &str, digest: &str) -> PathBuf {
        let short = &digest[..digest.len().min(16)];
        self.dir
            .join("packages")
            .join(format!("{id}-{short}.kulext"))
    }

    fn installed(&self) -> Result<BTreeMap<String, Installed>> {
        self.read(INSTALLED)
    }

    fn approved(&self) -> Result<BTreeMap<String, Approval>> {
        self.read(APPROVED)
    }

    fn read<T: DeserializeOwned + Default>(&self, name: &str) -> Result<T> {
        let path = self.dir.join(name);
        if !path.exists() {
            return Ok(T::default());
        }
        read_json(&path)
    }

    fn write<T: Serialize>(&self, name: &str, value: &T) -> Result<()> {
        fs::create_dir_all(&self.dir).at(&self.dir)?;
        let bytes = serde_json::to_vec_pretty(value).map_err(|e| invalid(e.to_string()))?;
        write_atomic(&self.dir.join(name), &bytes)
    }
}

/// Directories any sandboxed command may write.
const AGENT_WRITABLE: [&str; 4] = ["/tmp", "/var/tmp", "/dev/shm", "/private/tmp"];

/// The first of `roots` that `dir` is in, both compared as they really are
/// (`/tmp` is `/private/tmp` on macOS) and as written.
pub fn under_any(dir: &Path, roots: &[PathBuf]) -> Option<PathBuf> {
    let real = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
    roots.iter().find_map(|r| {
        let root = r.canonicalize().unwrap_or_else(|_| r.clone());
        (real.starts_with(&root) || dir.starts_with(r)).then_some(root)
    })
}

fn read_json<T: DeserializeOwned>(path: &Path) -> Result<T> {
    let bytes = fs::read(path).at(path)?;
    serde_json::from_slice(&bytes).map_err(|e| invalid(format!("{}: {e}", path.display())))
}

/// Writes through a rename, so a crash leaves the old file or the new one.
fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let tmp = path.with_file_name(format!(".kule-tmp-{}-{n}", std::process::id()));
    fs::write(&tmp, bytes).at(&tmp)?;
    fs::rename(&tmp, path).at(path)
}
