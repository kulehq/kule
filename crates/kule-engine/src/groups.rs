//! Groups a person puts tasks in: a name, a colour, an order, and the
//! policy their tasks' commits and connections are judged by. A group's
//! policy is the one chosen for every task (`default`), one of Kule's two,
//! or one the person wrote for the group. All of it lives in Kule's home,
//! which no task's commands can read or write: `groups.json`, and a group's
//! own policy in `group-policies/<id>.cedar`.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Error, IoContext, Result};
use crate::fsops::{read_json, write_json};
use crate::labels::{self, TaskColor};
use crate::policy::{CAREFUL_POLICY, DEFAULT_POLICY, Policy};
use crate::task::Task;

/// Where the groups are kept, under Kule's home.
pub const GROUPS_FILE: &str = "groups.json";

/// Where a group's own policy is kept, under Kule's home.
pub const GROUP_POLICY_DIR: &str = "group-policies";

/// More groups than a list holds well.
pub const MAX_GROUPS: usize = 100;

/// The largest policy a group can have, in bytes.
pub const MAX_GROUP_POLICY: usize = 256 * 1024;

/// The policy a group's tasks are judged by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GroupPolicy {
    /// The one that holds for every task: Settings' choice, or the
    /// person's own `policy.cedar`.
    #[default]
    Default,
    /// Kule's balanced policy, whatever holds for the rest.
    Balanced,
    /// Kule's careful policy, whatever holds for the rest.
    Careful,
    /// The policy the person wrote for the group.
    Own,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Group {
    /// Never given to another group, after this one is deleted too.
    pub id: u64,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<TaskColor>,
    #[serde(default)]
    pub policy: GroupPolicy,
    /// Folded in the list, its tasks out of sight.
    #[serde(default)]
    pub collapsed: bool,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Groups {
    /// The id the next group gets.
    #[serde(default)]
    next: u64,
    /// In the order the list shows them.
    #[serde(default)]
    groups: Vec<Group>,
}

fn load(home: &Path) -> Result<Groups> {
    let path = home.join(GROUPS_FILE);
    if !path.is_file() {
        return Ok(Groups::default());
    }
    read_json(&path)
}

fn save(home: &Path, groups: &Groups) -> Result<()> {
    std::fs::create_dir_all(home).at(home)?;
    write_json(&home.join(GROUPS_FILE), groups)
}

fn policy_path(home: &Path, id: u64) -> PathBuf {
    home.join(GROUP_POLICY_DIR).join(format!("{id}.cedar"))
}

fn clean_name(name: &str) -> Result<String> {
    labels::clean_name("group", Some(name))?
        .ok_or_else(|| Error::Invalid("a group needs a name".to_owned()))
}

fn find(groups: &mut Groups, id: u64) -> Result<&mut Group> {
    groups
        .groups
        .iter_mut()
        .find(|g| g.id == id)
        .ok_or_else(|| Error::Invalid(format!("no group {id}")))
}

/// The groups under `home`, in the person's order.
pub fn groups(home: &Path) -> Result<Vec<Group>> {
    Ok(load(home)?.groups)
}

/// A new group, last in the order, with the policy that holds for every
/// task.
pub fn create_group(home: &Path, name: &str, color: Option<TaskColor>) -> Result<Group> {
    let name = clean_name(name)?;
    let mut groups = load(home)?;
    if groups.groups.len() >= MAX_GROUPS {
        return Err(Error::Invalid(format!(
            "at most {MAX_GROUPS} groups; delete one first"
        )));
    }
    groups.next += 1;
    let group = Group {
        id: groups.next,
        name,
        color,
        policy: GroupPolicy::Default,
        collapsed: false,
    };
    groups.groups.push(group.clone());
    save(home, &groups)?;
    Ok(group)
}

/// Renames, recolours, folds or unfolds group `id`.
pub fn update_group(
    home: &Path,
    id: u64,
    name: &str,
    color: Option<TaskColor>,
    collapsed: bool,
) -> Result<Group> {
    let name = clean_name(name)?;
    let mut groups = load(home)?;
    let group = find(&mut groups, id)?;
    group.name = name;
    group.color = color;
    group.collapsed = collapsed;
    let group = group.clone();
    save(home, &groups)?;
    Ok(group)
}

/// Sets the policy group `id`'s tasks are judged by. For
/// [`GroupPolicy::Own`], `text` is the policy, checked first; without one,
/// the one written for the group before holds again. A group's own policy
/// is kept when another is chosen, to come back to.
pub fn set_group_policy(
    home: &Path,
    id: u64,
    policy: GroupPolicy,
    text: Option<&str>,
) -> Result<Group> {
    let mut groups = load(home)?;
    find(&mut groups, id)?;
    let path = policy_path(home, id);
    if let Some(text) = text {
        if policy != GroupPolicy::Own {
            return Err(Error::Invalid(
                "a policy's text goes with a policy of the group's own".to_owned(),
            ));
        }
        if text.len() > MAX_GROUP_POLICY {
            return Err(Error::Invalid(format!(
                "a group's policy is at most {} KiB",
                MAX_GROUP_POLICY / 1024
            )));
        }
        Policy::from_texts(text, None)?;
        let dir = home.join(GROUP_POLICY_DIR);
        std::fs::create_dir_all(&dir).at(&dir)?;
        let tmp = dir.join(format!(".{id}.cedar.tmp"));
        std::fs::write(&tmp, text).at(&tmp)?;
        std::fs::rename(&tmp, &path).at(&path)?;
    } else if policy == GroupPolicy::Own && !path.is_file() {
        return Err(Error::Invalid("write the group's policy first".to_owned()));
    }
    let group = find(&mut groups, id)?;
    group.policy = policy;
    let group = group.clone();
    save(home, &groups)?;
    Ok(group)
}

/// The policy written for group `id`, if one was.
pub fn group_policy_text(home: &Path, id: u64) -> Result<Option<String>> {
    find(&mut load(home)?, id)?;
    let path = policy_path(home, id);
    match std::fs::read_to_string(&path) {
        Ok(text) => Ok(Some(text)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(Error::Io { path, source }),
    }
}

/// Puts the groups in the person's order: every group, once.
pub fn set_group_order(home: &Path, ids: &[u64]) -> Result<()> {
    let mut groups = load(home)?;
    let known: HashSet<u64> = groups.groups.iter().map(|g| g.id).collect();
    let given: HashSet<u64> = ids.iter().copied().collect();
    if given.len() != ids.len() || given != known {
        return Err(Error::Invalid(
            "the order names every group, once".to_owned(),
        ));
    }
    groups
        .groups
        .sort_by_key(|g| ids.iter().position(|id| *id == g.id));
    save(home, &groups)
}

/// Deletes group `id` and the policy written for it. Its tasks stay, in
/// no group, under the policy that holds for every task.
pub fn delete_group(home: &Path, id: u64) -> Result<()> {
    let mut groups = load(home)?;
    find(&mut groups, id)?;
    groups.groups.retain(|g| g.id != id);
    save(home, &groups)?;
    for info in Task::list(home)? {
        let task = Task::open(home, &info.id)?;
        if task.group()? == Some(id) {
            task.set_group(None)?;
        }
    }
    let path = policy_path(home, id);
    match std::fs::remove_file(&path) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(Error::Io { path, source: e }),
        _ => Ok(()),
    }
}

/// Whether group `id` is one under `home`.
pub(crate) fn exists(home: &Path, id: u64) -> Result<bool> {
    Ok(load(home)?.groups.iter().any(|g| g.id == id))
}

/// The policy text `task`'s group puts in place of the one for every task;
/// `None` when it has no group, or its group keeps that one. A group's own
/// policy that is gone is an error, never a quiet fall back to another.
pub(crate) fn base_policy(home: &Path, task: &Task) -> Result<Option<String>> {
    let Some(id) = task.group()? else {
        return Ok(None);
    };
    let Some(group) = load(home)?.groups.into_iter().find(|g| g.id == id) else {
        return Ok(None);
    };
    Ok(match group.policy {
        GroupPolicy::Default => None,
        GroupPolicy::Balanced => Some(DEFAULT_POLICY.to_owned()),
        GroupPolicy::Careful => Some(CAREFUL_POLICY.to_owned()),
        GroupPolicy::Own => {
            let path = policy_path(home, id);
            Some(std::fs::read_to_string(&path).map_err(|source| {
                Error::Invalid(format!(
                    "group {:?} judges by its own policy, and {} cannot be read: {source}",
                    group.name,
                    path.display()
                ))
            })?)
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groups_are_named_coloured_and_kept_in_order() {
        let home = tempfile::TempDir::new().unwrap();
        let home = home.path();
        assert!(groups(home).unwrap().is_empty());
        let a = create_group(home, "  clients ", Some(TaskColor::Teal)).unwrap();
        let b = create_group(home, "notes", None).unwrap();
        assert_eq!(
            (a.id, a.name.as_str(), a.policy),
            (1, "clients", GroupPolicy::Default)
        );
        assert!(create_group(home, "  ", None).is_err());
        assert!(create_group(home, "a\u{202E}b", None).is_err());

        update_group(home, a.id, "work", None, true).unwrap();
        set_group_order(home, &[b.id, a.id]).unwrap();
        let kept = groups(home).unwrap();
        assert_eq!(kept[0].id, b.id);
        assert_eq!(
            (kept[1].name.as_str(), kept[1].color, kept[1].collapsed),
            ("work", None, true)
        );
        // Every group, once.
        assert!(set_group_order(home, &[a.id]).is_err());
        assert!(set_group_order(home, &[a.id, a.id]).is_err());
        assert!(set_group_order(home, &[a.id, b.id, 9]).is_err());

        // A deleted group's id is never handed out again.
        delete_group(home, a.id).unwrap();
        assert_eq!(create_group(home, "later", None).unwrap().id, 3);
        assert!(update_group(home, a.id, "x", None, false).is_err());
    }

    #[test]
    fn a_groups_own_policy_is_checked_and_kept() {
        let home = tempfile::TempDir::new().unwrap();
        let home = home.path();
        let g = create_group(home, "strict", None).unwrap();
        // Nothing written yet.
        assert!(set_group_policy(home, g.id, GroupPolicy::Own, None).is_err());
        assert!(set_group_policy(home, g.id, GroupPolicy::Own, Some("permit (")).is_err());
        assert!(set_group_policy(home, g.id, GroupPolicy::Careful, Some(DEFAULT_POLICY)).is_err());
        let big = format!("// {}\n", "x".repeat(MAX_GROUP_POLICY));
        assert!(set_group_policy(home, g.id, GroupPolicy::Own, Some(&big)).is_err());
        assert_eq!(group_policy_text(home, g.id).unwrap(), None);
        assert_eq!(groups(home).unwrap()[0].policy, GroupPolicy::Default);

        set_group_policy(home, g.id, GroupPolicy::Own, Some(CAREFUL_POLICY)).unwrap();
        // Another choice keeps the text to come back to.
        set_group_policy(home, g.id, GroupPolicy::Balanced, None).unwrap();
        assert_eq!(
            group_policy_text(home, g.id).unwrap().as_deref(),
            Some(CAREFUL_POLICY)
        );
        set_group_policy(home, g.id, GroupPolicy::Own, None).unwrap();
        assert_eq!(groups(home).unwrap()[0].policy, GroupPolicy::Own);

        delete_group(home, g.id).unwrap();
        assert!(!policy_path(home, g.id).exists());
        assert!(group_policy_text(home, g.id).is_err());
    }
}
