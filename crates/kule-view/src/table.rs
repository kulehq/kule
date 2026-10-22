//! Tables: the sheets of a workbook or the rows of a CSV file, as cells of
//! text laid out from A1, so a cell's place here is its place in the file.

use serde::{Deserialize, Serialize};

use crate::office::clip;
use crate::{Budget, FRAME, MAX_VIEW, cost};

/// The most rows of a sheet shown.
pub const MAX_ROWS: usize = 5000;
/// The most columns of a sheet shown.
pub const MAX_COLS: usize = 200;
/// The most cells of all sheets together.
pub const MAX_CELLS: usize = 250_000;
/// The most characters of one cell shown.
pub const MAX_CELL: usize = 1000;
/// The most sheets shown.
pub const MAX_SHEETS: usize = 50;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sheet {
    pub name: String,
    /// Row 1 first, each row from column A; a row may stop short.
    pub rows: Vec<Vec<String>>,
    /// More rows, columns or text than are here.
    pub cut: bool,
    /// Formulas of the cells that have them, as written after `=`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub formulas: Vec<Formula>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Formula {
    /// From 0: row 1 is 0, column A is 0.
    pub row: usize,
    pub col: usize,
    pub text: String,
}

/// Cells put in place as they are found, within the limits and the
/// view's budget.
pub(crate) struct Grid<'b> {
    pub(crate) rows: Vec<Vec<String>>,
    pub(crate) cut: bool,
    formulas: Vec<Formula>,
    cells_left: usize,
    budget: &'b mut Budget,
}

impl<'b> Grid<'b> {
    pub(crate) fn new(cells_left: usize, budget: &'b mut Budget) -> Self {
        Grid {
            rows: Vec::new(),
            cut: false,
            formulas: Vec::new(),
            cells_left,
            budget,
        }
    }

    pub(crate) fn cells_left(&self) -> usize {
        self.cells_left
    }

    /// Puts `text` at `row`, `col` (from 0); what falls outside the limits
    /// marks the grid cut instead.
    pub(crate) fn put(&mut self, row: usize, col: usize, text: String) -> bool {
        if row >= MAX_ROWS || col >= MAX_COLS {
            self.cut = true;
            return false;
        }
        let text = clip(text, MAX_CELL);
        // Rows and cells it adds before it cost as empty ones.
        let rows = (row + 1).saturating_sub(self.rows.len());
        let added = (col + 1).saturating_sub(self.rows.get(row).map_or(0, Vec::len));
        if added > self.cells_left || !self.budget.take((rows + added) * cost("") + cost(&text)) {
            self.cut = true;
            return false;
        }
        self.cells_left -= added;
        if rows > 0 {
            self.rows.resize_with(row + 1, Vec::new);
        }
        let cells = &mut self.rows[row];
        if cells.len() <= col {
            cells.resize(col + 1, String::new());
        }
        cells[col] = text;
        true
    }

    /// Whether the view's budget is spent: no more cells fit.
    pub(crate) fn spent(&self) -> bool {
        self.budget.spent()
    }

    /// Adds the formula of the cell at `row`, `col`; what falls outside
    /// the limits marks the grid cut instead.
    pub(crate) fn formula(&mut self, row: usize, col: usize, text: String) {
        let text = clip(text, MAX_CELL);
        if self.formulas.len() >= MAX_CELLS || !self.budget.take(FRAME + cost(&text)) {
            self.cut = true;
            return;
        }
        self.formulas.push(Formula { row, col, text });
    }

    /// The sheet named `name`, cut to [`MAX_CELL`] characters as a cell is.
    pub(crate) fn sheet(self, name: String) -> Sheet {
        Sheet {
            name: clip(name, MAX_CELL),
            rows: self.rows,
            cut: self.cut,
            formulas: self.formulas,
        }
    }
}

/// Column `col` (from 0) as letters: 0 is A, 26 is AA.
pub fn column_name(mut col: usize) -> String {
    let mut name = Vec::new();
    loop {
        name.push(b'A' + (col % 26) as u8);
        if col < 26 {
            break;
        }
        col = col / 26 - 1;
    }
    name.reverse();
    String::from_utf8(name).expect("ASCII")
}

/// A cell reference such as `B4` as row and column from 0.
pub fn cell_at(reference: &str) -> Option<(usize, usize)> {
    let split = reference.find(|c: char| c.is_ascii_digit())?;
    let (letters, digits) = reference.split_at(split);
    if letters.is_empty() || letters.len() > 3 || !letters.bytes().all(|b| b.is_ascii_uppercase()) {
        return None;
    }
    let col = letters
        .bytes()
        .fold(0usize, |n, b| n * 26 + (b - b'A' + 1) as usize)
        - 1;
    let row: usize = digits.parse().ok()?;
    (row >= 1).then(|| (row - 1, col))
}

/// The delimiter a CSV file uses: a tab for `.tsv`, else whichever of
/// comma, semicolon and tab its first line has most of.
fn delimiter(path: &str, text: &str) -> char {
    if path.to_ascii_lowercase().ends_with(".tsv") {
        return '\t';
    }
    let first = text.lines().next().unwrap_or("");
    let mut best = (',', 0);
    for d in [',', ';', '\t'] {
        let mut quoted = false;
        let n = first
            .chars()
            .filter(|&c| {
                if c == '"' {
                    quoted = !quoted;
                }
                !quoted && c == d
            })
            .count();
        if n > best.1 {
            best = (d, n);
        }
    }
    best.0
}

