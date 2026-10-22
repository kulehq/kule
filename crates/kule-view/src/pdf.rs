//! Whether a PDF does nothing but show itself when the person's own app
//! opens it: no script, no program to start, no form that sends itself
//! anywhere, no other document or file it fetches or carries, nothing that
//! runs as it opens. Every object is read, those packed into compressed
//! object streams too, each inflated through a cap; names are read as PDF
//! spells them (`/J#61vaScript` is `/JavaScript`). What cannot be read to
//! tell, an encrypted file among them, counts as reaching out.

use std::io::Read;
use std::time::{Duration, Instant};

use flate2::read::ZlibDecoder;

/// Larger PDFs are not looked inside.
pub const MAX_FILE: u64 = 100 << 20;
/// The most one object stream may inflate to: Acrobat packs a few hundred
/// objects in one, which can come to tens of megabytes.
const MAX_STREAM: u64 = 64 << 20;
/// The most all object streams together may inflate to.
const MAX_TOTAL: u64 = 128 << 20;
/// How long reading one file may take.
const TIME: Duration = Duration::from_secs(5);
/// How deep arrays and dictionaries may nest.
const MAX_DEPTH: usize = 64;

/// Names that start a script, a program, a form's sending, another
/// document or a file the PDF carries, media that can run, or an action
/// that runs by itself (`AA`).
const HELD_NAMES: &[&str] = &[
    "JavaScript",
    "JS",
    "Launch",
    "SubmitForm",
    "ImportData",
    "GoToR",
    "GoToE",
    "RichMedia",
    "RichMediaExecute",
    "EmbeddedFile",
    "EmbeddedFiles",
    "FileAttachment",
    "XFA",
    "AA",
    "Rendition",
    "Movie",
    "Sound",
    "3D",
];

/// The actions a PDF may take as it opens: going to a page, or a menu
/// command (`/FullScreen`).
const OPENING_ACTIONS: &[&str] = &["GoTo", "Named"];

#[derive(Debug, Clone, PartialEq)]
enum Value {
    Name(String),
    Int(i64),
    Ref(i64),
    Array(Vec<Value>),
    Dict(Vec<(String, Value)>),
    Other,
}

impl Value {
    fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Value::Dict(entries) => entries.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    fn name(&self) -> Option<&str> {
        match self {
            Value::Name(n) => Some(n),
            _ => None,
        }
    }
}

/// Bytes read a token at a time, every name seen noted.
struct Lexer<'a> {
    bytes: &'a [u8],
    at: usize,
    names: &'a mut Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Name(String),
    Word(Vec<u8>),
    DictOpen,
    DictClose,
    ArrayOpen,
    ArrayClose,
    Str,
}

fn white(b: u8) -> bool {
    matches!(b, 0 | b'\t' | b'\n' | 0x0c | b'\r' | b' ')
}

fn delimiter(b: u8) -> bool {
    matches!(
        b,
        b'(' | b')' | b'<' | b'>' | b'[' | b']' | b'{' | b'}' | b'/' | b'%'
    )
}

