//! The show file spec sheet: what this show *is*, and how far to trust it.
//!
//! The fidelity section is the point of this document. Every adapter records
//! what it could not carry into the model, and this page prints that list
//! rather than burying it — a spec sheet that silently omits the talkback
//! routing it failed to parse is worse than no spec sheet.

use patchferret_model::*;

use crate::image::Image;

use crate::layout::*;
use crate::pdf::{Document, Font, Page, Rgb, A4_HEIGHT, A4_WIDTH};

fn count(show: &Show, kind: StripKind) -> usize {
    show.strips_of(kind).count()
}

fn named_count(show: &Show, kind: StripKind) -> usize {
    show.strips_of(kind).filter(|s| !s.name.trim().is_empty()).count()
}

pub fn build(show: &Show, job: &JobInfo, logo: Option<&Image>) -> Vec<u8> {
    let mut doc = Document::new(format!("Show file spec — {}", show.meta.name));
    let mut page = Page::new(A4_WIDTH, A4_HEIGHT);
    let mut page_no = 1;
    let mut y = cover_header(&mut page, show, job, logo, "Show file specification");
    let x = MARGIN;
    let usable_bottom = A4_HEIGHT - FOOTER_H - 8.0;

    // --- identity ---
    y = section(&mut page, x, y + 10.0, "Show");
    kv(&mut page, x, y, "Name", &show.meta.name, 90.0);
    y += 13.0;
    kv(&mut page, x, y, "Console", &show.meta.console, 90.0);
    y += 13.0;
    kv(&mut page, x, y, "Source format", &show.meta.source_format.to_uppercase(), 90.0);
    y += 13.0;
    if let Some(v) = &show.meta.format_version {
        kv(&mut page, x, y, "File version", v, 90.0);
        y += 13.0;
    }
    if let Some(n) = &show.meta.note {
        kv(&mut page, x, y, "Note", n, 90.0);
        y += 13.0;
    }

    // --- strip inventory ---
    y = section(&mut page, x, y + 10.0, "Strip inventory");
    let inventory = [
        ("Input channels", StripKind::Input),
        ("Aux inputs", StripKind::AuxIn),
        ("FX returns", StripKind::FxReturn),
        ("Mix buses", StripKind::Bus),
        ("Matrices", StripKind::Matrix),
        ("DCA groups", StripKind::Dca),
    ];
    let cols = vec![
        Col::new("Strip type", 160.0),
        Col::right("Present", 60.0),
        Col::right("Named", 60.0),
    ];
    y = table_header(&mut page, x, y, &cols, 14.0);
    let mut zebra = false;
    for (label, kind) in inventory {
        let n = count(show, kind);
        if n == 0 {
            continue;
        }
        let cells = vec![label.to_string(), n.to_string(), named_count(show, kind).to_string()];
        y = table_row(&mut page, x, y, &cols, &cells, 13.0, zebra);
        zebra = !zebra;
    }

    // --- I/O inventory ---
    y = section(&mut page, x, y + 14.0, "I/O devices");
    let dcols = vec![
        Col::new("Device", 170.0),
        Col::new("Transport", 110.0),
        Col::right("In", 45.0),
        Col::right("Out", 45.0),
        Col::right("Patched in", 60.0),
    ];
    y = table_header(&mut page, x, y, &dcols, 14.0);
    zebra = false;
    for d in &show.devices {
        let used = show
            .patch
            .inputs
            .iter()
            .filter(|p| p.socket.as_ref().map(|s| s.device == d.id).unwrap_or(false))
            .count();
        let outs = show.patch.outputs.iter().filter(|o| o.socket.device == d.id).count();
        if used == 0 && outs == 0 {
            continue;
        }
        let cells = vec![
            d.label.clone(),
            d.transport.as_str(),
            d.inputs.to_string(),
            d.outputs.to_string(),
            used.to_string(),
        ];
        y = table_row(&mut page, x, y, &dcols, &cells, 13.0, zebra);
        zebra = !zebra;
    }

    // --- patch summary ---
    y = section(&mut page, x, y + 14.0, "Patch summary");
    let routed = show.patch.inputs.iter().filter(|p| p.socket.is_some()).count();
    let reaching = show.patch.inputs.iter().filter(|p| p.strip.is_some()).count();
    let phantom = show.head_amps.iter().filter(|h| h.phantom).count();
    kv(&mut page, x, y, "Input slots", &show.patch.inputs.len().to_string(), 130.0);
    y += 13.0;
    kv(&mut page, x, y, "Slots with a connector", &routed.to_string(), 130.0);
    y += 13.0;
    kv(&mut page, x, y, "Slots reaching a channel", &reaching.to_string(), 130.0);
    y += 13.0;
    kv(&mut page, x, y, "Output assignments", &show.patch.outputs.len().to_string(), 130.0);
    y += 13.0;
    kv(&mut page, x, y, "Preamps with phantom on", &phantom.to_string(), 130.0);

    // --- fidelity ---
    y = section(&mut page, x, y + 27.0, "Conversion fidelity");
    if show.diagnostics.is_empty() {
        page.set_fill(Rgb::INK);
        page.text(
            x,
            y,
            Font::Regular,
            8.0,
            "Every recognised element of this show file is represented in the model.",
        );
    } else {
        page.set_fill(Rgb::MUTED);
        page.text(
            x,
            y,
            Font::Regular,
            8.0,
            "The following were read but could not be carried into the interchange model.",
        );
        y += 8.0;
        page.text(
            x,
            y,
            Font::Regular,
            8.0,
            "They will not survive a conversion to another console.",
        );
        y += 14.0;

        for d in &show.diagnostics {
            // Reserve room for the two lines this entry needs.
            if y + 22.0 > usable_bottom {
                footer(&mut page, page_no, &footer_note());
                doc.add_page(page);
                page_no += 1;
                page = Page::new(A4_WIDTH, A4_HEIGHT);
                y = header(&mut page, show, "Show file specification (continued)");
                y = section(&mut page, x, y + 10.0, "Conversion fidelity (continued)");
            }

            let (colour, tag) = match d.severity {
                Severity::Unmodelled => (Rgb::WARN, "UNMODELLED"),
                Severity::Suspect => (Rgb::ALERT, "SUSPECT"),
                Severity::Unknown => (Rgb::ALERT, "UNKNOWN"),
            };
            page.set_fill(colour);
            page.text(x, y, Font::Bold, 7.0, tag);
            page.set_fill(Rgb::MUTED);
            page.text(
                x + 66.0,
                y,
                Font::Mono,
                7.0,
                &Page::fit(Font::Mono, 7.0, &d.locus, 180.0),
            );
            y += 9.5;

            page.set_fill(Rgb::INK);
            for line in wrap(&d.message, Font::Regular, 8.0, A4_WIDTH - MARGIN * 2.0 - 12.0) {
                if y + 11.0 > usable_bottom {
                    footer(&mut page, page_no, &footer_note());
                    doc.add_page(page);
                    page_no += 1;
                    page = Page::new(A4_WIDTH, A4_HEIGHT);
                    y = header(&mut page, show, "Show file specification (continued)");
                    y = section(&mut page, x, y + 10.0, "Conversion fidelity (continued)");
                    page.set_fill(Rgb::INK);
                }
                page.text(x + 12.0, y, Font::Regular, 8.0, &line);
                y += 10.0;
            }
            y += 4.0;
        }
    }

    footer(&mut page, page_no, &footer_note());
    doc.add_page(page);
    doc.finish()
}

