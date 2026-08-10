//! A small, dependency-free PDF writer.
//!
//! Written rather than pulled in because the reports have to be generated
//! *inside the browser*: the whole privacy argument for PatchFerret is that a
//! show file never leaves the machine, so the PDF path must compile to
//! `wasm32-unknown-unknown`. General-purpose PDF crates drag in image decoders,
//! font shapers and filesystem access, none of which this needs — every report
//! is text, rules and boxes in the 14 standard PDF fonts, which require no
//! embedding at all.
//!
//! Coordinates are PDF points with the origin at the bottom-left, but the
//! helpers here take a top-down `y` because every report is laid out downward.

use std::fmt::Write as _;

pub const A4_WIDTH: f32 = 595.276;
pub const A4_HEIGHT: f32 = 841.89;
/// Landscape A4, used for the patch list and topology diagram.
pub const A4L_WIDTH: f32 = A4_HEIGHT;
pub const A4L_HEIGHT: f32 = A4_WIDTH;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Font {
    Regular,
    Bold,
    Mono,
}

impl Font {
    fn resource(self) -> &'static str {
        match self {
            Font::Regular => "F1",
            Font::Bold => "F2",
            Font::Mono => "F3",
        }
    }

    fn base_font(self) -> &'static str {
        match self {
            Font::Regular => "Helvetica",
            Font::Bold => "Helvetica-Bold",
            Font::Mono => "Courier",
        }
    }

    /// Glyph advance for `c` in units of 1/1000 em.
    fn width(self, c: char) -> u16 {
        if self == Font::Mono {
            return 600;
        }
        let i = c as usize;
        if !(32..127).contains(&i) {
            // Punctuation we actually emit needs its real advance, or `fit`
            // under-measures and text overruns its column.
            return match c {
                '\u{2014}' => 1000, // em dash
                '\u{2013}' => 556,  // en dash
                '\u{2026}' => 1000, // ellipsis
                '\u{2018}' | '\u{2019}' => 222,
                '\u{201C}' | '\u{201D}' => 333,
                '\u{2022}' => 350,
                '\u{00B7}' => 278,
                _ => self.width('?'),
            };
        }
        let table = match self {
            Font::Bold => &HELVETICA_BOLD_WIDTHS,
            _ => &HELVETICA_WIDTHS,
        };
        table[i - 32]
    }

    /// Width of `s` in points at `size`.
    pub fn text_width(self, s: &str, size: f32) -> f32 {
        s.chars().map(|c| self.width(c) as f32).sum::<f32>() * size / 1000.0
    }
}

#[rustfmt::skip]
const HELVETICA_WIDTHS: [u16; 95] = [
    278, 278, 355, 556, 556, 889, 667, 191, 333, 333, 389, 584, 278, 333, 278, 278,
    556, 556, 556, 556, 556, 556, 556, 556, 556, 556, 278, 278, 584, 584, 584, 556,
    1015, 667, 667, 722, 722, 667, 611, 778, 722, 278, 500, 667, 556, 833, 722, 778,
    667, 778, 722, 667, 611, 722, 667, 944, 667, 667, 611, 278, 278, 278, 469, 556,
    333, 556, 556, 500, 556, 556, 278, 556, 556, 222, 222, 500, 222, 833, 556, 556,
    556, 556, 333, 500, 278, 556, 500, 722, 500, 500, 500, 334, 260, 334, 584,
];

#[rustfmt::skip]
const HELVETICA_BOLD_WIDTHS: [u16; 95] = [
    278, 333, 474, 556, 556, 889, 722, 238, 333, 333, 389, 584, 278, 333, 278, 278,
    556, 556, 556, 556, 556, 556, 556, 556, 556, 556, 333, 333, 584, 584, 584, 611,
    975, 722, 722, 722, 722, 667, 611, 778, 722, 278, 556, 722, 611, 833, 722, 778,
    667, 778, 722, 667, 611, 722, 667, 944, 667, 667, 611, 333, 278, 333, 584, 556,
    333, 556, 611, 556, 611, 556, 333, 611, 611, 278, 278, 556, 278, 889, 611, 611,
    611, 611, 389, 556, 333, 611, 556, 778, 556, 556, 500, 389, 280, 389, 584,
];

