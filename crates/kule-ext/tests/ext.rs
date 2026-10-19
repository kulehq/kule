use std::fs;
use std::io::{Cursor, Write};
use std::path::Path;

use kule_ext::{Error, Package, Permission, Store, pack};
use serde_json::json;
use tempfile::TempDir;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

fn manifest(extra: serde_json::Value) -> String {
    let mut m = json!({
        "kule_extension": 1,
        "id": "acme.notes",
        "name": "Notes",
        "version": "1.0.0",
        "publisher": "acme",
        "main": "main.js",
        "permissions": ["tasks.read"],
        "contributes": {
            "commands": [{ "id": "scan", "title": "Scan" }],
            "panels": [{ "id": "notes", "title": "Notes", "entry": "panel.html" }],
            "configuration": [
                { "key": "level", "title": "Level", "type": "enum", "values": ["info", "warn"], "default": "warn" },
                { "key": "count", "title": "Count", "type": "number" }
            ]
        }
    });
    for (k, v) in extra.as_object().unwrap() {
        m[k] = v.clone();
    }
    serde_json::to_string_pretty(&m).unwrap()
}

fn folder(manifest: &str) -> TempDir {
    // Not under /tmp, where a folder under development is refused.
    let dir = TempDir::new_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    fs::write(dir.path().join("kule-extension.json"), manifest).unwrap();
    fs::write(
        dir.path().join("main.js"),
        "kule.commands.register('scan', () => {});",
    )
    .unwrap();
    fs::write(dir.path().join("panel.html"), "<!doctype html><p>notes</p>").unwrap();
    dir
}

