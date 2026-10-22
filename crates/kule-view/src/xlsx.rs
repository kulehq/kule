//! XLSX workbooks: each sheet's cells as Excel shows their values (shared
//! strings looked up, dates as dates), with the formulas behind them.
//! Formulas are not computed: a cell shows the value Excel saved with it.

use std::collections::HashMap;

use quick_xml::Reader;
use quick_xml::events::Event;

use crate::office::{Parts, Result, attr, reference, unread};
use crate::table::{Grid, MAX_CELLS, MAX_SHEETS, Sheet, cell_at};
use crate::{Budget, MAX_VIEW};

pub fn sheets(bytes: &[u8]) -> Result<Vec<Sheet>> {
    let mut parts = Parts::open(bytes)?;
    let workbook = parts
        .text("xl/workbook.xml")?
        .ok_or_else(|| unread("not a workbook Kule can read"))?;
    let (listed, date1904) = workbook_sheets(&workbook)?;
    let targets = relations(
        &parts
            .text("xl/_rels/workbook.xml.rels")?
            .unwrap_or_default(),
    )?;
    let strings = match parts.text("xl/sharedStrings.xml")? {
        Some(xml) => shared_strings(&xml, &parts)?,
        None => Vec::new(),
    };
    let dates = match parts.text("xl/styles.xml")? {
        Some(xml) => date_styles(&xml)?,
        None => Vec::new(),
    };
    let mut out = Vec::new();
    let mut cells_left = MAX_CELLS;
    let mut budget = Budget::new(MAX_VIEW);
    for (name, id) in listed.into_iter().take(MAX_SHEETS) {
        let Some(target) = targets.get(&id) else {
            continue;
        };
        let Some(xml) = parts.text(&part_path(target))? else {
            continue;
        };
        let mut grid = Grid::new(cells_left, &mut budget);
        sheet(&xml, &strings, &dates, date1904, &mut grid, &parts)?;
        cells_left = grid.cells_left();
        out.push(grid.sheet(name));
    }
    if out.is_empty() {
        return Err(unread("a workbook without sheets Kule can read"));
    }
    Ok(out)
}

/// A relation's target as a path in the archive: relative to `xl/`, or
/// from the archive's root when it starts with `/`.
fn part_path(target: &str) -> String {
    match target.strip_prefix('/') {
        Some(root) => root.to_owned(),
        None => format!("xl/{}", target.trim_start_matches("./")),
    }
}

fn malformed<E>(_: E) -> crate::office::Unread {
    unread("not a workbook Kule can read")
}

/// The sheets in their order, by name and relation id, and whether dates
/// count from 1904.
fn workbook_sheets(xml: &str) -> Result<(Vec<(String, String)>, bool)> {
    let mut reader = Reader::from_str(xml);
    let mut sheets = Vec::new();
    let mut date1904 = false;
    loop {
        match reader.read_event().map_err(malformed)? {
            Event::Eof => break,
            Event::Start(e) | Event::Empty(e) => match e.local_name().as_ref() {
                "sheet" => {
                    if let (Some(name), Some(id)) = (attr(&e, "name"), attr(&e, "id")) {
                        sheets.push((name, id));
                    }
                }
                "workbookPr" => {
                    date1904 = matches!(attr(&e, "date1904").as_deref(), Some("1" | "true"));
                }
                _ => {}
            },
            _ => {}
        }
    }
    Ok((sheets, date1904))
}

fn relations(xml: &str) -> Result<HashMap<String, String>> {
    let mut reader = Reader::from_str(xml);
    let mut out = HashMap::new();
    loop {
        match reader.read_event().map_err(malformed)? {
            Event::Eof => break,
            Event::Start(e) | Event::Empty(e) if e.local_name().as_ref() == "Relationship" => {
                if let (Some(id), Some(target)) = (attr(&e, "Id"), attr(&e, "Target")) {
                    out.insert(id, target);
                }
            }
            _ => {}
        }
    }
    Ok(out)
}

