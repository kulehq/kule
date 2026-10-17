//! What changed inside a file, named the way a person names it: functions,
//! methods, fields and types in code, sections in Markdown, keys in JSON and
//! TOML. A review that says "`Task::commit` changed, `Task::trash_dir` was
//! added" reads faster than one that says "task.rs changed", and two tasks
//! that touch the same file can be told apart from two that touch the same
//! method.
//!
//! Every byte of a file belongs to exactly one entity: the innermost one
//! around it, or the file's top level. An entity changed when its own bytes
//! did (its text with the entities inside it left out), so nothing a file
//! changed goes unlisted.
//!
//! The files are what an agent wrote, read outside any sandbox, so every
//! read is bounded: in size, in parse time, in the number of entities.

mod code;
mod data;
mod markdown;

use std::collections::{BTreeMap, HashSet};
use std::time::Duration;

use serde::{Deserialize, Serialize};

/// Larger files are not outlined.
pub const MAX_BYTES: usize = 1 << 20;
/// More entities than this on one side and the file is not outlined.
const MAX_ENTITIES: usize = 20_000;
/// What parsing one side of a file may take.
const PARSE_TIME: Duration = Duration::from_millis(500);
/// Names are cut here.
const MAX_NAME: usize = 120;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// What lies outside every other entity: imports, top-level statements.
    TopLevel,
    Function,
    Method,
    Constructor,
    Class,
    Struct,
    Record,
    Enum,
    Variant,
    Trait,
    Interface,
    Impl,
    Module,
    Namespace,
    Field,
    Property,
    Event,
    Indexer,
    Constant,
    Variable,
    Type,
    Macro,
    /// A Markdown heading and what follows it, up to the next heading of
    /// the same level or above.
    Section,
    /// A key in JSON or TOML, with its value.
    Key,
}

impl Kind {
    /// The word a person reads for it.
    pub fn label(self) -> &'static str {
        match self {
            Kind::TopLevel => "top level",
            Kind::Function => "function",
            Kind::Method => "method",
            Kind::Constructor => "constructor",
            Kind::Class => "class",
            Kind::Struct => "struct",
            Kind::Record => "record",
            Kind::Enum => "enum",
            Kind::Variant => "variant",
            Kind::Trait => "trait",
            Kind::Interface => "interface",
            Kind::Impl => "impl",
            Kind::Module => "module",
            Kind::Namespace => "namespace",
            Kind::Field => "field",
            Kind::Property => "property",
            Kind::Event => "event",
            Kind::Indexer => "indexer",
            Kind::Constant => "constant",
            Kind::Variable => "variable",
            Kind::Type => "type",
            Kind::Macro => "macro",
            Kind::Section => "section",
            Kind::Key => "key",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Edit {
    Added,
    Removed,
    Modified,
}

/// One entity that changed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Change {
    pub kind: Kind,
    /// Qualified the way the format does: `Task::commit` in Rust,
    /// `Task.Commit` in Go and C#, `Usage › Install` in Markdown,
    /// `dependencies.serde` in TOML. Empty for the top level.
    pub name: String,
    pub edit: Edit,
    /// Where it starts, 1-based: in the new file, or in the old one when it
    /// was removed. Unknown for keys.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
}

/// Why a file has no outline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Skip {
    /// Not a format Kule outlines.
    Unsupported,
    /// Over [`MAX_BYTES`].
    TooLarge,
    /// Holds NUL bytes: not text.
    Binary,
    /// Could not be parsed in time, or not at all.
    Unparsed,
    /// Holds more entities than Kule lists.
    TooMany,
    /// What the file held before is no longer known: the workspace's copy
    /// changed since the task began.
    BeforeUnknown,
}

/// What changed inside one file, or why that is not known.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outline", rename_all = "snake_case")]
pub enum Outline {
    Changes { changes: Vec<Change> },
    Skipped { reason: Skip },
}

/// Whether files at `path` are outlined at all, by their name.
pub fn supported(path: &str) -> bool {
    Format::of(path).is_some()
}

/// What changed in the file at `path` between `before` and `after`; `None`
/// for a side where the file does not exist.
pub fn diff(path: &str, before: Option<&[u8]>, after: Option<&[u8]>) -> Outline {
    let Some(format) = Format::of(path) else {
        return Outline::Skipped {
            reason: Skip::Unsupported,
        };
    };
    let side = |bytes: Option<&[u8]>| match bytes {
        Some(bytes) => format.entities(bytes),
        None => Ok(Vec::new()),
    };
    match (side(before), side(after)) {
        (Ok(before), Ok(after)) => Outline::Changes {
            changes: compare(&before, &after),
        },
        (Err(reason), _) | (_, Err(reason)) => Outline::Skipped { reason },
    }
}

#[derive(Debug, Clone, Copy)]
enum Format {
    Code(code::Lang),
    Markdown,
    Json,
    Toml,
}

