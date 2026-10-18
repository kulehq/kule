//! What a file of a task's copy looks like in the window's Preview: text
//! with its language, Markdown to render, a picture, or a note on why there
//! is nothing to show. Everything here works on bytes already read; what is
//! read, and from where, is the daemon's business.

pub mod docx;
pub mod image;
pub mod office;
pub mod refs;
pub mod table;
pub mod text;
pub mod xlsx;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use serde::{Deserialize, Serialize};

/// The most of a text file shown.
pub const MAX_TEXT: u64 = 1 << 20;

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
}
