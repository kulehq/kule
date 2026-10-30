//! What a task's commands may read beyond the copy: canaries planted where
//! a project, a sign-in or an app's data lies, each found by a read outside
//! the sandbox first, then refused inside it.
#![cfg(unix)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Output;

use kule_engine::{CacheSources, Sandbox, Task};
use tempfile::TempDir;

/// [`Task::apply_home`] by a person who reviewed the home changes as they
/// are now.
trait Reviewed {
    fn apply_reviewed(&self, paths: &[String]) -> kule_engine::Result<kule_engine::HomeApplied>;
}

impl Reviewed for Task {
    fn apply_reviewed(&self, paths: &[String]) -> kule_engine::Result<kule_engine::HomeApplied> {
        let digest = kule_engine::home_digest(&self.home_changes()?);
        self.apply_home(paths, &digest)
    }
}

fn sh(sandbox: &Sandbox, script: &str) -> Output {
    sandbox
        .command("sh", &["-c".to_owned(), script.to_owned()])
        .unwrap()
        .output()
        .unwrap()
}

/// A person's home outside /tmp (the sandbox lets commands write there on
/// Linux), resolved, holding `paths`, each a canary.
fn home_with(paths: &[&str]) -> (TempDir, PathBuf) {
    let dir = TempDir::new_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let home = fs::canonicalize(dir.path()).unwrap();
    for path in paths {
        plant(&home.join(path));
    }
    (dir, home)
}

fn plant(path: &Path) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, "canary\n").unwrap();
}

fn sources(home: &Path) -> CacheSources {
    CacheSources {
        home: home.to_owned(),
        cargo_home: home.join(".cargo"),
    }
}

/// Whether a sandboxed `cat` reads `path`; a read outside finds it first.
fn reads(sandbox: &Sandbox, path: &Path) -> bool {
    assert_eq!(
        fs::read_to_string(path).unwrap(),
        "canary\n",
        "{} is not planted",
        path.display()
    );
    sh(sandbox, &format!("cat '{}'", path.display()))
        .status
        .success()
}

/// Every project Kule opened is hidden from every other task, with the
/// projects beside it, wherever it lies: in the home under another folder
/// than the workspace's, or outside the home.
#[test]
fn projects_kule_opened_are_hidden_from_every_other_task() {
    let (_home, home) = home_with(&[
        "work/app/a.txt",
        "projects/shop/.env",
        "projects/beside-shop/.env",
    ]);
    let outside_dir = TempDir::new_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let outside = fs::canonicalize(outside_dir.path()).unwrap();
    plant(&outside.join(".env"));
    let kule = TempDir::new_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let task = Task::create(kule.path(), &home.join("work/app")).unwrap();
    Task::create(kule.path(), &home.join("projects/shop")).unwrap();
    Task::create(kule.path(), &outside).unwrap();
    // A repository's settings, which a task's copy would not have.
    plant(&home.join("projects/shop/.git/config"));
    plant(&outside.join(".git/config"));
    let sandbox = Sandbox::for_task_with(&task, Some(sources(&home))).unwrap();
    for path in [
        home.join("projects/shop/.env"),
        home.join("projects/shop/.git/config"),
        home.join("projects/beside-shop/.env"),
        outside.join(".env"),
        outside.join(".git/config"),
    ] {
        assert!(!reads(&sandbox, &path), "{} was read", path.display());
    }
    // The task's own copy as ever.
    assert!(sh(&sandbox, "cat a.txt").status.success());
}

