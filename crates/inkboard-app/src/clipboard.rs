//! Clipboard: board objects, text and system images.

use crate::app::App;
use crate::i18n::{t, tr};
use crate::tools::Tool;
use inkboard_core::{Align, BoxGeom, Frame, Id, ImageObj, Kind, Label, Object, Pt, RichText, Style, Table};
use inkboard_core::{Measurer, num};

/// Text placed in the system clipboard to signal "the content to paste is in the internal clipboard".
pub const MARKER: &str = "__inkboard_objects__";

/// Limits of a pasted table: the rows, the columns, the characters of a cell and of the whole table (the session accepts 100 000
/// characters per object).
const MAX_ROWS: usize = 200;
const MAX_COLS: usize = 50;
const MAX_CELL_CHARS: usize = 5000;
const MAX_TOTAL_CHARS: usize = 90_000;

/// Parses spreadsheet text (tab-separated, with quoted cells as Excel writes them) into rows of cells made rectangular;
/// `None` if the text has no tab or fewer than two cells.
fn parse_tsv(text: &str) -> Option<Vec<Vec<String>>> {
    if !text.contains('\t') {
        return None;
    }
    let mut rows: Vec<Vec<String>> = Vec::new();
    let (mut row, mut cell) = (Vec::new(), String::new());
    let (mut quoted, mut at_start) = (false, true);
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '"' if quoted => {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    cell.push('"');
                } else {
                    quoted = false;
                }
            }
            '"' if at_start => (quoted, at_start) = (true, false),
            _ if quoted => cell.push(ch),
            '\t' => {
                row.push(std::mem::take(&mut cell));
                at_start = true;
            }
            '\r' if chars.peek() == Some(&'\n') => {}
            '\n' => {
                row.push(std::mem::take(&mut cell));
                rows.push(std::mem::take(&mut row));
                at_start = true;
            }
            _ => {
                cell.push(ch);
                at_start = false;
            }
        }
    }
    // a final line break does not start another row
    if !(row.is_empty() && cell.is_empty()) {
        row.push(cell);
        rows.push(row);
    }
    // spreadsheets always write rectangular data: ragged rows or an empty first column are tab-indented text (code), not a table
    let cols = rows.first()?.len();
    if rows.len() * cols < 2 || rows.iter().any(|r| r.len() != cols) || rows.iter().all(|r| r[0].is_empty()) {
        return None;
    }
    Some(rows)
}

impl App {
    /// Copies the selection (internally) and leaves a marker in the system clipboard.
    pub fn copy_to_system(&mut self, ctx: &eframe::egui::Context) {
        self.copy_selection();
        if !self.clipboard.is_empty() {
            ctx.copy_text(MARKER.to_owned());
        }
    }

    /// Paste: internal objects, otherwise the system image, otherwise text (a new text box).
    pub fn paste_event(&mut self, text: &str) {
        if text == MARKER && !self.clipboard.is_empty() {
            return self.paste();
        }
        if let Some((w, h, rgba)) = (self.system_image)() {
            let at = self.view_center();
            return self.insert_rgba(w, h, &rgba, at);
        }
        if !text.trim().is_empty() {
            return self.paste_text(text);
        }
        self.paste();
    }

    /// The "Paste" of the menus: like Ctrl+V (objects of the board, a system image, then the text of the clipboard).
    pub fn paste_from_menu(&mut self) {
        let text = crate::platform::system_text().unwrap_or_default();
        self.paste_event(&text);
    }

    pub fn paste_text(&mut self, text: &str) {
        let at = self.view_center();
        if let Some(grid) = parse_tsv(text) {
            return self.paste_table(grid, at);
        }
        let text: String = text.chars().take(MAX_CELL_CHARS).collect();
        let mut o = Object::new_box(
            BoxGeom::Text,
            Frame::new(at.x, at.y, 10.0, 10.0),
            Style { width: 0.0, fill: None, ..self.style.clone() },
            Label { text: RichText::plain(text), align: Align::Left, ..self.label.clone() },
        );
        if let Kind::Box(b) = &mut o.kind {
            b.fit(&self.measurer);
            b.frame.x = at.x - b.frame.w / 2.0;
            b.frame.y = at.y - b.frame.h / 2.0;
        }
        let id = self.ed.insert(o);
        self.sel = vec![id];
        self.tool = Tool::Select;
    }

