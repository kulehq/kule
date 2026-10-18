//! Text: what is text at all, which language a name says it is, and a
//! formatted view of JSON or XML that came packed onto a line or two.

/// Formatted views longer than this are not offered.
const MAX_FORMATTED: usize = 4 << 20;

/// `bytes` as text, or `None` for a binary file. `cut` says the bytes stop
/// short of the file's end; a cut can fall inside a character, and what is
/// whole of it is kept.
pub fn decode(bytes: Vec<u8>, cut: bool) -> Option<String> {
    if bytes.contains(&0) {
        return None;
    }
    match String::from_utf8(bytes) {
        Ok(text) => Some(text),
        Err(e) if cut && e.utf8_error().error_len().is_none() => {
            let valid = e.utf8_error().valid_up_to();
            let mut bytes = e.into_bytes();
            bytes.truncate(valid);
            String::from_utf8(bytes).ok()
        }
        Err(_) => None,
    }
}

fn extension(path: &str) -> Option<String> {
    let name = path.rsplit('/').next().unwrap_or(path);
    name.rsplit_once('.')
        .filter(|(stem, _)| !stem.is_empty())
        .map(|(_, ext)| ext.to_ascii_lowercase())
}

/// The language a file named `path` is written in, for the window to say.
pub fn language(path: &str) -> Option<&'static str> {
    let name = path.rsplit('/').next().unwrap_or(path);
    match name {
        "Dockerfile" | "Containerfile" => return Some("Dockerfile"),
        "Makefile" | "GNUmakefile" => return Some("Makefile"),
        _ => {}
    }
    Some(match extension(path)?.as_str() {
        "rs" => "Rust",
        "ts" | "mts" | "cts" => "TypeScript",
        "tsx" => "TSX",
        "js" | "mjs" | "cjs" => "JavaScript",
        "jsx" => "JSX",
        "json" | "jsonc" => "JSON",
        "xml" | "xsd" | "xsl" | "xslt" | "plist" | "csproj" | "fsproj" | "props" | "targets" => {
            "XML"
        }
        "svg" => "SVG",
        "html" | "htm" => "HTML",
        "css" => "CSS",
        "scss" => "SCSS",
        "py" => "Python",
        "go" => "Go",
        "cs" => "C#",
        "fs" => "F#",
        "java" => "Java",
        "kt" | "kts" => "Kotlin",
        "swift" => "Swift",
        "c" | "h" => "C",
        "cpp" | "cc" | "hpp" => "C++",
        "rb" => "Ruby",
        "php" => "PHP",
        "sh" | "bash" | "zsh" => "Shell",
        "sql" => "SQL",
        "toml" => "TOML",
        "yaml" | "yml" => "YAML",
        "ini" | "cfg" => "INI",
        "csv" => "CSV",
        "tsv" => "TSV",
        "txt" | "text" | "log" => "Text",
        _ => return None,
    })
}

/// Whether a file named `path` is Markdown.
pub fn is_markdown(path: &str) -> bool {
    matches!(extension(path).as_deref(), Some("md" | "markdown"))
}

/// A formatted view of `text` when it is JSON or XML packed so tight that
/// formatting it at least doubles its lines; `None` otherwise.
pub fn formatted(path: &str, text: &str) -> Option<String> {
    let pretty = match language(path)? {
        "JSON" => json(text)?,
        "XML" | "SVG" => xml(text)?,
        _ => return None,
    };
    let lines = |t: &str| t.lines().count().max(1);
    (pretty.len() <= MAX_FORMATTED && lines(&pretty) >= 2 * lines(text)).then_some(pretty)
}

/// Valid JSON, indented two spaces a level. Keys keep their order and
/// values their spelling: only the space between tokens changes.
fn json(text: &str) -> Option<String> {
    serde_json::from_str::<serde::de::IgnoredAny>(text).ok()?;
    let mut out = String::with_capacity(text.len() * 2);
    let mut depth = 0usize;
    let mut chars = text.chars().peekable();
    let newline = |out: &mut String, depth: usize| {
        out.push('\n');
        out.extend(std::iter::repeat_n("  ", depth));
    };
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                out.push('"');
                while let Some(c) = chars.next() {
                    out.push(c);
                    match c {
                        '\\' => out.extend(chars.next()),
                        '"' => break,
                        _ => {}
                    }
                }
            }
            '{' | '[' => {
                out.push(c);
                while chars.peek().is_some_and(|c| c.is_whitespace()) {
                    chars.next();
                }
                if matches!(chars.peek(), Some('}' | ']')) {
                    out.extend(chars.next());
                } else {
                    depth += 1;
                    newline(&mut out, depth);
                }
            }
            '}' | ']' => {
                depth = depth.saturating_sub(1);
                newline(&mut out, depth);
                out.push(c);
            }
            ',' => {
                out.push(',');
                newline(&mut out, depth);
            }
            ':' => out.push_str(": "),
            c if c.is_whitespace() => {}
            c => out.push(c),
        }
    }
    out.push('\n');
    Some(out)
}

