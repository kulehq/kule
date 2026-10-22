//! Whether a document an agent wrote does nothing but show itself when the
//! person's own app opens it, outside the sandbox and the proxy: no macros,
//! controls or embedded programs to run, nothing outside the file to fetch
//! (a template, a linked picture, another workbook, a data connection), no
//! field or formula that reaches the network or another program. What
//! cannot be read to tell counts as reaching out.

use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};

use crate::office::{Parts, attr, reference};

/// The kinds [`opens_nothing_else`] looks inside; the others are what their
/// first bytes say, and nothing in them runs or fetches.
pub const LOOKED_INSIDE: &[&str] = &["pdf", "docx", "xlsx", "pptx", "odt", "ods", "odp", "csv"];

/// The largest file of `kind` Kule looks inside; larger ones are not
/// opened with their app.
pub fn max_size(kind: &str) -> u64 {
    if kind == "pdf" {
        crate::pdf::MAX_FILE
    } else {
        crate::office::MAX_FILE as u64
    }
}

/// `Ok` when a file of `kind` (its name's extension, lower case) holding
/// `bytes` reaches nothing outside itself once its app opens it; what it
/// would reach otherwise.
pub fn opens_nothing_else(kind: &str, bytes: &[u8]) -> Result<(), String> {
    match kind {
        "docx" | "xlsx" | "pptx" => ooxml(bytes),
        "odt" | "ods" | "odp" => odf(bytes),
        "csv" => csv(bytes),
        "pdf" => crate::pdf::opens_nothing_else(bytes),
        _ => Ok(()),
    }
}

fn unreadable(e: crate::office::Unread) -> String {
    format!("Kule could not look inside it ({})", e.0)
}

/// Office Open XML: every part by name and content type, every
/// relationship, Word's fields and Excel's formulas.
fn ooxml(bytes: &[u8]) -> Result<(), String> {
    let mut parts = Parts::open(bytes).map_err(unreadable)?;
    let names = parts.names().map_err(unreadable)?;
    one_name_each(&names)?;
    for name in &names {
        let lower = name.to_ascii_lowercase();
        let file = lower.rsplit('/').next().unwrap_or(&lower);
        let in_dir = |dir: &str| lower.split('/').rev().skip(1).any(|d| d == dir);
        let held = [
            "activex",
            "embeddings",
            "externallinks",
            "querytables",
            "model",
            "customui",
        ]
        .into_iter()
        .any(in_dir)
            || file == "connections.xml"
            || file == "vbadata.xml"
            // Binary parts are macros, controls and embedded programs;
            // only a printer's settings are not.
            || (file.ends_with(".bin") && !in_dir("printersettings"));
        if held {
            return Err(format!(
                "it holds {name}: macros, a control, an embedded program or a link to data elsewhere"
            ));
        }
    }
    if let Some(xml) = parts.text("[Content_Types].xml").map_err(unreadable)? {
        content_types(&xml)?;
    }
    for name in &names {
        let lower = name.to_ascii_lowercase();
        let mut xml = || -> Result<String, String> {
            Ok(parts.text(name).map_err(unreadable)?.unwrap_or_default())
        };
        if lower.ends_with(".rels") {
            relationships(name, &xml()?)?;
        } else if lower.starts_with("word/") && lower.ends_with(".xml") {
            word_fields(name, &xml()?)?;
        } else if lower.starts_with("xl/") && lower.ends_with(".xml") {
            excel_formulas(name, &xml()?)?;
        }
    }
    Ok(())
}

/// Two parts whose names differ only in case are one part to Office, and
/// which of them it reads is not Kule's to guess.
fn one_name_each(names: &[String]) -> Result<(), String> {
    let mut seen = std::collections::HashSet::new();
    for name in names {
        if !seen.insert(name.to_ascii_lowercase()) {
            return Err(format!("it holds {name} twice"));
        }
    }
    Ok(())
}

/// Content types of macros, controls, embedded programs and links to data
/// elsewhere, as a part of any name may be declared.
const HELD_TYPES: &[&str] = &[
    "macroenabled",
    "vbaproject",
    "vbadata",
    "ms-office.activex",
    "officedocument.oleobject",
    "externallink",
    "connections+xml",
    "querytable",
];