/// An RGB colour, components in 0.0..=1.0.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rgb(pub f32, pub f32, pub f32);

impl Rgb {
    pub const BLACK: Rgb = Rgb(0.0, 0.0, 0.0);
    pub const WHITE: Rgb = Rgb(1.0, 1.0, 1.0);
    /// Body text.
    pub const INK: Rgb = Rgb(0.12, 0.13, 0.15);
    /// Secondary text.
    pub const MUTED: Rgb = Rgb(0.42, 0.45, 0.49);
    /// Hairlines and rules.
    pub const RULE: Rgb = Rgb(0.80, 0.82, 0.85);
    /// Table header / zebra fill.
    pub const WASH: Rgb = Rgb(0.945, 0.952, 0.960);
    pub const ACCENT: Rgb = Rgb(0.11, 0.38, 0.60);
    pub const WARN: Rgb = Rgb(0.70, 0.42, 0.05);
    pub const ALERT: Rgb = Rgb(0.68, 0.19, 0.19);
}

/// WinAnsiEncoding's 0x80–0x9F block, which is *not* Latin-1.
///
/// Without this, an em dash or a curly quote falls past the Latin-1 range and
/// is replaced with '?' — which is how the first draft of the patch list ended
/// up printing "?" in every empty cell.
fn winansi_high(c: char) -> Option<u8> {
    Some(match c {
        '\u{20AC}' => 0x80, // €
        '\u{201A}' => 0x82,
        '\u{0192}' => 0x83,
        '\u{201E}' => 0x84,
        '\u{2026}' => 0x85, // …
        '\u{2020}' => 0x86,
        '\u{2021}' => 0x87,
        '\u{02C6}' => 0x88,
        '\u{2030}' => 0x89,
        '\u{0160}' => 0x8A,
        '\u{2039}' => 0x8B,
        '\u{0152}' => 0x8C,
        '\u{017D}' => 0x8E,
        '\u{2018}' => 0x91, // ‘
        '\u{2019}' => 0x92, // ’
        '\u{201C}' => 0x93, // “
        '\u{201D}' => 0x94, // ”
        '\u{2022}' => 0x95, // •
        '\u{2013}' => 0x96, // – en dash
        '\u{2014}' => 0x97, // — em dash
        '\u{02DC}' => 0x98,
        '\u{2122}' => 0x99,
        '\u{0161}' => 0x9A,
        '\u{203A}' => 0x9B,
        '\u{0153}' => 0x9C,
        '\u{017E}' => 0x9E,
        '\u{0178}' => 0x9F,
        _ => return None,
    })
}

/// Escape a string for a PDF literal string, transliterating to WinAnsi.
fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        match c {
            '(' => out.push_str("\\("),
            ')' => out.push_str("\\)"),
            '\\' => out.push_str("\\\\"),
            c if (c as u32) < 32 => out.push(' '),
            c if (c as u32) < 127 => out.push(c),
            c => {
                // WinAnsi's high block first, then Latin-1, then give up. A
                // stray CJK or emoji channel name must not corrupt the file.
                if let Some(b) = winansi_high(c) {
                    let _ = write!(out, "\\{b:03o}");
                } else if (c as u32) <= 255 {
                    let _ = write!(out, "\\{:03o}", c as u32);
                } else {
                    out.push('?');
                }
            }
        }
    }
    out
}

/// One page's content stream, built up by the report code.
pub struct Page {
    width: f32,
    height: f32,
    ops: String,
}

impl Page {
    pub fn new(width: f32, height: f32) -> Self {
        Self { width, height, ops: String::new() }
    }

    pub fn width(&self) -> f32 {
        self.width
    }

    pub fn height(&self) -> f32 {
        self.height
    }

    /// Convert a top-down y to PDF's bottom-up space.
    fn flip(&self, y: f32) -> f32 {
        self.height - y
    }

    pub fn set_fill(&mut self, c: Rgb) -> &mut Self {
        let _ = writeln!(self.ops, "{:.4} {:.4} {:.4} rg", c.0, c.1, c.2);
        self
    }

    pub fn set_stroke(&mut self, c: Rgb) -> &mut Self {
        let _ = writeln!(self.ops, "{:.4} {:.4} {:.4} RG", c.0, c.1, c.2);
        self
    }

