//! Projects beside a workspace that its tasks may read: a folder the
//! workspace builds from (`path = "../shared"`, `file:../lib`, a project
//! reference), hidden with the rest of the person's own folders (see
//! [`Sandbox::hide_personal`](crate::Sandbox::hide_personal)) until the
//! person opens it here.
//!
//! Only the person opens one: the list lives in Kule's home, which no
//! sandbox can see, and what Kule suggests comes from the workspace's own
//! manifests, never from the copy an agent writes. An opened folder is
//! read, never written; what Kule hides inside it stays hidden; and it is
//! checked again before every run: a folder that has become a link, or is
//! no longer one of the person's own, is not opened.

use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::error::{Error, IoContext, Result};
use crate::fsops::{read_json, write_json};
use crate::sandbox::personal_folders;

pub const BESIDE_FILE: &str = "beside.json";

/// One folder the tasks of a workspace may read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Beside {
    pub id: u64,
    /// The workspace, resolved.
    pub workspace: PathBuf,
    /// The folder, resolved.
    pub path: PathBuf,
    pub added_unix: u64,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct File {
    #[serde(default)]
    next: u64,
    #[serde(default)]
    beside: Vec<Beside>,
}

fn read(home: &Path) -> Result<File> {
    let path = home.join(BESIDE_FILE);
    if !path.is_file() {
        return Ok(File::default());
    }
    read_json(&path)
}

/// Every folder opened in `home`, for every workspace.
pub fn besides(home: &Path) -> Result<Vec<Beside>> {
    Ok(read(home)?.beside)
}

/// The folders opened for `workspace` (resolved).
pub(crate) fn besides_of(home: &Path, workspace: &Path) -> Result<Vec<PathBuf>> {
    Ok(read(home)?
        .beside
        .into_iter()
        .filter(|b| b.workspace == workspace)
        .map(|b| b.path)
        .collect())
}

/// Lets the tasks of `workspace` read `path`, a folder in one Kule hides
/// as the person's own under `person` (their home directory); the one
/// already kept when it is there. `path` is taken as it is written: a link
/// is refused, not followed.
pub fn add_beside(home: &Path, workspace: &Path, path: &Path, person: &Path) -> Result<Beside> {
    let workspace = std::fs::canonicalize(workspace).at(workspace)?;
    let path = check(&workspace, path, person)?;
    let mut file = read(home)?;
    if let Some(kept) = file
        .beside
        .iter()
        .find(|b| b.workspace == workspace && b.path == path)
    {
        return Ok(kept.clone());
    }
    file.next += 1;
    let kept = Beside {
        id: file.next,
        workspace,
        path,
        added_unix: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs()),
    };
    file.beside.push(kept.clone());
    write_json(&home.join(BESIDE_FILE), &file)?;
    Ok(kept)
}

/// Closes folder `id` again; whether there was one.
pub fn remove_beside(home: &Path, id: u64) -> Result<bool> {
    let mut file = read(home)?;
    let before = file.beside.len();
    file.beside.retain(|b| b.id != id);
    let gone = file.beside.len() != before;
    if gone {
        write_json(&home.join(BESIDE_FILE), &file)?;
    }
    Ok(gone)
}

/// Why folder `path` cannot be opened for `workspace` now, if it cannot:
/// what a list of them shows next to each.
pub fn beside_unopened(workspace: &Path, path: &Path, person: &Path) -> Option<String> {
    check(workspace, path, person).err().map(|e| e.to_string())
}

/// `path` resolved, when it is a folder the tasks of `workspace` may be
/// let read: a directory as it is written (no link), inside a folder
/// hidden as the person's own, and neither the workspace nor holding it.
fn check(workspace: &Path, path: &Path, person: &Path) -> Result<PathBuf> {
    let refuse = |why: &str| Err(Error::Invalid(format!("{}: {why}", path.display())));
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return refuse("not found");
    };
    if meta.file_type().is_symlink() {
        return refuse("a link; name the folder it leads to");
    }
    if !meta.is_dir() {
        return refuse("not a folder");
    }
    let real = std::fs::canonicalize(path).at(path)?;
    if real.starts_with(workspace) || workspace.starts_with(&real) {
        return refuse("the workspace itself, or a folder holding it");
    }
    // Credentials are never in such a folder; one hidden inside it stays
    // hidden when it opens (see `Sandbox::open_beside`).
    let person = std::fs::canonicalize(person).at(person)?;
    let inside = personal_folders(workspace, &person)?
        .iter()
        .any(|folder| real.starts_with(folder) && real != *folder);
    if !inside {
        return refuse(
            "not in a folder Kule hides as yours (the workspace's own folder, Documents, …); the tasks read it already, or Kule keeps it hidden for good",
        );
    }
    Ok(real)
}

