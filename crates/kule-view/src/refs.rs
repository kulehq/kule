//! Places a person picked in the Preview to ask the agent about: lines of a
//! file, a section of a document, cells, a region of a picture. They go
//! with the prompt as a numbered list after it, so the agent, whose working
//! directory is the copy's root, can open each file and find the place.

use serde::{Deserialize, Serialize};

use crate::office::clip;
use crate::table::cell_at;

/// The most places one prompt names.
pub const MAX_REFS: usize = 20;
/// The most characters of a picked text sent.
pub const MAX_QUOTE: usize = 500;
/// The most cells whose values are sent, by rows and columns.
const MAX_VALUE_ROWS: usize = 10;
const MAX_VALUE_COLS: usize = 10;
const MAX_VALUE: usize = 200;
const MAX_TRAIL: usize = 6;
const MAX_NAME: usize = 200;
const MAX_PATH: usize = 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Reference {
    /// A file as a whole.
    File { path: String },
    /// Lines of a file as written, from 1, and the text picked in them.
    Lines {
        path: String,
        start: u32,
        end: u32,
        #[serde(default)]
        quote: String,
    },
    /// A place in laid-out Markdown: its source lines and the headings
    /// it is under, outermost first.
    Section {
        path: String,
        start: u32,
        end: u32,
        #[serde(default)]
        trail: Vec<String>,
        #[serde(default)]
        quote: String,
    },
    /// Text picked where lines do not match the file's (a formatted view).
    Quote { path: String, quote: String },
    /// A range of cells (`B4:C9`) of a sheet, with the values it shows.
    Cells {
        path: String,
        #[serde(default)]
        sheet: Option<String>,
        range: String,
        #[serde(default)]
        values: Vec<Vec<String>>,
    },
    /// A rectangle of a picture, in its own pixels.
    Region {
        path: String,
        x: u32,
        y: u32,
        w: u32,
        h: u32,
        width: u32,
        height: u32,
    },
    /// A block of a Word document, from 1, and the heading it is under.
    Block {
        path: String,
        index: u32,
        #[serde(default)]
        heading: Option<String>,
        #[serde(default)]
        quote: String,
    },
}

impl Reference {
    pub fn path(&self) -> &str {
        match self {
            Reference::File { path }
            | Reference::Lines { path, .. }
            | Reference::Section { path, .. }
            | Reference::Quote { path, .. }
            | Reference::Cells { path, .. }
            | Reference::Region { path, .. }
            | Reference::Block { path, .. } => path,
        }
    }

    /// The reference within its limits, or why it cannot be sent. Texts
    /// that are too long are cut; numbers and names that make no sense are
    /// refused.
    pub fn checked(self) -> Result<Reference, String> {
        let path = self.path().to_owned();
        let path = path.as_str();
        if path.is_empty()
            || path.len() > MAX_PATH
            || path.starts_with('/')
            || path
                .split('/')
                .any(|p| p.is_empty() || p == "." || p == "..")
            || path.chars().any(char::is_control)
        {
            return Err(format!("{path:?}: a path inside the copy, without .."));
        }
        let quote = |q: String| clip(q, MAX_QUOTE);
        let name = |n: String| clip(n.replace(char::is_control, " "), MAX_NAME);
        let lines = |start: u32, end: u32| {
            if start == 0 || end < start {
                Err(format!(
                    "{path}: lines {start} to {end} are not lines of a file"
                ))
            } else {
                Ok(())
            }
        };
        Ok(match self {
            Reference::File { path } => Reference::File { path },
            Reference::Lines {
                path,
                start,
                end,
                quote: q,
            } => {
                lines(start, end)?;
                Reference::Lines {
                    path,
                    start,
                    end,
                    quote: quote(q),
                }
            }
            Reference::Section {
                path,
                start,
                end,
                trail,
                quote: q,
            } => {
                lines(start, end)?;
                Reference::Section {
                    path,
                    start,
                    end,
                    trail: trail.into_iter().take(MAX_TRAIL).map(name).collect(),
                    quote: quote(q),
                }
            }
            Reference::Quote { path, quote: q } => {
                if q.trim().is_empty() {
                    return Err(format!("{path}: no text was picked"));
                }
                Reference::Quote {
                    path,
                    quote: quote(q),
                }
            }
            Reference::Cells {
                path,
                sheet,
                range,
                values,
            } => {
                let mut ends = range.split(':');
                let ok = ends.next().and_then(cell_at).is_some()
                    && ends.next().is_none_or(|e| cell_at(e).is_some())
                    && ends.next().is_none();
                if !ok {
                    return Err(format!("{path}: {range:?} is not a range of cells"));
                }
                Reference::Cells {
                    path,
                    sheet: sheet.map(name),
                    range,
                    values: values
                        .into_iter()
                        .take(MAX_VALUE_ROWS)
                        .map(|row| {
                            row.into_iter()
                                .take(MAX_VALUE_COLS)
                                .map(|v| clip(v, MAX_VALUE))
                                .collect()
                        })
                        .collect(),
                }
            }
            Reference::Region {
                path,
                x,
                y,
                w,
                h,
                width,
                height,
            } => {
                let fits = w > 0
                    && h > 0
                    && x.checked_add(w).is_some_and(|r| r <= width)
                    && y.checked_add(h).is_some_and(|b| b <= height);
                if !fits {
                    return Err(format!("{path}: the region is not inside the picture"));
                }
                Reference::Region {
                    path,
                    x,
                    y,
                    w,
                    h,
                    width,
                    height,
                }
            }
            Reference::Block {
                path,
                index,
                heading,
                quote: q,
            } => {
                if index == 0 {
                    return Err(format!("{path}: blocks count from 1"));
                }
                Reference::Block {
                    path,
                    index,
                    heading: heading.map(name),
                    quote: quote(q),
                }
            }
        })
    }

