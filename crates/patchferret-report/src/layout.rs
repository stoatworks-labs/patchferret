//! Shared page furniture: margins, headers, footers, tables.

use patchferret_model::Show;

use crate::pdf::{Font, Page, Rgb};

pub const MARGIN: f32 = 32.0;
pub const HEADER_H: f32 = 46.0;
pub const FOOTER_H: f32 = 24.0;

/// A table column: heading, width in points, and alignment.
#[derive(Debug, Clone, Copy)]
pub struct Col {
    pub title: &'static str,
    pub width: f32,
    pub align: Align,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Align {
    Left,
    Right,
    Centre,
}

impl Col {
    pub const fn new(title: &'static str, width: f32) -> Self {
        Self { title, width, align: Align::Left }
    }

    pub const fn right(title: &'static str, width: f32) -> Self {
        Self { title, width, align: Align::Right }
    }

    pub const fn centre(title: &'static str, width: f32) -> Self {
        Self { title, width, align: Align::Centre }
    }
}

/// Draw the page header. Returns the y to start content at.
pub fn header(page: &mut Page, show: &Show, doc_title: &str) -> f32 {
    let w = page.width();
    page.set_fill(Rgb::ACCENT);
    page.rect_fill(0.0, 0.0, w, 3.0);

    page.set_fill(Rgb::INK);
    page.text(MARGIN, 26.0, Font::Bold, 14.0, doc_title);

    let subtitle = if show.meta.name.is_empty() {
        show.meta.console.clone()
    } else {
        format!("{}  ·  {}", show.meta.name, show.meta.console)
    };
    page.set_fill(Rgb::MUTED);
    page.text_right(w - MARGIN, 26.0, Font::Regular, 9.0, &subtitle);

    page.set_stroke(Rgb::RULE);
    page.set_line_width(0.6);
    page.line(MARGIN, HEADER_H - 12.0, w - MARGIN, HEADER_H - 12.0);

    HEADER_H
}

/// Draw the page footer.
pub fn footer(page: &mut Page, page_no: usize, note: &str) {
    let w = page.width();
    let h = page.height();
    let y = h - FOOTER_H + 8.0;

    page.set_stroke(Rgb::RULE);
    page.set_line_width(0.6);
    page.line(MARGIN, y - 10.0, w - MARGIN, y - 10.0);

    page.set_fill(Rgb::MUTED);
    page.text(MARGIN, y, Font::Regular, 7.5, note);
    page.text_right(w - MARGIN, y, Font::Regular, 7.5, &format!("Page {page_no}"));
}

/// Draw a table header row at `y`. Returns the y of the first body row.
pub fn table_header(page: &mut Page, x0: f32, y: f32, cols: &[Col], row_h: f32) -> f32 {
    let total: f32 = cols.iter().map(|c| c.width).sum();
    page.set_fill(Rgb::WASH);
    page.rect_fill(x0, y, total, row_h);

    page.set_fill(Rgb::INK);
    let mut x = x0;
    for c in cols {
        draw_cell(page, x, y + row_h - 5.0, *c, c.title, Font::Bold, 7.5);
        x += c.width;
    }

    page.set_stroke(Rgb::RULE);
    page.set_line_width(0.6);
    page.line(x0, y + row_h, x0 + total, y + row_h);
    y + row_h
}

/// Draw one cell's text, honouring alignment and truncating to fit.
pub fn draw_cell(
    page: &mut Page,
    x: f32,
    baseline: f32,
    col: Col,
    text: &str,
    font: Font,
    size: f32,
) {
    let pad = 4.0;
    let avail = col.width - pad * 2.0;
    let fitted = Page::fit(font, size, text, avail);
    match col.align {
        Align::Left => {
            page.text(x + pad, baseline, font, size, &fitted);
        }
        Align::Right => {
            page.text_right(x + col.width - pad, baseline, font, size, &fitted);
        }
        Align::Centre => {
            page.text_centre(x + col.width / 2.0, baseline, font, size, &fitted);
        }
    }
}

/// Draw a body row with optional zebra fill. Returns the next row's y.
pub fn table_row(
    page: &mut Page,
    x0: f32,
    y: f32,
    cols: &[Col],
    cells: &[String],
    row_h: f32,
    zebra: bool,
) -> f32 {
    let total: f32 = cols.iter().map(|c| c.width).sum();
    if zebra {
        page.set_fill(Rgb::WASH);
        page.rect_fill(x0, y, total, row_h);
    }
    page.set_fill(Rgb::INK);
    let mut x = x0;
    for (i, c) in cols.iter().enumerate() {
        if let Some(text) = cells.get(i) {
            draw_cell(page, x, y + row_h - 4.5, *c, text, Font::Regular, 7.5);
        }
        x += c.width;
    }
    y + row_h
}

/// A short label/value pair, as used on the spec sheet.
pub fn kv(page: &mut Page, x: f32, y: f32, label: &str, value: &str, label_w: f32) {
    page.set_fill(Rgb::MUTED);
    page.text(x, y, Font::Regular, 8.0, label);
    page.set_fill(Rgb::INK);
    page.text(x + label_w, y, Font::Bold, 8.0, value);
}

/// Section heading.
pub fn section(page: &mut Page, x: f32, y: f32, title: &str) -> f32 {
    page.set_fill(Rgb::ACCENT);
    page.text(x, y, Font::Bold, 10.0, title);
    page.set_stroke(Rgb::RULE);
    page.set_line_width(0.6);
    page.line(x, y + 4.0, page.width() - MARGIN, y + 4.0);
    y + 16.0
}