/// Puts a link next to the task's copy to each folder of `opened` that
/// lies next to `workspace`, so a path out of the copy (`../shared`)
/// reaches it as it would from the workspace, and takes away the links
/// there that no longer lead to one of them. Nothing but links is touched
/// there. The links made, for the sandbox to let commands look at; none
/// for a task whose copy has nothing beside it (made before Kule put its
/// copies under `w/`).
pub(crate) fn place_links(
    task: &crate::Task,
    workspace: &Path,
    opened: &[PathBuf],
) -> Result<Vec<PathBuf>> {
    let Some(name) = task.info().copy_name.as_deref() else {
        return Ok(Vec::new());
    };
    let copy = task.copy();
    let Some(beside) = copy.parent() else {
        return Ok(Vec::new());
    };
    let wanted: Vec<(PathBuf, &PathBuf)> = opened
        .iter()
        .filter(|path| path.parent() == workspace.parent())
        .filter_map(|path| Some((beside.join(path.file_name()?), path)))
        .filter(|(link, _)| link.file_name() != Some(name.as_ref()))
        .collect();
    for entry in std::fs::read_dir(beside).at(beside)?.flatten() {
        let link = entry.path();
        let is_link = entry.file_type().is_ok_and(|t| t.is_symlink());
        let keep = wanted.iter().any(|(l, target)| {
            *l == link && std::fs::read_link(&link).is_ok_and(|t| t == **target)
        });
        if is_link && !keep {
            std::fs::remove_file(&link).at(&link)?;
        }
    }
    let mut made = Vec::new();
    for (link, target) in wanted {
        if std::fs::symlink_metadata(&link).is_err() {
            match std::os::unix::fs::symlink(target, &link) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(e) => return Err(e).at(&link),
            }
        }
        if std::fs::read_link(&link).is_ok_and(|t| t == *target) {
            made.push(link);
        }
    }
    Ok(made)
}

/// Manifests whose relative paths name what a project builds from.
fn is_manifest(name: &str) -> bool {
    matches!(
        name,
        "Cargo.toml"
            | "package.json"
            | "go.mod"
            | "go.work"
            | "pyproject.toml"
            | "settings.gradle"
            | "settings.gradle.kts"
            | "Directory.Build.props"
            | "Directory.Packages.props"
    ) || [".csproj", ".fsproj", ".vbproj", ".sln", ".slnx"]
        .iter()
        .any(|ext| name.ends_with(ext))
}

/// Directories no manifest of the project's own is in.
const SKIPPED: &[&str] = &[
    "node_modules",
    "target",
    "bin",
    "obj",
    "dist",
    "build",
    "vendor",
    "venv",
];

/// How deep, how many entries and how large a manifest Kule looks at.
const DEPTH: usize = 4;
const ENTRIES: usize = 20_000;
const LARGEST: u64 = 1024 * 1024;

/// Folders beside `workspace` that its manifests name and its tasks cannot
/// read now, each the top folder under the one Kule hides it in (a
/// project's root, whose manifest a build reads too). Read from the
/// workspace, the person's, never from a copy; the ones already opened are
/// left out.
pub fn suggest_beside(home: &Path, workspace: &Path, person: &Path) -> Result<Vec<PathBuf>> {
    let workspace = std::fs::canonicalize(workspace).at(workspace)?;
    let person = std::fs::canonicalize(person).at(person)?;
    let folders = personal_folders(&workspace, &person)?;
    let opened = besides_of(home, &workspace)?;
    let mut manifests = Vec::new();
    let mut seen = 0;
    collect(&workspace, 0, &mut seen, &mut manifests);
    let mut found: Vec<PathBuf> = Vec::new();
    for manifest in manifests {
        let Some(dir) = manifest.parent() else {
            continue;
        };
        let Ok(text) = std::fs::read_to_string(&manifest) else {
            continue;
        };
        for relative in relative_paths(&text) {
            let Some(target) = normalize(&dir.join(relative)) else {
                continue;
            };
            if target.starts_with(&workspace) {
                continue;
            }
            for folder in &folders {
                let Ok(within) = target.strip_prefix(folder) else {
                    continue;
                };
                let Some(first) = within.components().next() else {
                    continue;
                };
                let top = folder.join(first);
                if !found.contains(&top)
                    && !opened
                        .iter()
                        .any(|o| top.starts_with(o) || o.starts_with(&top))
                    && check(&workspace, &top, &person).is_ok()
                {
                    found.push(top);
                }
            }
        }
    }
    found.sort();
    Ok(found)
}