/// Text of an element, references resolved, until the end of `stop`;
/// text inside `skip` (phonetic runs) is left out.
fn text_until(reader: &mut Reader<&[u8]>, stop: &str, skip: &str, into: &mut String) -> Result<()> {
    let mut in_t = 0usize;
    let mut skipping = 0usize;
    loop {
        match reader.read_event().map_err(malformed)? {
            Event::Eof => return Err(unread("not a workbook Kule can read")),
            Event::Start(e) => match e.local_name().as_ref() {
                "t" => in_t += 1,
                n if n == skip => skipping += 1,
                _ => {}
            },
            Event::End(e) => match e.local_name().as_ref() {
                "t" => in_t = in_t.saturating_sub(1),
                n if n == skip => skipping = skipping.saturating_sub(1),
                n if n == stop => return Ok(()),
                _ => {}
            },
            Event::Text(t) if in_t > 0 && skipping == 0 => into.push_str(&t.xml10_content()),
            Event::GeneralRef(r) if in_t > 0 && skipping == 0 => into.extend(reference(&r)),
            _ => {}
        }
    }
}

fn shared_strings(xml: &str, parts: &Parts) -> Result<Vec<String>> {
    let mut reader = Reader::from_str(xml);
    let mut out = Vec::new();
    loop {
        match reader.read_event().map_err(malformed)? {
            Event::Eof => break,
            Event::Start(e) if e.local_name().as_ref() == "si" => {
                let mut s = String::new();
                text_until(&mut reader, "si", "rPh", &mut s)?;
                out.push(crate::office::clip(s, crate::table::MAX_CELL));
                if out.len().is_multiple_of(4096) {
                    parts.check_time()?;
                }
            }
            Event::Empty(e) if e.local_name().as_ref() == "si" => out.push(String::new()),
            _ => {}
        }
    }
    Ok(out)
}

/// For each cell style, in order, whether it shows a date.
fn date_styles(xml: &str) -> Result<Vec<bool>> {
    let mut reader = Reader::from_str(xml);
    let mut custom: HashMap<u32, bool> = HashMap::new();
    let mut styles = Vec::new();
    let mut in_xfs = false;
    loop {
        match reader.read_event().map_err(malformed)? {
            Event::Eof => break,
            Event::Start(e) | Event::Empty(e) => match e.local_name().as_ref() {
                "numFmt" => {
                    if let (Some(id), Some(code)) = (attr(&e, "numFmtId"), attr(&e, "formatCode"))
                        && let Ok(id) = id.parse()
                    {
                        custom.insert(id, is_date_format(&code));
                    }
                }
                "cellXfs" => in_xfs = true,
                "xf" if in_xfs => {
                    let id: u32 = attr(&e, "numFmtId")
                        .and_then(|v| v.parse().ok())
                        .unwrap_or(0);
                    let builtin = matches!(id, 14..=22 | 27..=36 | 45..=47 | 50..=58);
                    styles.push(custom.get(&id).copied().unwrap_or(builtin));
                }
                _ => {}
            },
            Event::End(e) if e.local_name().as_ref() == "cellXfs" => in_xfs = false,
            _ => {}
        }
    }
    Ok(styles)
}

/// Whether a number format shows a date: it has day or year codes outside
/// quoted text, escapes and brackets.
fn is_date_format(code: &str) -> bool {
    let mut chars = code.chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                for c in chars.by_ref() {
                    if c == '"' {
                        break;
                    }
                }
            }
            '\\' | '_' | '*' => {
                chars.next();
            }
            '[' => {
                for c in chars.by_ref() {
                    if c == ']' {
                        break;
                    }
                }
            }
            'd' | 'D' | 'y' | 'Y' => return true,
            _ => {}
        }
    }
    false
}

/// A date's serial number as `2026-07-14`, with the time when it has one.
fn date(serial: f64, date1904: bool) -> Option<String> {
    if !(0.0..2_958_466.0).contains(&serial) {
        return None;
    }
    let serial_day = serial.floor() as i64;
    let seconds = ((serial - serial.floor()) * 86_400.0).round() as i64;
    // Days since 1970-01-01. Excel's day 60 is a 29 February 1900 that
    // never was, so the days before it are one off.
    let days = if date1904 {
        serial_day - 24_107
    } else if serial_day == 60 {
        return None;
    } else if serial_day < 60 {
        serial_day - 25_568
    } else {
        serial_day - 25_569
    };
    // Howard Hinnant's civil_from_days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    let day = format!("{y:04}-{m:02}-{d:02}");
    Some(if seconds == 0 {
        day
    } else {
        let s = seconds.min(86_399);
        format!("{day} {:02}:{:02}", s / 3600, s / 60 % 60)
    })
}