fn content_types(xml: &str) -> Result<(), String> {
    each_element(xml, |e| {
        if let Some(kind) = attr(e, "ContentType") {
            let lower = kind.to_ascii_lowercase();
            if HELD_TYPES.iter().any(|t| lower.contains(t)) {
                return Err(format!(
                    "it declares {kind}: macros, a control, an embedded program or a link to data elsewhere"
                ));
            }
        }
        Ok(())
    })
}

/// Relationships whose target the app loads or runs even when it is inside
/// the file: macros, controls, embedded objects and documents, frames,
/// templates, other workbooks and data connections.
const HELD_RELATIONSHIPS: &[&str] = &[
    "attachedtemplate",
    "oleobject",
    "package",
    "control",
    "activexcontrol",
    "activexcontrolbinary",
    "vbaproject",
    "wordvbadata",
    "keymapcustomizations",
    "attachedtoolbars",
    "externallink",
    "externallinkpath",
    "frame",
    "subdocument",
    "connections",
    "querytable",
    "afchunk",
    "extensibility",
    "mailmergesource",
    "mailmergeheadersource",
    "recipientdata",
];

/// Every relationship of a `.rels` part: none of [`HELD_RELATIONSHIPS`],
/// and none to outside the file but a hyperlink to a web page or an email
/// address, which only a click follows.
fn relationships(part: &str, xml: &str) -> Result<(), String> {
    each_element(xml, |e| {
        if e.local_name().as_ref() != "Relationship" {
            return Ok(());
        }
        let kind = attr(e, "Type").unwrap_or_default().to_ascii_lowercase();
        let kind = kind.rsplit('/').next().unwrap_or_default();
        let target = attr(e, "Target").unwrap_or_default();
        let external = attr(e, "TargetMode").is_some_and(|m| m.eq_ignore_ascii_case("External"));
        if external && !(kind == "hyperlink" && web_or_mail(&target)) {
            return Err(format!(
                "{part} names {target} outside the file, which its app would fetch"
            ));
        }
        if HELD_RELATIONSHIPS.contains(&kind) {
            return Err(format!(
                "{part} names a {kind}, which its app would load or run"
            ));
        }
        Ok(())
    })
}

/// A web page or an email address: what a link the person clicks may lead
/// to.
fn web_or_mail(target: &str) -> bool {
    let lower = target.trim().to_ascii_lowercase();
    ["http://", "https://", "mailto:"]
        .iter()
        .any(|s| lower.starts_with(s))
}

/// Word's fields that show what the document or the computer already
/// knows (numbers, dates, references within the document, its properties)
/// and the links a click follows; never one that fetches, includes,
/// links, runs or asks.
const QUIET_FIELDS: &[&str] = &[
    "=",
    "ADVANCE",
    "AUTHOR",
    "AUTONUM",
    "AUTONUMLGL",
    "AUTONUMOUT",
    "BIBLIOGRAPHY",
    "CITATION",
    "COMMENTS",
    "COMPARE",
    "CREATEDATE",
    "DATE",
    "DOCPROPERTY",
    "EDITTIME",
    "EQ",
    "FILENAME",
    "FILESIZE",
    "FORMCHECKBOX",
    "FORMDROPDOWN",
    "FORMTEXT",
    "HYPERLINK",
    "IF",
    "INDEX",
    "KEYWORDS",
    "LASTSAVEDBY",
    "LISTNUM",
    "NOTEREF",
    "NUMCHARS",
    "NUMPAGES",
    "NUMWORDS",
    "PAGE",
    "PAGEREF",
    "PRINTDATE",
    "REF",
    "REVNUM",
    "SAVEDATE",
    "SECTION",
    "SECTIONPAGES",
    "SEQ",
    "STYLEREF",
    "SUBJECT",
    "SYMBOL",
    "TA",
    "TC",
    "TEMPLATE",
    "TIME",
    "TITLE",
    "TOA",
    "TOC",
    "USERADDRESS",
    "USERINITIALS",
    "USERNAME",
    "XE",
];

