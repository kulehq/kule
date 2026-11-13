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
    // One release of a registry package: what npm installs outside the
    // sandbox is never a range, a tag, an alias, a URL, a repository or a
    // file.
    let (name, _) = crate::profile::npm_registry_spec(spec)?;
    Some((spec.clone(), name.to_owned(), args[at + 1..].to_vec()))
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

/// The folder `spec` is installed in: named after it where every
/// character is one a registry spec has, otherwise after its digest, so
/// the name never leads elsewhere.
fn dir_of(packages: &Path, spec: &str) -> PathBuf {
    let plain = spec
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b"-._~@+/".contains(&b))
        && !spec.starts_with('.');
    let name = if plain {
        spec.replace('/', "+")
    } else {
        format!("spec-{}", blake3::hash(spec.as_bytes()).to_hex())
    };
    packages.join("npm").join(name)
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
    installed_with(packages, spec, name, announce, &[])
}

/// [`installed`], with `npm_env` set for npm (a test's registry).
fn installed_with(
    packages: &Path,
    spec: &str,
    name: &str,
    announce: impl FnOnce(),
    npm_env: &[(&str, &str)],
) -> Result<PathBuf> {
    let dir = dir_of(packages, spec);
    if !dir.join(COMPLETE).is_file() {
        announce();
        // Not whole: an install from before this way of keeping them.
        if dir.exists() {
            std::fs::remove_dir_all(&dir).map_err(|e| io(&dir, e))?;
        }
        install(&dir, spec, npm_env)?;
    }
    bin(&dir.join("node_modules").join(name), name)
}