    /// The reference as a line of the list sent with a prompt.
    fn line(&self) -> String {
        // Picked text goes quoted and escaped, so a line break or a quote
        // in it cannot pass for the list's own text.
        let q = |text: &str| serde_json::to_string(text).expect("a string");
        let picked = |text: &str| {
            if text.trim().is_empty() {
                String::new()
            } else {
                format!(": {}", q(text))
            }
        };
        let span = |start: u32, end: u32| {
            if start == end {
                format!("line {start}")
            } else {
                format!("lines {start}-{end}")
            }
        };
        match self {
            Reference::File { path } => format!("{path}, the whole file"),
            Reference::Lines {
                path,
                start,
                end,
                quote,
            } => format!("{path}, {}{}", span(*start, *end), picked(quote)),
            Reference::Section {
                path,
                start,
                end,
                trail,
                quote,
            } => {
                let under = if trail.is_empty() {
                    String::new()
                } else {
                    format!(" (under {})", q(&trail.join(" > ")))
                };
                format!("{path}, {}{under}{}", span(*start, *end), picked(quote))
            }
            Reference::Quote { path, quote } => format!("{path}, where it says {}", q(quote)),
            Reference::Cells {
                path,
                sheet,
                range,
                values,
            } => {
                let sheet = sheet
                    .as_ref()
                    .map(|s| format!("sheet {}, ", q(s)))
                    .unwrap_or_default();
                let cells = if range.contains(':') { "cells" } else { "cell" };
                let mut line = format!("{path}, {sheet}{cells} {range}");
                if let Some((row0, col0)) = range.split(':').next().and_then(cell_at)
                    && !values.is_empty()
                {
                    line.push_str(", showing:");
                    for (r, row) in values.iter().enumerate() {
                        let cells: Vec<String> = row
                            .iter()
                            .enumerate()
                            .map(|(c, v)| {
                                format!(
                                    "{}{}={}",
                                    crate::table::column_name(col0 + c),
                                    row0 + r + 1,
                                    q(v)
                                )
                            })
                            .collect();
                        line.push_str(&format!("\n   {}", cells.join("  ")));
                    }
                }
                line
            }
            Reference::Region {
                path,
                x,
                y,
                w,
                h,
                width,
                height,
            } => format!(
                "{path}, the region x={x} y={y} width={w} height={h} (pixels from the top left) of the {width}x{height} picture"
            ),
            Reference::Block {
                path,
                index,
                heading,
                quote,
            } => {
                let under = heading
                    .as_ref()
                    .map(|h| format!(" (under {})", q(h)))
                    .unwrap_or_default();
                format!("{path}, paragraph {index}{under}{}", picked(quote))
            }
        }
    }
}