fn quiet_field(part: &str, instruction: &str) -> Result<(), String> {
    let instruction = instruction.trim();
    if instruction.is_empty() {
        return Ok(());
    }
    let word = if instruction.starts_with('=') {
        "=".to_owned()
    } else {
        instruction
            .split(|c: char| c.is_whitespace() || c == '\\')
            .next()
            .unwrap_or_default()
            .to_ascii_uppercase()
    };
    if QUIET_FIELDS.contains(&word.as_str()) {
        Ok(())
    } else {
        Err(format!(
            "{part} has a {word} field, which Word could use to fetch, include or run something"
        ))
    }
}

/// Every field of a Word part, simple (`w:fldSimple`) or complex (its
/// instruction between `w:fldChar` begin and separate, in as many runs as
/// it takes, fields inside fields too).
fn word_fields(part: &str, xml: &str) -> Result<(), String> {
    let mut reader = Reader::from_str(xml);
    // The instruction of each field begun and not yet separated or ended.
    let mut open: Vec<Option<String>> = Vec::new();
    let mut in_instruction = 0usize;
    let mut loose = String::new();
    let malformed = |_| format!("{part}: Kule could not read it");
    loop {
        match reader.read_event().map_err(malformed)? {
            Event::Eof => break,
            Event::Start(e) | Event::Empty(e) if e.local_name().as_ref() == "fldSimple" => {
                quiet_field(part, &attr(&e, "instr").unwrap_or_default())?;
            }
            Event::Start(e) | Event::Empty(e) if e.local_name().as_ref() == "fldChar" => {
                match attr(&e, "fldCharType").as_deref() {
                    Some("begin") => open.push(Some(String::new())),
                    Some("separate") => {
                        if let Some(top) = open.last_mut()
                            && let Some(instruction) = top.take()
                        {
                            quiet_field(part, &instruction)?;
                        }
                    }
                    Some("end") => {
                        if let Some(Some(instruction)) = open.pop() {
                            quiet_field(part, &instruction)?;
                        }
                    }
                    _ => {}
                }
            }
            Event::Start(e) if matches!(e.local_name().as_ref(), "instrText" | "delInstrText") => {
                in_instruction += 1;
            }
            Event::End(e) if matches!(e.local_name().as_ref(), "instrText" | "delInstrText") => {
                in_instruction = in_instruction.saturating_sub(1);
                // An instruction outside any begun field is read alone.
                if in_instruction == 0 && !loose.is_empty() {
                    quiet_field(part, &std::mem::take(&mut loose))?;
                }
            }
            Event::Text(t) if in_instruction > 0 => {
                push_instruction(&mut open, &mut loose, &t.xml10_content())
            }
            Event::CData(t) if in_instruction > 0 => {
                push_instruction(&mut open, &mut loose, &t.into_inner())
            }
            Event::GeneralRef(r) if in_instruction > 0 => {
                let c: String = reference(&r).into_iter().collect();
                push_instruction(&mut open, &mut loose, &c)
            }
            _ => {}
        }
    }
    // A field begun and never ended is checked as far as it goes.
    for instruction in open.into_iter().flatten() {
        quiet_field(part, &instruction)?;
    }
    Ok(())
}

fn push_instruction(open: &mut [Option<String>], loose: &mut String, text: &str) {
    match open.last_mut() {
        Some(Some(instruction)) => instruction.push_str(text),
        _ => loose.push_str(text),
    }
}

/// Functions that fetch from the network, ask another program for data or
/// run code: Excel's and LibreOffice's.
const REACHING_FUNCTIONS: &[&str] = &[
    "IMAGE",
    "WEBSERVICE",
    "RTD",
    "CALL",
    "REGISTER",
    "REGISTER.ID",
    "EXEC",
    "DDE",
    "STOCKHISTORY",
];

