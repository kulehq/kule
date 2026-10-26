//! Agents that `npx` fetches, installed once.
//!
//! A task's commands get npm caches of the task's own (see the engine's
//! `caches`), so `npx` in a new task fetches the agent's package again: on
//! Linux, where a cache cannot be cloned in one go, that is hundreds of
//! megabytes before the agent says a word. So Kule installs the exact
//! version a profile names once, outside the sandbox and without running
//! the package's install scripts, into a directory of its own that no
//! sandbox can write to, and starts the agent from there with `node`.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use kule_engine::{Error, Result};

/// Where Kule keeps the agents' packages, relative to the home directory:
/// beside the agents' own homes, outside Kule's home, which the sandbox
/// hides whole.
#[cfg(target_os = "macos")]
pub(crate) const PACKAGES: &str = "Library/Application Support/Kule/packages";
#[cfg(not(target_os = "macos"))]
pub(crate) const PACKAGES: &str = ".local/share/kule/packages";

/// Left in an installed package's directory once it is whole.
const COMPLETE: &str = ".kule-complete";

/// The package an `npx` command runs, and the arguments that follow it:
/// `npx -y @scope/name@1.2.3 acp` → (`@scope/name@1.2.3`, `@scope/name`,
/// `["acp"]`). `None` for any other command, or one with flags of npx's
/// own that Kule does not read the same way.
pub fn npx_package(command: &[String]) -> Option<(String, String, Vec<String>)> {
    let (program, args) = command.split_first()?;
    if Path::new(program).file_name()? != "npx" {
        return None;
    }
    let at = args.iter().position(|a| !a.starts_with('-'))?;
    if !args[..at].iter().all(|f| f == "-y" || f == "--yes") {
        return None;
    }
    let spec = &args[at];
    let (name, version) = spec.rsplit_once('@').filter(|(n, _)| !n.is_empty())?;
    // One release of a registry package: what npm installs outside the
    // sandbox is never a range, a tag, an alias, a URL or a repository.
    if !package_name(name) || !crate::profile::exact_version(version) {
        return None;
    }
    Some((spec.clone(), name.to_owned(), args[at + 1..].to_vec()))
}

/// An npm package's name as the registry has them: `name` or
/// `@scope/name`, lower case.
fn package_name(name: &str) -> bool {
    let part = |p: &str| {
        !p.is_empty()
            && !p.starts_with('.')
            && !p.starts_with('_')
            && p.bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"-._~".contains(&b))
    };
    match name.strip_prefix('@') {
        Some(scoped) => scoped
            .split_once('/')
            .is_some_and(|(scope, rest)| part(scope) && part(rest)),
        None => part(name),
    }
}

/// Lockfiles for the packages Kule's own agents run, made with
/// `npm install --package-lock-only` when the version was pinned: `npm ci`
/// installs each dependency at the version and integrity hash in them.
/// A new pin in agents.toml needs its lockfile here (a test says so).
const LOCKS: &[(&str, &str)] = &[
    (
        "@agentclientprotocol/claude-agent-acp@0.92.0",
        include_str!("locks/claude-agent-acp@0.92.0.json"),
    ),
    (
        "@agentclientprotocol/codex-acp@2.2.2",
        include_str!("locks/codex-acp@2.2.2.json"),
    ),
];

/// Whether Kule ships a lockfile for `spec`: it installs as locked or not
/// at all.
pub(crate) fn locked(spec: &str) -> bool {
    shipped_lock(spec).is_some()
}

fn shipped_lock(spec: &str) -> Option<&'static str> {
    LOCKS
        .iter()
        .find(|(s, _)| *s == spec)
        .map(|(_, lock)| *lock)
}

/// Whether `spec` is installed whole under `packages`.
pub fn is_installed(packages: &Path, spec: &str) -> bool {
    dir_of(packages, spec).join(COMPLETE).is_file()
}

fn dir_of(packages: &Path, spec: &str) -> PathBuf {
    packages.join("npm").join(spec.replace('/', "+"))
}

/// The script that starts package `name` as installed under `packages`
/// from `spec`, installing it first when it is not there yet; `announce`
/// is told before an install, which takes a while.
pub fn installed(
    packages: &Path,
    spec: &str,
    name: &str,
    announce: impl FnOnce(),
) -> Result<PathBuf> {
    let dir = dir_of(packages, spec);
    if !dir.join(COMPLETE).is_file() {
        announce();
        // Not whole: an install from before this way of keeping them.
        if dir.exists() {
            std::fs::remove_dir_all(&dir).map_err(|e| io(&dir, e))?;
        }
        install(&dir, spec)?;
    }
    bin(&dir.join("node_modules").join(name), name)
}