impl Lexer<'_> {
    fn skip_white(&mut self) {
        while let Some(&b) = self.bytes.get(self.at) {
            if white(b) {
                self.at += 1;
            } else if b == b'%' {
                while self
                    .bytes
                    .get(self.at)
                    .is_some_and(|&b| b != b'\n' && b != b'\r')
                {
                    self.at += 1;
                }
            } else {
                break;
            }
        }
    }

    /// The next token; `Err` where the bytes do not read as PDF.
    fn token(&mut self) -> Result<Option<Token>, ()> {
        self.skip_white();
        let Some(&b) = self.bytes.get(self.at) else {
            return Ok(None);
        };
        let next = self.bytes.get(self.at + 1).copied();
        self.at += 1;
        Ok(Some(match b {
            b'<' if next == Some(b'<') => {
                self.at += 1;
                Token::DictOpen
            }
            b'>' if next == Some(b'>') => {
                self.at += 1;
                Token::DictClose
            }
            b'[' => Token::ArrayOpen,
            b']' => Token::ArrayClose,
            b'<' => {
                while self.bytes.get(self.at).is_some_and(|&b| b != b'>') {
                    self.at += 1;
                }
                if self.at >= self.bytes.len() {
                    return Err(());
                }
                self.at += 1;
                Token::Str
            }
            b'(' => {
                let mut depth = 1;
                while depth > 0 {
                    match self.bytes.get(self.at) {
                        None => return Err(()),
                        Some(b'\\') => self.at += 1,
                        Some(b'(') => depth += 1,
                        Some(b')') => depth -= 1,
                        _ => {}
                    }
                    self.at += 1;
                }
                Token::Str
            }
            b'/' => {
                let start = self.at;
                while self
                    .bytes
                    .get(self.at)
                    .is_some_and(|&b| !white(b) && !delimiter(b))
                {
                    self.at += 1;
                }
                let name = decode_name(&self.bytes[start..self.at]);
                self.names.push(name.clone());
                Token::Name(name)
            }
            b')' | b'>' | b'{' | b'}' => Token::Word(vec![b]),
            _ => {
                let start = self.at - 1;
                while self
                    .bytes
                    .get(self.at)
                    .is_some_and(|&b| !white(b) && !delimiter(b))
                {
                    self.at += 1;
                }
                Token::Word(self.bytes[start..self.at].to_vec())
            }
        }))
    }

    /// A value starting with `first`; an integer followed by `G R` is a
    /// reference.
    fn value(&mut self, first: Token, depth: usize) -> Result<Value, ()> {
        if depth > MAX_DEPTH {
            return Err(());
        }
        Ok(match first {
            Token::Name(n) => Value::Name(n),
            Token::Str => Value::Other,
            Token::ArrayOpen => {
                let mut items = Vec::new();
                loop {
                    match self.token()?.ok_or(())? {
                        Token::ArrayClose => break,
                        t => items.push(self.value(t, depth + 1)?),
                    }
                }
                Value::Array(fold_refs(items))
            }
            Token::DictOpen => {
                let mut entries = Vec::new();
                loop {
                    match self.token()?.ok_or(())? {
                        Token::DictClose => break,
                        Token::Name(key) => {
                            let first = self.token()?.ok_or(())?;
                            let value = self.value(first, depth + 1)?;
                            // `/Key 12 0 R`: the number and the rest.
                            let value = match value {
                                Value::Int(n) => self.maybe_ref(n)?,
                                v => v,
                            };
                            entries.push((key, value));
                        }
                        _ => return Err(()),
                    }
                }
                Value::Dict(entries)
            }
            Token::Word(w) => match std::str::from_utf8(&w).ok().and_then(|s| s.parse().ok()) {
                Some(n) => Value::Int(n),
                None => Value::Other,
            },
            Token::DictClose | Token::ArrayClose => return Err(()),
        })
    }

    /// `n` alone, or `n G R`.
    fn maybe_ref(&mut self, n: i64) -> Result<Value, ()> {
        let back = self.at;
        let names = self.names.len();
        if let Ok(Some(Token::Word(g))) = self.token()
            && g.iter().all(u8::is_ascii_digit)
            && let Ok(Some(Token::Word(r))) = self.token()
            && r == b"R"
        {
            return Ok(Value::Ref(n));
        }
        self.at = back;
        self.names.truncate(names);
        Ok(Value::Int(n))
    }
}

/// `n G R` in an array, read as the reference it is.
fn fold_refs(items: Vec<Value>) -> Vec<Value> {
    let mut out = Vec::with_capacity(items.len());
    let mut i = 0;
    while i < items.len() {
        if let (Value::Int(n), Some(Value::Int(_)), Some(Value::Other)) =
            (&items[i], items.get(i + 1), items.get(i + 2))
        {
            // `R` reads as a word that is no number.
            out.push(Value::Ref(*n));
            i += 3;
            continue;
        }
        out.push(items[i].clone());
        i += 1;
    }
    out
}