/// The manifests under `dir`, depth-first, within [`DEPTH`] and [`ENTRIES`];
/// links are not followed.
fn collect(dir: &Path, depth: usize, seen: &mut usize, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        *seen += 1;
        if *seen > ENTRIES {
            return;
        }
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        let name = entry.file_name().to_string_lossy().into_owned();
        if kind.is_dir() {
            if depth < DEPTH && !name.starts_with('.') && !SKIPPED.contains(&name.as_str()) {
                collect(&entry.path(), depth + 1, seen, out);
            }
        } else if kind.is_file()
            && is_manifest(&name)
            && entry.metadata().is_ok_and(|m| m.len() <= LARGEST)
        {
            out.push(entry.path());
        }
    }
}

/// Every quoted string in `text` that is a path out of its folder
/// (`../x`, `..\x`), after a `file:`, `link:` or `portal:` scheme, and
/// every `=> ../x` (go.mod's `replace`) or bare `use ../x` (go.work).
fn relative_paths(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut push = |s: &str| {
        let s = s
            .trim()
            .trim_start_matches("file:")
            .trim_start_matches("link:")
            .trim_start_matches("portal:");
        if s.starts_with("../") || s.starts_with("..\\") || s == ".." {
            out.push(s.replace('\\', "/"));
        }
    };
    for quote in ['"', '\''] {
        let mut parts = text.split(quote);
        parts.next();
        while let Some(inside) = parts.next() {
            push(inside);
            parts.next();
        }
    }
    for line in text.lines() {
        let line = line.trim();
        if let Some((_, to)) = line.split_once("=>") {
            push(to.split_whitespace().next().unwrap_or(""));
        }
        if let Some(rest) = line.strip_prefix("use ") {
            push(rest);
        }
        if line.starts_with("../") {
            push(line.split_whitespace().next().unwrap_or(""));
        }
    }
    out
}

/// `path` with `.` and `..` worked out by name, links not followed; `None`
/// when it climbs above the root.
fn normalize(path: &Path) -> Option<PathBuf> {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            Component::ParentDir => {
                if !out.pop() {
                    return None;
                }
            }
            Component::CurDir => {}
            other => out.push(other),
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_out_of_a_folder_are_found_in_every_manifest_kind() {
        let cargo = "[dependencies]\nshared = { path = \"../shared/core\" }\nlocal = { path = \"crates/x\" }\n";
        assert_eq!(relative_paths(cargo), ["../shared/core"]);
        let npm =
            r#"{"dependencies": {"ui": "file:../ui", "left-pad": "^1.0.0", "x": "link:../x"}}"#;
        assert_eq!(relative_paths(npm), ["../ui", "../x"]);
        let gomod = "module a\nreplace example.com/lib => ../lib\n";
        assert_eq!(relative_paths(gomod), ["../lib"]);
        let gowork = "go 1.22\nuse (\n\t.\n\t../tools\n)\nuse ../other\n";
        assert_eq!(relative_paths(gowork), ["../tools", "../other"]);
        let csproj = r#"<ProjectReference Include="..\Shared\Shared.csproj" />"#;
        assert_eq!(relative_paths(csproj), ["../Shared/Shared.csproj"]);
    }

    #[test]
    fn a_path_is_worked_out_by_name() {
        assert_eq!(
            normalize(Path::new("/a/b/../c/./d")),
            Some(PathBuf::from("/a/c/d"))
        );
        assert_eq!(normalize(Path::new("/../x")), None);
    }
}
