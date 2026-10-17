//! The keyboard shortcuts a person chose in the app, kept in Kule's home so
//! they are there the next time the window opens. Only the person's own
//! choices are kept: a command they never changed keeps the app's default.
//!
//! The daemon does not know the app's commands; it keeps what the window
//! sends as long as it looks like a command id and a key combination.

use std::collections::BTreeMap;
use std::path::Path;

use crate::Result;

/// Where they are kept, under Kule's home.
pub const SHORTCUTS_FILE: &str = "shortcuts.json";

/// More than the app has commands, many times over.
const MAX_ENTRIES: usize = 512;
const MAX_LEN: usize = 64;

/// Command id → key combination (`Mod+Shift+K`); an empty one takes the
/// command's default away.
pub type Shortcuts = BTreeMap<String, String>;

/// What is kept; nothing when nothing was ever changed. A file that cannot
/// be read is reported rather than taken as empty, so a save never writes
/// over choices that are still there.
pub fn load(home: &Path) -> Result<Shortcuts> {
    let path = home.join(SHORTCUTS_FILE);
    match std::fs::read(&path) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(|e| format!("{}: {e}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Shortcuts::new()),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

/// Keeps `shortcuts` in place of what was there, whole or not at all.
pub fn save(home: &Path, shortcuts: &Shortcuts) -> Result<()> {
    check(shortcuts)?;
    let path = home.join(SHORTCUTS_FILE);
    let tmp = home.join(format!(".{SHORTCUTS_FILE}.{}", std::process::id()));
    let bytes = serde_json::to_vec_pretty(shortcuts).map_err(|e| e.to_string())?;
    std::fs::write(&tmp, bytes).map_err(|e| format!("{}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, &path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("{}: {e}", path.display())
    })
}

fn check(shortcuts: &Shortcuts) -> Result<()> {
    if shortcuts.len() > MAX_ENTRIES {
        return Err(format!("at most {MAX_ENTRIES} shortcuts"));
    }
    for (id, keys) in shortcuts {
        let id_ok = !id.is_empty()
            && id.len() <= MAX_LEN
            && id
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '.' || c == '-');
        if !id_ok {
            return Err(format!("{id:?} is not a command"));
        }
        let keys_ok = keys.chars().count() <= MAX_LEN && !keys.chars().any(char::is_control);
        if !keys_ok {
            return Err(format!("{id}: {keys:?} is not a key combination"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn choices_are_kept_and_read_back_whole() {
        let home = tempfile::TempDir::new().unwrap();
        assert_eq!(load(home.path()).unwrap(), Shortcuts::new());

        let mut chosen = Shortcuts::new();
        chosen.insert("task.new".into(), "Mod+Shift+N".into());
        chosen.insert("task.terminal".into(), String::new());
        save(home.path(), &chosen).unwrap();
        assert_eq!(load(home.path()).unwrap(), chosen);

        // What does not look like a command or a key combination is
        // refused, and what was kept stays.
        for (id, keys) in [
            ("Task.New", "Mod+N"),
            ("task/../x", "Mod+N"),
            ("task.new", "Mod+\u{1b}"),
            ("task.new", &"K".repeat(65)),
        ] {
            let mut bad = chosen.clone();
            bad.insert(id.into(), keys.into());
            assert!(save(home.path(), &bad).is_err(), "{id} {keys}");
        }
        let too_many: Shortcuts = (0..=MAX_ENTRIES)
            .map(|i| (format!("c{i}"), "Mod+K".to_owned()))
            .collect();
        assert!(save(home.path(), &too_many).is_err());
        assert_eq!(load(home.path()).unwrap(), chosen);

        // A file that cannot be read is an error, not an empty set.
        std::fs::write(home.path().join(SHORTCUTS_FILE), "{ not json").unwrap();
        assert!(load(home.path()).is_err());
    }
}
