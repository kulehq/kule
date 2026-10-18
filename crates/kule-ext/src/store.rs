//! The person's extensions, under `<kule home>/extensions/`:
//!
//! - `installed.json`: id → version, digest, on or off, and the folder of
//!   one under development;
//! - `approved.json`: id → the digest, permissions and hosts the person
//!   approved;
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
use crate::manifest::{Manifest, Permission};
use crate::package::{Package, Source};

const INSTALLED: &str = "installed.json";
const APPROVED: &str = "approved.json";
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
}

impl Listed {
    /// Whether it may run now.
    pub fn runs(&self) -> bool {
        self.enabled && self.held.is_none()
    }
}

pub struct Store {
    dir: PathBuf,
}

impl Store {
    pub fn new(kule_home: &Path) -> Store {
        Store {
            dir: kule_home.join("extensions"),
        }
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
    pub fn install_folder(&self, dir: &Path) -> Result<Listed> {
        let dir = dir.canonicalize().at(dir)?;
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
        Ok(installed
            .iter()
            .map(|(id, i)| self.listed(id, i, approved.get(id)))
            .collect())
    }

    pub fn get(&self, id: &str) -> Result<Listed> {
        let installed = self.installed()?;
        let i = installed
            .get(id)
            .ok_or_else(|| Error::NotFound(id.to_owned()))?;
        Ok(self.listed(id, i, self.approved()?.get(id)))
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
        }
    }

    fn source(&self, id: &str, i: &Installed) -> Result<Source> {
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
        let mut approved = self.approved()?;
        approved.insert(
            id.to_owned(),
            Approval {
                digest: digest.to_owned(),
                permissions: source.manifest().permissions.clone(),
                hosts: source.manifest().hosts.clone(),
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
        if i.dev.is_none() {
            let _ = fs::remove_file(self.package_path(id, &i.digest));
        }
        let data = self.data_dir(id)?;
        if data.exists() {
            fs::remove_dir_all(&data).at(&data)?;
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
            Some(a) if a.digest == source.digest() => Ok(source),
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
