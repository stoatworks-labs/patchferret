//! Shared page furniture: margins, headers, footers, tables.

use patchferret_model::{JobInfo, Show};

use crate::image::Image;

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

pub const LOGO_W: f32 = 120.0;
pub const LOGO_MAX_H: f32 = 52.0;

const GRID_TOP: f32 = 60.0;
const GRID_ROW: f32 = 12.5;

/// The label/value rows the cover header prints, in order.
///
/// Shared by the height calculation and the drawing code. They used to compute
/// the row count separately, which drifted the moment console/firmware rows
/// were added conditionally — the rule ended up drawn straight through the
/// notes line.
fn header_rows(show: &Show, job: &JobInfo) -> Vec<(String, String)> {
    let mut rows: Vec<(String, String)> =
        job.fields().into_iter().map(|(l, v)| (l.to_string(), v.to_string())).collect();
    rows.push(("Console".into(), job.console_or(&show.meta.console).to_string()));
    if let Some(fw) = job.firmware_or(show.meta.format_version.as_deref()) {
        rows.push(("Firmware".into(), fw.to_string()));
    }
    if !show.meta.name.is_empty() && job.event.is_some() {
        rows.push(("Show file".into(), show.meta.name.clone()));
    }
    rows.extend(job.custom.iter().cloned());
    rows
}

/// Height of the cover header block.
pub fn cover_height(show: &Show, job: &JobInfo, logo: Option<&Image>) -> f32 {
    if job.is_empty() && logo.is_none() {
        return HEADER_H;
    }
    let pairs = header_rows(show, job).len().div_ceil(2) as f32;
    let grid_end = GRID_TOP + pairs * GRID_ROW;
    let notes = if job.notes.is_some() { 13.0 } else { 0.0 };
    let logo_h = logo.map(|i| i.height_for_width(LOGO_W).min(LOGO_MAX_H)).unwrap_or(0.0);
    (grid_end + notes + 14.0).max(logo_h + 34.0)
}

/// Draw the full cover header: logo, event, and the metadata grid.
///
/// Only the first page of each report gets this; continuation pages get the
/// slim [`header`] instead, so a twelve-page patch list does not repeat the
/// engineer's phone number twelve times.
pub fn cover_header(
    page: &mut Page,
    show: &Show,
    job: &JobInfo,
    logo: Option<&Image>,
    doc_title: &str,
) -> f32 {
    if job.is_empty() && logo.is_none() {
        return header(page, show, doc_title);
    }

    let w = page.width();
    page.set_fill(Rgb::ACCENT);
    page.rect_fill(0.0, 0.0, w, 3.0);

    // Logo sits top-right, where it does not fight the document title.
    let mut text_right = w - MARGIN;
    if let Some(img) = logo {
        let natural = img.height_for_width(LOGO_W);
        let (drawn_w, drawn_h) = if natural > LOGO_MAX_H {
            (LOGO_MAX_H * img.width as f32 / img.height.max(1) as f32, LOGO_MAX_H)
        } else {
            (LOGO_W, natural)
        };
        page.image(img, w - MARGIN - drawn_w, 16.0, drawn_w, drawn_h);
        text_right = w - MARGIN - drawn_w - 16.0;
    }

    page.set_fill(Rgb::MUTED);
    page.text(MARGIN, 24.0, Font::Bold, 8.0, &doc_title.to_uppercase());

    let headline = job
        .event
        .clone()
        .or_else(|| Some(show.meta.name.clone()).filter(|s| !s.is_empty()))
        .unwrap_or_else(|| "Untitled show".into());
    page.set_fill(Rgb::INK);
    let avail = (text_right - MARGIN).max(60.0);
    page.text(MARGIN, 44.0, Font::Bold, 17.0, &Page::fit(Font::Bold, 17.0, &headline, avail));

    let rows = header_rows(show, job);
    let col_w = (w - MARGIN * 2.0) / 2.0;
    for (i, (label, value)) in rows.iter().enumerate() {
        let x = MARGIN + if i % 2 == 1 { col_w } else { 0.0 };
        let y = GRID_TOP + (i / 2) as f32 * GRID_ROW;
        page.set_fill(Rgb::MUTED);
        page.text(x, y, Font::Regular, 7.5, label);
        page.set_fill(Rgb::INK);
        page.text(
            x + 74.0,
            y,
            Font::Bold,
            8.0,
            &Page::fit(Font::Bold, 8.0, value, col_w - 82.0),
        );
    }

    let bottom = cover_height(show, job, logo);
    if let Some(note) = &job.notes {
        let pairs = rows.len().div_ceil(2) as f32;
        page.set_fill(Rgb::MUTED);
        page.text(
            MARGIN,
            GRID_TOP + pairs * GRID_ROW + 2.0,
            Font::Regular,
            7.5,
            &Page::fit(Font::Regular, 7.5, note, w - MARGIN * 2.0),
        );
    }

    page.set_stroke(Rgb::RULE);
    page.set_line_width(0.6);
    page.line(MARGIN, bottom - 9.0, w - MARGIN, bottom - 9.0);
    bottom
}