/// The rows of CSV `text` (RFC 4180: quoted fields hold delimiters, line
/// breaks and doubled quotes). `cut` says the text stops short of the
/// file's end: its last row may be partial and is left out.
pub fn csv(path: &str, text: &str, cut: bool) -> Sheet {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let d = delimiter(path, text);
    let mut budget = Budget::new(MAX_VIEW);
    let mut grid = Grid::new(MAX_CELLS, &mut budget);
    let (mut row, mut col) = (0usize, 0usize);
    let mut field = String::new();
    let mut quoted = false;
    let mut chars = text.chars().peekable();
    let mut ended = true;
    while let Some(c) = chars.next() {
        ended = false;
        if quoted {
            match c {
                '"' if chars.peek() == Some(&'"') => {
                    chars.next();
                    field.push('"');
                }
                '"' => quoted = false,
                c => field.push(c),
            }
            continue;
        }
        match c {
            '"' if field.is_empty() => quoted = true,
            c if c == d => {
                grid.put(row, col, std::mem::take(&mut field));
                col += 1;
            }
            '\r' if chars.peek() == Some(&'\n') => {}
            '\n' | '\r' => {
                grid.put(row, col, std::mem::take(&mut field));
                row += 1;
                col = 0;
                ended = true;
                if row >= MAX_ROWS {
                    grid.cut = true;
                    break;
                }
            }
            c => field.push(c),
        }
    }
    if !ended {
        if cut {
            // A row the cut fell inside.
            grid.rows.truncate(row);
        } else {
            grid.put(row, col, field);
        }
    }
    if cut {
        grid.cut = true;
    }
    let name = path.rsplit('/').next().unwrap_or(path).to_owned();
    grid.sheet(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn columns_are_named_as_spreadsheets_name_them() {
        for (n, name) in [
            (0, "A"),
            (25, "Z"),
            (26, "AA"),
            (27, "AB"),
            (701, "ZZ"),
            (702, "AAA"),
        ] {
            assert_eq!(column_name(n), name);
            assert_eq!(cell_at(&format!("{name}7")), Some((6, n)));
        }
        assert_eq!(cell_at("b4"), None);
        assert_eq!(cell_at("B0"), None);
        assert_eq!(cell_at("4"), None);
    }

    #[test]
    fn csv_fields_hold_quotes_delimiters_and_line_breaks() {
        let sheet = csv(
            "x.csv",
            "\u{feff}name,note\r\n\"Ada, Countess\",\"said \"\"hi\"\"\nthen left\"\r\nBob,\n",
            false,
        );
        assert_eq!(
            sheet.rows,
            [
                vec!["name", "note"],
                vec!["Ada, Countess", "said \"hi\"\nthen left"],
                vec!["Bob", ""]
            ]
        );
        assert!(!sheet.cut);
        assert_eq!(sheet.name, "x.csv");
        // Semicolons where a spreadsheet wrote them so, tabs in a .tsv.
        assert_eq!(
            csv("a/b.csv", "a;b\n1;2", false).rows,
            [["a", "b"], ["1", "2"]]
        );
        assert_eq!(csv("b.tsv", "a,b\tc", false).rows, [["a,b", "c"]]);
    }

    #[test]
    fn a_cut_csv_leaves_out_the_row_it_cut_and_says_so() {
        let sheet = csv("x.csv", "a,b\n1,2\n3,\"unfinish", true);
        assert_eq!(sheet.rows, [["a", "b"], ["1", "2"]]);
        assert!(sheet.cut);
    }

    #[test]
    fn a_grid_keeps_to_its_limits() {
        let mut budget = Budget::new(MAX_VIEW);
        let mut grid = Grid::new(10, &mut budget);
        assert!(grid.put(0, 9, "x".into()));
        assert!(!grid.put(1, 0, "y".into()));
        assert!(grid.cut);
        let mut grid = Grid::new(MAX_CELLS, &mut budget);
        assert!(!grid.put(MAX_ROWS, 0, "x".into()));
        assert!(!grid.put(0, MAX_COLS, "x".into()));
        assert!(grid.put(0, 0, "ç".repeat(MAX_CELL + 5)));
        assert_eq!(grid.rows[0][0].chars().count(), MAX_CELL + 1);
        let many = csv("x.csv", &"1\n".repeat(MAX_ROWS + 3), false);
        assert_eq!(many.rows.len(), MAX_ROWS);
        assert!(many.cut);
    }

    #[test]
    fn cells_and_formulas_draw_on_one_budget() {
        let mut budget = Budget::new(2 * cost("") + cost("abc") + FRAME + cost("A1"));
        let mut grid = Grid::new(MAX_CELLS, &mut budget);
        assert!(grid.put(0, 0, "abc".into()));
        grid.formula(0, 0, "A1".into());
        assert!(!grid.cut);
        // Nothing is left: not even an empty cell fits.
        assert!(!grid.put(0, 1, String::new()));
        assert!(grid.cut);
        grid.cut = false;
        grid.formula(0, 0, "A1".into());
        assert!(grid.cut);
        let sheet = grid.sheet("s".repeat(MAX_CELL * 2));
        assert_eq!(sheet.rows, [["abc"]]);
        assert_eq!(sheet.formulas.len(), 1);
        assert_eq!(sheet.name.chars().count(), MAX_CELL + 1);
        // The row and the cell added cost as empty ones: all is taken.
        assert_eq!(budget.left, 0);
    }
}
