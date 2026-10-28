//! The one validator every grant goes through: each path that would weaken
//! Kule is refused whoever names it, and each rule is tried here on the
//! real filesystem.

use std::fs;
use std::path::{Path, PathBuf};

use kule_engine::grants::{Bounds, Refused, Source, granted, validate};
use tempfile::TempDir;

/// A person's home at `homes/me`, with a neighbour at `homes/other`, Kule's
/// home inside it and a few folders of each kind; all resolved.
struct Setup {
    _dir: TempDir,
    homes: PathBuf,
    home: PathBuf,
    kule: PathBuf,
    bounds: Bounds,
}

impl Setup {
    fn new() -> Setup {
        let dir = TempDir::new_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
        let root = fs::canonicalize(dir.path()).unwrap();
        let homes = root.join("homes");
        let home = homes.join("me");
        for rel in [
            "me/.ssh",
            "me/.config/git",
            "me/.config/gh",
            "me/AppData/Roaming/npm",
            "me/Documents/sdk/bin",
            "me/.cargo/bin",
            "me/.kule/tasks/t1/copy",
            "me/code/app",
            "other/.ssh",
            "other/sdk",
        ] {
            fs::create_dir_all(homes.join(rel)).unwrap();
        }
        fs::write(home.join(".ssh/id_ed25519"), "key\n").unwrap();
        fs::write(home.join(".cargo/credentials.toml"), "token\n").unwrap();
        let kule = home.join(".kule");
        let mut bounds = Bounds::for_person(&kule, &home);
        bounds.writable.push(kule.join("tasks/t1/copy"));
        Setup {
            _dir: dir,
            homes,
            home,
            kule,
            bounds,
        }
    }

    fn person(&self, path: &Path) -> Result<PathBuf, Refused> {
        validate(path, Source::Person, &self.bounds)
    }
}

#[test]
fn kules_home_and_what_holds_it_are_refused() {
    let s = Setup::new();
    assert_eq!(s.person(&s.kule), Err(Refused::KuleHome));
    assert_eq!(s.person(&s.kule.join("tasks")), Err(Refused::KuleHome));
    assert_eq!(
        s.person(&s.kule.join("tasks/t1/copy")),
        Err(Refused::KuleHome)
    );
    // Kule's home elsewhere, and the folder above it, the same.
    let other = TempDir::new_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let elsewhere = fs::canonicalize(other.path()).unwrap().join("kule-home");
    fs::create_dir_all(elsewhere.join("tasks")).unwrap();
    let bounds = Bounds::for_person(&elsewhere, &s.home);
    assert_eq!(
        validate(&elsewhere, Source::Person, &bounds),
        Err(Refused::KuleHome)
    );
    assert_eq!(
        validate(elsewhere.parent().unwrap(), Source::Person, &bounds),
        Err(Refused::KuleHome)
    );
    // Kule's own base is held to this too.
    assert_eq!(
        validate(&elsewhere, Source::System, &bounds),
        Err(Refused::KuleHome)
    );
}

#[test]
fn the_home_and_the_folders_above_it_are_refused() {
    let s = Setup::new();
    assert_eq!(s.person(&s.home), Err(Refused::Home));
    assert_eq!(s.person(&s.homes), Err(Refused::Home));
    assert_eq!(s.person(s.homes.parent().unwrap()), Err(Refused::Home));
    assert_eq!(
        validate(&s.home, Source::System, &s.bounds),
        Err(Refused::Home)
    );
    assert_eq!(
        validate(&s.home, Source::Admin, &s.bounds),
        Err(Refused::Home)
    );
}

#[test]
fn another_persons_home_is_refused() {
    let s = Setup::new();
    assert_eq!(s.person(&s.homes.join("other")), Err(Refused::OtherHome));
    assert_eq!(
        s.person(&s.homes.join("other/sdk")),
        Err(Refused::OtherHome)
    );
    assert_eq!(
        validate(&s.homes.join("other/sdk"), Source::System, &s.bounds),
        Err(Refused::OtherHome)
    );
}

/// The system's own first-level folder and a folder inside it, and the
/// root of the filesystem, as this platform spells them.
#[cfg(not(windows))]
const SYSTEM: (&str, &str, &str) = ("/usr", "/usr/bin", "/");
#[cfg(windows)]
const SYSTEM: (&str, &str, &str) = ("C:\\Windows", "C:\\Windows\\System32", "C:\\");

#[test]
fn filesystem_and_volume_roots_are_refused() {
    let s = Setup::new();
    assert_eq!(s.person(Path::new(SYSTEM.2)), Err(Refused::Root));
    #[cfg(target_os = "macos")]
    assert_eq!(
        s.person(Path::new("/System/Volumes/Data")),
        Err(Refused::Root)
    );
}