impl Format {
    fn of(path: &str) -> Option<Format> {
        let name = path.rsplit('/').next().unwrap_or(path);
        let ext = name.rsplit_once('.')?.1.to_ascii_lowercase();
        Some(match ext.as_str() {
            "md" | "markdown" => Format::Markdown,
            "json" => Format::Json,
            "toml" => Format::Toml,
            _ => Format::Code(code::Lang::of(&ext)?),
        })
    }

    fn entities(self, bytes: &[u8]) -> Result<Vec<Entity>, Skip> {
        if bytes.len() > MAX_BYTES {
            return Err(Skip::TooLarge);
        }
        if bytes[..bytes.len().min(8000)].contains(&0) {
            return Err(Skip::Binary);
        }
        let entities = match self {
            Format::Code(lang) => code::entities(lang, bytes)?,
            Format::Markdown => markdown::entities(bytes)?,
            Format::Json => data::json(bytes)?,
            Format::Toml => data::toml(bytes)?,
        };
        if entities.len() > MAX_ENTITIES {
            return Err(Skip::TooMany);
        }
        Ok(entities)
    }
}

/// One entity on one side of a diff. The first of a side's entities is
/// always its top level, and every other one has a parent before it.
#[derive(Debug, Clone)]
struct Entity {
    kind: Kind,
    name: String,
    parent: Option<usize>,
    line: Option<u32>,
    /// Its bytes but for the entities inside it.
    own: Vec<u8>,
}

impl Entity {
    fn top(own: Vec<u8>) -> Entity {
        Entity {
            kind: Kind::TopLevel,
            name: String::new(),
            parent: None,
            line: None,
            own,
        }
    }
}

/// An entity's place in a file, before its own bytes are cut out.
struct Spanned {
    kind: Kind,
    name: String,
    parent: Option<usize>,
    start: usize,
    end: usize,
    line: u32,
}

/// Cuts each entity's own bytes out of `src`: its span without the spans of
/// the entities directly inside it, each taken with the comma or semicolon
/// and the spaces after it, and without blank lines. So adding a method or
/// a field is a change of that method or field, not of the type around it,
/// while every other byte (a blank inside a string, indentation) still
/// counts. `spanned[0]` is the top level and spans the whole file.
fn own_bytes(src: &[u8], spanned: Vec<Spanned>) -> Vec<Entity> {
    let mut children: Vec<Vec<(usize, usize)>> = vec![Vec::new(); spanned.len()];
    for entity in &spanned {
        if let Some(parent) = entity.parent {
            children[parent].push((entity.start, entity.end));
        }
    }
    spanned
        .into_iter()
        .zip(children)
        .map(|(entity, mut inside)| {
            inside.sort_unstable();
            let mut own = Vec::new();
            let mut at = entity.start;
            let blank = |i: usize| src[i].is_ascii_whitespace();
            for (start, end) in inside {
                let start = start.max(at);
                let mut end = end.max(start).min(entity.end);
                let mut next = end;
                while next < entity.end && blank(next) {
                    next += 1;
                }
                if next < entity.end && matches!(src[next], b',' | b';') {
                    end = next + 1;
                }
                // And the spaces after it on its line, where the next one
                // starts or the line ends.
                while end < entity.end && matches!(src[end], b' ' | b'\t') {
                    end += 1;
                }
                own.extend_from_slice(&src[at..start]);
                at = end;
            }
            own.extend_from_slice(&src[at..entity.end.max(at)]);
            let mut own = without_blank_lines(&own);
            // Blanks at the end come and go with what follows.
            while own.last().is_some_and(u8::is_ascii_whitespace) {
                own.pop();
            }
            Entity {
                kind: entity.kind,
                line: (entity.kind != Kind::TopLevel).then_some(entity.line),
                name: entity.name,
                parent: entity.parent,
                own,
            }
        })
        .collect()
}

/// `text` with every line that holds nothing but blanks taken out.
fn without_blank_lines(text: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len());
    for line in text.split_inclusive(|&b| b == b'\n') {
        if !(line.ends_with(b"\n") && line.iter().all(u8::is_ascii_whitespace)) {
            out.extend_from_slice(line);
        }
    }
    out
}

/// A name as one line, cut to [`MAX_NAME`].
fn tidy(name: &str) -> String {
    let name = name.split_whitespace().collect::<Vec<_>>().join(" ");
    match name.char_indices().nth(MAX_NAME) {
        Some((cut, _)) => format!("{}…", &name[..cut]),
        None => name,
    }
}