#[cfg(unix)]
/// Whatever lies at the top of the home but its settings is the person's
/// own: a project Kule never opened, a file. The toolchains' own folders
/// and settings there stay readable, the tools on `PATH` in such a folder
/// too; a link at the top leads where it leads, which is readable only
/// where that is granted.
#[test]
fn what_lies_at_the_top_of_the_home_is_the_persons_own() {
    let (_home, home) = home_with(&[
        "projects/never-opened/.env",
        "notes.txt",
        "Applications/Some.app/token",
        ".gitconfig",
        ".rustup/settings.toml",
        "sdk/tool/bin/tool",
        "sdk/other/secret",
    ]);
    let elsewhere_dir = TempDir::new_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let elsewhere = fs::canonicalize(elsewhere_dir.path()).unwrap();
    plant(&elsewhere.join("cellar/tool"));
    std::os::unix::fs::symlink(&elsewhere, home.join("brew")).unwrap();
    let s = Setup::new();
    let tools = [home.join("sdk/tool/bin")];
    let mut sandbox = Sandbox::new(&s.task).unwrap();
    sandbox.hide_personal(&home, &tools).unwrap();
    sandbox.grant_env(&tools, Some(&home));
    sandbox.grant_defaults(&home);
    for path in [
        "projects/never-opened/.env",
        "notes.txt",
        "Applications/Some.app/token",
        "sdk/other/secret",
        "brew/cellar/tool",
    ] {
        assert!(!reads(&sandbox, &home.join(path)), "{path} was read");
    }
    for path in [".gitconfig", ".rustup/settings.toml", "sdk/tool/bin/tool"] {
        assert!(reads(&sandbox, &home.join(path)), "{path} was not read");
    }
}

/// A tool's folder inside where apps keep settings and data opens that
/// folder, or the toolchain's root above a `bin`, never the app's whole
/// folder there: beside a launcher's scripts lie every editor's settings.
#[test]
fn a_tool_inside_an_apps_data_opens_no_more_than_its_own_folder() {
    let (_home, home) = home_with(&[
        ".local/share/JetBrains/Toolbox/scripts/idea",
        ".local/share/JetBrains/Toolbox/settings.json",
        ".local/share/JetBrains/IntelliJIdea/options/other.xml",
        ".local/share/app/bin/tool",
        ".local/share/app/session",
        ".local/share/nvim/mason/bin/linter",
        ".local/share/nvim/mason/packages/linter/lib.txt",
        ".local/share/nvim/shada/main.shada",
    ]);
    let s = Setup::new();
    let tools = [
        home.join(".local/share/JetBrains/Toolbox/scripts"),
        home.join(".local/share/app/bin"),
        home.join(".local/share/nvim/mason/bin"),
    ];
    let mut sandbox = Sandbox::new(&s.task).unwrap();
    sandbox.hide_settings(&home, &tools).unwrap();
    sandbox.grant_env(&tools, Some(&home));
    for path in [
        ".local/share/JetBrains/Toolbox/scripts/idea",
        ".local/share/app/bin/tool",
        ".local/share/nvim/mason/bin/linter",
        // The toolchain's root above its `bin`, inside the app's folder.
        ".local/share/nvim/mason/packages/linter/lib.txt",
    ] {
        assert!(reads(&sandbox, &home.join(path)), "{path} was not read");
    }
    for path in [
        ".local/share/JetBrains/Toolbox/settings.json",
        ".local/share/JetBrains/IntelliJIdea/options/other.xml",
        ".local/share/app/session",
        ".local/share/nvim/shada/main.shada",
    ] {
        assert!(!reads(&sandbox, &home.join(path)), "{path} was read");
    }
}

