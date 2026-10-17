//! Where two tasks on the same workspace meet: the files both change and,
//! inside them, the functions, sections or keys both change. Each task
//! works in a copy of its own, so they never see each other's work; this
//! tells a person while both are still going, not when the second commit
//! is refused.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::effects::EffectSet;
use crate::entities::Outline;
use crate::manifest::Kind;
use crate::outline::FileOutline;

/// What a task changes, file by file: the entities it changes there (as
/// `method Task::commit`), or `None` for a file Kule cannot see inside.
pub type Touched = BTreeMap<String, Option<BTreeSet<String>>>;

/// One file two tasks both change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Meeting {
    pub path: String,
    /// What both change in it; empty when Kule cannot tell for one of them,
    /// and then the file as a whole is what they meet on.
    pub entities: Vec<String>,
}

/// The entities a change set touches, from its outline. A file without an
/// outline (not a format Kule reads, or not readable now) counts whole.
pub fn touched(set: &EffectSet, outlines: &[FileOutline]) -> Touched {
    set.files()
        .filter(|e| e.kind != Kind::Dir)
        .map(|effect| {
            let inside = outlines
                .iter()
                .find(|o| o.path == effect.path)
                .and_then(|o| changed_entities(&o.outline));
            (effect.path.clone(), inside)
        })
        .collect()
}

/// The entities of one outline, or `None` when it has none.
pub fn changed_entities(outline: &Outline) -> Option<BTreeSet<String>> {
    match outline {
        Outline::Changes { changes } => Some(
            changes
                .iter()
                .map(|c| match c.name.as_str() {
                    "" => c.kind.label().to_owned(),
                    name => format!("{} {name}", c.kind.label()),
                })
                .collect(),
        ),
        Outline::Skipped { .. } => None,
    }
}

/// Where `a` and `b` meet.
pub fn meet(a: &Touched, b: &Touched) -> Vec<Meeting> {
    a.iter()
        .filter_map(|(path, ours)| {
            let theirs = b.get(path)?;
            let entities = match (ours, theirs) {
                (Some(ours), Some(theirs)) => {
                    let both: Vec<String> = ours.intersection(theirs).cloned().collect();
                    if both.is_empty() {
                        return None;
                    }
                    both
                }
                _ => Vec::new(),
            };
            Some(Meeting {
                path: path.clone(),
                entities,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn touched(files: &[(&str, Option<&[&str]>)]) -> Touched {
        files
            .iter()
            .map(|(path, inside)| {
                let inside = inside.map(|names| names.iter().map(|n| n.to_string()).collect());
                (path.to_string(), inside)
            })
            .collect()
    }

    #[test]
    fn tasks_meet_on_what_both_change() {
        let a = touched(&[
            (
                "task.rs",
                Some(&["method Task::commit", "method Task::undo"]),
            ),
            ("lib.rs", Some(&["function a"])),
            ("logo.png", None),
            ("only_a.md", Some(&["section A"])),
        ]);
        let b = touched(&[
            (
                "task.rs",
                Some(&["method Task::commit", "method Task::check"]),
            ),
            ("lib.rs", Some(&["function b"])),
            ("logo.png", None),
        ]);
        assert_eq!(
            meet(&a, &b),
            vec![
                Meeting {
                    path: "logo.png".into(),
                    entities: vec![]
                },
                Meeting {
                    path: "task.rs".into(),
                    entities: vec!["method Task::commit".into()]
                },
            ]
        );
    }

    #[test]
    fn a_file_one_of_them_cannot_be_seen_inside_meets_whole() {
        let a = touched(&[("data.bin", None)]);
        let b = touched(&[("data.bin", Some(&["key x"]))]);
        assert_eq!(meet(&a, &b)[0].entities, Vec::<String>::new());
    }
}