/// A zip made by hand, for packages `pack` would never make.
fn zip_of(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    for (name, bytes) in files {
        zip.start_file(*name, options).unwrap();
        zip.write_all(bytes).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

fn good_files(manifest: &str) -> Vec<(&'static str, Vec<u8>)> {
    vec![
        ("kule-extension.json", manifest.as_bytes().to_vec()),
        ("main.js", b"1".to_vec()),
        ("panel.html", b"<p>".to_vec()),
    ]
}

fn zip_with(extra: &[(&str, &[u8])]) -> Vec<u8> {
    let m = manifest(json!({}));
    let base = good_files(&m);
    let mut all: Vec<(&str, &[u8])> = base.iter().map(|(n, b)| (*n, b.as_slice())).collect();
    all.extend_from_slice(extra);
    zip_of(&all)
}

fn refused(bytes: Vec<u8>, says: &str) {
    match Package::open(bytes) {
        Err(Error::Invalid(text)) => assert!(text.contains(says), "{text:?} should say {says:?}"),
        other => panic!("expected a refusal saying {says:?}, got {other:?}"),
    }
}

#[test]
fn the_same_folder_packs_to_the_same_bytes() {
    let dir = folder(&manifest(json!({})));
    let first = pack(dir.path()).unwrap();
    // A later touch changes times, not the package.
    std::thread::sleep(std::time::Duration::from_millis(1100));
    fs::write(
        dir.path().join("main.js"),
        "kule.commands.register('scan', () => {});",
    )
    .unwrap();
    assert_eq!(first, pack(dir.path()).unwrap());
    let package = Package::open(first).unwrap();
    assert_eq!(package.manifest().id, "acme.notes");
    assert_eq!(
        package.paths().collect::<Vec<_>>(),
        ["kule-extension.json", "main.js", "panel.html"]
    );
}

#[test]
fn packing_leaves_hidden_files_out_and_refuses_links_and_odd_kinds() {
    let dir = folder(&manifest(json!({})));
    fs::create_dir(dir.path().join(".git")).unwrap();
    fs::write(dir.path().join(".git/config"), "x").unwrap();
    fs::write(dir.path().join(".DS_Store"), "x").unwrap();
    let package = Package::open(pack(dir.path()).unwrap()).unwrap();
    assert_eq!(package.paths().count(), 3);

    std::os::unix::fs::symlink("/etc/hosts", dir.path().join("hosts.txt")).unwrap();
    let e = pack(dir.path()).unwrap_err().to_string();
    assert!(e.contains("is a link"), "{e}");
    fs::remove_file(dir.path().join("hosts.txt")).unwrap();

    fs::write(dir.path().join("tool.exe"), "MZ").unwrap();
    let e = pack(dir.path()).unwrap_err().to_string();
    assert!(e.contains("tool.exe"), "{e}");
}

#[test]
fn a_package_that_inflates_past_the_cap_is_refused() {
    // 6 MiB of zeros deflates to a few KiB.
    let zeros = vec![0u8; 6 * 1024 * 1024];
    refused(zip_with(&[("big.txt", &zeros)]), "inflates to over 5 MiB");
    // Nine files under the per-file cap, over the total.
    let nearly = vec![0u8; 5 * 1024 * 1024 - 1];
    let names: Vec<String> = (0..9).map(|i| format!("f{i}.txt")).collect();
    let files: Vec<(&str, &[u8])> = names
        .iter()
        .map(|n| (n.as_str(), nearly.as_slice()))
        .collect();
    refused(zip_with(&files), "inflates to over 40 MiB");
}

#[test]
fn entries_that_climb_hide_or_link_are_refused() {
    refused(zip_with(&[("../evil.js", b"x")]), "not a plain path");
    refused(zip_with(&[("/etc/evil.js", b"x")]), "not a plain path");
    refused(zip_with(&[("a\\evil.js", b"x")]), "not a plain path");
    refused(zip_with(&[(".hidden.js", b"x")]), "not a plain path");
    refused(zip_with(&[("tool.exe", b"MZ")]), "not a kind of file");

    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    let m = manifest(json!({}));
    for (name, bytes) in good_files(&m) {
        zip.start_file(name, SimpleFileOptions::default()).unwrap();
        zip.write_all(&bytes).unwrap();
    }
    zip.add_symlink("link.txt", "/etc/passwd", SimpleFileOptions::default())
        .unwrap();
    refused(zip.finish().unwrap().into_inner(), "is a link");
}

#[test]
fn a_name_twice_in_a_package_is_refused() {
    // The zip writer refuses duplicates, so rename one entry in the bytes.
    let mut bytes = zip_with(&[("aaaa.txt", b"one"), ("bbbb.txt", b"two")]);
    let (from, to) = (b"bbbb.txt", b"aaaa.txt");
    let mut i = 0;
    while i + from.len() <= bytes.len() {
        if &bytes[i..i + from.len()] == from {
            bytes[i..i + from.len()].copy_from_slice(to);
        }
        i += 1;
    }
    refused(bytes, "a name twice");
}

#[test]
fn a_local_header_that_disagrees_with_the_directory_is_refused() {
    // Rename the entry in its local header only.
    let mut bytes = zip_with(&[("aaaa.txt", b"one")]);
    let at = bytes.windows(8).position(|w| w == b"aaaa.txt").unwrap();
    bytes[at..at + 8].copy_from_slice(b"zzzz.txt");
    refused(bytes, "inconsistent");
}

#[test]
fn what_is_not_a_package_or_lacks_its_files_is_refused() {
    refused(b"MZ not a zip".to_vec(), "not a zip");
    refused(zip_of(&[("main.js", b"1")]), "no kule-extension.json");
    let m = manifest(json!({}));
    refused(
        zip_of(&[("kule-extension.json", m.as_bytes()), ("main.js", b"1")]),
        "panel.html, which is not in the package",
    );
}

#[test]
fn manifests_are_refused_for_what_they_should_not_say() {
    let cases = [
        (
            json!({ "permissions": ["secrets.read"] }),
            "unknown variant",
        ),
        (
            json!({ "permissions": ["tasks.read", "tasks.read"] }),
            "twice",
        ),
        (json!({ "surprise": true }), "unknown field"),
        (
            json!({ "permissions": ["net"] }),
            "net needs at least one host",
        ),
        (
            json!({ "hosts": ["hooks.slack.com"] }),
            "without the net permission",
        ),
        (
            json!({ "permissions": ["net"], "hosts": ["127.0.0.1"] }),
            "plain name",
        ),
        (
            json!({ "permissions": ["net"], "hosts": ["https://x.com"] }),
            "plain name",
        ),
        (
            json!({ "id": "other.notes" }),
            "must start with the publisher",
        ),
        (json!({ "id": "acme" }), "publisher.name"),
        (json!({ "id": "acme.Notes" }), "lowercase"),
        (json!({ "name": "Pay\u{202E}lacol" }), "text-direction"),
        (json!({ "name": "Kule: Update" }), "Kule's own"),
        (json!({ "name": "kule notes" }), "Kule's own"),
        (
            json!({ "id": "kule.notes", "publisher": "kule" }),
            "Kule's own",
        ),
        (json!({ "version": "1.0" }), "X.Y.Z"),
        (json!({ "kule_extension": 2 }), "format 2"),
        (json!({ "main": null }), "need a main script"),
        (json!({ "main": "../main.js" }), "plain path"),
        (
            json!({ "contributes": { "commands": [{ "id": "notes", "title": "N" }],
                "panels": [{ "id": "notes", "title": "Notes", "entry": "panel.html" }] } }),
            "both a command and a panel",
        ),
    ];
    for (extra, says) in cases {
        let m = manifest(extra.clone());
        let files = good_files(&m);
        let all: Vec<(&str, &[u8])> = files.iter().map(|(n, b)| (*n, b.as_slice())).collect();
        match Package::open(zip_of(&all)) {
            Err(Error::Invalid(text)) => assert!(text.contains(says), "{extra}: {text:?}"),
            other => panic!("{extra} should be refused with {says:?}, got {other:?}"),
        }
    }
}

#[test]
fn mcp_servers_may_not_set_the_loader_or_the_proxy() {
    for (env, ok) in [
        (json!({ "DB_URL": "x" }), true),
        (json!({ "LD_PRELOAD": "x" }), false),
        (json!({ "DYLD_INSERT_LIBRARIES": "x" }), false),
        (json!({ "HTTPS_PROXY": "x" }), false),
        (json!({ "NODE_OPTIONS": "--require x" }), false),
        (json!({ "PATH": "/tmp" }), false),
        (json!({ "UV_INDEX_URL": "https://evil" }), false),
        (json!({ "NPM_CONFIG_REGISTRY": "https://evil" }), false),
        (json!({ "PYTHONPATH": "/tmp" }), false),
        (json!({ "PIP_INDEX_URL": "https://evil" }), false),
        (json!({ "GIT_SSH_COMMAND": "x" }), false),
    ] {
        let m = manifest(json!({ "contributes": {
            "mcpServers": [{ "name": "db", "command": "npx", "args": ["-y", "@acme/db@1.2.3"], "env": env }]
        }, "main": null }));
        let all = [("kule-extension.json", m.as_bytes())];
        assert_eq!(Package::open(zip_of(&all)).is_ok(), ok, "{env}");
    }
    let m = manifest(json!({ "contributes": {
        "mcpServers": [{ "name": "db", "command": "bash", "args": ["-c", "x"] }]
    }, "main": null }));
    refused(
        zip_of(&[("kule-extension.json", m.as_bytes())]),
        "npx or uvx",
    );
    // One release of a registry package, never a range, a tag, an alias, a
    // URL or a repository.
    for (command, package, ok) in [
        ("npx", "@acme/db@1.2.3", true),
        ("uvx", "acme-db==1.2.3", true),
        ("uvx", "acme-db@1.2.3", true),
        ("npx", "@acme/db@latest", false),
        ("npx", "@acme/db@^1.2.0", false),
        ("npx", "db@npm:other@1.2.3", false),
        ("npx", "git+ssh://git@github.com/acme/db.git", false),
        ("npx", "https://evil.example/db-1.2.3.tgz", false),
        ("uvx", "acme-db", false),
        ("uvx", "acme-db>=1.2", false),
        ("uvx", "git+https://github.com/acme/db@1.2.3", false),
    ] {
        let m = manifest(json!({ "contributes": {
            "mcpServers": [{ "name": "db", "command": command, "args": ["-y", package] }]
        }, "main": null }));
        let all = [("kule-extension.json", m.as_bytes())];
        assert_eq!(
            Package::open(zip_of(&all)).is_ok(),
            ok,
            "{command} {package}"
        );
    }
}

#[test]
fn an_installed_extension_waits_for_approval_of_what_was_seen() {
    let home = TempDir::new().unwrap();
    let store = Store::new(home.path());
    let package = Package::open(pack(folder(&manifest(json!({}))).path()).unwrap()).unwrap();
    let listed = store.install(&package).unwrap();
    assert_eq!(listed.held.as_deref(), Some("not approved yet"));
    assert!(!listed.runs());
    assert_eq!(listed.new_permissions, [Permission::TasksRead]);
    assert!(matches!(
        store.runnable("acme.notes"),
        Err(Error::Held { .. })
    ));

    let e = store.approve("acme.notes", "0000").unwrap_err().to_string();
    assert!(e.contains("changed since you looked"), "{e}");
    let listed = store.approve("acme.notes", package.digest()).unwrap();
    assert!(listed.runs());
    assert!(listed.new_permissions.is_empty());
    assert!(store.runnable("acme.notes").is_ok());

    store.set_enabled("acme.notes", false).unwrap();
    let e = store.runnable("acme.notes").unwrap_err().to_string();
    assert!(e.contains("turned off"), "{e}");
}

#[test]
fn an_update_is_held_and_names_what_it_newly_asks_for() {
    let home = TempDir::new().unwrap();
    let store = Store::new(home.path());
    let v1 = Package::open(pack(folder(&manifest(json!({}))).path()).unwrap()).unwrap();
    store.install(&v1).unwrap();
    store.approve("acme.notes", v1.digest()).unwrap();

    let m2 = manifest(json!({
        "version": "1.1.0",
        "permissions": ["tasks.read", "events.read", "net"],
        "hosts": ["hooks.slack.com"]
    }));
    let v2 = Package::open(pack(folder(&m2).path()).unwrap()).unwrap();
    let listed = store.install(&v2).unwrap();
    assert_eq!(
        listed.held.as_deref(),
        Some("it changed since you approved it")
    );
    assert_eq!(
        listed.new_permissions,
        [Permission::EventsRead, Permission::Net]
    );
    assert_eq!(listed.new_hosts, ["hooks.slack.com"]);
    assert!(store.runnable("acme.notes").is_err());
    // The old package is gone; only the new one is kept.
    assert_eq!(
        fs::read_dir(home.path().join("extensions/packages"))
            .unwrap()
            .count(),
        1
    );
}

#[test]
fn a_package_changed_on_disk_is_held() {
    let home = TempDir::new().unwrap();
    let store = Store::new(home.path());
    let package = Package::open(pack(folder(&manifest(json!({}))).path()).unwrap()).unwrap();
    store.install(&package).unwrap();
    store.approve("acme.notes", package.digest()).unwrap();
    let other =
        Package::open(pack(folder(&manifest(json!({ "version": "9.9.9" }))).path()).unwrap())
            .unwrap();
    let file = fs::read_dir(home.path().join("extensions/packages"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    fs::write(&file, other.bytes()).unwrap();
    let e = store.runnable("acme.notes").unwrap_err().to_string();
    assert!(e.contains("changed on disk"), "{e}");
}

#[test]
fn a_folder_under_development_needs_approval_again_only_when_its_manifest_changes() {
    let home = TempDir::new().unwrap();
    let store = Store::new(home.path());
    let dir = folder(&manifest(json!({})));
    let listed = store.install_folder(dir.path(), &[]).unwrap();
    assert!(listed.dev.is_some());
    store.approve("acme.notes", &listed.digest).unwrap();
    fs::write(dir.path().join("main.js"), "// new code").unwrap();
    assert!(store.runnable("acme.notes").is_ok());
    assert_eq!(
        store
            .runnable("acme.notes")
            .unwrap()
            .read("main.js")
            .unwrap(),
        b"// new code"
    );

    let wider = manifest(json!({ "permissions": ["tasks.read", "files.read"] }));
    fs::write(dir.path().join("kule-extension.json"), wider).unwrap();
    let listed = store.get("acme.notes").unwrap();
    assert_eq!(
        listed.held.as_deref(),
        Some("it changed since you approved it")
    );
    assert_eq!(listed.new_permissions, [Permission::FilesRead]);

    // A link inside the folder is not followed.
    fs::remove_file(dir.path().join("panel.html")).unwrap();
    std::os::unix::fs::symlink("/etc/hosts", dir.path().join("panel.html")).unwrap();
    assert!(
        store
            .get("acme.notes")
            .unwrap()
            .held
            .unwrap()
            .contains("is a link")
    );
}

#[test]
fn settings_keep_only_values_that_fit_and_fall_back_to_defaults() {
    let home = TempDir::new().unwrap();
    let store = Store::new(home.path());
    let package = Package::open(pack(folder(&manifest(json!({}))).path()).unwrap()).unwrap();
    store.install(&package).unwrap();
    assert_eq!(store.config("acme.notes").unwrap()["level"], json!("warn"));
    assert!(!store.config("acme.notes").unwrap().contains_key("count"));
    assert!(
        store
            .set_config("acme.notes", "level", &json!("loud"))
            .is_err()
    );
    assert!(
        store
            .set_config("acme.notes", "count", &json!("3"))
            .is_err()
    );
    assert!(store.set_config("acme.notes", "nope", &json!(1)).is_err());
    store
        .set_config("acme.notes", "level", &json!("info"))
        .unwrap();
    store.set_config("acme.notes", "count", &json!(3)).unwrap();
    let config = store.config("acme.notes").unwrap();
    assert_eq!(
        (config["level"].clone(), config["count"].clone()),
        (json!("info"), json!(3))
    );
    store
        .set_config("acme.notes", "level", &serde_json::Value::Null)
        .unwrap();
    assert_eq!(store.config("acme.notes").unwrap()["level"], json!("warn"));
}

#[test]
fn storage_is_capped_and_goes_with_the_extension() {
    let home = TempDir::new().unwrap();
    let store = Store::new(home.path());
    let package = Package::open(pack(folder(&manifest(json!({}))).path()).unwrap()).unwrap();
    store.install(&package).unwrap();
    assert_eq!(store.storage("acme.notes").unwrap(), json!(null));
    store
        .set_storage("acme.notes", &json!({ "seen": 3 }))
        .unwrap();
    assert_eq!(store.storage("acme.notes").unwrap(), json!({ "seen": 3 }));
    let big = json!("x".repeat(1024 * 1024));
    assert!(store.set_storage("acme.notes", &big).is_err());
    assert!(store.set_storage("nobody.here", &json!(1)).is_err());
    assert!(store.storage("../../etc").is_err());

    store.remove("acme.notes").unwrap();
    assert!(!home.path().join("extensions/data/acme.notes").exists());
    assert!(store.list().unwrap().is_empty());
    assert_eq!(
        fs::read_dir(home.path().join("extensions/packages"))
            .unwrap()
            .count(),
        0
    );
}

#[test]
fn a_file_is_read_through_its_cap() {
    let package = Package::open(pack(folder(&manifest(json!({}))).path()).unwrap()).unwrap();
    assert_eq!(
        package.read("panel.html").unwrap(),
        b"<!doctype html><p>notes</p>"
    );
    assert!(package.read("missing.js").is_err());
    let _ = Path::new("unused");
}

#[test]
fn a_folder_agents_can_write_is_not_taken_for_development() {
    let home = TempDir::new_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let store = Store::new(home.path());
    let m = manifest(json!({}));
    let write = |dir: &Path| {
        fs::write(dir.join("kule-extension.json"), &m).unwrap();
        fs::write(dir.join("main.js"), "1").unwrap();
        fs::write(dir.join("panel.html"), "<p>").unwrap();
    };
    // /tmp: every sandboxed command may write there.
    let tmp = TempDir::new_in("/tmp").unwrap();
    write(tmp.path());
    let e = store
        .install_folder(tmp.path(), &[])
        .unwrap_err()
        .to_string();
    assert!(e.contains("where agents can write"), "{e}");
    // Kule's home: the tasks' copies are there.
    let inside = home.path().join("tasks/abc/copy/ext");
    fs::create_dir_all(&inside).unwrap();
    write(&inside);
    assert!(store.install_folder(&inside, &[]).is_err());
    // An agent's own directory, which the caller names.
    let agent = TempDir::new_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let ext = agent.path().join("plugins/x");
    fs::create_dir_all(&ext).unwrap();
    write(&ext);
    let e = store
        .install_folder(&ext, &[agent.path().to_path_buf()])
        .unwrap_err()
        .to_string();
    assert!(e.contains("where agents can write"), "{e}");
    // Elsewhere it is taken.
    assert!(store.install_folder(&ext, &[]).is_ok());
    assert!(
        store
            .list()
            .unwrap()
            .iter()
            .all(|l| l.dev.as_deref() != Some(tmp.path()))
    );
}

#[test]
fn a_folder_already_installed_where_agents_can_write_is_held() {
    let home = TempDir::new_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let store = Store::new(home.path());
    let dir = folder(&manifest(json!({})));
    let listed = store.install_folder(dir.path(), &[]).unwrap();
    store.approve("acme.notes", &listed.digest).unwrap();
    // Installed by an older Kule from /tmp: it no longer runs.
    let tmp = TempDir::new_in("/tmp").unwrap();
    for f in ["kule-extension.json", "main.js", "panel.html"] {
        fs::copy(dir.path().join(f), tmp.path().join(f)).unwrap();
    }
    let installed = home.path().join("extensions/installed.json");
    let text = fs::read_to_string(&installed).unwrap();
    let real = dir.path().canonicalize().unwrap();
    let json = serde_json::to_string(&real.display().to_string()).unwrap();
    let to = serde_json::to_string(&tmp.path().display().to_string()).unwrap();
    fs::write(&installed, text.replace(&json, &to)).unwrap();
    let held = store.get("acme.notes").unwrap().held.unwrap();
    assert!(held.contains("where agents can write"), "{held}");
    assert!(store.runnable("acme.notes").is_err());
}