/// A tool pipx installed runs from wherever pipx keeps its home: its
/// launcher in `~/.local/bin` is a link into the venv, and the venv reads
/// pip and setuptools from the `shared` venv beside it (`pipx_shared.pth`),
/// so pipx's home is needed whole: `~/.local/pipx` (pipx before 1.5, kept
/// where it exists), `~/.local/share/pipx`, or on macOS
/// `~/Library/Application Support/pipx`. What else lies where apps keep
/// their data stays hidden.
#[test]
fn a_tool_pipx_installed_reads_its_venv_wherever_pipx_keeps_it() {
    let homes = [
        ("old", ".local/pipx"),
        ("xdg", ".local/share/pipx"),
        ("mac", "Library/Application Support/pipx"),
    ];
    let planted: Vec<String> = homes
        .iter()
        .flat_map(|(name, pipx)| {
            [
                format!("{pipx}/venvs/{name}/bin/{name}"),
                format!("{pipx}/venvs/{name}/lib/site.py"),
                format!("{pipx}/shared/lib/pip.py"),
            ]
        })
        // Beside pipx's: an app's, and one whose name starts like pip's.
        .chain([
            "Library/Application Support/other/token".to_owned(),
            "Library/Application Support/pipfoo/token".to_owned(),
        ])
        .collect();
    let (_home, home) = home_with(&planted.iter().map(String::as_str).collect::<Vec<_>>());
    fs::create_dir_all(home.join(".local/bin")).unwrap();
    for (name, pipx) in homes {
        std::os::unix::fs::symlink(
            home.join(pipx)
                .join("venvs")
                .join(name)
                .join("bin")
                .join(name),
            home.join(".local/bin").join(name),
        )
        .unwrap();
    }
    let s = Setup::new();
    let tools = [home.join(".local/bin")];
    let mut sandbox = Sandbox::new(&s.task).unwrap();
    sandbox.hide_personal(&home, &tools).unwrap();
    sandbox.hide_settings(&home, &tools).unwrap();
    sandbox.grant_env(&tools, Some(&home));
    sandbox.grant_defaults(&home);
    for (name, pipx) in homes {
        for file in [
            format!("venvs/{name}/lib/site.py"),
            "shared/lib/pip.py".to_owned(),
        ] {
            let path = home.join(pipx).join(&file);
            assert!(reads(&sandbox, &path), "{pipx}/{file} was not read");
        }
    }
    for app in ["other", "pipfoo"] {
        let token = home
            .join("Library/Application Support")
            .join(app)
            .join("token");
        assert!(!reads(&sandbox, &token), "{app}'s token was read");
    }
}

/// A toolchain kept in a folder of the person's own, its `bin` on `PATH`,
/// is readable from its root (Go finds its own beside its program); what
/// lies beside it stays the person's.
#[test]
fn a_toolchain_in_a_persons_folder_opens_its_root_alone() {
    let (_home, home) = home_with(&[
        "sdk/go1.22/bin/go",
        "sdk/go1.22/src/runtime.go",
        "sdk/notes.txt",
        "sdk/go1.21/src/runtime.go",
    ]);
    let s = Setup::new();
    let tools = [home.join("sdk/go1.22/bin")];
    let mut sandbox = Sandbox::new(&s.task).unwrap();
    sandbox.hide_personal(&home, &tools).unwrap();
    sandbox.grant_env(&tools, Some(&home));
    assert!(reads(&sandbox, &home.join("sdk/go1.22/src/runtime.go")));
    for path in ["sdk/notes.txt", "sdk/go1.21/src/runtime.go"] {
        assert!(!reads(&sandbox, &home.join(path)), "{path} was read");
    }
}

/// A project Kule opened that holds the home itself (a task on the whole
/// home) hides nothing more from the others: hiding it would take their
/// toolchains away.
#[test]
fn a_project_holding_the_home_hides_nothing_more() {
    let (_home, home) = home_with(&[".gitconfig", ".rustup/settings.toml", "work/app/a.txt"]);
    let kule = TempDir::new_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let task = Task::create(kule.path(), &home.join("work/app")).unwrap();
    Task::create(kule.path(), &home).unwrap();
    let sandbox = Sandbox::for_task_with(&task, Some(sources(&home))).unwrap();
    assert!(reads(&sandbox, &home.join(".gitconfig")));
    assert!(reads(&sandbox, &home.join(".rustup/settings.toml")));
    assert!(sh(&sandbox, "cat a.txt").status.success());
}

