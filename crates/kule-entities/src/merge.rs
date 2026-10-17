//! Merging two changes of one file: what a task changed (`ours`) and what
//! the folder holds now (`theirs`), both from the same `base`. Lines are
//! merged three ways, as git does, with one rule more for the formats Kule
//! outlines: two changes to the same function, method, section or key
//! conflict even where their lines do not, and two changes to different
//! ones merge even where they touch (two methods added at the end of one
//! class). What merges must still parse.

use std::collections::BTreeSet;

use similar::{Algorithm, DiffOp, capture_diff_slices};

use crate::{Format, Kind, MAX_BYTES, Outline, diff};

/// Why two changes do not merge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conflict {
    /// What both changed, as a person names it (`method Task::commit`,
    /// `lines 12–14`), or why the file cannot be merged at all.
    pub what: Vec<String>,
}

impl Conflict {
    fn of(what: impl Into<String>) -> Conflict {
        Conflict {
            what: vec![what.into()],
        }
    }
}

/// One change of a side: base lines `start..end` become `lines`.
#[derive(Debug, Clone)]
struct Hunk<'a> {
    start: usize,
    end: usize,
    lines: Vec<&'a str>,
}

/// `ours` and `theirs` in one, or why not.
pub fn merge(path: &str, base: &[u8], theirs: &[u8], ours: &[u8]) -> Result<Vec<u8>, Conflict> {
    if theirs == ours || base == theirs {
        return Ok(ours.to_vec());
    }
    if base == ours {
        return Ok(theirs.to_vec());
    }
    let text = |bytes: &[u8]| -> Result<String, Conflict> {
        if bytes.len() > MAX_BYTES || bytes[..bytes.len().min(8000)].contains(&0) {
            return Err(Conflict::of("the whole file (not text Kule merges)"));
        }
        String::from_utf8(bytes.to_vec())
            .map_err(|_| Conflict::of("the whole file (not UTF-8 text)"))
    };
    let (base_text, theirs_text, ours_text) = (text(base)?, text(theirs)?, text(ours)?);

    // Both changed the same entity: a conflict, whatever the lines say.
    let format = Format::of(path);
    if format.is_some() {
        let touched = |side: &[u8]| -> Option<BTreeSet<(Kind, String)>> {
            match diff(path, Some(base), Some(side)) {
                Outline::Changes { changes } => Some(
                    changes
                        .into_iter()
                        .filter(|c| c.kind != Kind::TopLevel)
                        .map(|c| (c.kind, c.name))
                        .collect(),
                ),
                Outline::Skipped { .. } => None,
            }
        };
        if let (Some(t), Some(o)) = (touched(theirs), touched(ours)) {
            let both: Vec<String> = t
                .intersection(&o)
                .map(|(kind, name)| format!("{} {name}", kind.label()))
                .collect();
            if !both.is_empty() {
                return Err(Conflict { what: both });
            }
        }
    }

    let base_lines: Vec<&str> = base_text.split_inclusive('\n').collect();
    let theirs_lines: Vec<&str> = theirs_text.split_inclusive('\n').collect();
    let ours_lines: Vec<&str> = ours_text.split_inclusive('\n').collect();
    // Two insertions at one place are both kept only where Kule can check
    // what they are (below); in plain text, as in git, they conflict.
    let merged = merge_lines(
        &base_lines,
        &hunks(&base_lines, &theirs_lines),
        &hunks(&base_lines, &ours_lines),
        format.is_some(),
    )?;

    // Merged, every part must have changed as one side changed it: two
    // insertions at one place can move a method into the wrong type.
    if format.is_some() {
        let changes = |side: &[u8]| -> Option<BTreeSet<(Kind, String, crate::Edit)>> {
            match diff(path, Some(base), Some(side)) {
                Outline::Changes { changes } => Some(
                    changes
                        .into_iter()
                        .filter(|c| c.kind != Kind::TopLevel)
                        .map(|c| (c.kind, c.name, c.edit))
                        .collect(),
                ),
                Outline::Skipped { .. } => None,
            }
        };
        if let (Some(t), Some(o), Some(m)) =
            (changes(theirs), changes(ours), changes(merged.as_bytes()))
            && m != t.union(&o).cloned().collect()
        {
            return Err(Conflict::of(
                "the whole file (merged, its parts would not be where either side put them)",
            ));
        }
    }

    // Merged, it must still parse, when both sides did.
    if let Some(format) = format
        && format.parses(theirs)
        && format.parses(ours)
        && !format.parses(merged.as_bytes())
    {
        return Err(Conflict::of("the whole file (merged, it no longer parses)"));
    }
    Ok(merged.into_bytes())
}

