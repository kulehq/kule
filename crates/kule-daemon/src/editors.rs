//! The editor the person opens a task's copy in, and the editors they were
//! already told once what an editor runs by itself from a folder. Kept in
//! Kule's home.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::Result;

/// Where it is kept, under Kule's home.
pub const EDITORS_FILE: &str = "editors.json";

/// The editors Kule opens a folder in: id and name. The window finds which
/// of them the computer has.
pub const EDITORS: &[(&str, &str)] = &[("vscode", "VS Code"), ("cursor", "Cursor"), ("zed", "Zed")];

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EditorChoice {
    /// The one the task's menu opens the copy in; `None` until picked.
    #[serde(default)]
    pub chosen: Option<String>,
    /// The ones the person needs no note for any more.
    #[serde(default)]
    pub told: Vec<String>,
}

fn known(id: &str) -> bool {
    EDITORS.iter().any(|(known, _)| *known == id)
}

/// What is kept; nothing picked when nothing is.
pub fn load(home: &Path) -> Result<EditorChoice> {
    let path = home.join(EDITORS_FILE);
    match std::fs::read(&path) {
        Ok(bytes) => {
            let choice: EditorChoice =
                serde_json::from_slice(&bytes).map_err(|e| format!("{}: {e}", path.display()))?;
            check(&choice).map_err(|e| format!("{}: {e}", path.display()))?;
            Ok(choice)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(EditorChoice::default()),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

/// Keeps `choice`, whole or not at all.
pub fn save(home: &Path, choice: &EditorChoice) -> Result<EditorChoice> {
    check(choice)?;
    let path = home.join(EDITORS_FILE);
    let tmp = home.join(format!(".{EDITORS_FILE}.{}", std::process::id()));
    let bytes = serde_json::to_vec_pretty(choice).map_err(|e| e.to_string())?;
    std::fs::write(&tmp, bytes).map_err(|e| format!("{}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, &path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("{}: {e}", path.display())
    })?;
    Ok(choice.clone())
}

fn check(choice: &EditorChoice) -> Result<()> {
    for id in choice.chosen.iter().chain(&choice.told) {
        if !known(id) {
            return Err(format!("{id:?} is not an editor Kule opens folders in"));
        }
    }
    let mut told = choice.told.clone();
    told.sort();
    told.dedup();
    if told.len() != choice.told.len() {
        return Err("an editor is named twice".to_owned());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_choice_is_kept_and_only_known_editors_are() {
        let home = tempfile::TempDir::new().unwrap();
        assert_eq!(load(home.path()).unwrap(), EditorChoice::default());

        let choice = EditorChoice {
            chosen: Some("cursor".into()),
            told: vec!["cursor".into(), "vscode".into()],
        };
        assert_eq!(save(home.path(), &choice).unwrap(), choice);
        assert_eq!(load(home.path()).unwrap(), choice);

        for bad in [
            EditorChoice {
                chosen: Some("/bin/sh".into()),
                told: vec![],
            },
            EditorChoice {
                chosen: None,
                told: vec!["open -a Terminal".into()],
            },
            EditorChoice {
                chosen: None,
                told: vec!["zed".into(), "zed".into()],
            },
        ] {
            assert!(save(home.path(), &bad).is_err(), "{bad:?} was kept");
        }
        assert_eq!(load(home.path()).unwrap(), choice);

        // A file changed by hand to name something else is not believed.
        std::fs::write(home.path().join(EDITORS_FILE), r#"{"chosen":"bash"}"#).unwrap();
        assert!(load(home.path()).is_err());
    }
}