/// Pairs the entities of both sides by kind and name and lists what
/// differs. Entities that share a kind and a name (overloads, a type's
/// several `impl` blocks) pair those identical with what they hold first,
/// then those identical on their own, then in order. An entity added or
/// removed with the one around it is not listed apart.
fn compare(before: &[Entity], after: &[Entity]) -> Vec<Change> {
    let (old_groups, new_groups) = (groups(before), groups(after));
    let (old_held, new_held) = (held(before), held(after));
    let mut pairs: Vec<(usize, usize)> = Vec::new();
    for (key, new) in &new_groups {
        let Some(old) = old_groups.get(key) else {
            continue;
        };
        let mut old_left: Vec<usize> = old.clone();
        let mut new_left: Vec<usize> = new.clone();
        let same_whole =
            |o: usize, n: usize| before[o].own == after[n].own && old_held[o] == new_held[n];
        let same_own = |o: usize, n: usize| before[o].own == after[n].own;
        for same in [&same_whole as &dyn Fn(usize, usize) -> bool, &same_own] {
            new_left.retain(|&n| match old_left.iter().position(|&o| same(o, n)) {
                Some(at) => {
                    pairs.push((old_left.remove(at), n));
                    false
                }
                None => true,
            });
        }
        pairs.extend(old_left.into_iter().zip(new_left));
    }
    let old_paired: HashSet<usize> = pairs.iter().map(|&(o, _)| o).collect();
    let new_paired: HashSet<usize> = pairs.iter().map(|&(_, n)| n).collect();

    let mut changes: Vec<(u32, Change)> = Vec::new();
    let listed = |entity: &Entity, paired: &HashSet<usize>| {
        // Inside an entity that came or went with it: that one is listed.
        entity.kind != Kind::TopLevel && entity.parent.is_none_or(|p| p == 0 || paired.contains(&p))
    };
    let change = |entity: &Entity, edit| Change {
        kind: entity.kind,
        name: entity.name.clone(),
        edit,
        line: entity.line,
    };
    for (n, entity) in after.iter().enumerate() {
        if !new_paired.contains(&n) && listed(entity, &new_paired) {
            changes.push((entity.line.unwrap_or(0), change(entity, Edit::Added)));
        }
    }
    for (o, entity) in before.iter().enumerate() {
        if !old_paired.contains(&o) && listed(entity, &old_paired) {
            changes.push((entity.line.unwrap_or(0), change(entity, Edit::Removed)));
        }
    }
    for &(o, n) in &pairs {
        if before[o].own != after[n].own {
            changes.push((
                after[n].line.unwrap_or(0),
                change(&after[n], Edit::Modified),
            ));
        }
    }
    changes.sort_by(|a, b| (a.0, &a.1.name).cmp(&(b.0, &b.1.name)));
    changes.into_iter().map(|(_, change)| change).collect()
}

/// The names of what each entity directly holds, sorted.
fn held(side: &[Entity]) -> Vec<Vec<&str>> {
    let mut held: Vec<Vec<&str>> = vec![Vec::new(); side.len()];
    for entity in side {
        if let Some(parent) = entity.parent {
            held[parent].push(entity.name.as_str());
        }
    }
    for names in &mut held {
        names.sort_unstable();
    }
    held
}

/// A side's entities by kind and name, each group in file order.
fn groups(side: &[Entity]) -> BTreeMap<(Kind, &str), Vec<usize>> {
    let mut groups: BTreeMap<(Kind, &str), Vec<usize>> = BTreeMap::new();
    for (i, entity) in side.iter().enumerate() {
        groups
            .entry((entity.kind, entity.name.as_str()))
            .or_default()
            .push(i);
    }
    groups
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) fn changes(path: &str, before: &str, after: &str) -> Vec<(Edit, Kind, String)> {
        let side = |s: &str| (!s.is_empty()).then_some(s.as_bytes().to_vec());
        let (before, after) = (side(before), side(after));
        match diff(path, before.as_deref(), after.as_deref()) {
            Outline::Changes { changes } => changes
                .into_iter()
                .map(|c| (c.edit, c.kind, c.name))
                .collect(),
            Outline::Skipped { reason } => panic!("{path} was skipped: {reason:?}"),
        }
    }

    #[test]
    fn unknown_formats_are_skipped() {
        assert_eq!(
            diff("photo.jpg", None, Some(b"x")),
            Outline::Skipped {
                reason: Skip::Unsupported
            }
        );
        assert!(!supported("Makefile"));
        assert!(supported("src/lib.rs"));
    }

    #[test]
    fn large_and_binary_files_are_skipped() {
        let large = vec![b'a'; MAX_BYTES + 1];
        assert_eq!(
            diff("a.rs", None, Some(&large)),
            Outline::Skipped {
                reason: Skip::TooLarge
            }
        );
        assert_eq!(
            diff("a.rs", None, Some(b"fn a() {}\0")),
            Outline::Skipped {
                reason: Skip::Binary
            }
        );
    }

    #[test]
    fn too_many_entities_are_skipped() {
        let many: String = (0..MAX_ENTITIES + 1)
            .map(|i| format!("fn f{i}() {{}}\n"))
            .collect();
        assert_eq!(
            diff("a.rs", None, Some(many.as_bytes())),
            Outline::Skipped {
                reason: Skip::TooMany
            }
        );
    }

    #[test]
    fn names_are_one_line_and_cut() {
        assert_eq!(tidy("a\n   b"), "a b");
        let long = "x".repeat(MAX_NAME + 5);
        assert_eq!(tidy(&long).chars().count(), MAX_NAME + 1);
    }
}
