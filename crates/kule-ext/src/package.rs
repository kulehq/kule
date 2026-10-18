//! The `.kulext` package: a zip of an extension's folder.
//!
//! A package is never unpacked to disk. It is read whole once, every entry
//! is checked (name, kind, size after inflating), and files are later read
//! from it one at a time, each through a size cap. A folder under
//! development is read the same way, file by file, with the same checks.

use std::collections::BTreeMap;
use std::fs;
use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};

use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, DateTime, ZipArchive, ZipWriter};

use crate::error::{At, Result, invalid};
use crate::manifest::{MANIFEST, Manifest};

/// The largest package.
pub const MAX_PACKAGE: usize = 20 * 1024 * 1024;
/// The largest file inside it, inflated.
pub const MAX_ENTRY: u64 = 5 * 1024 * 1024;
/// All its files together, inflated.
const MAX_TOTAL: u64 = 40 * 1024 * 1024;
const MAX_ENTRIES: usize = 500;
const MAX_PATH: usize = 256;

/// A path inside an extension: segments of letters, digits, `-`, `_` and
/// `.`, none empty, none starting with `.` (so no `..` and no hidden files).
pub fn plain_path(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= MAX_PATH
        && path.split('/').all(|segment| {
            !segment.is_empty()
                && !segment.starts_with('.')
                && segment
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
        })
}

/// The media type a file is served as, or None for a kind an extension may
/// not carry. A name without an extension (LICENSE) is plain text.
pub fn media_type(path: &str) -> Option<&'static str> {
    let name = path.rsplit('/').next().unwrap_or(path);
    let Some((_, ext)) = name.rsplit_once('.') else {
        return Some("text/plain; charset=utf-8");
    };
    Some(match ext {
        "html" => "text/html; charset=utf-8",
        "js" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" => "application/json",
        "png" => "image/png",
        "svg" => "image/svg+xml",
        "woff2" => "font/woff2",
        "toml" | "md" | "txt" | "ts" => "text/plain; charset=utf-8",
        _ => return None,
    })
}

/// A checked `.kulext`.
#[derive(Debug, Clone)]
pub struct Package {
    bytes: Vec<u8>,
    /// Path to the entry's index in the zip.
    entries: BTreeMap<String, usize>,
    manifest: Manifest,
    digest: String,
}

impl Package {
    /// Checks a package's every entry and its manifest.
    pub fn open(bytes: Vec<u8>) -> Result<Package> {
        if bytes.len() > MAX_PACKAGE {
            return Err(invalid(format!(
                "the package is larger than {} MiB",
                MAX_PACKAGE / 1024 / 1024
            )));
        }
        if !bytes.starts_with(b"PK\x03\x04") {
            return Err(invalid("not a .kulext package (it is not a zip file)"));
        }
        let mut zip = ZipArchive::new(Cursor::new(bytes.as_slice()))
            .map_err(|e| invalid(format!("the package cannot be read: {e}")))?;
        // A name twice, or local headers that disagree with the central
        // directory, would let two readers see two different extensions.
        zip.validate_header_consistency().map_err(|_| {
            invalid(
                "the package's file list is inconsistent: a name twice, or headers that disagree",
            )
        })?;
        if zip.len() > MAX_ENTRIES {
            return Err(invalid(format!(
                "the package holds over {MAX_ENTRIES} files"
            )));
        }
        let mut entries = BTreeMap::new();
        let mut total = 0u64;
        for index in 0..zip.len() {
            let mut entry = zip
                .by_index(index)
                .map_err(|e| invalid(format!("the package cannot be read: {e}")))?;
            let name = entry
                .name()
                .map_err(|_| invalid("a file name in the package is not UTF-8"))?
                .into_owned();
            if entry.is_dir() {
                if !plain_path(name.trim_end_matches('/')) {
                    return Err(invalid(format!("folder {name:?} is not a plain path")));
                }
                continue;
            }
            if entry.is_symlink() {
                return Err(invalid(format!(
                    "{name:?} is a link; packages hold files only"
                )));
            }
            if entry.encrypted() {
                return Err(invalid(format!("{name:?} is encrypted")));
            }
            if !matches!(
                entry.compression(),
                CompressionMethod::Stored | CompressionMethod::Deflated
            ) {
                return Err(invalid(format!("{name:?} uses an unsupported compression")));
            }
            if !plain_path(&name) {
                return Err(invalid(format!("{name:?} is not a plain path")));
            }
            if media_type(&name).is_none() {
                return Err(invalid(format!(
                    "{name:?} is not a kind of file extensions carry"
                )));
            }
            // The header's size can lie; what counts is what inflates.
            let size = capped_len(&mut entry, &name)?;
            total += size;
            if total > MAX_TOTAL {
                return Err(invalid(format!(
                    "the package inflates to over {} MiB",
                    MAX_TOTAL / 1024 / 1024
                )));
            }
            if entries.insert(name.clone(), index).is_some() {
                return Err(invalid(format!("{name:?} is in the package twice")));
            }
        }
        drop(zip);
        let mut package = Package {
            digest: blake3::hash(&bytes).to_hex().to_string(),
            bytes,
            entries,
            manifest: placeholder(),
        };
        let manifest = Manifest::parse(
            &package
                .read(MANIFEST)
                .map_err(|_| invalid(format!("the package has no {MANIFEST} at its top")))?,
        )?;
        for file in manifest.files() {
            if !package.entries.contains_key(file) {
                return Err(invalid(format!(
                    "the manifest names {file}, which is not in the package"
                )));
            }
        }
        package.manifest = manifest;
        Ok(package)
    }