/// Installs `spec` into `dir`: in a directory of its own first, moved into
/// place once whole, so a run never starts a half-made one and two at once
/// do not mix.
fn install(dir: &Path, spec: &str) -> Result<()> {
    let parent = dir.parent().expect("a package's directory has a parent");
    std::fs::create_dir_all(parent).map_err(|e| io(parent, e))?;
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let staging = parent.join(format!(".part-{}-{nanos:x}", std::process::id()));
    let mut npm = Command::new("npm");
    match shipped_lock(spec) {
        // Every dependency at the version and the integrity hash recorded
        // when the release was made, not what the registry says today; in
        // the staging folder Kule just made, which holds no .npmrc.
        Some(lock) => {
            let (name, version) = spec
                .rsplit_once('@')
                .expect("a shipped spec names its version");
            std::fs::create_dir_all(&staging).map_err(|e| io(&staging, e))?;
            let manifest = serde_json::json!({
                "name": "kule-agent", "private": true, "dependencies": { name: version }
            });
            std::fs::write(staging.join("package.json"), manifest.to_string())
                .map_err(|e| io(&staging, e))?;
            std::fs::write(staging.join("package-lock.json"), lock).map_err(|e| io(&staging, e))?;
            npm.arg("ci").current_dir(&staging);
        }
        // Not from wherever the daemon was started, which could hold an
        // .npmrc of a repository's.
        None => {
            npm.arg("install")
                .arg("--prefix")
                .arg(&staging)
                .arg("--no-save")
                .arg(spec)
                .current_dir(parent);
        }
    }
    let out = npm
        // Nothing of the package runs outside the sandbox.
        .args([
            "--ignore-scripts",
            "--no-audit",
            "--no-fund",
            "--omit=dev",
            "--loglevel=error",
        ])
        .stdin(Stdio::null())
        .output()
        .map_err(|e| Error::Invalid(format!("npm to install {spec}: {e}")))?;
    if !out.status.success() {
        let _ = std::fs::remove_dir_all(&staging);
        let said = String::from_utf8_lossy(&out.stderr);
        let last: Vec<&str> = said.lines().rev().take(3).collect();
        return Err(Error::Invalid(format!(
            "npm could not install {spec}: {}",
            last.into_iter().rev().collect::<Vec<_>>().join(" / ")
        )));
    }
    std::fs::write(staging.join(COMPLETE), spec).map_err(|e| io(&staging, e))?;
    match std::fs::rename(&staging, dir) {
        Ok(()) => Ok(()),
        // Another run put it there first.
        Err(_) if dir.join(COMPLETE).is_file() => {
            let _ = std::fs::remove_dir_all(&staging);
            Ok(())
        }
        Err(e) => {
            let _ = std::fs::remove_dir_all(&staging);
            Err(io(dir, e))
        }
    }
}

/// The script package `name` in `package` starts, as `npx` picks it: its
/// only `bin`, or the one named after the package.
fn bin(package: &Path, name: &str) -> Result<PathBuf> {
    let manifest = package.join("package.json");
    let text = std::fs::read_to_string(&manifest).map_err(|e| io(&manifest, e))?;
    let json: serde_json::Value = serde_json::from_str(&text)
        .map_err(|e| Error::Invalid(format!("{}: {e}", manifest.display())))?;
    let unscoped = name.rsplit('/').next().unwrap_or(name);
    let script = match &json["bin"] {
        serde_json::Value::String(path) => Some(path.as_str()),
        serde_json::Value::Object(bins) if bins.len() == 1 => {
            bins.values().next().and_then(|v| v.as_str())
        }
        serde_json::Value::Object(bins) => bins.get(unscoped).and_then(|v| v.as_str()),
        _ => None,
    }
    .ok_or_else(|| Error::Invalid(format!("{name} names no command to start")))?;
    let path = package.join(script);
    // Inside the package, as npm would refuse otherwise too.
    let inside = Path::new(script).components().all(|c| {
        matches!(
            c,
            std::path::Component::Normal(_) | std::path::Component::CurDir
        )
    });
    if !inside || !path.is_file() {
        return Err(Error::Invalid(format!(
            "{name}: its command {script:?} is not a file in the package"
        )));
    }
    Ok(path)
}