#[test]
fn a_first_level_system_folder_is_refused_but_one_inside_it_is_not() {
    let s = Setup::new();
    let (first, inside, _) = SYSTEM;
    if cfg!(unix) && Path::new("/opt").is_dir() {
        assert_eq!(s.person(Path::new("/opt")), Err(Refused::FirstLevel));
    }
    assert_eq!(s.person(Path::new(first)), Err(Refused::FirstLevel));
    assert!(s.person(Path::new(inside)).is_ok());
    // Kule's own base names them.
    assert_eq!(
        validate(Path::new(first), Source::System, &s.bounds),
        Ok(fs::canonicalize(first).unwrap())
    );
}

#[test]
fn a_hidden_path_and_what_is_inside_it_are_refused() {
    let s = Setup::new();
    assert_eq!(s.person(&s.home.join(".ssh")), Err(Refused::Hidden));
    assert_eq!(
        s.person(&s.home.join(".ssh/id_ed25519")),
        Err(Refused::Hidden)
    );
    assert_eq!(s.person(&s.home.join(".config/gh")), Err(Refused::Hidden));
    assert_eq!(
        s.person(&s.home.join(".cargo/credentials.toml")),
        Err(Refused::Hidden)
    );
    // The toolchain's folder around a credential file is fine: the file
    // stays denied by name inside it.
    assert!(s.person(&s.home.join(".cargo")).is_ok());
}

#[test]
fn a_personal_folder_is_refused_but_a_tools_folder_inside_it_is_not() {
    let s = Setup::new();
    assert_eq!(s.person(&s.home.join("Documents")), Err(Refused::Personal));
    assert!(s.person(&s.home.join("Documents/sdk")).is_ok());
}

#[test]
fn a_store_is_refused_but_a_tools_folder_inside_it_is_not() {
    let s = Setup::new();
    let (store, inside) = if cfg!(windows) {
        ("AppData/Roaming", "AppData/Roaming/npm")
    } else {
        (".config", ".config/git")
    };
    assert_eq!(s.person(&s.home.join(store)), Err(Refused::Store));
    assert!(s.person(&s.home.join(inside)).is_ok());
}

#[test]
fn what_a_task_writes_is_refused() {
    let s = Setup::new();
    let copy = s.kule.join("tasks/t1/copy");
    // Inside Kule's home too, so a copy elsewhere is what this rule is for.
    let other = TempDir::new_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let allowed = fs::canonicalize(other.path()).unwrap().join("out");
    fs::create_dir_all(allowed.join("sub")).unwrap();
    let mut bounds = s.bounds.clone();
    bounds.writable.push(allowed.clone());
    assert_eq!(
        validate(&allowed, Source::Person, &bounds),
        Err(Refused::TaskPath)
    );
    assert_eq!(
        validate(&allowed.join("sub"), Source::Person, &bounds),
        Err(Refused::TaskPath)
    );
    assert_eq!(
        validate(allowed.parent().unwrap(), Source::Person, &bounds),
        Err(Refused::TaskPath)
    );
    assert_eq!(s.person(&copy), Err(Refused::KuleHome));
}

#[test]
fn a_relative_or_missing_path_grants_nothing() {
    let s = Setup::new();
    assert_eq!(s.person(Path::new("sdk")), Err(Refused::NotAbsolute));
    assert_eq!(
        s.person(&s.home.join("Documents/none")),
        Err(Refused::NotFound)
    );
}

#[test]
fn a_link_is_judged_by_where_it_leads() {
    let s = Setup::new();
    let link = s.home.join("code/app/keys");
    #[cfg(unix)]
    std::os::unix::fs::symlink(s.home.join(".ssh"), &link).unwrap();
    #[cfg(windows)]
    std::os::windows::fs::symlink_dir(s.home.join(".ssh"), &link).unwrap();
    assert_eq!(s.person(&link), Err(Refused::Hidden));
    let fine = s.home.join("code/app/tools");
    #[cfg(unix)]
    std::os::unix::fs::symlink(s.home.join("Documents/sdk"), &fine).unwrap();
    #[cfg(windows)]
    std::os::windows::fs::symlink_dir(s.home.join("Documents/sdk"), &fine).unwrap();
    assert_eq!(s.person(&fine), Ok(s.home.join("Documents/sdk")));
}

#[test]
fn granted_roots_are_resolved_deduplicated_and_told_apart_from_the_refused() {
    let s = Setup::new();
    let (grants, refused) = granted(
        [
            s.home.join("Documents/sdk"),
            s.home.join("Documents/sdk/bin"),
            s.home.join("Documents/sdk/../sdk"),
            s.home.join(".ssh"),
            s.home.join("Documents/none"),
        ],
        Source::Person,
        &s.bounds,
    );
    let paths: Vec<&Path> = grants.iter().map(|g| g.path.as_path()).collect();
    assert_eq!(paths, vec![s.home.join("Documents/sdk").as_path()]);
    assert!(grants.iter().all(|g| g.source == Source::Person));
    assert_eq!(refused, vec![(s.home.join(".ssh"), Refused::Hidden)]);
}
