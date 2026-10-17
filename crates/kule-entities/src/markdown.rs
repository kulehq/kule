//! Markdown, as sections: a heading (`#` to `######`) and what follows it,
//! up to the next heading of the same level or above. Headings inside code
//! fences are code.

use crate::{Entity, Kind, Skip, Spanned, own_bytes, tidy};

pub(crate) fn entities(src: &[u8]) -> Result<Vec<Entity>, Skip> {
    let text = std::str::from_utf8(src).map_err(|_| Skip::Unparsed)?;
    let mut spanned = vec![Spanned {
        kind: Kind::TopLevel,
        name: String::new(),
        parent: None,
        start: 0,
        end: src.len(),
        line: 1,
    }];
    // The sections still open, innermost last, with their levels.
    let mut open: Vec<(usize, usize)> = Vec::new();
    let mut fence: Option<(char, usize)> = None;
    let mut at = 0;
    for (number, line) in text.split_inclusive('\n').enumerate() {
        let start = at;
        at += line.len();
        let indent = line.len() - line.trim_start_matches(' ').len();
        let body = line.trim_start_matches(' ').trim_end();
        if indent <= 3
            && let Some(marker) = fence_marker(body)
        {
            match fence {
                None => fence = Some(marker),
                Some((c, n))
                    if marker.0 == c
                        && marker.1 >= n
                        && body.trim_start_matches(c).trim().is_empty() =>
                {
                    fence = None;
                }
                Some(_) => {}
            }
            continue;
        }
        if fence.is_some() || indent > 3 {
            continue;
        }
        let Some((level, title)) = heading(body) else {
            continue;
        };
        while open.last().is_some_and(|&(_, l)| l >= level) {
            let (closed, _) = open.pop().unwrap_or_default();
            spanned[closed].end = start;
        }
        let parent = open.last().map_or(0, |&(i, _)| i);
        let name = match parent {
            0 => title,
            p => format!("{} › {title}", spanned[p].name),
        };
        spanned.push(Spanned {
            kind: Kind::Section,
            name,
            parent: Some(parent),
            start,
            end: src.len(),
            line: u32::try_from(number + 1).unwrap_or(u32::MAX),
        });
        open.push((spanned.len() - 1, level));
    }
    Ok(own_bytes(src, spanned))
}

/// The fence a line opens or closes: its character and length.
fn fence_marker(body: &str) -> Option<(char, usize)> {
    let c = body.chars().next().filter(|c| matches!(c, '`' | '~'))?;
    let n = body.chars().take_while(|&x| x == c).count();
    (n >= 3).then_some((c, n))
}

/// An ATX heading's level and title.
fn heading(body: &str) -> Option<(usize, String)> {
    let level = body.chars().take_while(|&c| c == '#').count();
    if !(1..=6).contains(&level) {
        return None;
    }
    let rest = &body[level..];
    if !(rest.is_empty() || rest.starts_with([' ', '\t'])) {
        return None;
    }
    let title = rest.trim().trim_end_matches('#').trim();
    Some((level, tidy(title)))
}

#[cfg(test)]
mod tests {
    use crate::tests::changes;
    use crate::{Edit, Kind};

    use Edit::*;

    fn c(edit: Edit, name: &str) -> (Edit, Kind, String) {
        (edit, Kind::Section, name.to_owned())
    }

    #[test]
    fn sections_nest_by_level() {
        let before = "# Guide\nIntro.\n\n## Install\nRun it.\n\n## Use\nLike so.\n\n# Notes\n";
        let after = "# Guide\nIntro.\n\n## Install\nRun it twice.\n\n## Use\nLike so.\n\n### Tips\nNone.\n\n# Notes\n";
        assert_eq!(
            changes("README.md", before, after),
            vec![
                c(Modified, "Guide › Install"),
                c(Added, "Guide › Use › Tips")
            ]
        );
    }

    #[test]
    fn headings_in_code_are_code() {
        let before = "# A\n```sh\n# not a heading\n```\n";
        let after = "# A\n```sh\n# still not one\n```\n";
        assert_eq!(changes("a.md", before, after), vec![c(Modified, "A")]);
    }

    #[test]
    fn text_before_the_first_heading_is_the_top_level() {
        assert_eq!(
            changes("a.md", "Hello.\n# A\n", "Hi.\n# A\n"),
            vec![(Modified, Kind::TopLevel, String::new())]
        );
    }

    #[test]
    fn a_hash_without_a_space_is_text() {
        assert_eq!(
            changes("a.md", "# A\n", "# A\n#tag\n"),
            vec![c(Modified, "A")]
        );
    }
}