fn io(path: &Path, source: std::io::Error) -> Error {
    Error::Io {
        path: path.to_owned(),
        source,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(text: &str) -> Vec<String> {
        text.split_whitespace().map(str::to_owned).collect()
    }

    #[cfg(unix)]
    /// A package of its own, packed as the registry would serve it, with an
    /// install script that leaves a mark if it ever runs.
    fn packed(dir: &Path, mark: &Path) -> PathBuf {
        let src = dir.join("src");
        std::fs::create_dir_all(&src).unwrap();
        let manifest = serde_json::json!({
            "name": "kule-fake-agent",
            "version": "1.0.0",
            "bin": { "kule-fake-agent": "start.js" },
            "scripts": { "postinstall": format!("touch {}", mark.display()) },
        });
        std::fs::write(src.join("package.json"), manifest.to_string()).unwrap();
        std::fs::write(
            src.join("start.js"),
            "console.log('started', process.argv[2]);\n",
        )
        .unwrap();
        let out = Command::new("npm")
            .args(["pack", "--silent", "--pack-destination"])
            .arg(dir)
            .current_dir(&src)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        dir.join("kule-fake-agent-1.0.0.tgz")
    }

    #[cfg(unix)]
    #[test]
    fn a_package_is_installed_once_and_its_scripts_never_run() {
        let dir = tempfile::TempDir::new().unwrap();
        let mark = dir.path().join("install-script-ran");
        let tarball = packed(dir.path(), &mark);
        let spec = tarball.to_str().unwrap();
        let packages = dir.path().join("packages");
        let mut installs = 0;
        assert!(!is_installed(&packages, spec));

        let script = installed(&packages, spec, "kule-fake-agent", || installs += 1).unwrap();
        assert_eq!(installs, 1);
        assert!(!mark.exists(), "the package's install script ran");
        assert!(is_installed(&packages, spec));
        let out = Command::new("node")
            .arg(&script)
            .arg("acp")
            .output()
            .unwrap();
        assert_eq!(String::from_utf8_lossy(&out.stdout), "started acp\n");

        // From then on it is there.
        let again = installed(&packages, spec, "kule-fake-agent", || installs += 1).unwrap();
        assert_eq!((again, installs), (script.clone(), 1));

        // A directory left half made is made again.
        let whole = script.ancestors().nth(3).unwrap().join(COMPLETE);
        std::fs::remove_file(&whole).unwrap();
        installed(&packages, spec, "kule-fake-agent", || installs += 1).unwrap();
        assert_eq!(installs, 2);
        assert!(whole.is_file());

        // One that cannot be had says why.
        let err = installed(&packages, "/nowhere/missing-1.0.0.tgz", "missing", || {}).unwrap_err();
        assert!(err.to_string().contains("could not install"), "{err}");
    }

    /// Every package Kule's own agents run comes with a lockfile for its
    /// pinned version, and every dependency in it with its registry and
    /// integrity hash.
    #[test]
    fn kules_agents_install_from_their_lockfiles() {
        let home = tempfile::TempDir::new().unwrap();
        let agents = crate::Agents::load(home.path()).unwrap();
        for listed in agents.listed() {
            let Some((spec, name, _)) = npx_package(&listed.profile.command) else {
                continue;
            };
            let lock = shipped_lock(&spec).unwrap_or_else(|| panic!("no lockfile for {spec}"));
            let lock: serde_json::Value = serde_json::from_str(lock).unwrap();
            let version = spec.rsplit_once('@').unwrap().1;
            assert_eq!(
                lock["packages"][""]["dependencies"][&name], version,
                "{spec}"
            );
            for (path, package) in lock["packages"].as_object().unwrap() {
                if path.is_empty() {
                    continue;
                }
                // A link installs a folder from disk, not the registry.
                assert!(package.get("link").is_none(), "{spec}: {path} is a link");
                let resolved = package["resolved"].as_str().unwrap_or_default();
                assert!(
                    resolved.starts_with("https://registry.npmjs.org/"),
                    "{spec}: {path} {resolved}"
                );
                assert!(
                    package["integrity"]
                        .as_str()
                        .is_some_and(|i| i.starts_with("sha512-")),
                    "{spec}: {path}"
                );
            }
        }
    }

    #[test]
    fn an_npx_command_is_read_as_its_package_and_arguments() {
        assert_eq!(
            npx_package(&words(
                "npx -y @agentclientprotocol/codex-acp@2.2.2 -c mode=x"
            )),
            Some((
                "@agentclientprotocol/codex-acp@2.2.2".into(),
                "@agentclientprotocol/codex-acp".into(),
                words("-c mode=x")
            ))
        );
        assert_eq!(
            npx_package(&words("/usr/local/bin/npx opencode-ai@1.2.3 acp")),
            Some((
                "opencode-ai@1.2.3".into(),
                "opencode-ai".into(),
                words("acp")
            ))
        );
        // npx's own flags Kule does not read the same way, and other
        // launchers, stay with npx.
        assert_eq!(npx_package(&words("npx --package=other tool@1.0.0")), None);
        assert_eq!(
            npx_package(&words("npx --yes tool@1.0.0")).unwrap().0,
            "tool@1.0.0"
        );
        assert_eq!(npx_package(&words("bunx tool@1.0.0")), None);
        assert_eq!(npx_package(&words("gemini --acp")), None);
        assert_eq!(npx_package(&words("npx -y @scope/tool")), None);
        // Installed outside the sandbox only as one release of a registry
        // package: never a range, a tag, an alias, a URL or a repository.
        for spec in [
            "tool@latest",
            "tool@^1.0.0",
            "tool@1",
            "tool@npm:other@1.0.0",
            "git+ssh://git@github.com/x/y.git",
            "https://example.com/x@1.0.0",
            "Tool@1.0.0",
        ] {
            assert_eq!(
                npx_package(&words(&format!("npx -y {spec}"))),
                None,
                "{spec}"
            );
        }
        assert!(npx_package(&words("npx -y tool@1.0.0-beta.2")).is_some());
    }
}