/// Where apps and tools keep settings and data, an app's sign-in or a
/// new tool's included, nothing is read but what toolchains, shells and
/// prompts read; editors' stores and saved shell sessions neither.
#[test]
fn settings_and_app_data_are_hidden_but_for_what_toolchains_read() {
    let hidden = [
        ".config/github-copilot/apps.json",
        ".config/configstore/firebase-tools.json",
        ".config/containers/auth.json",
        ".config/fly/config.yml",
        ".fly/config.yml",
        ".config/doppler/.doppler.yaml",
        ".config/sops/age/keys.txt",
        ".config/.wrangler/config/default.toml",
        ".config/pypoetry/auth.toml",
        ".config/some-new-tool/token",
        ".config/Cursor/User/globalStorage/state.vscdb",
        ".config/Windsurf/User/globalStorage/state.vscdb",
        ".config/VSCodium/User/globalStorage/state.vscdb",
        ".local/share/some-app/session",
        ".local/share/fish/fish_history",
        "Library/Preferences/.wrangler/config/default.toml",
        "Library/Preferences/com.example.app.plist",
        "Library/Preferences/ByHost/com.example.app.0A1B.plist",
        "Library/Application Support/pypoetry/auth.toml",
        "Library/Application Support/Cursor/User/globalStorage/state.vscdb",
        "Library/Application Support/Windsurf/User/globalStorage/state.vscdb",
        "Library/Application Support/Some App/token",
        ".zsh_sessions/A1.session",
        ".bash_sessions/A1.session",
        ".yarnrc.yml",
        ".bunfig.toml",
        ".nuget/NuGet/NuGet.Config",
    ];
    let read = [
        ".config/git/config",
        ".config/uv/uv.toml",
        ".config/pypoetry/config.toml",
        ".config/fish/config.fish",
        ".config/starship.toml",
        ".config/mise/config.toml",
        ".local/share/uv/python/marker",
        ".local/share/fish/vendor_completions.d/x.fish",
        ".local/share/man/man1/x.1",
        "Library/Preferences/.GlobalPreferences.plist",
        "Library/Preferences/.GlobalPreferences_m.plist",
        "Library/Preferences/ByHost/.GlobalPreferences.0A1B.plist",
        "Library/Application Support/go/env",
        "Library/Application Support/pypoetry/config.toml",
    ];
    let (_home, home) = home_with(&[hidden.as_slice(), read.as_slice()].concat());
    let s = Setup::new();
    let sandbox = Sandbox::for_task_with(&s.task, Some(sources(&home))).unwrap();
    for path in hidden {
        assert!(!reads(&sandbox, &home.join(path)), "{path} was read");
    }
    for path in read {
        assert!(reads(&sandbox, &home.join(path)), "{path} was not read");
    }
    // Revealed whole for a command, a store keeps the sign-ins in it hidden.
    let mut sandbox = Sandbox::for_task_with(&s.task, Some(sources(&home))).unwrap();
    sandbox.reveal(&home.join(".config")).unwrap();
    assert!(reads(&sandbox, &home.join(".config/some-new-tool/token")));
    for path in hidden
        .iter()
        .filter(|p| p.starts_with(".config/") && **p != ".config/some-new-tool/token")
    {
        assert!(
            !reads(&sandbox, &home.join(path)),
            "{path} was read once revealed"
        );
    }
}

#[cfg(unix)]
/// A tool on `PATH` whose program is a link into where apps keep their
/// data (Claude Code's native install, uv's tools) still runs: what the
/// link leads to is read, and nothing beside it.
#[test]
fn a_tool_linked_from_path_into_app_data_runs() {
    let (_home, home) = home_with(&[".local/share/other-app/session"]);
    let tool = home.join(".local/share/a-tool/versions/1.0/a-tool");
    fs::create_dir_all(tool.parent().unwrap()).unwrap();
    fs::write(&tool, "#!/bin/sh\necho ran\n").unwrap();
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(&tool, fs::Permissions::from_mode(0o755)).unwrap();
    let bin = home.join(".local/bin");
    fs::create_dir_all(&bin).unwrap();
    std::os::unix::fs::symlink(&tool, bin.join("a-tool")).unwrap();
    // A link to a file at the top of the home makes no tool of the home.
    plant(&home.join("notes.txt"));
    std::os::unix::fs::symlink(home.join("notes.txt"), bin.join("notes")).unwrap();
    let s = Setup::new();
    let mut sandbox = Sandbox::new(&s.task).unwrap();
    sandbox
        .hide_settings(&home, std::slice::from_ref(&bin))
        .unwrap();
    sandbox.grant_env(std::slice::from_ref(&bin), Some(&home));
    let out = sh(&sandbox, &format!("'{}'", bin.join("a-tool").display()));
    assert!(out.status.success(), "{out:?}");
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "ran");
    assert!(!reads(
        &sandbox,
        &home.join(".local/share/other-app/session")
    ));
}