/// A name as PDF spells it, its `#xx` escapes read.
fn decode_name(raw: &[u8]) -> String {
    let mut out = Vec::with_capacity(raw.len());
    let mut i = 0;
    while i < raw.len() {
        if raw[i] == b'#'
            && let Some(b) = raw
                .get(i + 1..i + 3)
                .and_then(|h| std::str::from_utf8(h).ok())
                .and_then(|h| u8::from_str_radix(h, 16).ok())
        {
            out.push(b);
            i += 3;
            continue;
        }
        out.push(raw[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The objects read, by number, and the dictionaries of their streams.
struct Seen {
    objects: Vec<(i64, Value)>,
    names: Vec<String>,
    left: u64,
    until: Instant,
}

/// `Ok` when the PDF in `bytes` reaches nothing outside itself once its
/// app opens it; what it would reach otherwise.
pub fn opens_nothing_else(bytes: &[u8]) -> Result<(), String> {
    let mut read = Seen {
        objects: Vec::new(),
        names: Vec::new(),
        left: MAX_TOTAL,
        until: Instant::now() + TIME,
    };
    let encrypted = || "it is encrypted, so Kule cannot look inside".to_owned();
    if let Err(()) = read_objects(bytes, &mut read, true) {
        // What is encrypted does not read; said as it is.
        return Err(if bytes.windows(8).any(|w| w == b"/Encrypt") {
            encrypted()
        } else {
            "Kule could not read it as a PDF".to_owned()
        });
    }
    if let Some(held) = read.names.iter().find(|n| HELD_NAMES.contains(&n.as_str())) {
        return Err(format!(
            "it has a {held}, which its app would run, send, fetch or open"
        ));
    }
    if read.names.iter().any(|n| n == "Encrypt") {
        return Err(encrypted());
    }
    // What it does as it opens: a page to show, never an address to go to.
    for (_, object) in &read.objects {
        let Some(opening) = object.get("OpenAction") else {
            continue;
        };
        let action = match opening {
            Value::Ref(n) => read
                .objects
                .iter()
                .find(|(m, _)| m == n)
                .map(|(_, v)| v)
                .unwrap_or(&Value::Other),
            v => v,
        };
        let fine = match action {
            Value::Array(_) => true,
            Value::Dict(_) => {
                action.get("Next").is_none()
                    && action
                        .get("S")
                        .and_then(Value::name)
                        .is_some_and(|s| OPENING_ACTIONS.contains(&s))
            }
            _ => false,
        };
        if !fine {
            return Err("it acts by itself as it opens".to_owned());
        }
    }
    Ok(())
}

/// Reads `N G obj … endobj` objects from `bytes`, their streams skipped,
/// object streams inflated and read the same way. `top` for the file
/// itself, where anything between objects (cross-references, the
/// trailer) is read for its names.
fn read_objects(bytes: &[u8], read: &mut Seen, top: bool) -> Result<(), ()> {
    let mut names = Vec::new();
    let mut lexer = Lexer {
        bytes,
        at: 0,
        names: &mut names,
    };
    // The last two integers seen: an object's number and generation.
    let mut last: [Option<i64>; 2] = [None, None];
    let mut streams = Vec::new();
    while let Some(token) = lexer.token()? {
        if Instant::now() > read.until {
            return Err(());
        }
        match token {
            Token::Word(w) if w == b"obj" => {
                let number = last[0].ok_or(())?;
                last = [None, None];
                let first = lexer.token()?.ok_or(())?;
                let value = lexer.value(first, 0)?;
                let back = lexer.at;
                if let Ok(Some(Token::Word(w))) = lexer.token()
                    && w == b"stream"
                {
                    let data = stream_data(bytes, &mut lexer, &value)?;
                    if object_stream(&value) {
                        streams.push((value.clone(), data));
                    }
                } else {
                    lexer.at = back;
                }
                read.objects.push((number, value));
            }
            Token::Word(w) => {
                let n = std::str::from_utf8(&w).ok().and_then(|s| s.parse().ok());
                last = [last[1], n];
                if n.is_none() {
                    last = [None, None];
                }
            }
            Token::DictOpen | Token::ArrayOpen if top => {
                // The trailer, or a stray value: read for its names.
                lexer.value(token, 0)?;
                last = [None, None];
            }
            _ => last = [None, None],
        }
    }
    read.names.extend(names);
    for (dict, data) in streams {
        let inflated = inflate(&dict, &bytes[data.0..data.1], read)?;
        read_object_stream(&dict, &inflated, read)?;
    }
    Ok(())
}

/// Where a stream's data is, its lexer moved past `endstream`: by its
/// `Length` where that holds, by the next `endstream` otherwise.
fn stream_data(bytes: &[u8], lexer: &mut Lexer, dict: &Value) -> Result<(usize, usize), ()> {
    let mut start = lexer.at;
    if bytes.get(start) == Some(&b'\r') {
        start += 1;
    }
    if bytes.get(start) == Some(&b'\n') {
        start += 1;
    }
    let by_length = match dict.get("Length") {
        Some(Value::Int(n)) if *n >= 0 => Some(start + *n as usize),
        _ => None,
    };
    let ends_at = |end: usize| {
        let rest = bytes.get(end..).unwrap_or_default();
        let after = rest.iter().position(|b| !white(*b)).unwrap_or(rest.len());
        rest[after..]
            .starts_with(b"endstream")
            .then_some(end + after)
    };
    let (end, endstream) = match by_length.and_then(|end| ends_at(end).map(|e| (end, e))) {
        Some(found) => found,
        None => {
            let at = bytes[start..]
                .windows(9)
                .position(|w| w == b"endstream")
                .ok_or(())?;
            (start + at, start + at)
        }
    };
    lexer.at = endstream + 9;
    Ok((start, end))
}

/// A stream that holds objects: so typed, or with an object stream's
/// own keys (a viewer may not ask for the type).
fn object_stream(dict: &Value) -> bool {
    dict.get("Type").and_then(Value::name) == Some("ObjStm")
        || (dict.get("N").is_some() && dict.get("First").is_some())
}

/// A stream's data inflated as its filters say, through the caps; only
/// what Kule can read: no filter, or Flate, with or without a PNG
/// predictor.
fn inflate(dict: &Value, data: &[u8], read: &mut Seen) -> Result<Vec<u8>, ()> {
    let filters: Vec<&str> = match dict.get("Filter") {
        None => Vec::new(),
        Some(Value::Name(n)) => vec![n.as_str()],
        Some(Value::Array(items)) => items
            .iter()
            .map(|v| v.name().ok_or(()))
            .collect::<Result<_, _>>()?,
        _ => return Err(()),
    };
    let parms = match dict.get("DecodeParms") {
        Some(Value::Array(items)) => items.first().cloned(),
        other => other.cloned(),
    };
    let int = |key: &str, default: i64| match parms.as_ref().and_then(|p| p.get(key)) {
        None => Ok(default),
        Some(Value::Int(n)) => Ok(*n),
        Some(_) => Err(()),
    };
    let predictor = int("Predictor", 1)?;
    match filters[..] {
        [] if predictor == 1 => Ok(data.to_vec()),
        ["FlateDecode"] if predictor == 1 || (10..=15).contains(&predictor) => {
            let cap = MAX_STREAM.min(read.left);
            let mut out = Vec::new();
            ZlibDecoder::new(data)
                .take(cap + 1)
                .read_to_end(&mut out)
                .map_err(|_| ())?;
            if out.len() as u64 > cap {
                return Err(());
            }
            read.left -= out.len() as u64;
            if predictor == 1 {
                return Ok(out);
            }
            let bytes_per_pixel = int("Colors", 1)? * int("BitsPerComponent", 8)?;
            let columns = int("Columns", 1)?;
            let (Ok(pixel), Ok(row)) = (
                usize::try_from((bytes_per_pixel + 7) / 8),
                usize::try_from((bytes_per_pixel * columns + 7) / 8),
            ) else {
                return Err(());
            };
            unpredict(&out, pixel.max(1), row)
        }
        _ => Err(()),
    }
}

/// Rows undone of the PNG predictors (`Predictor` 10 to 15): each row of
/// `row` bytes after a byte naming its filter, `pixel` bytes a pixel.
fn unpredict(data: &[u8], pixel: usize, row: usize) -> Result<Vec<u8>, ()> {
    if row == 0 || row > 1 << 20 {
        return Err(());
    }
    let mut out = Vec::with_capacity(data.len());
    let mut above = vec![0u8; row];
    for line in data.chunks(row + 1) {
        let (&kind, line) = line.split_first().ok_or(())?;
        let mut current = line.to_vec();
        current.resize(row, 0);
        for i in 0..row {
            let left = if i >= pixel { current[i - pixel] } else { 0 };
            let up = above[i];
            let up_left = if i >= pixel { above[i - pixel] } else { 0 };
            let guess = match kind {
                0 => 0,
                1 => left,
                2 => up,
                3 => ((u16::from(left) + u16::from(up)) / 2) as u8,
                4 => {
                    let p = i16::from(left) + i16::from(up) - i16::from(up_left);
                    let (a, b, c) = (
                        (p - i16::from(left)).abs(),
                        (p - i16::from(up)).abs(),
                        (p - i16::from(up_left)).abs(),
                    );
                    if a <= b && a <= c {
                        left
                    } else if b <= c {
                        up
                    } else {
                        up_left
                    }
                }
                _ => return Err(()),
            };
            current[i] = current[i].wrapping_add(guess);
        }
        out.extend_from_slice(&current[..line.len().min(row)]);
        above = current;
    }
    Ok(out)
}

/// The objects an object stream holds: `N` pairs of number and offset,
/// then each object at `First` plus its offset.
fn read_object_stream(dict: &Value, data: &[u8], read: &mut Seen) -> Result<(), ()> {
    let (Some(Value::Int(n)), Some(Value::Int(first))) = (dict.get("N"), dict.get("First")) else {
        return Err(());
    };
    let (n, first) = (
        usize::try_from(*n).map_err(|_| ())?,
        usize::try_from(*first).map_err(|_| ())?,
    );
    let header = data.get(..first).ok_or(())?;
    let numbers: Vec<i64> = std::str::from_utf8(header)
        .map_err(|_| ())?
        .split_ascii_whitespace()
        .map(|w| w.parse().map_err(|_| ()))
        .collect::<Result<_, _>>()?;
    if numbers.len() != n * 2 {
        return Err(());
    }
    for pair in numbers.chunks(2) {
        let at = first
            .checked_add(usize::try_from(pair[1]).map_err(|_| ())?)
            .ok_or(())?;
        let mut names = Vec::new();
        let mut lexer = Lexer {
            bytes: data,
            at,
            names: &mut names,
        };
        let token = lexer.token()?.ok_or(())?;
        let value = lexer.value(token, 0)?;
        read.names.extend(names);
        read.objects.push((pair[0], value));
    }
    // Names anywhere else in it (between objects) are read too.
    let mut names = Vec::new();
    let mut lexer = Lexer {
        bytes: data,
        at: 0,
        names: &mut names,
    };
    while lexer.token()?.is_some() {}
    read.names.extend(names);
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use flate2::Compression;
    use flate2::write::ZlibEncoder;

    use super::opens_nothing_else;

    /// A PDF of `objects` (their bodies, numbered from 1), with a trailer.
    fn pdf(objects: &[&str]) -> Vec<u8> {
        let mut out = b"%PDF-1.7\n%\xe2\xe3\xcf\xd3\n".to_vec();
        for (i, body) in objects.iter().enumerate() {
            out.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
        }
        out.extend_from_slice(b"trailer\n<< /Root 1 0 R >>\n%%EOF\n");
        out
    }

    const PAGES: &str = "<< /Type /Pages /Kids [3 0 R] /Count 1 >>";
    const PAGE: &str = "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Annots [<< /Type /Annot /Subtype /Link /Rect [0 0 1 1] /A << /S /URI /URI (https://example.com) >> >>] >>";

    fn deflated(data: &[u8]) -> Vec<u8> {
        let mut e = ZlibEncoder::new(Vec::new(), Compression::default());
        e.write_all(data).unwrap();
        e.finish().unwrap()
    }

    /// An object stream holding `objects`, as object 4.
    fn with_object_stream(catalog: &str, objects: &[(u32, &str)], filter: &str) -> Vec<u8> {
        let mut header = String::new();
        let mut body = String::new();
        for (n, text) in objects {
            header.push_str(&format!("{n} {} ", body.len()));
            body.push_str(text);
            body.push('\n');
        }
        let raw = format!("{header}\n{body}");
        let data = if filter.is_empty() {
            raw.clone().into_bytes()
        } else {
            deflated(raw.as_bytes())
        };
        let mut out = b"%PDF-1.7\n".to_vec();
        for (i, body) in [catalog, PAGES, PAGE].iter().enumerate() {
            out.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
        }
        out.extend_from_slice(
            format!(
                "4 0 obj\n<< /Type /ObjStm /N {} /First {} /Length {} {filter} >>\nstream\n",
                objects.len(),
                header.len() + 1,
                data.len()
            )
            .as_bytes(),
        );
        out.extend_from_slice(&data);
        out.extend_from_slice(b"\nendstream\nendobj\ntrailer\n<< /Root 1 0 R >>\n%%EOF\n");
        out
    }

    #[test]
    fn a_plain_pdf_with_links_and_pages_opens() {
        let bytes = pdf(&[
            "<< /Type /Catalog /Pages 2 0 R /OpenAction [3 0 R /Fit] >>",
            PAGES,
            PAGE,
        ]);
        assert_eq!(opens_nothing_else(&bytes), Ok(()));
        // A stream whose data says anything at all is not read as names.
        let mut drawn = pdf(&["<< /Type /Catalog /Pages 2 0 R >>", PAGES, PAGE]);
        let content = b"BT /F1 12 Tf (JavaScript) Tj ET /JS /Launch";
        drawn.extend_from_slice(
            format!("5 0 obj\n<< /Length {} >>\nstream\n", content.len()).as_bytes(),
        );
        drawn.extend_from_slice(content);
        drawn.extend_from_slice(b"\nendstream\nendobj\n");
        assert_eq!(opens_nothing_else(&drawn), Ok(()));
    }

    #[test]
    fn scripts_programs_forms_and_attachments_are_refused() {
        for action in [
            "<< /S /JavaScript /JS (app.alert(1)) >>",
            "<< /S /Launch /F (calc.exe) >>",
            "<< /S /SubmitForm /F << /FS /URL /F (https://example.com) >> >>",
            "<< /S /ImportData /F (data.fdf) >>",
            "<< /S /GoToR /F (other.pdf) /D [0 /Fit] >>",
            "<< /S /GoToE /T << /R /C /N (x) >> >>",
            "<< /S /Rendition /JS (1) >>",
        ] {
            let bytes = pdf(&[
                "<< /Type /Catalog /Pages 2 0 R >>",
                PAGES,
                &PAGE.replace("<< /S /URI /URI (https://example.com) >>", action),
            ]);
            assert!(opens_nothing_else(&bytes).is_err(), "{action}");
        }
        for catalog in [
            "<< /Type /Catalog /Pages 2 0 R /Names << /JavaScript 5 0 R >> >>",
            "<< /Type /Catalog /Pages 2 0 R /Names << /EmbeddedFiles 5 0 R >> >>",
            "<< /Type /Catalog /Pages 2 0 R /AcroForm << /XFA 5 0 R >> >>",
            "<< /Type /Catalog /Pages 2 0 R /AA << /WC 5 0 R >> >>",
            // Spelled with escapes.
            "<< /Type /Catalog /Pages 2 0 R /Names << /J#61vaScript 5 0 R >> >>",
        ] {
            let bytes = pdf(&[catalog, PAGES, PAGE]);
            assert!(opens_nothing_else(&bytes).is_err(), "{catalog}");
        }
    }

    #[test]
    fn what_runs_as_it_opens_is_refused_but_a_page_is_not() {
        let opening = |open: &str, extra: &[&str]| {
            let catalog = format!("<< /Type /Catalog /Pages 2 0 R /OpenAction {open} >>");
            let mut objects = vec![catalog.as_str(), PAGES, PAGE];
            objects.extend_from_slice(extra);
            opens_nothing_else(&pdf(&objects))
        };
        assert_eq!(opening("<< /S /GoTo /D [3 0 R /Fit] >>", &[]), Ok(()));
        assert_eq!(opening("4 0 R", &["[3 0 R /FitH 792]"]), Ok(()));
        assert!(opening("<< /S /URI /URI (https://example.com/t) >>", &[]).is_err());
        assert!(opening("4 0 R", &["<< /S /URI /URI (https://example.com/t) >>"]).is_err());
        assert!(
            opening(
                "<< /S /GoTo /D [3 0 R /Fit] /Next 4 0 R >>",
                &["<< /S /URI /URI (x) >>"]
            )
            .is_err()
        );
        assert!(opening("9 0 R", &[]).is_err());
    }

    #[test]
    fn objects_packed_in_a_compressed_stream_are_read_too() {
        let catalog = "<< /Type /Catalog /Pages 2 0 R /Names 5 0 R >>";
        let hidden = with_object_stream(
            catalog,
            &[
                (5, "<< /JavaScript 6 0 R >>"),
                (6, "<< /Names [(a) 7 0 R] >>"),
            ],
            "/Filter /FlateDecode",
        );
        assert!(opens_nothing_else(&hidden).is_err());
        let plain = with_object_stream(
            catalog,
            &[
                (5, "<< /Dests 6 0 R >>"),
                (6, "<< /Names [(a) [3 0 R /Fit]] >>"),
            ],
            "/Filter /FlateDecode",
        );
        assert_eq!(opens_nothing_else(&plain), Ok(()));
        // Not typed as one, but read as one by its keys.
        let untyped = String::from_utf8_lossy(&with_object_stream(
            "<< /Type /Catalog /Pages 2 0 R /OpenAction 5 0 R >>",
            &[(5, "<< /S /URI /URI (https://example.com) >>")],
            "",
        ))
        .replace("/Type /ObjStm ", "")
        .into_bytes();
        assert!(opens_nothing_else(&untyped).is_err());
    }

    /// An object stream behind a PNG predictor, as Acrobat writes some,
    /// is read through it.
    #[test]
    fn an_object_stream_behind_a_predictor_is_read() {
        let objects = "5 0 << /JavaScript 6 0 R >>\n";
        // Rows of 4 bytes, each "Up" (2) from the one above.
        let raw = objects.as_bytes();
        let mut predicted = Vec::new();
        let mut above = [0u8; 4];
        for chunk in raw.chunks(4) {
            let mut row = [0u8; 4];
            row[..chunk.len()].copy_from_slice(chunk);
            predicted.push(2);
            for i in 0..4 {
                predicted.push(row[i].wrapping_sub(above[i]));
            }
            above = row;
        }
        let data = deflated(&predicted);
        let mut bytes = b"%PDF-1.7\n1 0 obj\n<< /Type /Catalog /Names 5 0 R >>\nendobj\n".to_vec();
        bytes.extend_from_slice(
            format!(
                "4 0 obj\n<< /Type /ObjStm /N 1 /First 4 /Length {} /Filter /FlateDecode /DecodeParms << /Predictor 12 /Columns 4 >> >>\nstream\n",
                data.len()
            )
            .as_bytes(),
        );
        bytes.extend_from_slice(&data);
        bytes.extend_from_slice(b"\nendstream\nendobj\n");
        let why = opens_nothing_else(&bytes).unwrap_err();
        assert!(why.contains("JavaScript"), "{why}");
    }

    #[test]
    fn what_cannot_be_read_does_not_open() {
        // Encrypted, a filter Kule does not read, a bomb, or not a PDF.
        let encrypted =
            String::from_utf8_lossy(&pdf(&["<< /Type /Catalog /Pages 2 0 R >>", PAGES, PAGE]))
                .replace("<< /Root 1 0 R >>", "<< /Root 1 0 R /Encrypt 9 0 R >>")
                .into_bytes();
        assert!(opens_nothing_else(&encrypted).is_err());
        let lzw = with_object_stream(
            "<< /Type /Catalog >>",
            &[(5, "<< >>")],
            "/Filter /LZWDecode",
        );
        assert!(opens_nothing_else(&lzw).is_err());
        let tiff = with_object_stream(
            "<< /Type /Catalog >>",
            &[(5, "<< >>")],
            "/Filter /FlateDecode /DecodeParms << /Predictor 2 /Columns 4 >>",
        );
        assert!(opens_nothing_else(&tiff).is_err());
        // A readable object stream, but for the space after it.
        let mut inflated = b"5 0 << >>".to_vec();
        inflated.resize(70 << 20, b' ');
        let mut bomb = b"%PDF-1.7\n1 0 obj\n<< /Type /Catalog >>\nendobj\n".to_vec();
        let data = deflated(&inflated);
        bomb.extend_from_slice(
            format!("4 0 obj\n<< /Type /ObjStm /N 1 /First 4 /Length {} /Filter /FlateDecode >>\nstream\n", data.len()).as_bytes(),
        );
        bomb.extend_from_slice(&data);
        bomb.extend_from_slice(b"\nendstream\nendobj\n");
        assert!(bomb.len() < 1 << 20);
        assert!(opens_nothing_else(&bomb).is_err());
        assert!(
            opens_nothing_else(b"%PDF-1.7\n1 0 obj\n<< /Type /Catalog (unclosed >>\nendobj\n")
                .is_err()
        );
    }
}