fn sheet(
    xml: &str,
    strings: &[String],
    dates: &[bool],
    date1904: bool,
    grid: &mut Grid,
    parts: &Parts,
) -> Result<()> {
    let mut reader = Reader::from_str(xml);
    let mut row = 0usize;
    let mut next_row = 0usize;
    let mut next_col = 0usize;
    let mut seen = 0usize;
    loop {
        match reader.read_event().map_err(malformed)? {
            Event::Eof => break,
            Event::Start(e) | Event::Empty(e) if e.local_name().as_ref() == "row" => {
                row = attr(&e, "r")
                    .and_then(|r| r.parse::<usize>().ok())
                    .and_then(|r| r.checked_sub(1))
                    .unwrap_or(next_row);
                next_row = row + 1;
                next_col = 0;
            }
            Event::Start(e) if e.local_name().as_ref() == "c" => {
                let (r, col) = attr(&e, "r")
                    .and_then(|r| cell_at(&r))
                    .unwrap_or((row, next_col));
                next_col = col + 1;
                let kind = attr(&e, "t").unwrap_or_default();
                let style: usize = attr(&e, "s").and_then(|s| s.parse().ok()).unwrap_or(0);
                let (value, formula) = cell(&mut reader)?;
                let shown = match kind.as_str() {
                    "s" => value
                        .trim()
                        .parse::<usize>()
                        .ok()
                        .and_then(|i| strings.get(i).cloned())
                        .unwrap_or_default(),
                    "b" => match value.trim() {
                        "1" => "TRUE".into(),
                        "0" => "FALSE".into(),
                        other => other.into(),
                    },
                    "" | "n" if dates.get(style).copied().unwrap_or(false) => value
                        .trim()
                        .parse::<f64>()
                        .ok()
                        .and_then(|n| date(n, date1904))
                        .unwrap_or(value),
                    _ => value,
                };
                if grid.put(r, col, shown)
                    && let Some(text) = formula
                {
                    grid.formula(r, col, text);
                }
                if grid.spent() {
                    break;
                }
                seen += 1;
                if seen.is_multiple_of(4096) {
                    parts.check_time()?;
                }
            }
            _ => {}
        }
    }
    Ok(())
}