/// A writable folder where apps keep their data, such as an agent's home
/// of its own in Kule's folder there, stays writable; what is hidden inside
/// it stays hidden, and what lies beside it too.
#[test]
fn a_writable_folder_inside_app_data_stays_writable() {
    let (_home, home) = home_with(&[
        ".local/share/kule/agents/mine/settings.json",
        ".local/share/kule/agents/mine/sessions/one.json",
        ".local/share/kule/agents/mine/login.json",
        ".local/share/kule/agents/other/settings.json",
    ]);
    let own = home.join(".local/share/kule/agents/mine");
    let s = Setup::new();
    let mut sandbox = Sandbox::new(&s.task).unwrap();
    sandbox.hide_settings(&home, &[]).unwrap();
    sandbox.allow_write(&own).unwrap();
    sandbox.hide(&own.join("login.json")).unwrap();
    let out = sh(
        &sandbox,
        &format!(
            // In a folder of its own: on Linux nothing new is made right
            // in a writable folder that holds a hidden path (see the
            // sandbox's carving).
            "cd '{}' && cat settings.json && echo new > sessions/new.json && cat sessions/new.json",
            own.display()
        ),
    );
    assert!(out.status.success(), "{out:?}");
    assert!(!reads(&sandbox, &own.join("login.json")));
    assert!(!reads(
        &sandbox,
        &home.join(".local/share/kule/agents/other/settings.json")
    ));
}

/// The commands' home is the task's layer over the person's: what it
/// holds of their settings is what the sandbox lets them read there, and
/// nothing hidden is cloned into it.
#[test]
fn the_home_layer_holds_only_settings_the_sandbox_shows() {
    let (_home, home) = home_with(&[
        ".config/git/config",
        ".config/git/credentials",
        ".config/github-copilot/apps.json",
        ".config/some-new-tool/token",
        ".zshrc",
    ]);
    let s = Setup::new();
    let mut sandbox = Sandbox::for_task_with(&s.task, Some(sources(&home))).unwrap();
    s.task
        .prepare_home(&home, &[], sandbox.hidden(), &sandbox.shown(), &[])
        .unwrap();
    sandbox.use_home(&s.task, &home).unwrap();
    let out = sh(
        &sandbox,
        "cat \"$HOME/.config/git/config\" && cat \"$HOME/.zshrc\"",
    );
    assert!(out.status.success(), "{out:?}");
    let layer = s.task.home_layer();
    for gone in [
        ".config/git/credentials",
        ".config/github-copilot",
        ".config/some-new-tool",
    ] {
        assert!(
            fs::symlink_metadata(layer.join(gone)).is_err(),
            "{gone} is in the layer"
        );
        let out = sh(&sandbox, &format!("cat \"$HOME/{gone}\"*"));
        assert!(!out.status.success(), "{gone} was read through the layer");
    }
    // What a command writes in its place is its own, offered for review,
    // and never applied over the person's file it could not see.
    let out = sh(
        &sandbox,
        "mkdir -p \"$HOME/.config/some-new-tool\" && echo mine > \"$HOME/.config/some-new-tool/token\"",
    );
    assert!(out.status.success(), "{out:?}");
    assert!(
        s.task
            .home_changes()
            .unwrap()
            .iter()
            .any(|c| c.path == ".config/some-new-tool/token")
    );
    assert!(
        s.task
            .apply_reviewed(&[".config/some-new-tool/token".into()])
            .is_err()
    );
    assert_eq!(
        fs::read_to_string(home.join(".config/some-new-tool/token")).unwrap(),
        "canary\n"
    );
}