/// Greedy word wrap to a pixel width.
fn wrap(text: &str, font: Font, size: f32, max_w: f32) -> Vec<String> {
    let mut lines = Vec::new();
    let mut cur = String::new();
    for word in text.split_whitespace() {
        let candidate = if cur.is_empty() { word.to_string() } else { format!("{cur} {word}") };
        if font.text_width(&candidate, size) > max_w && !cur.is_empty() {
            lines.push(std::mem::take(&mut cur));
            cur = word.to_string();
        } else {
            cur = candidate;
        }
    }
    if !cur.is_empty() {
        lines.push(cur);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

fn footer_note() -> String {
    "Generated by PatchFerret · the fidelity list states what a conversion would lose".into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn produces_a_valid_pdf_for_an_empty_show() {
        let pdf = build(&Show::default(), &JobInfo::default(), None);
        assert!(pdf.starts_with(b"%PDF-1.4"));
        assert!(pdf.ends_with(b"%%EOF\n"));
    }

    #[test]
    fn wrap_respects_the_width() {
        let text = "the quick brown fox jumps over the lazy dog and keeps on running";
        for line in wrap(text, Font::Regular, 8.0, 100.0) {
            assert!(Font::Regular.text_width(&line, 8.0) <= 100.0 + 0.01, "too wide: {line}");
        }
    }

    #[test]
    fn wrap_never_loses_words() {
        let text = "alpha beta gamma delta epsilon";
        let joined = wrap(text, Font::Regular, 8.0, 40.0).join(" ");
        assert_eq!(joined.split_whitespace().count(), 5);
    }

    #[test]
    fn many_diagnostics_paginate() {
        let mut show = Show::default();
        for i in 0..120 {
            show.diagnostics.push(Diagnostic {
                severity: Severity::Unmodelled,
                locus: format!("/some/path/{i}"),
                message: "a reasonably long explanation of what could not be represented in \
                          the interchange model and why that matters"
                    .into(),
            });
        }
        let pdf = build(&show, &JobInfo::default(), None);
        let s = String::from_utf8_lossy(&pdf).into_owned();
        let count: usize = s
            .split("/Count ")
            .nth(1)
            .and_then(|t| t.split_whitespace().next())
            .and_then(|n| n.parse().ok())
            .unwrap();
        assert!(count > 1, "120 diagnostics should paginate, got {count} page(s)");
    }
}
