//! Office files (XLSX, DOCX) are zip archives of XML parts. They are read
//! in memory, never unpacked, and only the parts a view needs are inflated,
//! each through a cap: the sizes an archive states can lie, and a few
//! kilobytes can inflate to gigabytes.

use std::io::{Cursor, Read};
use std::time::{Duration, Instant};

use quick_xml::events::{BytesRef, BytesStart};
use zip::ZipArchive;

/// Larger office files are not read.
pub const MAX_FILE: usize = 20 << 20;
/// The most one part may inflate to.
const MAX_PART: u64 = 20 << 20;
/// The most all parts read for one view may inflate to.
const MAX_TOTAL: u64 = 64 << 20;
/// Archives with more entries are not read.
const MAX_ENTRIES: usize = 10_000;
/// How long reading one file may take.
const TIME: Duration = Duration::from_secs(5);

/// Why an office file has no view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unread(pub String);

pub(crate) fn unread(why: impl Into<String>) -> Unread {
    Unread(why.into())
}

pub(crate) type Result<T> = std::result::Result<T, Unread>;

/// The parts of one archive, read against one budget.
pub(crate) struct Parts<'a> {
    zip: ZipArchive<Cursor<&'a [u8]>>,
    left: u64,
    until: Instant,
}

impl<'a> Parts<'a> {
    pub(crate) fn open(bytes: &'a [u8]) -> Result<Self> {
        if bytes.len() > MAX_FILE {
            return Err(unread("too large to preview"));
        }
        let zip =
            ZipArchive::new(Cursor::new(bytes)).map_err(|_| unread("not a file Kule can read"))?;
        if zip.len() > MAX_ENTRIES {
            return Err(unread("too many parts to preview"));
        }
        Ok(Parts {
            zip,
            left: MAX_TOTAL,
            until: Instant::now() + TIME,
        })
    }

    /// Part `name` as text, or `None` when the archive has no such part.
    pub(crate) fn text(&mut self, name: &str) -> Result<Option<String>> {
        let Some(index) = self.zip.index_for_name(name) else {
            return Ok(None);
        };
        let entry = self
            .zip
            .by_index(index)
            .map_err(|_| unread("not a file Kule can read"))?;
        if entry.encrypted() {
            return Err(unread("encrypted"));
        }
        let cap = MAX_PART.min(self.left);
        let mut bytes = Vec::new();
        entry
            .take(cap + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| unread("not a file Kule can read"))?;
        if bytes.len() as u64 > cap {
            return Err(unread("too large to preview"));
        }
        self.left -= bytes.len() as u64;
        self.check_time()?;
        // XML is UTF-8 unless it says otherwise; office files do not.
        let text = String::from_utf8(bytes).map_err(|_| unread("not a file Kule can read"))?;
        Ok(Some(
            text.strip_prefix('\u{feff}')
                .map(str::to_owned)
                .unwrap_or(text),
        ))
    }

    pub(crate) fn check_time(&self) -> Result<()> {
        if Instant::now() > self.until {
            return Err(unread("took too long to read"));
        }
        Ok(())
    }
}

/// An attribute's value by its local name (`r:id` is `id`).
pub(crate) fn attr(e: &BytesStart, local: &str) -> Option<String> {
    e.attributes().with_checks(false).flatten().find_map(|a| {
        (a.key.local_name().as_ref() == local)
            .then(|| {
                a.normalized_value(quick_xml::XmlVersion::Implicit1_0)
                    .ok()
                    .map(|v| v.into_owned())
            })
            .flatten()
    })
}

/// The character a reference in text stands for: a numbered one or one of
/// XML's five. Nothing else is expanded.
pub(crate) fn reference(r: &BytesRef) -> Option<char> {
    if let Ok(Some(c)) = r.resolve_char_ref() {
        return Some(c);
    }
    match r.as_ref() {
        "amp" => Some('&'),
        "lt" => Some('<'),
        "gt" => Some('>'),
        "quot" => Some('"'),
        "apos" => Some('\''),
        _ => None,
    }
}

/// `text` cut to `max` characters, with an ellipsis when cut.
pub(crate) fn clip(mut text: String, max: usize) -> String {
    if let Some((at, _)) = text.char_indices().nth(max) {
        text.truncate(at);
        text.push('…');
    }
    text
}

#[cfg(test)]
pub(crate) mod tests {
    use std::io::Write;

    use zip::write::SimpleFileOptions;

    /// A zip archive of `parts`, deflated.
    pub(crate) fn archive(parts: &[(&str, &[u8])]) -> Vec<u8> {
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let options =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        for (name, bytes) in parts {
            zip.start_file(*name, options).unwrap();
            zip.write_all(bytes).unwrap();
        }
        zip.finish().unwrap().into_inner()
    }

    #[test]
    fn a_part_that_inflates_past_its_cap_is_not_read() {
        // 30 MB of zeros deflate to a few tens of kilobytes.
        let bomb = vec![0u8; 30 << 20];
        let bytes = archive(&[("a.xml", &bomb), ("b.xml", b"<b/>")]);
        assert!(bytes.len() < 1 << 20);
        let mut parts = super::Parts::open(&bytes).unwrap();
        assert_eq!(parts.text("b.xml").unwrap().as_deref(), Some("<b/>"));
        assert_eq!(parts.text("nope.xml").unwrap(), None);
        assert_eq!(
            parts.text("a.xml").unwrap_err(),
            super::unread("too large to preview")
        );
        // Not a zip at all.
        assert!(super::Parts::open(b"PK\x03\x04 not really").is_err());
    }

    #[test]
    fn parts_together_stay_within_one_budget() {
        let part = vec![b' '; 15 << 20];
        let bytes = archive(&[
            ("1", &part),
            ("2", &part),
            ("3", &part),
            ("4", &part),
            ("5", &part),
        ]);
        let mut parts = super::Parts::open(&bytes).unwrap();
        for name in ["1", "2", "3", "4"] {
            parts.text(name).unwrap();
        }
        assert!(parts.text("5").is_err());
    }

    #[test]
    fn text_is_clipped_on_a_character() {
        assert_eq!(super::clip("çççç".into(), 2), "çç…");
        assert_eq!(super::clip("çç".into(), 2), "çç");
    }
}