/// A cell's saved value (`<v>`, or inline `<is>` text) and its formula.
fn cell(reader: &mut Reader<&[u8]>) -> Result<(String, Option<String>)> {
    let mut value = String::new();
    let mut formula: Option<String> = None;
    let mut in_part: Option<&'static str> = None;
    loop {
        match reader.read_event().map_err(malformed)? {
            Event::Eof => return Err(unread("not a workbook Kule can read")),
            Event::Start(e) => match e.local_name().as_ref() {
                "v" => in_part = Some("v"),
                "f" => {
                    in_part = Some("f");
                    formula = Some(String::new());
                }
                "is" => text_until(reader, "is", "rPh", &mut value)?,
                _ => {}
            },
            Event::End(e) => match e.local_name().as_ref() {
                "c" => break,
                "v" | "f" => in_part = None,
                _ => {}
            },
            Event::Text(t) => match in_part {
                Some("v") => value.push_str(&t.xml10_content()),
                Some("f") => formula.get_or_insert_default().push_str(&t.xml10_content()),
                _ => {}
            },
            Event::GeneralRef(r) => match in_part {
                Some("v") => value.extend(reference(&r)),
                Some("f") => formula.get_or_insert_default().extend(reference(&r)),
                _ => {}
            },
            _ => {}
        }
    }
    // A shared formula's other cells have an empty <f/>: nothing to show.
    Ok((value, formula.filter(|f| !f.is_empty())))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::office::tests::archive;
    use crate::table::Formula;

    /// A workbook of two sheets: shared and inline strings, numbers, a
    /// formula, a date, a boolean and a cell far from A1.
    pub(crate) fn workbook() -> Vec<u8> {
        archive(&[
            (
                "xl/workbook.xml",
                br#"<?xml version="1.0"?><workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="Sales" sheetId="1" r:id="rId1"/><sheet name="Notes &amp; more" sheetId="2" r:id="rId2"/></sheets></workbook>"#,
            ),
            (
                "xl/_rels/workbook.xml.rels",
                br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Target="worksheets/sheet1.xml" Type="x"/><Relationship Id="rId2" Target="/xl/worksheets/sheet2.xml" Type="x"/></Relationships>"#,
            ),
            (
                "xl/sharedStrings.xml",
                br#"<sst><si><t>Month</t></si><si><r><t>Reve</t></r><r><t xml:space="preserve">nue &amp; co</t></r><rPh><t>ignored</t></rPh></si><si><t>Jan</t></si></sst>"#,
            ),
            (
                "xl/styles.xml",
                br#"<styleSheet><numFmts count="1"><numFmt numFmtId="164" formatCode="&quot;day&quot; yyyy-mm-dd"/></numFmts><cellXfs count="3"><xf numFmtId="0"/><xf numFmtId="164"/><xf numFmtId="14"/></cellXfs></styleSheet>"#,
            ),
            (
                "xl/worksheets/sheet1.xml",
                br#"<worksheet><sheetData><row r="1"><c r="A1" t="s"><v>0</v></c><c r="B1" t="s"><v>1</v></c><c r="C1" t="inlineStr"><is><t>Paid?</t></is></c></row><row r="2"><c r="A2" t="s"><v>2</v></c><c r="B2"><f>C9-B9*14.5</f><v>412300</v></c><c r="C2" t="b"><v>1</v></c><c r="D2" s="1"><v>46217</v></c><c r="E2" s="2"><v>46217.5</v></c></row><row r="5"><c r="C5"><v>7</v></c></row></sheetData></worksheet>"#,
            ),
            (
                "xl/worksheets/sheet2.xml",
                br#"<worksheet><sheetData><row><c><v>1</v></c><c><v>2</v></c></row></sheetData></worksheet>"#,
            ),
        ])
    }

    #[test]
    fn a_workbook_reads_as_excel_shows_it() {
        let sheets = sheets(&workbook()).unwrap();
        assert_eq!(sheets.len(), 2);
        let sales = &sheets[0];
        assert_eq!(sales.name, "Sales");
        assert_eq!(sales.rows[0], ["Month", "Revenue & co", "Paid?"]);
        assert_eq!(
            sales.rows[1],
            ["Jan", "412300", "TRUE", "2026-07-14", "2026-07-14 12:00"]
        );
        assert!(sales.rows[2].is_empty() && sales.rows[3].is_empty());
        assert_eq!(sales.rows[4], ["", "", "7"]);
        assert_eq!(
            sales.formulas,
            [Formula {
                row: 1,
                col: 1,
                text: "C9-B9*14.5".into()
            }]
        );
        assert_eq!(sheets[1].name, "Notes & more");
        assert_eq!(sheets[1].rows, [["1", "2"]]);
    }

    #[test]
    fn dates_count_from_excel_s_epochs() {
        assert_eq!(date(1.0, false).as_deref(), Some("1900-01-01"));
        assert_eq!(date(59.0, false).as_deref(), Some("1900-02-28"));
        assert_eq!(date(61.0, false).as_deref(), Some("1900-03-01"));
        assert_eq!(date(45_658.0, false).as_deref(), Some("2025-01-01"));
        assert_eq!(date(0.0, true).as_deref(), Some("1904-01-01"));
        assert_eq!(date(-1.0, false), None);
        assert!(is_date_format("dd/mm/yyyy"));
        assert!(!is_date_format("0.00\" days\""));
        assert!(!is_date_format("[Red]0.00"));
    }

    #[test]
    fn what_is_not_a_workbook_has_no_sheets() {
        assert!(sheets(b"not a zip").is_err());
        assert!(sheets(&archive(&[("word/document.xml", b"<w/>")])).is_err());
        let broken = archive(&[
            (
                "xl/workbook.xml",
                br#"<workbook><sheets><sheet name="A" r:id="1"/></sheets></workbook>"#,
            ),
            (
                "xl/_rels/workbook.xml.rels",
                br#"<Relationships><Relationship Id="1" Target="s.xml"/></Relationships>"#,
            ),
            ("xl/s.xml", b"<worksheet><sheetData><row><c><v>1</v></row>"),
        ]);
        assert!(sheets(&broken).is_err());
    }
}