    /// Reads a package from a file.
    pub fn read_file(path: &Path) -> Result<Package> {
        let size = fs::metadata(path).at(path)?.len();
        if size > MAX_PACKAGE as u64 {
            return Err(invalid(format!(
                "the package is larger than {} MiB",
                MAX_PACKAGE / 1024 / 1024
            )));
        }
        Package::open(fs::read(path).at(path)?)
    }

    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    /// The blake3 of the whole package, which approvals pin.
    pub fn digest(&self) -> &str {
        &self.digest
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn paths(&self) -> impl Iterator<Item = &str> {
        self.entries.keys().map(String::as_str)
    }

    /// One file's bytes.
    pub fn read(&self, path: &str) -> Result<Vec<u8>> {
        let index = *self
            .entries
            .get(path)
            .ok_or_else(|| invalid(format!("{path} is not in the package")))?;
        let mut zip = ZipArchive::new(Cursor::new(self.bytes.as_slice()))
            .map_err(|e| invalid(format!("the package cannot be read: {e}")))?;
        let entry = zip
            .by_index(index)
            .map_err(|e| invalid(format!("the package cannot be read: {e}")))?;
        let mut out = Vec::new();
        entry
            .take(MAX_ENTRY + 1)
            .read_to_end(&mut out)
            .map_err(|e| invalid(format!("{path} cannot be read: {e}")))?;
        if out.len() as u64 > MAX_ENTRY {
            return Err(invalid(format!("{path} inflates to over 5 MiB")));
        }
        Ok(out)
    }
}

fn capped_len(entry: &mut impl Read, name: &str) -> Result<u64> {
    let mut sink = std::io::sink();
    let n = std::io::copy(&mut entry.take(MAX_ENTRY + 1), &mut sink)
        .map_err(|e| invalid(format!("{name:?} cannot be read: {e}")))?;
    if n > MAX_ENTRY {
        return Err(invalid(format!("{name:?} inflates to over 5 MiB")));
    }
    Ok(n)
}

fn placeholder() -> Manifest {
    Manifest {
        kule_extension: 1,
        id: String::new(),
        name: String::new(),
        version: String::new(),
        publisher: String::new(),
        description: None,
        main: None,
        permissions: Vec::new(),
        hosts: Vec::new(),
        contributes: Default::default(),
    }
}

/// Where an installed extension's files come from.
#[derive(Debug, Clone)]
pub enum Source {
    Packed(Package),
    /// A folder under development, read live.
    Folder {
        dir: PathBuf,
        manifest: Manifest,
        digest: String,
    },
}

impl Source {
    /// A folder under development. Its digest covers the manifest only, so
    /// code changes need no new approval but new permissions do.
    pub fn folder(dir: &Path) -> Result<Source> {
        let bytes = read_in(dir, MANIFEST)?;
        let manifest = Manifest::parse(&bytes)?;
        for file in manifest.files() {
            read_in(dir, file)?;
        }
        let digest = format!("dev-{}", blake3::hash(&bytes).to_hex());
        Ok(Source::Folder {
            dir: dir.to_path_buf(),
            manifest,
            digest,
        })
    }

