//! What changed in a text file, line by line, for a person to read before
//! taking it somewhere it matters.

use serde::{Deserialize, Serialize};
use similar::{ChangeTag, TextDiff as Lines};

/// A side larger than this is not compared.
const MAX_SIDE: usize = 1024 * 1024;
/// The most lines a diff shows; the rest is cut and said so.
const MAX_LINES: usize = 2_000;
/// Unchanged lines shown around each change.
const CONTEXT: usize = 3;

/// What changed between two versions of a file, or why that is not shown.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "diff", rename_all = "snake_case")]
pub enum TextDiff {
    Lines {
        hunks: Vec<Hunk>,
        /// Whether lines past [`MAX_LINES`] were left out.
        cut: bool,
    },
    Skipped {
        reason: DiffSkip,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiffSkip {
    /// Either side is not UTF-8 text.
    Binary,
    TooLarge,
}

/// A run of changed lines with the unchanged ones around them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hunk {
    /// Where the hunk starts, 1-based, before and after.
    pub before: usize,
    pub after: usize,
    pub lines: Vec<Line>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Line {
    pub kind: LineKind,
    /// The line without its line ending.
    pub text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LineKind {
    Same,
    Added,
    Removed,
}

/// The file at `path` as one side of a diff: `None` unless it is a regular
/// file (a link is not followed), and only as much as a side may hold.
pub(crate) fn side(path: &std::path::Path) -> std::io::Result<Option<Vec<u8>>> {
    use std::io::Read;
    let Some(file) = crate::osfs::open_regular(path)? else {
        return Ok(None);
    };
    let mut bytes = Vec::new();
    file.take(MAX_SIDE as u64 + 1).read_to_end(&mut bytes)?;
    Ok(Some(bytes))
}

/// What changed from `before` to `after`; `None` for a side where the file
/// does not exist.
pub fn diff(before: Option<&[u8]>, after: Option<&[u8]>) -> TextDiff {
    let (before, after) = (before.unwrap_or_default(), after.unwrap_or_default());
    if before.len() > MAX_SIDE || after.len() > MAX_SIDE {
        return TextDiff::Skipped {
            reason: DiffSkip::TooLarge,
        };
    }
    let text = |bytes: &[u8]| {
        std::str::from_utf8(bytes)
            .ok()
            .filter(|t| !t.contains('\0'))
            .map(str::to_owned)
    };
    let (Some(before), Some(after)) = (text(before), text(after)) else {
        return TextDiff::Skipped {
            reason: DiffSkip::Binary,
        };
    };
    let lines = Lines::from_lines(&before, &after);
    let (mut hunks, mut shown, mut cut) = (Vec::new(), 0, false);
    'groups: for group in lines.grouped_ops(CONTEXT) {
        let (Some(first), Some(_)) = (group.first(), group.last()) else {
            continue;
        };
        let mut hunk = Hunk {
            before: first.old_range().start + 1,
            after: first.new_range().start + 1,
            lines: Vec::new(),
        };
        for op in &group {
            for change in lines.iter_changes(op) {
                if shown == MAX_LINES {
                    cut = true;
                    hunks.push(hunk);
                    break 'groups;
                }
                let kind = match change.tag() {
                    ChangeTag::Equal => LineKind::Same,
                    ChangeTag::Insert => LineKind::Added,
                    ChangeTag::Delete => LineKind::Removed,
                };
                let text = change.value();
                let text = text.strip_suffix('\n').unwrap_or(text);
                let text = text.strip_suffix('\r').unwrap_or(text);
                hunk.lines.push(Line {
                    kind,
                    text: text.to_owned(),
                });
                shown += 1;
            }
        }
        hunks.push(hunk);
    }
    TextDiff::Lines { hunks, cut }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(d: &TextDiff) -> Vec<(LineKind, &str)> {
        let TextDiff::Lines { hunks, .. } = d else {
            panic!("{d:?}");
        };
        hunks
            .iter()
            .flat_map(|h| h.lines.iter().map(|l| (l.kind, l.text.as_str())))
            .collect()
    }

    #[test]
    fn a_line_added_at_the_end_shows_with_what_is_around_it() {
        let before = "a\nb\nc\nd\ne\n";
        let after = "a\nb\nc\nd\ne\nexport B=2\n";
        let d = diff(Some(before.as_bytes()), Some(after.as_bytes()));
        assert_eq!(
            lines(&d),
            [
                (LineKind::Same, "c"),
                (LineKind::Same, "d"),
                (LineKind::Same, "e"),
                (LineKind::Added, "export B=2"),
            ]
        );
        let TextDiff::Lines { hunks, cut } = d else {
            unreachable!()
        };
        assert_eq!((hunks[0].before, hunks[0].after, cut), (3, 3, false));
    }

    #[test]
    fn a_new_file_is_all_added_and_a_removed_one_all_removed() {
        assert_eq!(
            lines(&diff(None, Some(b"x\r\ny"))),
            [(LineKind::Added, "x"), (LineKind::Added, "y")]
        );
        assert_eq!(lines(&diff(Some(b"x\n"), None)), [(LineKind::Removed, "x")]);
        assert_eq!(lines(&diff(Some(b"same\n"), Some(b"same\n"))), []);
    }

    #[test]
    fn binary_and_large_files_are_not_shown() {
        let skipped = |reason| TextDiff::Skipped { reason };
        assert_eq!(diff(None, Some(b"a\0b")), skipped(DiffSkip::Binary));
        assert_eq!(diff(Some(&[0xff, 0xfe]), None), skipped(DiffSkip::Binary));
        let big = vec![b'a'; MAX_SIDE + 1];
        assert_eq!(diff(None, Some(&big)), skipped(DiffSkip::TooLarge));
    }

    #[test]
    fn a_long_diff_is_cut() {
        let after: String = (0..MAX_LINES + 10).map(|i| format!("{i}\n")).collect();
        let d = diff(None, Some(after.as_bytes()));
        assert_eq!(lines(&d).len(), MAX_LINES);
        assert!(matches!(d, TextDiff::Lines { cut: true, .. }));
    }
}