/// A formula that names none of [`REACHING_FUNCTIONS`], no other workbook
/// (`[1]Sheet!A1`, `'file'#$Sheet.A1`) and no other program (`app|topic`).
fn quiet_formula(part: &str, formula: &str) -> Result<(), String> {
    let refuse = || {
        Err(format!(
            "{part} has a formula that reaches outside the file: {}",
            formula.chars().take(80).collect::<String>()
        ))
    };
    let chars: Vec<char> = formula.chars().collect();
    let mut quoted = false;
    let mut word = String::new();
    for (i, &c) in chars.iter().enumerate() {
        if c == '"' {
            quoted = !quoted;
            word.clear();
            continue;
        }
        if quoted {
            continue;
        }
        match c {
            '|' => return refuse(),
            '[' if chars
                .get(i + 1)
                .is_some_and(|n| n.is_ascii_digit() || *n == '\'') =>
            {
                return refuse();
            }
            '#' if i > 0 && chars[i - 1] == '\'' => return refuse(),
            '(' => {
                let name = word.to_ascii_uppercase();
                let name = name
                    .strip_prefix("_XLFN.")
                    .or_else(|| name.strip_prefix("_XLWS."))
                    .or_else(|| name.strip_prefix("COM.MICROSOFT."))
                    .or_else(|| name.strip_prefix("ORG.OPENOFFICE."))
                    .unwrap_or(&name);
                if REACHING_FUNCTIONS.contains(&name) {
                    return refuse();
                }
                word.clear();
            }
            c if c.is_alphanumeric() || c == '_' || c == '.' => word.push(c),
            _ => word.clear(),
        }
    }
    Ok(())
}

/// Every formula of an Excel part: cells' (`<f>`) and defined names'.
fn excel_formulas(part: &str, xml: &str) -> Result<(), String> {
    let mut reader = Reader::from_str(xml);
    let mut depth = 0usize;
    let mut formula = String::new();
    let malformed = |_| format!("{part}: Kule could not read it");
    loop {
        match reader.read_event().map_err(malformed)? {
            Event::Eof => break,
            Event::Start(e) if matches!(e.local_name().as_ref(), "f" | "definedName") => {
                depth += 1;
            }
            Event::End(e) if matches!(e.local_name().as_ref(), "f" | "definedName") => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    quiet_formula(part, &std::mem::take(&mut formula))?;
                }
            }
            Event::Text(t) if depth > 0 => formula.push_str(&t.xml10_content()),
            Event::CData(t) if depth > 0 => formula.push_str(&t.into_inner()),
            Event::GeneralRef(r) if depth > 0 => formula.extend(reference(&r)),
            _ => {}
        }
    }
    Ok(())
}

/// OpenDocument: no Basic or scripts, no embedded objects, and in its XML
/// no link but a hyperlink to outside the file, no linked section or
/// table, no DDE, no event that runs a macro, and no formula that reaches
/// out.
fn odf(bytes: &[u8]) -> Result<(), String> {
    let mut parts = Parts::open(bytes).map_err(unreadable)?;
    let names = parts.names().map_err(unreadable)?;
    one_name_each(&names)?;
    for name in &names {
        let lower = name.to_ascii_lowercase();
        if lower.starts_with("basic/")
            || lower.starts_with("scripts/")
            || lower.starts_with("object ")
        {
            return Err(format!(
                "it holds {name}: macros, scripts or an embedded object"
            ));
        }
    }
    for name in &names {
        if !name.to_ascii_lowercase().ends_with(".xml") {
            continue;
        }
        let xml = parts.text(name).map_err(unreadable)?.unwrap_or_default();
        each_element(&xml, |e| odf_element(name, e))?;
    }
    Ok(())
}

/// Elements that link data in from elsewhere or run a macro.
const HELD_ODF: &[&str] = &[
    "dde-source",
    "dde-connection-decl",
    "dde-link",
    "dde-connection",
    "table-source",
    "section-source",
    "cell-range-source",
    "event-listener",
];

fn odf_element(part: &str, e: &BytesStart) -> Result<(), String> {
    let local = e.local_name();
    let local = local.as_ref().to_owned();
    if HELD_ODF.contains(&local.as_str()) {
        return Err(format!(
            "{part} has a {local}, which links in data from elsewhere or runs a macro"
        ));
    }
    if let Some(href) = attr(e, "href") {
        // A hyperlink may lead to a page or an address; anything else
        // (a picture, a frame, an object) must be inside the file.
        let link = local == "a" && web_or_mail(&href);
        if !link && !inside_the_file(&href) {
            return Err(format!(
                "{part} names {href} outside the file, which its app would fetch"
            ));
        }
    }
    if let Some(formula) = attr(e, "formula") {
        quiet_formula(part, &formula)?;
    }
    Ok(())
}

