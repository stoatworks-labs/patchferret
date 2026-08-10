//! The wiring topology diagram.
//!
//! Answers the question a patch list cannot: *what is plugged into what, and
//! how much of it is actually in use*. Devices are drawn as boxes down each
//! side — sources on the left, destinations on the right — with the console in
//! the middle and a link per device whose weight reflects how many connectors
//! that device actually contributes.
//!
//! Deliberately not a signal-flow diagram. Drawing 32 individual channel paths
//! produces a hairball nobody reads; grouping by device and labelling the link
//! with the connector range is what an engineer needs when they are looking at
//! a stage box wondering which cable to chase.

use patchferret_model::*;

use crate::image::Image;

use crate::layout::{cover_header, footer, MARGIN};
use crate::pdf::{Document, Font, Page, Rgb, A4L_HEIGHT, A4L_WIDTH};

/// One device's contribution to the patch, ready to draw.
struct Node {
    label: String,
    transport: String,
    /// Connectors of this device that the show actually uses.
    used: usize,
    /// Connectors the device has in that direction.
    capacity: u16,
    /// Contiguous ranges of connector indices in use, e.g. "1–24".
    ranges: String,
}

/// Collapse a sorted list of indices into a compact range string.
fn ranges(mut idx: Vec<u16>) -> String {
    idx.sort_unstable();
    idx.dedup();
    if idx.is_empty() {
        return String::new();
    }
    let mut out: Vec<String> = Vec::new();
    let mut start = idx[0];
    let mut prev = idx[0];
    for &i in &idx[1..] {
        if i == prev + 1 {
            prev = i;
            continue;
        }
        out.push(if start == prev { format!("{start}") } else { format!("{start}-{prev}") });
        start = i;
        prev = i;
    }
    out.push(if start == prev { format!("{start}") } else { format!("{start}-{prev}") });
    // Keep the label short; the patch list has the detail.
    if out.len() > 3 {
        format!("{}, +{} more", out[..2].join(", "), out.len() - 2)
    } else {
        out.join(", ")
    }
}

fn input_nodes(show: &Show) -> Vec<Node> {
    let mut nodes = Vec::new();
    for d in &show.devices {
        let idx: Vec<u16> = show
            .patch
            .inputs
            .iter()
            .filter_map(|p| p.socket.as_ref())
            .filter(|s| s.device == d.id)
            .map(|s| s.index)
            .collect();
        if idx.is_empty() {
            continue;
        }
        nodes.push(Node {
            label: d.label.clone(),
            transport: d.transport.as_str(),
            used: {
                let mut u = idx.clone();
                u.sort_unstable();
                u.dedup();
                u.len()
            },
            capacity: d.inputs,
            ranges: ranges(idx),
        });
    }
    nodes.sort_by_key(|n| std::cmp::Reverse(n.used));
    nodes
}

fn output_nodes(show: &Show) -> Vec<Node> {
    let mut nodes = Vec::new();
    for d in &show.devices {
        let idx: Vec<u16> = show
            .patch
            .outputs
            .iter()
            .filter(|o| o.socket.device == d.id && o.source != SignalRef::Off)
            .map(|o| o.socket.index)
            .collect();
        if idx.is_empty() {
            continue;
        }
        nodes.push(Node {
            label: d.label.clone(),
            transport: d.transport.as_str(),
            used: {
                let mut u = idx.clone();
                u.sort_unstable();
                u.dedup();
                u.len()
            },
            capacity: d.outputs,
            ranges: ranges(idx),
        });
    }
    nodes.sort_by_key(|n| std::cmp::Reverse(n.used));
    nodes
}

const BOX_W: f32 = 168.0;
const BOX_H: f32 = 44.0;
const GAP: f32 = 14.0;

fn draw_node(page: &mut Page, x: f32, y: f32, node: &Node, accent: Rgb) {
    page.set_fill(Rgb::WHITE);
    page.round_rect(x, y, BOX_W, BOX_H, 4.0, true);
    page.set_stroke(Rgb::RULE);
    page.set_line_width(0.8);
    page.round_rect(x, y, BOX_W, BOX_H, 4.0, false);

    // Accent stripe showing how full the device is.
    let frac = if node.capacity == 0 {
        0.0
    } else {
        (node.used as f32 / node.capacity as f32).clamp(0.0, 1.0)
    };
    page.set_fill(accent);
    page.rect_fill(x, y + 4.0, 3.0, BOX_H - 8.0);
    // Inset so the bar stays clear of the rounded corners.
    page.rect_fill(x + 5.0, y + BOX_H - 6.0, (BOX_W - 10.0) * frac, 2.5);

    page.set_fill(Rgb::INK);
    page.text(
        x + 9.0,
        y + 15.0,
        Font::Bold,
        8.5,
        &Page::fit(Font::Bold, 8.5, &node.label, BOX_W - 18.0),
    );
    page.set_fill(Rgb::MUTED);
    page.text(
        x + 9.0,
        y + 26.0,
        Font::Regular,
        7.0,
        &Page::fit(Font::Regular, 7.0, &node.transport, BOX_W - 18.0),
    );
    let usage = format!("{} of {} · {}", node.used, node.capacity, node.ranges);
    page.text(
        x + 9.0,
        y + 37.0,
        Font::Regular,
        7.0,
        &Page::fit(Font::Regular, 7.0, &usage, BOX_W - 18.0),
    );
}

