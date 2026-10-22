//! DOCX documents as text to read: headings, paragraphs, list items with
//! their numbers, and tables. Formatting, pictures, notes and deleted text
//! of tracked changes are left out.

use std::collections::HashMap;

use quick_xml::Reader;
use quick_xml::events::Event;
use serde::{Deserialize, Serialize};

use crate::office::{Parts, Result, attr, clip, reference, unread};
use crate::{Budget, FRAME, MAX_VIEW, cost};

/// The most blocks shown.
pub const MAX_BLOCKS: usize = 5000;
/// The most characters of one block shown.
const MAX_BLOCK: usize = 10_000;
/// The most characters of all blocks together, list markers counted.
const MAX_CHARS: usize = 2_000_000;
/// The most characters of a list item's marker, and of the pattern a list
/// level makes its markers from.
const MAX_MARKER: usize = 64;
/// The most rows and columns of one table shown.
const MAX_TABLE_ROWS: usize = 500;
const MAX_TABLE_COLS: usize = 50;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Block {
    Heading {
        level: u8,
        text: String,
    },
    Paragraph {
        text: String,
    },
    /// `marker` is what Word shows before it: `1.`, `a)`, `•`.
    ListItem {
        level: u8,
        marker: String,
        text: String,
    },
    Table {
        rows: Vec<Vec<String>>,
    },
}

/// The document's blocks, and whether there were more than are here.
pub fn blocks(bytes: &[u8]) -> Result<(Vec<Block>, bool)> {
    let mut parts = Parts::open(bytes)?;
    let document = parts
        .text("word/document.xml")?
        .ok_or_else(|| unread("not a document Kule can read"))?;
    let headings = match parts.text("word/styles.xml")? {
        Some(xml) => heading_styles(&xml)?,
        None => HashMap::new(),
    };
    let numbering = match parts.text("word/numbering.xml")? {
        Some(xml) => Numbering::read(&xml)?,
        None => Numbering::default(),
    };
    read(
        &document,
        &headings,
        numbering,
        &mut Budget::new(MAX_VIEW),
        &parts,
    )
}

fn malformed<E>(_: E) -> crate::office::Unread {
    unread("not a document Kule can read")
}