/// A reference into the file itself: a part's relative name or a place
/// in the document.
fn inside_the_file(href: &str) -> bool {
    let href = href.trim();
    let href = href.strip_prefix("./").unwrap_or(href);
    if href.starts_with('#') {
        return true;
    }
    !href.is_empty()
        && !href.starts_with('/')
        && !href.starts_with('\\')
        && !href.contains(':')
        && !href.split(['/', '\\']).any(|s| s == "..")
}

/// A spreadsheet app reads a cell that starts as a formula does as one, so
/// none may: `=`, `+`, `@`, or `-` before anything but a number.
fn csv(bytes: &[u8]) -> Result<(), String> {
    let text = String::from_utf8_lossy(bytes);
    let mut quoted = false;
    let mut cell = String::new();
    let check = |cell: &str| {
        let lead = cell.trim_start_matches([' ', '\t', '\r', '\n', '\u{feff}']);
        let mut chars = lead.chars();
        let formula = match chars.next() {
            Some('=' | '+' | '@') => true,
            Some('-') => !chars.next().is_some_and(|c| c.is_ascii_digit() || c == '.'),
            _ => false,
        };
        if formula && lead.len() > 1 {
            Err(format!(
                "a cell starts as a formula, which its app would run: {}",
                lead.chars().take(80).collect::<String>()
            ))
        } else {
            Ok(())
        }
    };
    let mut prev_quote = false;
    for c in text.chars() {
        match c {
            '"' => {
                // A doubled quote inside a quoted cell is one quote.
                if quoted || !prev_quote {
                    quoted = !quoted;
                }
                prev_quote = true;
                continue;
            }
            ',' | ';' | '\t' | '\n' | '\r' if !quoted => {
                check(&cell)?;
                cell.clear();
            }
            c => cell.push(c),
        }
        prev_quote = false;
    }
    check(&cell)
}