/// The changes from `base` to `side`, in base lines.
fn hunks<'a>(base: &[&str], side: &[&'a str]) -> Vec<Hunk<'a>> {
    capture_diff_slices(Algorithm::Myers, base, side)
        .into_iter()
        .filter_map(|op| match op {
            DiffOp::Equal { .. } => None,
            DiffOp::Delete {
                old_index, old_len, ..
            } => Some(Hunk {
                start: old_index,
                end: old_index + old_len,
                lines: Vec::new(),
            }),
            DiffOp::Insert {
                old_index,
                new_index,
                new_len,
            } => Some(Hunk {
                start: old_index,
                end: old_index,
                lines: side[new_index..new_index + new_len].to_vec(),
            }),
            DiffOp::Replace {
                old_index,
                old_len,
                new_index,
                new_len,
            } => Some(Hunk {
                start: old_index,
                end: old_index + old_len,
                lines: side[new_index..new_index + new_len].to_vec(),
            }),
        })
        .collect()
}

/// Walks the base, taking each region one side changed from that side.
/// Where both changed the same or touching lines: the same result is taken
/// once, two insertions at one place are both taken (theirs first) when
/// `keep_both_insertions`, and anything else is a conflict.
fn merge_lines(
    base: &[&str],
    theirs: &[Hunk],
    ours: &[Hunk],
    keep_both_insertions: bool,
) -> Result<String, Conflict> {
    let mut all: Vec<(bool, &Hunk)> = theirs
        .iter()
        .map(|h| (true, h))
        .chain(ours.iter().map(|h| (false, h)))
        .collect();
    all.sort_by_key(|(_, h)| (h.start, h.end));

    let mut out = String::new();
    let mut at = 0;
    let mut conflicts = Vec::new();
    let mut i = 0;
    while i < all.len() {
        // A region: hunks that overlap or touch, from either side.
        let (start, mut end) = (all[i].1.start, all[i].1.end);
        let mut region = vec![all[i]];
        i += 1;
        while i < all.len() && all[i].1.start <= end {
            end = end.max(all[i].1.end);
            region.push(all[i]);
            i += 1;
        }
        out.extend(base[at..start].iter().copied());
        at = end;
        let side = |of_theirs: bool| -> Option<String> {
            let hunks: Vec<&Hunk> = region
                .iter()
                .filter(|(t, _)| *t == of_theirs)
                .map(|(_, h)| *h)
                .collect();
            if hunks.is_empty() {
                return None;
            }
            let mut text = String::new();
            let mut pos = start;
            for hunk in hunks {
                text.extend(base[pos..hunk.start].iter().copied());
                text.extend(hunk.lines.iter().copied());
                pos = hunk.end;
            }
            text.extend(base[pos..end].iter().copied());
            Some(text)
        };
        match (side(true), side(false)) {
            (Some(one), None) | (None, Some(one)) => out.push_str(&one),
            (Some(t), Some(o)) if t == o => out.push_str(&t),
            (Some(t), Some(o)) if start == end && keep_both_insertions => {
                out.push_str(&t);
                out.push_str(&o);
            }
            _ => conflicts.push(if end > start + 1 {
                format!("lines {}–{}", start + 1, end)
            } else {
                format!("line {}", start + 1)
            }),
        }
    }
    out.extend(base[at..].iter().copied());
    if conflicts.is_empty() {
        Ok(out)
    } else {
        Err(Conflict { what: conflicts })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn merged(path: &str, base: &str, theirs: &str, ours: &str) -> Result<String, Vec<String>> {
        merge(path, base.as_bytes(), theirs.as_bytes(), ours.as_bytes())
            .map(|b| String::from_utf8(b).unwrap())
            .map_err(|c| c.what)
    }

    const BASE: &str = "use a;\n\nimpl Task {\n    fn commit(&self) {\n        1;\n    }\n\n    fn undo(&self) {\n        2;\n    }\n}\n";

    #[test]
    fn changes_to_different_methods_merge() {
        let theirs = BASE.replace("1;", "10;");
        let ours = BASE.replace("2;", "20;");
        assert_eq!(
            merged("task.rs", BASE, &theirs, &ours).unwrap(),
            BASE.replace("1;", "10;").replace("2;", "20;")
        );
    }

    #[test]
    fn changes_to_the_same_method_conflict_even_on_different_lines() {
        let base = "impl Task {\n    fn commit(&self) {\n        a();\n        b();\n        c();\n    }\n}\n";
        let theirs = base.replace("a();", "a2();");
        let ours = base.replace("c();", "c2();");
        assert_eq!(
            merged("task.rs", base, &theirs, &ours),
            Err(vec!["method Task::commit".to_owned()])
        );
        // A plain text file has no methods: different lines merge there.
        assert!(merged("task.txt", base, &theirs, &ours).is_ok());
    }

    #[test]
    fn two_methods_added_at_the_same_place_are_both_kept() {
        let theirs = BASE.replace(
            "        2;\n    }\n}",
            "        2;\n    }\n\n    fn check(&self) {}\n}",
        );
        let ours = BASE.replace(
            "        2;\n    }\n}",
            "        2;\n    }\n\n    fn trash(&self) {}\n}",
        );
        let both = merged("task.rs", BASE, &theirs, &ours).unwrap();
        assert!(
            both.contains("fn check(&self) {}") && both.contains("fn trash(&self) {}"),
            "{both}"
        );
        assert!(both.find("fn check").unwrap() < both.find("fn trash").unwrap());
        // The same method added on both sides is a conflict.
        assert_eq!(
            merged(
                "task.rs",
                BASE,
                &theirs,
                &theirs.replace("{}\n}", "{ 1; }\n}")
            ),
            Err(vec!["method Task::check".to_owned()])
        );
    }

    #[test]
    fn the_same_lines_changed_two_ways_conflict() {
        let base = "a\nb\nc\n";
        assert_eq!(
            merged("notes.txt", base, "a\nB\nc\n", "a\nb2\nc\n"),
            Err(vec!["line 2".to_owned()])
        );
        assert_eq!(
            merged("notes.txt", base, "a\nB\nc\n", "a\nB\nc\nd\n").unwrap(),
            "a\nB\nc\nd\n"
        );
    }

    #[test]
    fn two_insertions_at_one_place_in_plain_text_conflict() {
        // Nothing tells what they are: an empty file both wrote, a line both
        // added at the end.
        assert_eq!(
            merged("busy.txt", "", "h", "agent\n"),
            Err(vec!["line 1".to_owned()])
        );
        assert!(merged("notes.txt", "a\n", "a\nb\n", "a\nc\n").is_err());
    }

    #[test]
    fn sections_and_keys_merge_by_name() {
        let base = "# Plan\nFirst.\n\n# Done\nNothing.\n";
        let theirs = base.replace("First.", "First, now.");
        let ours = base.replace("Nothing.", "Something.");
        assert_eq!(
            merged("notes.md", base, &theirs, &ours).unwrap(),
            "# Plan\nFirst, now.\n\n# Done\nSomething.\n"
        );
        let base = "[a]\nx = 1\n\n[b]\ny = 2\n";
        assert_eq!(
            merged(
                "c.toml",
                base,
                &base.replace("x = 1", "x = 3"),
                &base.replace("y = 2", "y = 4")
            )
            .unwrap(),
            "[a]\nx = 3\n\n[b]\ny = 4\n"
        );
        assert_eq!(
            merged(
                "c.toml",
                base,
                &base.replace("x = 1", "x = 3"),
                &base.replace("x = 1", "x = 5")
            ),
            Err(vec!["key a.x".to_owned()])
        );
    }

    #[test]
    fn a_merge_that_moves_a_method_into_another_type_is_refused() {
        // At the same place, one side closes the type and opens another, the
        // other adds a method: merged line by line, the method would land in
        // the new type.
        let base = "impl A {\n    fn a() {}\n}\n";
        let theirs = "impl A {\n    fn a() {}\n} // A\nimpl B {\n}\n";
        let ours = "impl A {\n    fn a() {}\n    fn m() {}\n}\n";
        let refused = merged("a.rs", base, theirs, ours).unwrap_err();
        assert!(refused[0].contains("would not be where"), "{refused:?}");
    }

    #[test]
    fn a_merge_that_no_longer_parses_is_refused() {
        // Both add a key at the end of an object: the lines merge, the
        // JSON does not.
        let base = "{\n  \"a\": 1\n}\n";
        let theirs = "{\n  \"a\": 1,\n  \"b\": 2\n}\n";
        let ours = "{\n  \"a\": 1,\n  \"c\": 3\n}\n";
        assert!(merged("x.json", base, theirs, ours).is_err());
    }

    #[test]
    fn one_side_unchanged_takes_the_other() {
        assert_eq!(merged("a.rs", "x", "x", "y").unwrap(), "y");
        assert_eq!(merged("a.rs", "x", "y", "x").unwrap(), "y");
        assert_eq!(
            merged("a.bin", "a\0", "b\0", "c\0"),
            Err(vec!["the whole file (not text Kule merges)".to_owned()])
        );
    }
}