/// Installs `spec` into `dir`: in a directory of its own first, moved into
/// place once whole, so a run never starts a half-made one and two at once
/// do not mix.
fn install(dir: &Path, spec: &str, npm_env: &[(&str, &str)]) -> Result<()> {
    let parent = dir.parent().expect("a package's directory has a parent");
    std::fs::create_dir_all(parent).map_err(|e| io(parent, e))?;
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let staging = parent.join(format!(".part-{}-{nanos:x}", std::process::id()));
    let mut npm = Command::new("npm");
    npm.envs(npm_env.iter().copied());
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
        // Nothing of the package runs outside the sandbox, and nothing of
        // its tree comes from anywhere but the registry: a repository
        // (which npm would build), a URL, a file or a folder (npm 11.10 and
        // later; an older npm ignores these).
        .args([
            "--ignore-scripts",
            "--allow-git=none",
            "--allow-remote=none",
            "--allow-file=none",
            "--allow-directory=none",
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

    /// A registry of one package on this computer, serving `tarball` as
    /// `kule-fake-agent@1.0.0`, for as long as the test runs; its address.
    #[cfg(unix)]
    fn registry(tarball: &Path) -> String {
        use std::io::{BufRead, BufReader, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let bytes = std::fs::read(tarball).unwrap();
        let packument = serde_json::json!({
            "name": "kule-fake-agent",
            "dist-tags": { "latest": "1.0.0" },
            "versions": { "1.0.0": {
                "name": "kule-fake-agent",
                "version": "1.0.0",
                "bin": { "kule-fake-agent": "start.js" },
                "scripts": { "postinstall": "true" },
                "dist": { "tarball": format!("{url}/kule-fake-agent/-/kule-fake-agent-1.0.0.tgz") },
            }},
        })
        .to_string();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let mut reader = BufReader::new(&stream);
                let mut line = String::new();
                if reader.read_line(&mut line).is_err() {
                    continue;
                }
                // The rest of the request's head.
                let mut header = String::new();
                while reader.read_line(&mut header).is_ok_and(|n| n > 2) {
                    header.clear();
                }
                let path = line.split_whitespace().nth(1).unwrap_or("");
                let (status, body, kind) = match path {
                    "/kule-fake-agent" => {
                        ("200 OK", packument.as_bytes().to_vec(), "application/json")
                    }
                    p if p.ends_with(".tgz") => {
                        ("200 OK", bytes.clone(), "application/octet-stream")
                    }
                    _ => ("404 Not Found", b"{}".to_vec(), "application/json"),
                };
                let mut stream = &stream;
                let _ = write!(
                    stream,
                    "HTTP/1.1 {status}\r\ncontent-type: {kind}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(&body);
            }
        });
        format!("{url}/")
    }

    #[cfg(unix)]
    #[test]
    fn a_package_is_installed_once_and_its_scripts_never_run() {
        let dir = tempfile::TempDir::new().unwrap();
        let mark = dir.path().join("install-script-ran");
        let tarball = packed(dir.path(), &mark);
        let registry = registry(&tarball);
        // The registry alone: no settings of the person's, no cache of theirs.
        let cache = dir.path().join("npm-cache");
        let npm_env = [
            ("npm_config_registry", registry.as_str()),
            ("npm_config_cache", cache.to_str().unwrap()),
            ("npm_config_userconfig", "/dev/null"),
        ];
        let spec = "kule-fake-agent@1.0.0";
        let packages = dir.path().join("packages");
        let mut installs = 0;
        assert!(!is_installed(&packages, spec));

        let script = installed_with(
            &packages,
            spec,
            "kule-fake-agent",
            || installs += 1,
            &npm_env,
        )
        .unwrap();
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
        let again = installed_with(
            &packages,
            spec,
            "kule-fake-agent",
            || installs += 1,
            &npm_env,
        )
        .unwrap();
        assert_eq!((again, installs), (script.clone(), 1));

        // A directory left half made is made again.
        let whole = script.ancestors().nth(3).unwrap().join(COMPLETE);
        std::fs::remove_file(&whole).unwrap();
        installed_with(
            &packages,
            spec,
            "kule-fake-agent",
            || installs += 1,
            &npm_env,
        )
        .unwrap();
        assert_eq!(installs, 2);
        assert!(whole.is_file());

        // One that cannot be had says why.
        let err =
            installed_with(&packages, "missing@1.0.0", "missing", || {}, &npm_env).unwrap_err();
        assert!(err.to_string().contains("could not install"), "{err}");
        // npm takes nothing but the registry's packages, whatever the spec.
        let err = installed_with(
            &packages,
            tarball.to_str().unwrap(),
            "kule-fake-agent",
            || {},
            &npm_env,
        )
        .unwrap_err();
        assert!(err.to_string().contains("could not install"), "{err}");
    }

    /// Every lockfile Kule installs from lies in `src/locks`, which the
    /// release gate's `deps` step hands to osv-scanner file by file: no
    /// adapter's tree goes unscanned.
    #[test]
    fn every_shipped_lockfile_is_scanned() {
        let gate = include_str!("../../../scripts/release-gate.sh");
        assert!(
            gate.contains("for lock in crates/kule-acp/src/locks/*.json")
                && gate.contains("-L \"package-lock.json:$lock\""),
            "the gate no longer names the adapter lockfiles to osv-scanner"
        );
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/locks");
        let on_disk: std::collections::BTreeSet<String> = std::fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        for (spec, lock) in LOCKS {
            let name = format!("{}.json", spec.rsplit('/').next().unwrap());
            assert!(on_disk.contains(&name), "{spec}: no {name} in src/locks");
            let text = std::fs::read_to_string(dir.join(&name)).unwrap();
            assert_eq!(
                &text, lock,
                "{spec}: shipped from another file than src/locks/{name}"
            );
        }
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
        // Never installed outside the sandbox: npm reads each as a range or
        // a file, not one release.
        for spec in [
            "sometool@1.2.3-x || >=0.0.0",
            "sometool@1.2.3-0 - 9",
            "sometool@1.2.3-x.tgz",
        ] {
            assert_eq!(
                npx_package(&[String::from("npx"), spec.into()]),
                None,
                "{spec}"
            );
        }
    }

    /// A package's folder is named after its spec only where every
    /// character is one a registry spec has.
    #[test]
    fn a_packages_folder_never_leads_elsewhere() {
        let packages = Path::new("/p");
        assert_eq!(
            dir_of(packages, "@scope/tool@1.2.3"),
            Path::new("/p/npm/@scope+tool@1.2.3")
        );
        for spec in ["..\\x@1.2.3", "../x@1.2.3", "x@1.2.3 y", ".x@1.2.3"] {
            let dir = dir_of(packages, spec);
            assert_eq!(dir.parent(), Some(Path::new("/p/npm")), "{spec}");
            let name = dir.file_name().unwrap().to_string_lossy().into_owned();
            assert!(name.starts_with("spec-"), "{spec}: {name}");
        }
    }
}