    pub fn set_line_width(&mut self, w: f32) -> &mut Self {
        let _ = writeln!(self.ops, "{w:.3} w");
        self
    }

    /// Draw text with its left edge at `x` and baseline at `y` (top-down).
    pub fn text(&mut self, x: f32, y: f32, font: Font, size: f32, s: &str) -> &mut Self {
        let _ = writeln!(
            self.ops,
            "BT /{} {:.2} Tf {:.2} {:.2} Td ({}) Tj ET",
            font.resource(),
            size,
            x,
            self.flip(y),
            escape(s)
        );
        self
    }

    /// Draw text right-aligned to `x`.
    pub fn text_right(&mut self, x: f32, y: f32, font: Font, size: f32, s: &str) -> &mut Self {
        self.text(x - font.text_width(s, size), y, font, size, s)
    }

    /// Draw text centred on `x`.
    pub fn text_centre(&mut self, x: f32, y: f32, font: Font, size: f32, s: &str) -> &mut Self {
        self.text(x - font.text_width(s, size) / 2.0, y, font, size, s)
    }

    /// Truncate `s` with an ellipsis so it fits `max_w` at `size`.
    pub fn fit(font: Font, size: f32, s: &str, max_w: f32) -> String {
        if font.text_width(s, size) <= max_w {
            return s.to_string();
        }
        let ell = "…";
        let ell_w = font.text_width(ell, size);
        let mut out = String::new();
        let mut w = 0.0;
        for c in s.chars() {
            let cw = font.width(c) as f32 * size / 1000.0;
            if w + cw + ell_w > max_w {
                break;
            }
            out.push(c);
            w += cw;
        }
        out.push_str(ell);
        out
    }

    pub fn line(&mut self, x1: f32, y1: f32, x2: f32, y2: f32) -> &mut Self {
        let _ = writeln!(
            self.ops,
            "{:.2} {:.2} m {:.2} {:.2} l S",
            x1,
            self.flip(y1),
            x2,
            self.flip(y2)
        );
        self
    }

    pub fn rect_fill(&mut self, x: f32, y: f32, w: f32, h: f32) -> &mut Self {
        let _ = writeln!(self.ops, "{:.2} {:.2} {:.2} {:.2} re f", x, self.flip(y + h), w, h);
        self
    }

    pub fn rect_stroke(&mut self, x: f32, y: f32, w: f32, h: f32) -> &mut Self {
        let _ = writeln!(self.ops, "{:.2} {:.2} {:.2} {:.2} re S", x, self.flip(y + h), w, h);
        self
    }

    /// Rounded-corner box, stroked. Radius is clamped to half the short side.
    pub fn round_rect(
        &mut self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        r: f32,
        fill: bool,
    ) -> &mut Self {
        let r = r.min(w / 2.0).min(h / 2.0).max(0.0);
        let k = r * 0.5523;
        let (x0, x1) = (x, x + w);
        let (y0, y1) = (self.flip(y + h), self.flip(y));
        let _ = writeln!(self.ops, "{:.2} {:.2} m", x0 + r, y0);
        let _ = writeln!(self.ops, "{:.2} {:.2} l", x1 - r, y0);
        let _ = writeln!(
            self.ops,
            "{:.2} {:.2} {:.2} {:.2} {:.2} {:.2} c",
            x1 - r + k,
            y0,
            x1,
            y0 + r - k,
            x1,
            y0 + r
        );
        let _ = writeln!(self.ops, "{:.2} {:.2} l", x1, y1 - r);
        let _ = writeln!(
            self.ops,
            "{:.2} {:.2} {:.2} {:.2} {:.2} {:.2} c",
            x1,
            y1 - r + k,
            x1 - r + k,
            y1,
            x1 - r,
            y1
        );
        let _ = writeln!(self.ops, "{:.2} {:.2} l", x0 + r, y1);
        let _ = writeln!(
            self.ops,
            "{:.2} {:.2} {:.2} {:.2} {:.2} {:.2} c",
            x0 + r - k,
            y1,
            x0,
            y1 - r + k,
            x0,
            y1 - r
        );
        let _ = writeln!(self.ops, "{:.2} {:.2} l", x0, y0 + r);
        let _ = writeln!(
            self.ops,
            "{:.2} {:.2} {:.2} {:.2} {:.2} {:.2} c",
            x0,
            y0 + r - k,
            x0 + r - k,
            y0,
            x0 + r,
            y0
        );
        let _ = writeln!(self.ops, "{}", if fill { "f" } else { "S" });
        self
    }
}

