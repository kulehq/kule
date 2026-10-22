//! What a file of a task's copy looks like in the window's Preview: text
//! with its language, Markdown to render, a picture, or a note on why there
//! is nothing to show. Everything here works on bytes already read; what is
//! read, and from where, is the daemon's business.

pub mod docx;
pub mod image;
pub mod office;
pub mod opens;
pub mod pdf;
pub mod refs;
pub mod table;
pub mod text;
pub mod xlsx;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use serde::{Deserialize, Serialize};

/// The most of a text file shown.
pub const MAX_TEXT: u64 = 1 << 20;

/// The most text one view of an office or CSV file carries, in bytes as
/// JSON writes it: every block, list marker, cell and formula draws on it.
/// A small file can name the same text many times over.
pub const MAX_VIEW: usize = 16 << 20;

/// What an entry of a view costs besides its text: its keys, numbers and
/// punctuation as JSON writes them.
pub(crate) const FRAME: usize = 64;

/// What is left of one view's [`MAX_VIEW`].
pub(crate) struct Budget {
    left: usize,
    /// Something did not fit: nothing more is taken.
    spent: bool,
}

impl Budget {
    pub(crate) fn new(left: usize) -> Self {
        Budget { left, spent: false }
    }

    /// Takes `bytes` from what is left; false when fewer are left, and
    /// from then on nothing more is taken.
    pub(crate) fn take(&mut self, bytes: usize) -> bool {
        match self.left.checked_sub(bytes) {
            Some(left) if !self.spent => {
                self.left = left;
                true
            }
            _ => {
                self.spent = true;
                false
            }
        }
    }

    /// Whether something did not fit: what is still to be read is not
    /// shown.
    pub(crate) fn spent(&self) -> bool {
        self.spent
    }
}

/// What `text` costs a view: its bytes as a JSON string, with its quotes,
/// its escapes at their longest and the comma after it.
pub(crate) fn cost(text: &str) -> usize {
    3 + text
        .bytes()
        .map(|b| match b {
            b'"' | b'\\' => 2,
            0..0x20 => 6,
            _ => 1,
        })
        .sum::<usize>()
}

/// A file as the Preview shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum View {
    Text {
        text: String,
        /// Only the first [`MAX_TEXT`] bytes are here.
        cut: bool,
        size: u64,
        lang: Option<String>,
        /// JSON or XML packed onto a few lines, formatted.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        formatted: Option<String>,
    },
    Markdown {
        text: String,
        cut: bool,
        size: u64,
    },
    Image {
        mime: String,
        width: usize,
        height: usize,
        size: u64,
        /// The file, base64.
        data: String,
    },
    /// A workbook's sheets, or a CSV file as one sheet.
    Table {
        sheets: Vec<table::Sheet>,
        size: u64,
    },
    /// A Word document's text.
    Document {
        blocks: Vec<docx::Block>,
        /// More blocks than are here.
        cut: bool,
        size: u64,
    },
    /// Nothing to show, and why.
    Other {
        size: u64,
        reason: String,
    },
}

fn extension(path: &str) -> String {
    let name = path.rsplit('/').next().unwrap_or(path);
    name.rsplit_once('.')
        .map(|(_, ext)| ext.to_ascii_lowercase())
        .unwrap_or_default()
}

fn is_workbook(path: &str) -> bool {
    matches!(extension(path).as_str(), "xlsx" | "xlsm")
}

fn is_document(path: &str) -> bool {
    matches!(extension(path).as_str(), "docx" | "docm")
}

fn is_csv(path: &str) -> bool {
    matches!(extension(path).as_str(), "csv" | "tsv")
}

/// How much of a file named `path` [`view`] needs: a picture or an office
/// file whole, text up to [`MAX_TEXT`]. One byte more than a whole file may
/// have shows that it is too large without reading it all.
pub fn wanted(path: &str) -> u64 {
    if image::named_like_one(path) {
        image::MAX_IMAGE as u64 + 1
    } else if is_workbook(path) || is_document(path) {
        office::MAX_FILE as u64 + 1
    } else {
        MAX_TEXT
    }
}