/// At the top of the home only what toolchains and shells read is there
/// for a command: a toolchain's folder is linked, a startup file cloned,
/// and any other dot entry (a tool's settings that may hold a token) is
/// simply not in the commands' home, nor readable where it really is.
/// Package index settings are cloned with their tokens masked.
#[test]
fn only_what_toolchains_read_is_in_the_commands_home() {
    let (_home, home) = home_with(&[
        ".condarc",
        ".terraformrc",
        ".composer/auth.json",
        ".cargo/config.toml",
        ".zshrc",
        ".m2/settings.xml",
        ".m2/toolchains.xml",
        ".nuget/NuGet/NuGet.Config",
        ".nuget/packages/newtonsoft.json/13.0.3/lib/net6.0/Newtonsoft.Json.dll",
    ]);
    let token = "ghp_1234567890abcdefghijklmnopqrstuvwxyz";
    for conf in [".config/pip/pip.conf", ".pip/pip.conf"] {
        plant(&home.join(conf));
        fs::write(
            home.join(conf),
            format!(
                "[global]\nindex-url = https://me:{token}@pypi.example.com/simple\ntimeout = 60\n"
            ),
        )
        .unwrap();
    }
    fs::write(
        home.join(".pydistutils.cfg"),
        format!("[easy_install]\nindex_url = https://me:{token}@pypi.example.com/simple\n"),
    )
    .unwrap();
    fs::write(
        home.join(".config/uv/uv.toml"),
        format!("index-url = \"https://me:{token}@pypi.example.com/simple\"\n"),
    )
    .map_or_else(
        |_| {
            fs::create_dir_all(home.join(".config/uv")).unwrap();
            fs::write(
                home.join(".config/uv/uv.toml"),
                format!("index-url = \"https://me:{token}@pypi.example.com/simple\"\n"),
            )
            .unwrap();
        },
        |_| {},
    );
    let s = Setup::new();
    let mut sandbox = Sandbox::for_task_with(&s.task, Some(sources(&home))).unwrap();
    s.task
        .prepare_home(&home, &[], sandbox.hidden(), &sandbox.shown(), &[])
        .unwrap();
    sandbox.use_home(&s.task, &home).unwrap();
    let layer = s.task.home_layer();
    // Not there, not refused: as if the person had none.
    for gone in [".condarc", ".terraformrc", ".composer/auth.json"] {
        assert!(
            fs::symlink_metadata(layer.join(gone)).is_err(),
            "{gone} is in the layer"
        );
        let out = sh(&sandbox, &format!("cat \"$HOME/{gone}\""));
        let said = String::from_utf8_lossy(&out.stderr);
        assert!(said.contains("No such file"), "{gone}: {out:?}");
        assert!(
            !reads(&sandbox, &home.join(gone)),
            "{gone} was read where it is"
        );
    }
    // What toolchains read is there: linked, or cloned. A credential file
    // inside a linked folder is refused as ever.
    for there in [".cargo/config.toml", ".zshrc", ".m2/toolchains.xml"] {
        let out = sh(&sandbox, &format!("cat \"$HOME/{there}\""));
        assert!(out.status.success(), "{there}: {out:?}");
    }
    let out = sh(&sandbox, "cat \"$HOME/.m2/settings.xml\"");
    assert!(!out.status.success() && out.stdout.is_empty(), "{out:?}");
    // NuGet's settings can hold feed credentials and NuGet stops at a file
    // it may not read: not there, so NuGet makes its own; its packages are.
    let out = sh(&sandbox, "cat \"$HOME/.nuget/NuGet/NuGet.Config\"");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("No such file"),
        "{out:?}"
    );
    let out = sh(
        &sandbox,
        "cat \"$HOME/.nuget/packages/newtonsoft.json/13.0.3/lib/net6.0/Newtonsoft.Json.dll\"",
    );
    assert!(out.status.success(), "{out:?}");
    // Index settings: masked in the layer, hidden where they are.
    for conf in [
        ".config/pip/pip.conf",
        ".pip/pip.conf",
        ".pydistutils.cfg",
        ".config/uv/uv.toml",
    ] {
        let out = sh(&sandbox, &format!("cat \"$HOME/{conf}\""));
        assert!(out.status.success(), "{conf}: {out:?}");
        let text = String::from_utf8_lossy(&out.stdout);
        assert!(!text.contains(token), "{conf} shows the token: {text}");
        assert!(
            text.contains("pypi.example.com"),
            "{conf} lost its URL: {text}"
        );
        let out = sh(&sandbox, &format!("cat '{}'", home.join(conf).display()));
        assert!(!out.status.success(), "{conf} was read where it is");
    }
}