/// Well-formed XML, an element a line, indented two spaces a level. Text
/// is trimmed; nothing is expanded, so a document type's entities stay as
/// they were written.
fn xml(text: &str) -> Option<String> {
    use quick_xml::events::{BytesText, Event};
    let mut reader = quick_xml::Reader::from_str(text);
    let mut writer = quick_xml::Writer::new_with_indent(Vec::new(), b' ', 2);
    let mut depth = 0usize;
    let mut elements = 0usize;
    // Text and the references inside it come as separate events; they are
    // written as one, as written, so the writer does not break the line
    // inside them.
    let mut pending = String::new();
    loop {
        let event = reader.read_event().ok()?;
        match &event {
            Event::Text(t) => {
                pending.push_str(t.as_ref());
                continue;
            }
            Event::GeneralRef(r) => {
                pending.push('&');
                pending.push_str(r.as_ref());
                pending.push(';');
                continue;
            }
            _ => {}
        }
        let trimmed = pending.trim();
        if !trimmed.is_empty() {
            writer
                .write_event(Event::Text(BytesText::from_escaped(trimmed)))
                .ok()?;
        }
        pending.clear();
        match event {
            Event::Eof => break,
            event => {
                match &event {
                    Event::Start(_) => {
                        depth += 1;
                        elements += 1;
                    }
                    Event::End(_) => depth = depth.checked_sub(1)?,
                    Event::Empty(_) => elements += 1,
                    _ => {}
                }
                writer.write_event(event).ok()?;
            }
        }
    }
    if depth != 0 || elements == 0 {
        return None;
    }
    let mut out = String::from_utf8(writer.into_inner()).ok()?;
    out.push('\n');
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_is_what_decodes_whole() {
        assert_eq!(decode(b"hi\n".to_vec(), false).as_deref(), Some("hi\n"));
        assert_eq!(decode(b"a\0b".to_vec(), false), None);
        assert_eq!(decode(vec![0xff, b'a'], false), None);
        // "ç" is two bytes; a cut after the first keeps what is whole.
        let mut bytes = "aç".as_bytes().to_vec();
        bytes.pop();
        assert_eq!(decode(bytes.clone(), true).as_deref(), Some("a"));
        assert_eq!(decode(bytes, false), None);
    }

    #[test]
    fn names_say_the_language() {
        for (path, lang) in [
            ("src/main.rs", Some("Rust")),
            ("a/B.JSON", Some("JSON")),
            ("Dockerfile", Some("Dockerfile")),
            ("x/.gitignore", None),
            ("notes", None),
            ("app.csproj", Some("XML")),
        ] {
            assert_eq!(language(path), lang, "{path}");
        }
        assert!(is_markdown("docs/Report.MD"));
        assert!(!is_markdown("docs/report.mdx"));
    }

    #[test]
    fn packed_json_is_formatted_in_its_own_order() {
        let packed = r#"{"z":1,"a":[1,2,{"q":"a, \"b\": {c}"}],"e":{},"f":[ ]}"#;
        assert_eq!(
            formatted("x.json", packed).unwrap(),
            "{\n  \"z\": 1,\n  \"a\": [\n    1,\n    2,\n    {\n      \"q\": \"a, \\\"b\\\": {c}\"\n    }\n  ],\n  \"e\": {},\n  \"f\": []\n}\n"
        );
        // Already formatted, not JSON, or not named JSON: nothing offered.
        let pretty = formatted("x.json", packed).unwrap();
        assert_eq!(formatted("x.json", &pretty), None);
        assert_eq!(formatted("x.json", r#"{"a": "#), None);
        assert_eq!(formatted("x.txt", packed), None);
    }

    #[test]
    fn packed_xml_is_formatted_and_nothing_expands() {
        let packed = r#"<?xml version="1.0"?><!DOCTYPE r [<!ENTITY x "boom">]><r a="1"><b>t &x;</b><c/></r>"#;
        let pretty = formatted("x.xml", packed).unwrap();
        assert!(pretty.contains("\n  <b>t &x;</b>\n"), "{pretty}");
        assert!(pretty.contains("\n  <c/>\n"), "{pretty}");
        assert!(!pretty.contains("boom</b>"), "{pretty}");
        // Not well formed: nothing offered.
        assert_eq!(formatted("x.xml", "<r><b></r>"), None);
        assert_eq!(formatted("x.xml", "<r><b>"), None);
        assert_eq!(formatted("x.xml", "just text"), None);
    }
}