/// The view of file `path`, `size` bytes long, from its first `bytes`
/// (at most [`wanted`] of them).
pub fn view(path: &str, bytes: Vec<u8>, size: u64) -> View {
    let other = |reason: &str| View::Other {
        size,
        reason: reason.to_owned(),
    };
    let cut = (bytes.len() as u64) < size;
    if image::named_like_one(path) {
        if cut {
            return other(image::NotShown::TooLarge.reason());
        }
        return match image::picture(&bytes) {
            Ok(p) => View::Image {
                mime: p.mime.to_owned(),
                width: p.width,
                height: p.height,
                size,
                data: STANDARD.encode(&bytes),
            },
            Err(why) => other(why.reason()),
        };
    }
    if is_workbook(path) || is_document(path) {
        if cut {
            return other("too large to preview");
        }
        let read = if is_workbook(path) {
            xlsx::sheets(&bytes).map(|sheets| View::Table { sheets, size })
        } else {
            docx::blocks(&bytes).map(|(blocks, cut)| View::Document { blocks, cut, size })
        };
        return read.unwrap_or_else(|why| other(&why.0));
    }
    let Some(text) = text::decode(bytes, cut) else {
        return other("a binary file");
    };
    if text::is_markdown(path) {
        return View::Markdown { text, cut, size };
    }
    if is_csv(path) {
        return View::Table {
            sheets: vec![table::csv(path, &text, cut)],
            size,
        };
    }
    let formatted = if cut {
        None
    } else {
        text::formatted(path, &text)
    };
    View::Text {
        lang: text::language(path).map(str::to_owned),
        text,
        cut,
        size,
        formatted,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_kind_of_file_has_its_view() {
        let png = image::tests::png(4, 3);
        let View::Image {
            mime,
            width,
            height,
            data,
            ..
        } = view("logo.png", png.clone(), png.len() as u64)
        else {
            panic!("not an image");
        };
        assert_eq!((mime.as_str(), width, height), ("image/png", 4, 3));
        assert_eq!(STANDARD.decode(data).unwrap(), png);

        assert!(matches!(
            view("README.md", b"# Hi\n".to_vec(), 5),
            View::Markdown { cut: false, .. }
        ));
        assert_eq!(
            view("src/a.rs", b"fn a() {}\n".to_vec(), 10),
            View::Text {
                text: "fn a() {}\n".into(),
                cut: false,
                size: 10,
                lang: Some("Rust".into()),
                formatted: None
            }
        );
        let json = br#"{"a":[1,2],"b":{"c":true}}"#.to_vec();
        let View::Text { formatted, .. } = view("x.json", json.clone(), json.len() as u64) else {
            panic!("not text");
        };
        assert!(formatted.unwrap().starts_with("{\n  \"a\": [\n"));
        assert_eq!(
            view("a.bin", vec![1, 0, 2], 3),
            View::Other {
                size: 3,
                reason: "a binary file".into()
            }
        );
    }

    #[test]
    fn what_is_cut_says_so_and_a_cut_picture_is_not_shown() {
        // A picture larger than a picture may be: only its start was read.
        let mut png = image::tests::png(4, 3);
        png.resize(64, 0);
        assert_eq!(
            view("big.png", png, (image::MAX_IMAGE as u64) * 2),
            View::Other {
                size: (image::MAX_IMAGE as u64) * 2,
                reason: "too large to preview".into()
            }
        );
        // A file named a picture that is not one.
        assert_eq!(
            view("x.png", b"<svg/>".to_vec(), 6),
            View::Other {
                size: 6,
                reason: "not a picture Kule shows".into()
            }
        );
        // Packed JSON that was cut is not formatted: it would not parse.
        let View::Text { cut, formatted, .. } = view("x.json", br#"{"a":[1,"#.to_vec(), 100) else {
            panic!("not text");
        };
        assert!(cut && formatted.is_none());
        assert_eq!(wanted("a.png"), image::MAX_IMAGE as u64 + 1);
        assert_eq!(wanted("a.txt"), MAX_TEXT);
        assert_eq!(wanted("b.XLSX"), office::MAX_FILE as u64 + 1);
        // A workbook larger than one may be is not read in part.
        assert_eq!(
            view("b.xlsx", vec![b'P'; 10], office::MAX_FILE as u64 * 2),
            View::Other {
                size: office::MAX_FILE as u64 * 2,
                reason: "too large to preview".into()
            }
        );
    }

    #[test]
    fn workbooks_and_csv_files_are_tables() {
        let book = xlsx::tests::workbook();
        let View::Table { sheets, .. } = view("data/sales.xlsx", book.clone(), book.len() as u64)
        else {
            panic!("not a table");
        };
        assert_eq!(sheets[0].name, "Sales");
        let View::Table { sheets, .. } = view("r.csv", b"a,b\n1,2\n".to_vec(), 8) else {
            panic!("not a table");
        };
        assert_eq!(sheets[0].rows, [["a", "b"], ["1", "2"]]);
        // Named a workbook, but not one.
        assert!(matches!(
            view("x.xlsx", b"hello".to_vec(), 5),
            View::Other { .. }
        ));
    }

    #[test]
    fn a_word_document_is_its_blocks() {
        let doc = docx::tests::document();
        let View::Document { blocks, cut, .. } =
            view("Contract.DOCX", doc.clone(), doc.len() as u64)
        else {
            panic!("not a document");
        };
        assert!(!cut && blocks.len() == 8);
        assert_eq!(wanted("a.docx"), office::MAX_FILE as u64 + 1);
    }

    #[test]
    fn a_budget_once_spent_takes_nothing_more() {
        let mut budget = Budget::new(10);
        assert!(budget.take(10) && !budget.spent());
        assert!(!budget.take(1) && budget.spent());
        let mut budget = Budget::new(10);
        assert!(!budget.take(11));
        assert!(!budget.take(1) && budget.spent());
        // Quotes, an escaped quote, an escaped control character, a comma.
        assert_eq!(cost("a\"\u{1}"), 3 + 1 + 2 + 6);
    }

    /// Memory allocated on each thread, counted so a test can see the most
    /// a call held at once.
    pub(crate) mod memory {
        use std::alloc::{GlobalAlloc, Layout, System};
        use std::cell::Cell;

        thread_local! {
            static LIVE: Cell<isize> = const { Cell::new(0) };
            static PEAK: Cell<isize> = const { Cell::new(0) };
        }

        fn grew(by: isize) {
            let _ = LIVE.try_with(|live| {
                let now = live.get() + by;
                live.set(now);
                let _ = PEAK.try_with(|peak| peak.set(peak.get().max(now)));
            });
        }

        struct Counting;

        // SAFETY: every call is passed on to the system allocator as is;
        // only the sizes are counted.
        unsafe impl GlobalAlloc for Counting {
            unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
                let p = unsafe { System.alloc(layout) };
                if !p.is_null() {
                    grew(layout.size() as isize);
                }
                p
            }

            unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
                let p = unsafe { System.alloc_zeroed(layout) };
                if !p.is_null() {
                    grew(layout.size() as isize);
                }
                p
            }

            unsafe fn dealloc(&self, p: *mut u8, layout: Layout) {
                unsafe { System.dealloc(p, layout) };
                grew(-(layout.size() as isize));
            }

            unsafe fn realloc(&self, p: *mut u8, layout: Layout, size: usize) -> *mut u8 {
                let q = unsafe { System.realloc(p, layout, size) };
                if !q.is_null() {
                    grew(size as isize - layout.size() as isize);
                }
                q
            }
        }

        #[global_allocator]
        static COUNTING: Counting = Counting;

        /// What `f` returns, and the most memory it held at once beyond
        /// what was held before it.
        pub(crate) fn peak<T>(f: impl FnOnce() -> T) -> (T, usize) {
            let before = LIVE.with(Cell::get);
            PEAK.with(|p| p.set(before));
            let out = f();
            (out, (PEAK.with(Cell::get) - before).max(0) as usize)
        }
    }

    /// The view of `bytes` named `path`, which must come quickly, hold
    /// little memory while it is made, and stay within one view's budget
    /// as JSON.
    fn bounded(path: &str, bytes: Vec<u8>) -> View {
        let size = bytes.len() as u64;
        let start = std::time::Instant::now();
        let (view, held) = memory::peak(|| view(path, bytes, size));
        let took = start.elapsed();
        let json = serde_json::to_vec(&view).unwrap().len();
        assert!(
            // Well above the quarter second it takes unoptimised, for a
            // busy machine; below the seconds it took unbounded.
            took < std::time::Duration::from_secs(2)
                && held <= 2 * MAX_VIEW
                && json <= MAX_VIEW + (1 << 20),
            "{path} of {size} bytes made {json} bytes of JSON in {took:?}, holding {held} bytes"
        );
        view
    }

    #[test]
    fn deeply_nested_xml_is_shown_as_written() {
        let n = 12_000;
        let xml = format!("{}{}", "<a>".repeat(n), "</a>".repeat(n));
        let View::Text { formatted, .. } = bounded("deep.xml", xml.into_bytes()) else {
            panic!("not text");
        };
        assert_eq!(formatted, None);
    }

    #[test]
    fn deeply_nested_json_is_shown_as_written() {
        let n = 12_000;
        let json = format!("{}{}", "[".repeat(n), "]".repeat(n));
        let View::Text { formatted, .. } = bounded("deep.json", json.into_bytes()) else {
            panic!("not text");
        };
        assert_eq!(formatted, None);
    }

    #[test]
    fn a_long_list_marker_keeps_a_document_within_its_budget() {
        const W: &str = r#"xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main""#;
        let item = r#"<w:p><w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="1"/></w:numPr></w:pPr><w:r><w:t>x</w:t></w:r></w:p>"#;
        let body = format!(
            "<w:document {W}><w:body>{}</w:body></w:document>",
            item.repeat(docx::MAX_BLOCKS)
        );
        let numbering = format!(
            r#"<w:numbering {W}><w:abstractNum w:abstractNumId="7"><w:lvl w:ilvl="0"><w:numFmt w:val="decimal"/><w:lvlText w:val="{}%1."/></w:lvl></w:abstractNum><w:num w:numId="1"><w:abstractNumId w:val="7"/></w:num></w:numbering>"#,
            "x".repeat(50_000)
        );
        let doc = office::tests::archive(&[
            ("word/document.xml", body.as_bytes()),
            ("word/numbering.xml", numbering.as_bytes()),
        ]);
        assert!(doc.len() < 4 << 10, "{}", doc.len());
        let View::Document { blocks, .. } = bounded("list.docx", doc) else {
            panic!("not a document");
        };
        assert!(!blocks.is_empty());
    }

    #[test]
    fn a_shared_string_in_many_cells_keeps_a_workbook_within_its_budget() {
        let row = format!(
            "<row>{}</row>",
            r#"<c t="s"><v>0</v></c>"#.repeat(table::MAX_COLS)
        );
        let rows = row.repeat(250);
        let sheet = format!("<worksheet><sheetData>{rows}</sheetData></worksheet>");
        // Four bytes a character: the most one cell may show.
        let strings = format!(
            "<sst><si><t>{}</t></si></sst>",
            "😀".repeat(table::MAX_CELL)
        );
        let book = office::tests::archive(&[
            (
                "xl/workbook.xml",
                br#"<workbook xmlns:r="r"><sheets><sheet name="S" r:id="1"/></sheets></workbook>"#,
            ),
            (
                "xl/_rels/workbook.xml.rels",
                br#"<Relationships><Relationship Id="1" Target="s.xml"/></Relationships>"#,
            ),
            ("xl/sharedStrings.xml", strings.as_bytes()),
            ("xl/s.xml", sheet.as_bytes()),
        ]);
        assert!(book.len() < 64 << 10, "{}", book.len());
        let View::Table { sheets, .. } = bounded("many.xlsx", book) else {
            panic!("not a table");
        };
        assert!(sheets[0].cut);
    }
}