/// Paragraph styles that are headings, by id, with their level: Title is
/// 1, "heading N" is N, and a style with an outline level is that plus 1.
fn heading_styles(xml: &str) -> Result<HashMap<String, u8>> {
    let mut reader = Reader::from_str(xml);
    let mut out = HashMap::new();
    let mut current: Option<String> = None;
    loop {
        match reader.read_event().map_err(malformed)? {
            Event::Eof => break,
            Event::Start(e) if e.local_name().as_ref() == "style" => {
                current = (attr(&e, "type").as_deref() == Some("paragraph"))
                    .then(|| attr(&e, "styleId"))
                    .flatten();
            }
            Event::End(e) if e.local_name().as_ref() == "style" => current = None,
            Event::Start(e) | Event::Empty(e) => {
                let Some(id) = &current else { continue };
                match e.local_name().as_ref() {
                    "name" => {
                        let name = attr(&e, "val").unwrap_or_default().to_ascii_lowercase();
                        if name == "title" {
                            out.insert(id.clone(), 1);
                        } else if let Some(n) = name.strip_prefix("heading ")
                            && let Ok(n) = n.trim().parse::<u8>()
                        {
                            out.insert(id.clone(), n.clamp(1, 6));
                        }
                    }
                    "outlineLvl" => {
                        if let Some(n) = attr(&e, "val").and_then(|v| v.parse::<u8>().ok())
                            && n < 9
                        {
                            out.entry(id.clone()).or_insert((n + 1).min(6));
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }
    Ok(out)
}

/// What list numbers look like, and how far each list has counted.
#[derive(Default)]
struct Numbering {
    /// numId → abstractNumId.
    nums: HashMap<String, String>,
    /// (abstractNumId, level) → (format, text such as "%1.").
    levels: HashMap<(String, u8), (String, String)>,
    /// (numId, level) → the count so far.
    counts: HashMap<(String, u8), u32>,
}

impl Numbering {
    fn read(xml: &str) -> Result<Self> {
        let mut reader = Reader::from_str(xml);
        let mut n = Numbering::default();
        let mut abstract_id: Option<String> = None;
        let mut level: Option<u8> = None;
        let mut format = String::new();
        let mut text = String::new();
        let mut num_id: Option<String> = None;
        loop {
            match reader.read_event().map_err(malformed)? {
                Event::Eof => break,
                Event::Start(e) | Event::Empty(e) => match e.local_name().as_ref() {
                    "abstractNum" => abstract_id = attr(&e, "abstractNumId"),
                    "lvl" => {
                        level = attr(&e, "ilvl").and_then(|v| v.parse().ok());
                        format = "decimal".into();
                        text = String::new();
                    }
                    "numFmt" if level.is_some() => format = attr(&e, "val").unwrap_or_default(),
                    "lvlText" if level.is_some() => {
                        text = clip(attr(&e, "val").unwrap_or_default(), MAX_MARKER)
                    }
                    "num" => num_id = attr(&e, "numId"),
                    "abstractNumId" => {
                        if let (Some(id), Some(a)) = (&num_id, attr(&e, "val")) {
                            n.nums.insert(id.clone(), a);
                        }
                    }
                    _ => {}
                },
                Event::End(e) => match e.local_name().as_ref() {
                    "lvl" => {
                        if let (Some(a), Some(l)) = (&abstract_id, level.take()) {
                            n.levels.insert(
                                (a.clone(), l),
                                (std::mem::take(&mut format), std::mem::take(&mut text)),
                            );
                        }
                    }
                    "abstractNum" => abstract_id = None,
                    "num" => num_id = None,
                    _ => {}
                },
                _ => {}
            }
        }
        Ok(n)
    }

    /// The marker of the next item of list `num` at `level`; deeper levels
    /// start counting again.
    fn next(&mut self, num: &str, level: u8) -> String {
        let count = {
            let c = self.counts.entry((num.to_owned(), level)).or_insert(0);
            *c += 1;
            *c
        };
        self.counts.retain(|(n, l), _| n != num || *l <= level);
        let Some((format, text)) = self
            .nums
            .get(num)
            .and_then(|a| self.levels.get(&(a.clone(), level)))
        else {
            return "•".into();
        };
        if format == "bullet" || format == "none" {
            return "•".into();
        }
        // "%1.%2." names the counts of levels 1 and 2.
        let mut out = String::new();
        let mut chars = text.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '%'
                && let Some(d) = chars.peek().and_then(|d| d.to_digit(10))
            {
                chars.next();
                let l = (d as u8).saturating_sub(1);
                let n = if l == level {
                    count
                } else {
                    self.counts.get(&(num.to_owned(), l)).copied().unwrap_or(1)
                };
                let f = if l == level {
                    format.as_str()
                } else {
                    self.nums
                        .get(num)
                        .and_then(|a| self.levels.get(&(a.clone(), l)))
                        .map(|(f, _)| f.as_str())
                        .unwrap_or("decimal")
                };
                out.push_str(&counted(n, f));
            } else {
                out.push(c);
            }
        }
        if out.is_empty() {
            "•".into()
        } else {
            clip(out, MAX_MARKER)
        }
    }
}

/// `n` written in list format `format`.
fn counted(n: u32, format: &str) -> String {
    let letters = |base: u8| {
        let mut n = n.max(1) - 1;
        let mut s = Vec::new();
        loop {
            s.push(base + (n % 26) as u8);
            if n < 26 {
                break;
            }
            n = n / 26 - 1;
        }
        s.reverse();
        String::from_utf8(s).expect("ASCII")
    };
    match format {
        "lowerLetter" => letters(b'a'),
        "upperLetter" => letters(b'A'),
        "lowerRoman" => roman(n).to_lowercase(),
        "upperRoman" => roman(n),
        _ => n.to_string(),
    }
}

fn roman(mut n: u32) -> String {
    if n == 0 || n >= 4000 {
        return n.to_string();
    }
    let mut out = String::new();
    for (value, symbol) in [
        (1000, "M"),
        (900, "CM"),
        (500, "D"),
        (400, "CD"),
        (100, "C"),
        (90, "XC"),
        (50, "L"),
        (40, "XL"),
        (10, "X"),
        (9, "IX"),
        (5, "V"),
        (4, "IV"),
        (1, "I"),
    ] {
        while n >= value {
            out.push_str(symbol);
            n -= value;
        }
    }
    out
}

/// One paragraph as it is read.
#[derive(Default)]
struct Para {
    text: String,
    style: Option<String>,
    outline: Option<u8>,
    num: Option<String>,
    level: u8,
}

fn read(
    xml: &str,
    headings: &HashMap<String, u8>,
    mut numbering: Numbering,
    budget: &mut Budget,
    parts: &Parts,
) -> Result<(Vec<Block>, bool)> {
    let mut reader = Reader::from_str(xml);
    let mut blocks = Vec::new();
    let mut chars = 0usize;
    let mut cut = false;
    let mut para = Para::default();
    let mut para_depth = 0usize;
    let mut in_text = 0usize;
    // Tables: only the outermost becomes a table; one inside it is text.
    let mut table_depth = 0usize;
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut events = 0usize;
    loop {
        let event = reader.read_event().map_err(malformed)?;
        events += 1;
        if events.is_multiple_of(8192) {
            parts.check_time()?;
        }
        match event {
            Event::Eof => break,
            Event::Start(e) => match e.local_name().as_ref() {
                "p" => {
                    para_depth += 1;
                    if para_depth == 1 {
                        para = Para::default();
                    }
                }
                "t" => in_text += 1,
                "tbl" => {
                    table_depth += 1;
                    if table_depth == 1 {
                        rows = Vec::new();
                    }
                }
                "tr" if table_depth == 1 => {
                    if rows.len() < MAX_TABLE_ROWS {
                        rows.push(Vec::new());
                    } else {
                        cut = true;
                    }
                }
                "tc" if table_depth == 1 => {
                    if let Some(row) = rows.last_mut()
                        && row.len() < MAX_TABLE_COLS
                    {
                        row.push(String::new());
                    }
                }
                _ => properties(&e, &mut para),
            },
            Event::Empty(e) => match e.local_name().as_ref() {
                "tab" if para_depth > 0 => para.text.push('\t'),
                "br" | "cr" if para_depth > 0 => para.text.push('\n'),
                _ => properties(&e, &mut para),
            },
            Event::Text(t) if in_text > 0 && para_depth > 0 => {
                para.text.push_str(&t.xml10_content())
            }
            Event::GeneralRef(r) if in_text > 0 && para_depth > 0 => {
                para.text.extend(reference(&r))
            }
            Event::End(e) => match e.local_name().as_ref() {
                "t" => in_text = in_text.saturating_sub(1),
                "p" => {
                    para_depth = para_depth.saturating_sub(1);
                    if para_depth > 0 {
                        continue;
                    }
                    let p = std::mem::take(&mut para);
                    if table_depth > 0 {
                        // A paragraph of a cell: a line of its text.
                        if let Some(cell) = rows.last_mut().and_then(|r| r.last_mut())
                            && !p.text.trim().is_empty()
                        {
                            if !cell.is_empty() {
                                cell.push('\n');
                            }
                            cell.push_str(p.text.trim());
                        }
                        continue;
                    }
                    if p.text.trim().is_empty() {
                        continue;
                    }
                    let level = p
                        .style
                        .as_ref()
                        .and_then(|s| headings.get(s).copied())
                        .or(p.outline.map(|o| (o + 1).min(6)));
                    let text = clip(p.text.trim().to_owned(), MAX_BLOCK);
                    let block = match (level, &p.num) {
                        (Some(level), _) => Block::Heading { level, text },
                        (None, Some(num)) if num != "0" => Block::ListItem {
                            level: p.level,
                            marker: numbering.next(num, p.level),
                            text,
                        },
                        _ => Block::Paragraph { text },
                    };
                    if !push(&mut blocks, &mut chars, budget, block) {
                        cut = true;
                        break;
                    }
                }
                "tbl" => {
                    table_depth = table_depth.saturating_sub(1);
                    if table_depth == 0 && !rows.is_empty() {
                        let rows = std::mem::take(&mut rows)
                            .into_iter()
                            .map(|r| {
                                r.into_iter()
                                    .map(|c| clip(c, crate::table::MAX_CELL))
                                    .collect()
                            })
                            .collect();
                        if !push(&mut blocks, &mut chars, budget, Block::Table { rows }) {
                            cut = true;
                            break;
                        }
                    }
                }
                _ => {}
            },
            _ => {}
        }
    }
    Ok((blocks, cut))
}

/// A paragraph's style, outline level and list membership.
fn properties(e: &quick_xml::events::BytesStart, para: &mut Para) {
    match e.local_name().as_ref() {
        "pStyle" => para.style = attr(e, "val"),
        "outlineLvl" => {
            para.outline = attr(e, "val")
                .and_then(|v| v.parse().ok())
                .filter(|&n: &u8| n < 9)
        }
        "numId" => para.num = attr(e, "val"),
        "ilvl" => {
            para.level = attr(e, "val")
                .and_then(|v| v.parse().ok())
                .unwrap_or(0)
                .min(8)
        }
        _ => {}
    }
}

/// Adds `block` within the limits and the view's budget; false when it
/// does not fit.
fn push(blocks: &mut Vec<Block>, chars: &mut usize, budget: &mut Budget, block: Block) -> bool {
    let texts: Vec<&str> = match &block {
        Block::Heading { text, .. } | Block::Paragraph { text } => vec![text],
        Block::ListItem { marker, text, .. } => vec![marker, text],
        Block::Table { rows } => rows.iter().flatten().map(String::as_str).collect(),
    };
    let size: usize = texts.iter().map(|t| t.len()).sum();
    let costs = FRAME + texts.iter().map(|t| cost(t)).sum::<usize>();
    if blocks.len() >= MAX_BLOCKS || *chars + size > MAX_CHARS || !budget.take(costs) {
        return false;
    }
    *chars += size;
    blocks.push(block);
    true
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::office::tests::archive;

    const W: &str = r#"xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main""#;

    /// A document with a title, a heading, a paragraph, a numbered list
    /// two levels deep, a bullet, a table and a tracked deletion.
    pub(crate) fn document() -> Vec<u8> {
        let body = format!(
            r#"<w:document {W}><w:body>
<w:p><w:pPr><w:pStyle w:val="Title"/></w:pPr><w:r><w:t>Service contract</w:t></w:r></w:p>
<w:p><w:pPr><w:pStyle w:val="Baslik1"/></w:pPr><w:r><w:t>4. Payment</w:t></w:r></w:p>
<w:p><w:r><w:t xml:space="preserve">Invoices are issued </w:t></w:r><w:del><w:r><w:delText>weekly</w:delText></w:r></w:del><w:r><w:t>monthly &amp; paid</w:t><w:tab/><w:t>late.</w:t></w:r></w:p>
<w:p/>
<w:p><w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="1"/></w:numPr></w:pPr><w:r><w:t>First</w:t></w:r></w:p>
<w:p><w:pPr><w:numPr><w:ilvl w:val="1"/><w:numId w:val="1"/></w:numPr></w:pPr><w:r><w:t>Inner</w:t></w:r></w:p>
<w:p><w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="1"/></w:numPr></w:pPr><w:r><w:t>Second</w:t></w:r></w:p>
<w:p><w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="2"/></w:numPr></w:pPr><w:r><w:t>A point</w:t></w:r></w:p>
<w:tbl><w:tr><w:tc><w:p><w:r><w:t>Item</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>Price</w:t></w:r></w:p></w:tc></w:tr>
<w:tr><w:tc><w:p><w:r><w:t>Rent</w:t></w:r></w:p><w:p><w:r><w:t>(yearly)</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>1200</w:t></w:r></w:p></w:tc></w:tr></w:tbl>
</w:body></w:document>"#
        );
        let styles = format!(
            r#"<w:styles {W}><w:style w:type="paragraph" w:styleId="Title"><w:name w:val="Title"/></w:style><w:style w:type="paragraph" w:styleId="Baslik1"><w:name w:val="heading 1"/></w:style><w:style w:type="character" w:styleId="X"><w:name w:val="heading 2"/></w:style></w:styles>"#
        );
        let numbering = format!(
            r#"<w:numbering {W}><w:abstractNum w:abstractNumId="7"><w:lvl w:ilvl="0"><w:numFmt w:val="decimal"/><w:lvlText w:val="%1."/></w:lvl><w:lvl w:ilvl="1"><w:numFmt w:val="lowerLetter"/><w:lvlText w:val="%1.%2)"/></w:lvl></w:abstractNum><w:abstractNum w:abstractNumId="8"><w:lvl w:ilvl="0"><w:numFmt w:val="bullet"/><w:lvlText w:val="o"/></w:lvl></w:abstractNum><w:num w:numId="1"><w:abstractNumId w:val="7"/></w:num><w:num w:numId="2"><w:abstractNumId w:val="8"/></w:num></w:numbering>"#
        );
        archive(&[
            ("word/document.xml", body.as_bytes()),
            ("word/styles.xml", styles.as_bytes()),
            ("word/numbering.xml", numbering.as_bytes()),
        ])
    }

    #[test]
    fn a_document_reads_as_its_blocks() {
        let (blocks, cut) = blocks(&document()).unwrap();
        assert!(!cut);
        let item = |level, marker: &str, text: &str| Block::ListItem {
            level,
            marker: marker.into(),
            text: text.into(),
        };
        assert_eq!(
            blocks,
            [
                Block::Heading {
                    level: 1,
                    text: "Service contract".into()
                },
                Block::Heading {
                    level: 1,
                    text: "4. Payment".into()
                },
                Block::Paragraph {
                    text: "Invoices are issued monthly & paid\tlate.".into()
                },
                item(0, "1.", "First"),
                item(1, "1.a)", "Inner"),
                item(0, "2.", "Second"),
                item(0, "•", "A point"),
                Block::Table {
                    rows: vec![
                        vec!["Item".into(), "Price".into()],
                        vec!["Rent\n(yearly)".into(), "1200".into()]
                    ]
                },
            ]
        );
    }

    #[test]
    fn list_numbers_are_written_as_word_writes_them() {
        assert_eq!(counted(28, "upperLetter"), "AB");
        assert_eq!(counted(4, "lowerRoman"), "iv");
        assert_eq!(roman(1994), "MCMXCIV");
    }

    #[test]
    fn what_is_not_a_document_has_no_blocks() {
        assert!(blocks(b"plain").is_err());
        assert!(blocks(&archive(&[("xl/workbook.xml", b"<w/>")])).is_err());
        let broken = archive(&[("word/document.xml", b"<w:document><w:body><w:p></w:body>")]);
        assert!(blocks(&broken).is_err());
    }

    #[test]
    fn a_long_document_stops_at_its_limit() {
        let paragraph = "<w:p><w:r><w:t>x</w:t></w:r></w:p>".repeat(MAX_BLOCKS + 10);
        let body = format!("<w:document {W}><w:body>{paragraph}</w:body></w:document>");
        let (blocks, cut) = blocks(&archive(&[("word/document.xml", body.as_bytes())])).unwrap();
        assert_eq!(blocks.len(), MAX_BLOCKS);
        assert!(cut);
    }

    #[test]
    fn a_list_marker_keeps_to_its_limit() {
        let numbering = |pattern: &str| {
            Numbering::read(&format!(
                r#"<w:numbering {W}><w:abstractNum w:abstractNumId="7"><w:lvl w:ilvl="0"><w:numFmt w:val="decimal"/><w:lvlText w:val="{pattern}"/></w:lvl></w:abstractNum><w:num w:numId="1"><w:abstractNumId w:val="7"/></w:num></w:numbering>"#
            ))
            .unwrap()
        };
        // A pattern is cut where it is read.
        let long = numbering(&"x".repeat(MAX_MARKER * 10));
        let (_, pattern) = &long.levels[&("7".to_owned(), 0)];
        assert_eq!(pattern.chars().count(), MAX_MARKER + 1);
        // A pattern within the limit can still count past it.
        let mut counting = numbering(&"%1".repeat(MAX_MARKER / 2));
        assert_eq!(counting.next("1", 0), "1".repeat(MAX_MARKER / 2));
        let marker = (0..99).map(|_| counting.next("1", 0)).last().unwrap();
        assert_eq!(marker, clip("100".repeat(MAX_MARKER / 2), MAX_MARKER));
    }

    #[test]
    fn markers_count_toward_a_document_s_limits() {
        let item = |marker: String| Block::ListItem {
            level: 0,
            marker,
            text: "a".into(),
        };
        let mut blocks = Vec::new();
        let mut chars = 0;
        let mut budget = Budget::new(MAX_VIEW);
        assert!(!push(
            &mut blocks,
            &mut chars,
            &mut budget,
            item("1".repeat(MAX_CHARS))
        ));
        assert!(push(
            &mut blocks,
            &mut chars,
            &mut budget,
            item("1.".into())
        ));
        assert_eq!(chars, 3);
        // What the view's budget cannot carry is not added either.
        let mut budget = Budget::new(FRAME + cost("1.") + cost("a"));
        assert!(push(
            &mut blocks,
            &mut chars,
            &mut budget,
            item("1.".into())
        ));
        assert!(!push(
            &mut blocks,
            &mut chars,
            &mut budget,
            item("1.".into())
        ));
        assert_eq!(blocks.len(), 2);
    }
}