    pub fn manifest(&self) -> &Manifest {
        match self {
            Source::Packed(p) => p.manifest(),
            Source::Folder { manifest, .. } => manifest,
        }
    }

    pub fn digest(&self) -> &str {
        match self {
            Source::Packed(p) => p.digest(),
            Source::Folder { digest, .. } => digest,
        }
    }

    pub fn read(&self, path: &str) -> Result<Vec<u8>> {
        match self {
            Source::Packed(p) => p.read(path),
            Source::Folder { dir, .. } => read_in(dir, path),
        }
    }
}

/// Reads `path` inside `dir` with the package's rules: a plain path of a
/// carried kind, no link anywhere on the way, at most [`MAX_ENTRY`].
fn read_in(dir: &Path, path: &str) -> Result<Vec<u8>> {
    if !plain_path(path) || media_type(path).is_none() {
        return Err(invalid(format!(
            "{path:?} is not a plain path to a carried kind of file"
        )));
    }
    let mut at = dir.to_path_buf();
    for segment in path.split('/') {
        at.push(segment);
        let meta = fs::symlink_metadata(&at).at(&at)?;
        if meta.file_type().is_symlink() {
            return Err(invalid(format!("{} is a link", at.display())));
        }
    }
    let file = fs::File::open(&at).at(&at)?;
    if !file.metadata().at(&at)?.is_file() {
        return Err(invalid(format!("{} is not a file", at.display())));
    }
    let mut out = Vec::new();
    file.take(MAX_ENTRY + 1).read_to_end(&mut out).at(&at)?;
    if out.len() as u64 > MAX_ENTRY {
        return Err(invalid(format!("{} is over 5 MiB", at.display())));
    }
    Ok(out)
}

/// Packs a folder into a `.kulext`. The same folder always gives the same
/// bytes: files in name order, a fixed time and mode. Hidden files and
/// folders (`.git`, `.DS_Store`) are left out; anything else that cannot be
/// carried is an error naming it.
pub fn pack(dir: &Path) -> Result<Vec<u8>> {
    let mut files = Vec::new();
    collect(dir, "", &mut files)?;
    if files.len() > MAX_ENTRIES {
        return Err(invalid(format!(
            "the folder holds over {MAX_ENTRIES} files"
        )));
    }
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .last_modified_time(DateTime::DEFAULT)
        .unix_permissions(0o644);
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    let mut total = 0u64;
    for name in &files {
        let bytes = read_in(dir, name)?;
        total += bytes.len() as u64;
        if total > MAX_TOTAL {
            return Err(invalid(format!(
                "the folder holds over {} MiB",
                MAX_TOTAL / 1024 / 1024
            )));
        }
        zip.start_file(name.as_str(), options)
            .map_err(|e| invalid(format!("packing {name}: {e}")))?;
        zip.write_all(&bytes)
            .map_err(|e| invalid(format!("packing {name}: {e}")))?;
    }
    let bytes = zip
        .finish()
        .map_err(|e| invalid(format!("packing: {e}")))?
        .into_inner();
    // What was packed must open; better to learn it now than at install.
    Package::open(bytes.clone())?;
    Ok(bytes)
}

fn collect(dir: &Path, prefix: &str, files: &mut Vec<String>) -> Result<()> {
    let mut names = Vec::new();
    for entry in fs::read_dir(dir).at(dir)? {
        let entry = entry.at(dir)?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|n| invalid(format!("{n:?} in {} is not UTF-8", dir.display())))?;
        if name.starts_with('.') {
            continue;
        }
        names.push((name, entry.path()));
    }
    names.sort();
    for (name, path) in names {
        let rel = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}/{name}")
        };
        let kind = fs::symlink_metadata(&path).at(&path)?.file_type();
        if kind.is_symlink() {
            return Err(invalid(format!(
                "{rel} is a link; packages hold files only"
            )));
        }
        if kind.is_dir() {
            collect(&path, &rel, files)?;
            continue;
        }
        if !plain_path(&rel) {
            return Err(invalid(format!(
                "{rel}: names may hold letters, digits, '-', '_' and '.' only"
            )));
        }
        if media_type(&rel).is_none() {
            return Err(invalid(format!(
                "{rel}: extensions carry html, js, css, json, png, svg, woff2, toml, md, txt and ts files"
            )));
        }
        files.push(rel);
    }
    Ok(())
}