/// A PDF document under construction.
pub struct Document {
    pages: Vec<Page>,
    title: String,
}

impl Document {
    pub fn new(title: impl Into<String>) -> Self {
        Self { pages: Vec::new(), title: title.into() }
    }

    pub fn add_page(&mut self, page: Page) {
        self.pages.push(page);
    }

    pub fn page_count(&self) -> usize {
        self.pages.len()
    }

    /// Serialise to PDF bytes.
    pub fn finish(self) -> Vec<u8> {
        // Object numbering: 1 catalog, 2 pages, 3..5 fonts, then per page a
        // page object and a content stream.
        let font_ids = [3usize, 4, 5];
        let first_page_obj = 6;
        let n_pages = self.pages.len().max(1);

        let mut objects: Vec<String> = Vec::new();

        let kids: Vec<String> =
            (0..n_pages).map(|i| format!("{} 0 R", first_page_obj + i * 2)).collect();

        objects.push("<< /Type /Catalog /Pages 2 0 R >>".into());
        objects.push(format!(
            "<< /Type /Pages /Count {} /Kids [{}] >>",
            n_pages,
            kids.join(" ")
        ));
        for f in [Font::Regular, Font::Bold, Font::Mono] {
            objects.push(format!(
                "<< /Type /Font /Subtype /Type1 /BaseFont /{} /Encoding /WinAnsiEncoding >>",
                f.base_font()
            ));
        }

        let resources = format!(
            "<< /Font << /F1 {} 0 R /F2 {} 0 R /F3 {} 0 R >> >>",
            font_ids[0], font_ids[1], font_ids[2]
        );

        let pages: Vec<Page> = if self.pages.is_empty() {
            vec![Page::new(A4_WIDTH, A4_HEIGHT)]
        } else {
            self.pages
        };

        for (i, page) in pages.iter().enumerate() {
            let content_obj = first_page_obj + i * 2 + 1;
            objects.push(format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {:.3} {:.3}] /Resources {} \
                 /Contents {} 0 R >>",
                page.width, page.height, resources, content_obj
            ));
            objects.push(format!(
                "<< /Length {} >>\nstream\n{}\nendstream",
                page.ops.len(),
                page.ops
            ));
        }

        // Info object, last.
        let info_obj = objects.len() + 1;
        objects.push(format!(
            "<< /Title ({}) /Producer (PatchFerret) /Creator (PatchFerret) >>",
            escape(&self.title)
        ));

        // Assembled as bytes, not as a String. Offsets in the xref table are
        // byte offsets into the finished file, so building in UTF-8 and
        // converting at the end would shift every entry by the width of the
        // non-ASCII header comment and produce a file some readers reject.
        let mut out: Vec<u8> = Vec::new();
        let push = |out: &mut Vec<u8>, s: &str| {
            out.extend(s.chars().map(|c| if (c as u32) < 256 { c as u8 } else { b'?' }));
        };

        out.extend_from_slice(b"%PDF-1.4\n");
        // Binary marker comment: tells tools the file is not plain text.
        out.extend_from_slice(&[b'%', 0xE2, 0xE3, 0xCF, 0xD3, b'\n']);

        let mut offsets = Vec::with_capacity(objects.len());
        for (i, body) in objects.iter().enumerate() {
            offsets.push(out.len());
            push(&mut out, &format!("{} 0 obj\n{}\nendobj\n", i + 1, body));
        }

        let xref_at = out.len();
        push(&mut out, &format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1));
        for off in &offsets {
            push(&mut out, &format!("{off:010} 00000 n \n"));
        }
        push(
            &mut out,
            &format!(
                "trailer\n<< /Size {} /Root 1 0 R /Info {} 0 R >>\nstartxref\n{}\n%%EOF\n",
                objects.len() + 1,
                info_obj,
                xref_at
            ),
        );
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn produces_a_parseable_header_and_trailer() {
        let mut doc = Document::new("Test");
        let mut p = Page::new(A4_WIDTH, A4_HEIGHT);
        p.text(50.0, 50.0, Font::Regular, 12.0, "Hello");
        doc.add_page(p);
        let bytes = doc.finish();
        assert!(bytes.starts_with(b"%PDF-1.4"));
        assert!(bytes.ends_with(b"%%EOF\n"));
        let s = String::from_utf8_lossy(&bytes);
        assert!(s.contains("/Type /Catalog"));
        assert!(s.contains("startxref"));
    }

    #[test]
    fn xref_offsets_point_at_their_objects() {
        let mut doc = Document::new("Offsets");
        doc.add_page(Page::new(A4_WIDTH, A4_HEIGHT));
        let bytes = doc.finish();
        // Byte-oriented throughout: the file is Latin-1, and checking offsets
        // via a UTF-8 string is exactly the mistake this test exists to catch.
        let tail =
            bytes.windows(9).rposition(|w| w == b"startxref").expect("startxref present");
        let xref_start: usize = String::from_utf8_lossy(&bytes[tail + 9..])
            .trim()
            .lines()
            .next()
            .unwrap()
            .parse()
            .unwrap();
        assert_eq!(&bytes[xref_start..xref_start + 4], b"xref");

        // Every offset in the table must land on "<n> 0 obj". Entry 0 is the
        // mandatory free-list head, so `i` is already the object number.
        let table = String::from_utf8_lossy(&bytes[xref_start..]).into_owned();
        let mut checked = 0;
        for (i, line) in table.lines().skip(2).enumerate() {
            let Some(off) = line.split_whitespace().next() else { break };
            let Ok(off) = off.parse::<usize>() else { break };
            if off == 0 {
                continue;
            }
            checked += 1;
            let expected = format!("{} 0 obj", i);
            assert!(
                bytes[off..].starts_with(expected.as_bytes()),
                "xref entry {} points at {:?}",
                i + 1,
                String::from_utf8_lossy(&bytes[off..(off + 20).min(bytes.len())])
            );
        }
        assert!(checked >= 6, "only {checked} xref entries were verified");
    }

    #[test]
    fn escapes_pdf_string_delimiters() {
        assert_eq!(escape("a(b)c\\d"), "a\\(b\\)c\\\\d");
    }

    #[test]
    fn em_dash_maps_into_winansi_not_a_question_mark() {
        // Regression: the first patch list printed "?" in every empty cell
        // because the em dash fell past Latin-1 and hit the fallback.
        let out = escape("—");
        assert_eq!(out, "\\227");
        assert!(!out.contains('?'));
        assert_eq!(escape("…"), "\\205");
    }

    #[test]
    fn transliterates_beyond_latin1() {
        // A channel called "日本" must not produce invalid bytes.
        let out = escape("日本 Café");
        assert!(out.is_ascii() || out.contains("\\3"));
        assert!(!out.contains('日'));
    }

    #[test]
    fn text_width_is_proportional_and_nonzero() {
        assert!(Font::Regular.text_width("iii", 10.0) < Font::Regular.text_width("WWW", 10.0));
        assert!(Font::Regular.text_width("Hello", 10.0) > 0.0);
        // Courier is monospaced.
        assert_eq!(Font::Mono.text_width("iii", 10.0), Font::Mono.text_width("WWW", 10.0));
    }

    #[test]
    fn fit_truncates_long_text() {
        let long = "A very long channel name that will not fit in the column";
        let fitted = Page::fit(Font::Regular, 8.0, long, 60.0);
        assert!(fitted.len() < long.len());
        assert!(fitted.ends_with('…'));
        assert!(Font::Regular.text_width(&fitted, 8.0) <= 60.0);
    }

    #[test]
    fn fit_leaves_short_text_alone() {
        assert_eq!(Page::fit(Font::Regular, 8.0, "Kick", 200.0), "Kick");
    }

    #[test]
    fn empty_document_still_emits_one_page() {
        let bytes = Document::new("Empty").finish();
        let s = String::from_utf8_lossy(&bytes);
        assert!(s.contains("/Count 1"));
    }
}
