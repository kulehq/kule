//! What a person gives their tasks to tell them apart in a list: a name, a
//! colour, and the order they come in. None of it is read by anything a
//! task runs; it lives outside the copies, in each task's `settings.json`
//! and in `<home>/order.json`.

use std::collections::HashSet;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::{Error, IoContext, Result};
use crate::fsops::{read_json, write_json};
use crate::task::{Task, TaskInfo};

/// Where the order a person put the tasks in is kept, under Kule's home.
pub const ORDER_FILE: &str = "order.json";

/// The longest name a task can have, in characters.
pub const MAX_LABEL: usize = 60;

/// The colours a task can be marked with. A fixed set: the window maps each
/// to its own shade for light and dark, and nothing else reaches a style.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TaskColor {
    Red,
    Orange,
    Yellow,
    Green,
    Teal,
    Blue,
    Purple,
    Pink,
}

impl TaskColor {
    pub const ALL: [TaskColor; 8] = [
        TaskColor::Red,
        TaskColor::Orange,
        TaskColor::Yellow,
        TaskColor::Green,
        TaskColor::Teal,
        TaskColor::Blue,
        TaskColor::Purple,
        TaskColor::Pink,
    ];

    pub fn name(self) -> &'static str {
        match self {
            TaskColor::Red => "red",
            TaskColor::Orange => "orange",
            TaskColor::Yellow => "yellow",
            TaskColor::Green => "green",
            TaskColor::Teal => "teal",
            TaskColor::Blue => "blue",
            TaskColor::Purple => "purple",
            TaskColor::Pink => "pink",
        }
    }

    pub fn parse(name: &str) -> Result<TaskColor> {
        TaskColor::ALL
            .into_iter()
            .find(|c| c.name() == name)
            .ok_or_else(|| {
                let names: Vec<&str> = TaskColor::ALL.iter().map(|c| c.name()).collect();
                Error::Invalid(format!(
                    "a task's colour is one of {}, not {name:?}",
                    names.join(", ")
                ))
            })
    }
}

/// A task's name as kept: trimmed, `None` when nothing is left. Refused
/// when longer than [`MAX_LABEL`] or with characters that can make it read
/// as something else: control characters and the ones that turn the
/// direction of text.
pub(crate) fn clean_label(label: Option<&str>) -> Result<Option<String>> {
    let Some(label) = label.map(str::trim).filter(|l| !l.is_empty()) else {
        return Ok(None);
    };
    if label.chars().count() > MAX_LABEL {
        return Err(Error::Invalid(format!(
            "a task's name is at most {MAX_LABEL} characters"
        )));
    }
    if label.chars().any(|c| c.is_control() || turns_text(c)) {
        return Err(Error::Invalid(
            "a task's name cannot hold control or text-direction characters".to_owned(),
        ));
    }
    Ok(Some(label.to_owned()))
}

/// Marks that change the direction text is shown in.
fn turns_text(c: char) -> bool {
    matches!(c, '\u{061C}' | '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}')
}

/// The order a person put the tasks in, as task ids; empty until they do.
pub fn task_order(home: &Path) -> Result<Vec<String>> {
    let path = home.join(ORDER_FILE);
    if !path.is_file() {
        return Ok(Vec::new());
    }
    read_json(&path)
}

/// Sets [`task_order`]. Every id must be a task under `home`, once.
pub fn set_task_order(home: &Path, ids: &[String]) -> Result<()> {
    let known: HashSet<String> = Task::list(home)?.into_iter().map(|i| i.id).collect();
    let mut seen = HashSet::new();
    for id in ids {
        if !known.contains(id) {
            return Err(Error::Invalid(format!("no task {id} to put in order")));
        }
        if !seen.insert(id) {
            return Err(Error::Invalid(format!("task {id} is in the order twice")));
        }
    }
    std::fs::create_dir_all(home).at(home)?;
    write_json(&home.join(ORDER_FILE), &ids)
}

/// The tasks under `home` as a list shows them: those the person has not
/// put in order yet first, newest first, then the rest in their order.
pub fn arranged(home: &Path) -> Result<Vec<TaskInfo>> {
    let order = task_order(home)?;
    let mut infos = Task::list(home)?;
    let at = |info: &TaskInfo| order.iter().position(|id| *id == info.id);
    infos.sort_by(|a, b| match (at(a), at(b)) {
        (Some(x), Some(y)) => x.cmp(&y),
        (None, Some(_)) => std::cmp::Ordering::Less,
        (Some(_), None) => std::cmp::Ordering::Greater,
        (None, None) => (b.created_unix, &b.id).cmp(&(a.created_unix, &a.id)),
    });
    Ok(infos)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_is_trimmed_and_kept_plain() {
        assert_eq!(clean_label(None).unwrap(), None);
        assert_eq!(clean_label(Some("   ")).unwrap(), None);
        assert_eq!(
            clean_label(Some("  invoices ")).unwrap().as_deref(),
            Some("invoices")
        );
        assert_eq!(
            clean_label(Some(&"é".repeat(MAX_LABEL)))
                .unwrap()
                .unwrap()
                .chars()
                .count(),
            MAX_LABEL
        );
        assert!(clean_label(Some(&"a".repeat(MAX_LABEL + 1))).is_err());
        assert!(clean_label(Some("a\nb")).is_err());
        assert!(clean_label(Some("abc\u{202E}gpj.exe")).is_err());
        assert!(clean_label(Some("x\u{2066}y")).is_err());
    }

    #[test]
    fn a_colour_is_one_of_the_set() {
        for c in TaskColor::ALL {
            assert_eq!(TaskColor::parse(c.name()).unwrap(), c);
            let json = serde_json::to_string(&c).unwrap();
            assert_eq!(json, format!("\"{}\"", c.name()));
        }
        assert!(TaskColor::parse("#ff0000").is_err());
        assert!(serde_json::from_str::<TaskColor>("\"url(x)\"").is_err());
    }
}