    /// Inserts spreadsheet data as a table centered on `at`, truncated to the table limits (a toast says so).
    fn paste_table(&mut self, mut grid: Vec<Vec<String>>, at: Pt) {
        let cols = grid.iter().map(Vec::len).max().unwrap_or(1);
        let mut truncated = grid.len() > MAX_ROWS || cols > MAX_COLS;
        grid.truncate(MAX_ROWS);
        let (rows, cols) = (grid.len(), cols.min(MAX_COLS));
        // the total text of an object is bounded by the session limits: share the budget between the cells
        let cap = MAX_CELL_CHARS.min(MAX_TOTAL_CHARS / (rows * cols)).max(1);
        let mut tb = Table::new(Pt::new(0.0, 0.0), rows, cols);
        tb.col_w.fill(0.0);
        let (line_h, pad) = (self.measurer.line_height(tb.size), 16.0);
        for (r, row) in grid.iter().enumerate() {
            let mut h = 0.0_f64;
            for (c, cell) in row.iter().take(cols).enumerate() {
                truncated |= cell.chars().count() > cap;
                let cell: String = cell.chars().take(cap).collect();
                let w = cell.lines().map(|l| self.measurer.width(l, tb.family, tb.size, false)).fold(0.0, f64::max);
                tb.col_w[c] = tb.col_w[c].max(w + 2.0 * pad);
                h = h.max(num::f64_of(cell.lines().count().max(1)) * line_h);
                tb.cells[r * cols + c] = RichText::plain(cell);
            }
            tb.row_h[r] = (h + pad).max(32.0);
        }
        for w in &mut tb.col_w {
            *w = w.clamp(60.0, 360.0);
        }
        let (w, h) = (tb.col_w.iter().sum::<f64>(), tb.row_h.iter().sum::<f64>());
        tb.origin = Pt::new(at.x - w / 2.0, at.y - h / 2.0);
        let id = self.ed.insert(Object::new(Kind::Table(tb), Style { width: 2.0, fill: None, ..self.style.clone() }));
        self.sel = vec![id];
        self.tool = Tool::Select;
        if truncated {
            self.toast(&t("Table truncated to fit the limits"));
        }
    }

    /// Inserts an image given by its RGBA pixels, centered on `at` (shrunk to fit the view).
    pub fn insert_rgba(&mut self, w: usize, h: usize, rgba: &[u8], at: Pt) {
        let img = u32::try_from(w).ok().zip(u32::try_from(h).ok()).and_then(|(w, h)| image::RgbaImage::from_raw(w, h, rgba.to_vec()));
        let Some(img) = img else {
            return self.toast(&t("Clipboard image unreadable"));
        };
        let mut png = Vec::new();
        if img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).is_err() {
            return self.toast(&t("Clipboard image unreadable"));
        }
        let aid = self.ed.doc.add_asset("image/png", png);
        let (fw, fh) = (num::f64_of(w), num::f64_of(h));
        let k = (480.0 / fw).min(360.0 / fh).min(1.0);
        let frame = Frame::new(at.x - fw * k / 2.0, at.y - fh * k / 2.0, fw * k, fh * k);
        let id = self.ed.insert(Object::new(
            Kind::Image(ImageObj { frame, asset: aid, latex: None, mono: false, recolor: Vec::new() }),
            Style { width: 0.0, fill: None, ..Style::default() },
        ));
        self.sel = vec![id];
        self.tool = Tool::Select;
    }

    /// Copies the selection (or the whole board) to the clipboard as an image.
    pub fn copy_as_image(&mut self) {
        let page = self.ed.page().clone();
        let ids: Option<Vec<Id>> = (!self.sel.is_empty()).then(|| self.sel.clone());
        let Some(svg) = inkboard_core::svg::export(&self.ed.doc, &page, ids.as_deref(), &self.measurer, &inkboard_core::svg::SvgOptions::default()) else {
            return self.toast(&t("Nothing to copy"));
        };
        let png = match inkboard_formats::raster::svg_to_png(&svg, 2.0) {
            Ok(p) => p,
            Err(e) => return self.toast(&tr!("Cannot copy: {}", e)),
        };
        let Ok(img) = image::load_from_memory(&png) else { return self.toast(&t("Cannot copy")) };
        let rgba = img.to_rgba8();
        let (width, height) = (rgba.width() as usize, rgba.height() as usize);
        match crate::platform::copy_image(width, height, rgba.into_raw()) {
            Ok(()) => self.toast(&t("Image copied")),
            Err(e) => self.toast(&tr!("Clipboard unavailable: {}", e)),
        }
    }
}