pub fn build(show: &Show, job: &JobInfo, logo: Option<&Image>) -> Vec<u8> {
    let mut doc = Document::new(format!("Wiring topology — {}", show.meta.name));
    let mut page = Page::new(A4L_WIDTH, A4L_HEIGHT);
    let y0 = cover_header(&mut page, show, job, logo, "Wiring topology");

    let ins = input_nodes(show);
    let outs = output_nodes(show);

    let left_x = MARGIN;
    let right_x = A4L_WIDTH - MARGIN - BOX_W;
    let centre_w = 150.0;
    let centre_x = (A4L_WIDTH - centre_w) / 2.0;

    let stack_h = |n: usize| n as f32 * BOX_H + n.saturating_sub(1) as f32 * GAP;
    let area_top = y0 + 18.0;
    let area_bottom = A4L_HEIGHT - crate::layout::FOOTER_H - 20.0;
    let area_h = area_bottom - area_top;

    let left_y = area_top + (area_h - stack_h(ins.len())).max(0.0) / 2.0;
    let right_y = area_top + (area_h - stack_h(outs.len())).max(0.0) / 2.0;

    // Console block in the middle, sized to span both stacks.
    let console_top = area_top + 6.0;
    let console_h = (area_h - 12.0).max(BOX_H);
    page.set_fill(Rgb::WASH);
    page.round_rect(centre_x, console_top, centre_w, console_h, 6.0, true);
    page.set_stroke(Rgb::ACCENT);
    page.set_line_width(1.2);
    page.round_rect(centre_x, console_top, centre_w, console_h, 6.0, false);

    page.set_fill(Rgb::INK);
    page.text_centre(
        centre_x + centre_w / 2.0,
        console_top + console_h / 2.0 - 4.0,
        Font::Bold,
        10.0,
        &Page::fit(Font::Bold, 10.0, &show.meta.console, centre_w - 16.0),
    );
    page.set_fill(Rgb::MUTED);
    page.text_centre(
        centre_x + centre_w / 2.0,
        console_top + console_h / 2.0 + 9.0,
        Font::Regular,
        7.5,
        &format!("{} input slots", show.patch.inputs.len()),
    );

    // Column captions.
    page.set_fill(Rgb::MUTED);
    page.text(left_x, area_top - 6.0, Font::Bold, 7.5, "SOURCES");
    page.text_right(right_x + BOX_W, area_top - 6.0, Font::Bold, 7.5, "DESTINATIONS");

    // Links first so boxes sit on top of them.
    page.set_stroke(Rgb::ACCENT);
    for (i, n) in ins.iter().enumerate() {
        let y = left_y + i as f32 * (BOX_H + GAP) + BOX_H / 2.0;
        // Line weight carries the channel count, capped so one big stagebox
        // cannot render the thin links invisible.
        page.set_line_width((n.used as f32 / 8.0).clamp(0.5, 3.0));
        page.line(left_x + BOX_W, y, centre_x, y);
    }
    for (i, n) in outs.iter().enumerate() {
        let y = right_y + i as f32 * (BOX_H + GAP) + BOX_H / 2.0;
        page.set_line_width((n.used as f32 / 8.0).clamp(0.5, 3.0));
        page.line(centre_x + centre_w, y, right_x, y);
    }

    for (i, n) in ins.iter().enumerate() {
        draw_node(&mut page, left_x, left_y + i as f32 * (BOX_H + GAP), n, Rgb::ACCENT);
    }
    for (i, n) in outs.iter().enumerate() {
        draw_node(&mut page, right_x, right_y + i as f32 * (BOX_H + GAP), n, Rgb::WARN);
    }

    if ins.is_empty() && outs.is_empty() {
        page.set_fill(Rgb::MUTED);
        page.text_centre(
            A4L_WIDTH / 2.0,
            area_top + area_h / 2.0 + 40.0,
            Font::Regular,
            9.0,
            "No routed I/O found in this show file.",
        );
    }

    footer(
        &mut page,
        1,
        "Generated by PatchFerret · bar along each box shows how much of that device is in use",
    );
    doc.add_page(page);
    doc.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collapses_contiguous_ranges() {
        assert_eq!(ranges(vec![1, 2, 3, 4]), "1-4");
        assert_eq!(ranges(vec![1, 3]), "1, 3");
        assert_eq!(ranges(vec![5]), "5");
        assert_eq!(ranges(vec![]), "");
    }

    #[test]
    fn deduplicates_and_sorts_before_collapsing() {
        assert_eq!(ranges(vec![3, 1, 2, 2, 3]), "1-3");
    }

    #[test]
    fn abbreviates_very_fragmented_ranges() {
        let out = ranges(vec![1, 3, 5, 7, 9]);
        assert!(out.contains("more"), "expected abbreviation, got {out}");
    }

    #[test]
    fn produces_a_valid_pdf_even_with_no_io() {
        let pdf = build(&Show::default(), &JobInfo::default(), None);
        assert!(pdf.starts_with(b"%PDF-1.4"));
        let s = String::from_utf8_lossy(&pdf);
        assert!(s.contains("No routed I/O found"));
    }
}