/// `prompt` with the places it is about after it. Each reference must have
/// been [`Reference::checked`].
pub fn with_places(prompt: &str, refs: &[Reference]) -> String {
    if refs.is_empty() {
        return prompt.to_owned();
    }
    let mut out = prompt.trim_end().to_owned();
    out.push_str(
        "\n\nAbout these places in the task's files (paths are from the working directory):",
    );
    for (i, r) in refs.iter().enumerate() {
        out.push_str(&format!("\n{}. {}", i + 1, r.line()));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(path: &str, start: u32, end: u32, quote: &str) -> Reference {
        Reference::Lines {
            path: path.into(),
            start,
            end,
            quote: quote.into(),
        }
    }

    #[test]
    fn places_follow_the_prompt_as_a_numbered_list() {
        let refs = [
            Reference::Section {
                path: "docs/report.md".into(),
                start: 14,
                end: 14,
                trail: vec!["Results".into(), "Q2".into()],
                quote: "12% in Q2".into(),
            },
            Reference::Cells {
                path: "data/sales.xlsx".into(),
                sheet: Some("Sales".into()),
                range: "B4:C5".into(),
                values: vec![
                    vec!["1180".into(), "412300".into()],
                    vec!["1204".into(), "".into()],
                ],
            },
            Reference::Region {
                path: "site/hero.png".into(),
                x: 640,
                y: 300,
                w: 360,
                h: 120,
                width: 1280,
                height: 720,
            },
            Reference::Block {
                path: "contract.docx".into(),
                index: 12,
                heading: Some("4. Payment".into()),
                quote: String::new(),
            },
            lines("src/a.rs", 40, 52, "fn a() {\n}"),
            Reference::File {
                path: "README.md".into(),
            },
        ];
        assert_eq!(
            with_places("Make these agree.\n", &refs),
            r#"Make these agree.

About these places in the task's files (paths are from the working directory):
1. docs/report.md, line 14 (under "Results > Q2"): "12% in Q2"
2. data/sales.xlsx, sheet "Sales", cells B4:C5, showing:
   B4="1180"  C4="412300"
   B5="1204"  C5=""
3. site/hero.png, the region x=640 y=300 width=360 height=120 (pixels from the top left) of the 1280x720 picture
4. contract.docx, paragraph 12 (under "4. Payment")
5. src/a.rs, lines 40-52: "fn a() {\n}"
6. README.md, the whole file"#
        );
        assert_eq!(with_places("hi", &[]), "hi");
    }

    #[test]
    fn a_reference_is_kept_to_its_limits_or_refused() {
        for (what, r) in [
            ("parent", lines("../x", 1, 1, "")),
            ("absolute", lines("/etc/passwd", 1, 1, "")),
            ("inner parent", lines("a/../../x", 1, 1, "")),
            ("empty", lines("", 1, 1, "")),
            ("line 0", lines("a", 0, 1, "")),
            ("backwards", lines("a", 5, 4, "")),
            (
                "range",
                Reference::Cells {
                    path: "a.csv".into(),
                    sheet: None,
                    range: "B4:Z".into(),
                    values: vec![],
                },
            ),
            (
                "outside",
                Reference::Region {
                    path: "a.png".into(),
                    x: 10,
                    y: 0,
                    w: 1,
                    h: 1,
                    width: 10,
                    height: 10,
                },
            ),
            (
                "block 0",
                Reference::Block {
                    path: "a.docx".into(),
                    index: 0,
                    heading: None,
                    quote: String::new(),
                },
            ),
        ] {
            assert!(r.checked().is_err(), "{what} was let through");
        }
        let Reference::Lines { quote, .. } = lines("a", 1, 2, &"ç".repeat(900)).checked().unwrap()
        else {
            panic!()
        };
        assert_eq!(quote.chars().count(), MAX_QUOTE + 1);
        let Reference::Cells { values, .. } = (Reference::Cells {
            path: "a.xlsx".into(),
            sheet: Some("S\nheet".into()),
            range: "A1".into(),
            values: vec![vec!["x".repeat(500); 30]; 30],
        })
        .checked()
        .unwrap() else {
            panic!()
        };
        assert_eq!(
            (values.len(), values[0].len()),
            (MAX_VALUE_ROWS, MAX_VALUE_COLS)
        );
        assert!(values[0][0].chars().count() <= MAX_VALUE + 1);
    }
}