/// Escape attempt (#46): a file outside every grant, where a tool was
/// pointed by a variable or a setting (`/opt/creds/aws`, a folder beside
/// the home), read outside Kule first, then from a task: with reads
/// granted rather than assumed, nothing outside the grants is readable,
/// hidden list or not, by its path or through a link made in the copy.
#[test]
fn escape_reads_a_file_outside_every_grant() {
    let (_home, home) = home_with(&[".zshrc"]);
    let outside = TempDir::new_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let outside = fs::canonicalize(outside.path()).unwrap();
    let creds = outside.join("creds/aws");
    plant(&creds);
    // Outside Kule the file reads as any other.
    let out = std::process::Command::new("sh")
        .arg("-c")
        .arg(format!("cat '{}'", creds.display()))
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&out.stdout), "canary\n");
    let s = Setup::new();
    let sandbox = Sandbox::for_task_with(&s.task, Some(sources(&home))).unwrap();
    assert!(!reads(&sandbox, &creds), "read by its path");
    let out = sh(
        &sandbox,
        &format!(
            "ln -s '{}' creds-link && cat creds-link; ln -s '{}' creds-dir && cat creds-dir/aws; cat \"$(dirname '{}')\"/*",
            creds.display(),
            creds.parent().unwrap().display(),
            creds.display()
        ),
    );
    assert!(
        !String::from_utf8_lossy(&out.stdout).contains("canary"),
        "{out:?}"
    );
    // Listed with its source once the person grants it, read-only; the
    // folder above the home is refused however it is named.
    let mut sandbox = Sandbox::for_task_with(&s.task, Some(sources(&home))).unwrap();
    sandbox.reveal(creds.parent().unwrap()).unwrap();
    assert!(reads(&sandbox, &creds));
    assert!(
        !sh(&sandbox, &format!("echo x > '{}'", creds.display()))
            .status
            .success()
    );
    assert!(
        sandbox
            .grants()
            .iter()
            .any(|g| g.path == creds.parent().unwrap() && g.source == kule_engine::Source::Revealed)
    );
    let mut sandbox = Sandbox::for_task_with(&s.task, Some(sources(&home))).unwrap();
    assert!(sandbox.reveal(home.parent().unwrap()).is_err());
}

/// On macOS a program reads preferences through the system's preferences
/// service, outside the sandbox: an app's hidden preferences stay out of
/// reach that way too.
#[cfg(target_os = "macos")]
#[test]
fn hidden_preferences_are_not_read_through_the_preferences_service() {
    let domain = format!("dev.kule.canary-{}", std::process::id());
    let defaults = |args: &[&str]| {
        std::process::Command::new("defaults")
            .args(args)
            .output()
            .unwrap()
    };
    assert!(
        defaults(&["write", &domain, "token", "canary"])
            .status
            .success()
    );
    let person = fs::canonicalize(std::env::var("HOME").unwrap()).unwrap();
    let s = Setup::new();
    let mut sandbox = Sandbox::new(&s.task).unwrap();
    sandbox.hide_settings(&person, &[]).unwrap();
    let out = sandbox
        .command("defaults", &["read".into(), domain.clone(), "token".into()])
        .unwrap()
        .output()
        .unwrap();
    let _ = defaults(&["delete", &domain]);
    assert!(
        !String::from_utf8_lossy(&out.stdout).contains("canary"),
        "{out:?}"
    );
}

struct Setup {
    _kule: TempDir,
    _ws: TempDir,
    task: Task,
}

impl Setup {
    fn new() -> Setup {
        let kule = TempDir::new_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
        let ws = TempDir::new().unwrap();
        fs::write(ws.path().join("a.txt"), "a\n").unwrap();
        let task = Task::create(kule.path(), ws.path()).unwrap();
        Setup {
            _kule: kule,
            _ws: ws,
            task,
        }
    }
}