/// `f` for every start and empty element of `xml`, which must read whole.
fn each_element(
    xml: &str,
    mut f: impl FnMut(&BytesStart) -> Result<(), String>,
) -> Result<(), String> {
    let mut reader = Reader::from_str(xml);
    loop {
        match reader
            .read_event()
            .map_err(|_| "Kule could not read a part of it".to_owned())?
        {
            Event::Eof => return Ok(()),
            Event::Start(e) | Event::Empty(e) => f(&e)?,
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::opens_nothing_else;
    use crate::office::tests::archive;

    const TYPES: &[u8] = br#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>"#;
    const ROOT_RELS: &[u8] = br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#;

    fn document(body: &str) -> Vec<u8> {
        format!(
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>{body}</w:body></w:document>"#
        )
        .into_bytes()
    }

    fn docx(extra: &[(&str, &[u8])], body: &str) -> Vec<u8> {
        let doc = document(body);
        let mut parts: Vec<(&str, &[u8])> = vec![
            ("[Content_Types].xml", TYPES),
            ("_rels/.rels", ROOT_RELS),
            ("word/document.xml", &doc),
        ];
        parts.extend_from_slice(extra);
        archive(&parts)
    }

    fn rels(relationship: &str) -> Vec<u8> {
        format!(
            r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">{relationship}</Relationships>"#
        )
        .into_bytes()
    }

    #[test]
    fn a_plain_document_and_its_links_open() {
        let links = rels(
            r#"<Relationship Id="rId9" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink" Target="https://example.com/a" TargetMode="External"/>"#,
        );
        let body = r#"<w:p><w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText> PAGE </w:instrText></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r><w:r><w:t>1</w:t></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r></w:p><w:fldSimple w:instr=" HYPERLINK &quot;https://example.com&quot; "/>"#;
        let bytes = docx(&[("word/_rels/document.xml.rels", &links)], body);
        assert_eq!(opens_nothing_else("docx", &bytes), Ok(()));
    }

    #[test]
    fn a_template_from_elsewhere_is_refused() {
        let template = rels(
            r#"<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/attachedTemplate" Target="https://example.com/t.dotm" TargetMode="External"/>"#,
        );
        let bytes = docx(&[("word/_rels/settings.xml.rels", &template)], "");
        let why = opens_nothing_else("docx", &bytes).unwrap_err();
        assert!(why.contains("https://example.com/t.dotm"), "{why}");
    }

    #[test]
    fn a_picture_from_elsewhere_is_refused() {
        let picture = rels(
            r#"<Relationship Id="rId4" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="https://example.com/p.png" TargetMode="External"/>"#,
        );
        let bytes = docx(&[("word/_rels/document.xml.rels", &picture)], "");
        assert!(opens_nothing_else("docx", &bytes).is_err());
        // A hyperlink to a file on the computer is no web page.
        let file = rels(
            r#"<Relationship Id="rId4" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink" Target="file:///etc/passwd" TargetMode="external"/>"#,
        );
        let bytes = docx(&[("word/_rels/document.xml.rels", &file)], "");
        assert!(opens_nothing_else("docx", &bytes).is_err());
    }

    #[test]
    fn macros_controls_and_embedded_programs_are_refused() {
        for name in [
            "word/vbaProject.bin",
            "word/activeX/activeX1.xml",
            "word/embeddings/oleObject1.bin",
            "xl/externalLinks/externalLink1.xml",
            "xl/connections.xml",
            "customUI/customUI14.xml",
            "word/attachedToolbars.bin",
        ] {
            let bytes = docx(&[(name, b"<x/>")], "");
            assert!(opens_nothing_else("docx", &bytes).is_err(), "{name}");
        }
        let printer = docx(&[("word/printerSettings/printerSettings1.bin", b"\0")], "");
        assert_eq!(opens_nothing_else("docx", &printer), Ok(()));
        // Declared as macros under another name.
        let types = br#"<Types><Override PartName="/word/a.xml" ContentType="application/vnd.ms-word.document.macroEnabled.main+xml"/></Types>"#;
        let doc = document("");
        let bytes = archive(&[("[Content_Types].xml", types), ("word/document.xml", &doc)]);
        assert!(opens_nothing_else("docx", &bytes).is_err());
        // Pointed at by a relationship inside the file.
        let ole = rels(
            r#"<Relationship Id="rId5" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/oleObject" Target="embeddings/x.dat"/>"#,
        );
        let bytes = docx(&[("word/_rels/document.xml.rels", &ole)], "");
        assert!(opens_nothing_else("docx", &bytes).is_err());
    }

    #[test]
    fn fields_that_fetch_include_or_run_are_refused() {
        // Split over runs, as Word writes them.
        let dde = r#"<w:p><w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText>DDE</w:instrText></w:r><w:r><w:instrText>AUTO cmd "/c calc"</w:instrText></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r></w:p>"#;
        assert!(opens_nothing_else("docx", &docx(&[], dde)).is_err());
        let include =
            r#"<w:fldSimple w:instr=" INCLUDEPICTURE &quot;https://example.com/p.png&quot; \d "/>"#;
        assert!(opens_nothing_else("docx", &docx(&[], include)).is_err());
        // A field inside the instruction of another.
        let nested = r#"<w:p><w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText>IF 1 = 1 "</w:instrText></w:r><w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText>INCLUDETEXT "https://example.com/x"</w:instrText></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r><w:r><w:instrText>"</w:instrText></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r></w:p>"#;
        assert!(opens_nothing_else("docx", &docx(&[], nested)).is_err());
        let macro_button = r#"<w:fldSimple w:instr="MACROBUTTON Run Click"/>"#;
        assert!(opens_nothing_else("docx", &docx(&[], macro_button)).is_err());
    }

    #[test]
    fn formulas_that_reach_out_are_refused() {
        let sheet = |f: &str| {
            let xml = format!(
                r#"<worksheet><sheetData><row><c r="A1"><f>{f}</f></c></row></sheetData></worksheet>"#
            );
            archive(&[("xl/worksheets/sheet1.xml", xml.as_bytes())])
        };
        for f in [
            r#"_xlfn.IMAGE("https://example.com/p.png")"#,
            r#"WEBSERVICE("https://example.com")"#,
            "cmd|' /c calc'!A0",
            "[1]Sheet1!A1",
        ] {
            assert!(opens_nothing_else("xlsx", &sheet(f)).is_err(), "{f}");
        }
        for f in [
            "SUM(A1:A3)",
            r#"HYPERLINK("https://example.com","x")"#,
            r#"IF(A1="IMAGE(",1,2)"#,
            "Table1[Amount]",
        ] {
            assert_eq!(opens_nothing_else("xlsx", &sheet(f)), Ok(()), "{f}");
        }
    }

    #[test]
    fn open_documents_keep_to_themselves() {
        let content = |body: &str| {
            format!(
                r#"<office:document-content xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" xmlns:draw="urn:oasis:names:tc:opendocument:xmlns:drawing:1.0" xmlns:xlink="http://www.w3.org/1999/xlink" xmlns:table="urn:oasis:names:tc:opendocument:xmlns:table:1.0"><office:body>{body}</office:body></office:document-content>"#
            )
        };
        let odt = |body: &str| {
            let xml = content(body);
            archive(&[
                ("mimetype", b"application/vnd.oasis.opendocument.text"),
                ("content.xml", xml.as_bytes()),
            ])
        };
        let fine = odt(
            r#"<text:p><text:a xlink:href="https://example.com">x</text:a></text:p><draw:frame><draw:image xlink:href="Pictures/a.png"/></draw:frame>"#,
        );
        assert_eq!(opens_nothing_else("odt", &fine), Ok(()));
        for body in [
            r#"<draw:frame><draw:image xlink:href="https://example.com/p.png"/></draw:frame>"#,
            r#"<draw:frame><draw:image xlink:href="../outside.png"/></draw:frame>"#,
            r#"<text:section><text:section-source xlink:href="https://example.com/s.odt"/></text:section>"#,
            r#"<table:table-cell table:formula="of:=WEBSERVICE(&quot;https://example.com&quot;)"/>"#,
            r#"<text:a xlink:href="vnd.sun.star.script:Standard.Module1.Main?language=Basic">x</text:a>"#,
            // Data from another program, named without a link.
            r#"<text:dde-connection-decls><text:dde-connection-decl office:name="x" office:dde-application="soffice" office:dde-topic="file:///etc/hosts" office:dde-item="a"/></text:dde-connection-decls>"#,
        ] {
            assert!(opens_nothing_else("odt", &odt(body)).is_err(), "{body}");
        }
        let basic = archive(&[("Basic/Standard/Module1.xml", b"<x/>")]);
        assert!(opens_nothing_else("odt", &basic).is_err());
        let object = archive(&[("Object 1/content.xml", b"<x/>")]);
        assert!(opens_nothing_else("ods", &object).is_err());
    }

    #[test]
    fn a_csv_cell_that_reads_as_a_formula_is_refused() {
        assert_eq!(
            opens_nothing_else(
                "csv",
                b"name,amount\nTea,-3.50\n\"a, \"\"quoted\"\" b\",-.5\n"
            ),
            Ok(())
        );
        for text in [
            "a,=IMAGE(\"https://example.com/p.png\")\n",
            "a,\"=HYPERLINK(\"\"https://example.com\"\")\"\n",
            "+cmd|' /c calc'!A0\n",
            "a;@SUM(1)\n",
            "a\t-cmd|x\n",
        ] {
            assert!(
                opens_nothing_else("csv", text.as_bytes()).is_err(),
                "{text}"
            );
        }
    }

    #[test]
    fn what_cannot_be_read_does_not_open() {
        assert!(opens_nothing_else("docx", b"PK\x03\x04 not really").is_err());
        let twice = archive(&[
            ("word/document.xml", b"<a/>"),
            ("Word/Document.xml", b"<b/>"),
        ]);
        assert!(opens_nothing_else("docx", &twice).is_err());
        let broken = docx(
            &[(
                "word/_rels/document.xml.rels",
                b"<Relationships><Relationship",
            )],
            "",
        );
        assert!(opens_nothing_else("docx", &broken).is_err());
    }
}
