//! The patch list: the document an engineer actually tapes to the desk.
//!
//! One row per input slot, tracing the whole chain — physical connector, the
//! device it is on, the preamp state at that connector, the slot, and the strip
//! that ends up carrying it. The output patch follows on its own pages.

use patchferret_model::*;

use crate::layout::*;
use crate::pdf::{Document, Page, Rgb, A4L_HEIGHT, A4L_WIDTH};

const ROW_H: f32 = 13.0;

fn input_cols() -> Vec<Col> {
    vec![
        Col::right("Slot", 34.0),
        Col::new("Connector", 96.0),
        Col::new("Device", 104.0),
        Col::right("Gain", 40.0),
        Col::centre("48V", 30.0),
        Col::right("Ch", 30.0),
        Col::new("Channel name", 150.0),
        Col::centre("Mute", 34.0),
        Col::right("Fader", 40.0),
        Col::new("Routing block", 70.0),
    ]
}

fn output_cols() -> Vec<Col> {
    vec![
        Col::new("Output connector", 130.0),
        Col::new("Device", 120.0),
        Col::new("Source", 150.0),
        Col::new("Tap", 80.0),
    ]
}

fn gain_text(h: Option<&HeadAmp>) -> String {
    match h.and_then(|h| h.gain_db) {
        Some(g) => format!("{g:+.1}"),
        None => "—".into(),
    }
}

fn device_label(show: &Show, id: &str) -> String {
    show.device(id).map(|d| d.label.clone()).unwrap_or_else(|| id.to_string())
}

/// Build the patch list PDF.
pub fn build(show: &Show) -> Vec<u8> {
    let mut doc = Document::new(format!("Patch list — {}", show.meta.name));
    let cols = input_cols();
    let table_w: f32 = cols.iter().map(|c| c.width).sum();
    let x0 = (A4L_WIDTH - table_w) / 2.0;
    let usable_bottom = A4L_HEIGHT - FOOTER_H - 8.0;

    let rows = show.patched_inputs();
    let mut page_no = 1;
    let mut page = Page::new(A4L_WIDTH, A4L_HEIGHT);
    let mut y = header(&mut page, show, "Input patch");
    y = table_header(&mut page, x0, y, &cols, 14.0);
    let mut zebra = false;

    for (patch, strip) in rows {
        if y + ROW_H > usable_bottom {
            footer(&mut page, page_no, &footer_note(show));
            doc.add_page(page);
            page_no += 1;
            page = Page::new(A4L_WIDTH, A4L_HEIGHT);
            y = header(&mut page, show, "Input patch (continued)");
            y = table_header(&mut page, x0, y, &cols, 14.0);
            zebra = false;
        }

        let socket_text = patch
            .socket
            .as_ref()
            .map(|s| format!("{} {}", connector_kind(show, s), s.index))
            .unwrap_or_else(|| "unrouted".into());
        let device_text = patch
            .socket
            .as_ref()
            .map(|s| device_label(show, &s.device))
            .unwrap_or_else(|| "—".into());
        let ha = patch.socket.as_ref().and_then(|s| show.head_amp(s));

        let cells = vec![
            patch.slot.to_string(),
            socket_text,
            device_text,
            gain_text(ha),
            match ha {
                Some(h) if h.phantom => "ON".into(),
                Some(_) => "".into(),
                None => "—".into(),
            },
            strip.map(|s| s.id.index.to_string()).unwrap_or_else(|| "—".into()),
            strip.map(|s| s.display_name()).unwrap_or_else(|| "not patched".into()),
            match strip {
                Some(s) if s.muted => "MUTE".into(),
                _ => "".into(),
            },
            strip.and_then(|s| s.fader_db).map(|f| format!("{f:+.1}")).unwrap_or_default(),
            patch.block_label.clone(),
        ];

        // Grey out rows that reach no channel — they are cable that lands
        // nowhere, which is exactly what a patch list should make obvious.
        if strip.is_none() {
            page.set_fill(Rgb::WASH);
            page.rect_fill(x0, y, table_w, ROW_H);
        }
        y = table_row(&mut page, x0, y, &cols, &cells, ROW_H, zebra && strip.is_some());
        zebra = !zebra;
    }

    footer(&mut page, page_no, &footer_note(show));
    doc.add_page(page);

    // --- output patch ---
    if !show.patch.outputs.is_empty() {
        let ocols = output_cols();
        let otable_w: f32 = ocols.iter().map(|c| c.width).sum();
        let ox0 = (A4L_WIDTH - otable_w) / 2.0;

        page_no += 1;
        let mut page = Page::new(A4L_WIDTH, A4L_HEIGHT);
        let mut y = header(&mut page, show, "Output patch");
        y = table_header(&mut page, ox0, y, &ocols, 14.0);
        let mut zebra = false;

        for out in &show.patch.outputs {
            if y + ROW_H > usable_bottom {
                footer(&mut page, page_no, &footer_note(show));
                doc.add_page(page);
                page_no += 1;
                page = Page::new(A4L_WIDTH, A4L_HEIGHT);
                y = header(&mut page, show, "Output patch (continued)");
                y = table_header(&mut page, ox0, y, &ocols, 14.0);
                zebra = false;
            }

            let source = match &out.source {
                SignalRef::Off => "—".to_string(),
                SignalRef::Strip(id) => show
                    .strip(*id)
                    .map(|s| {
                        if s.name.trim().is_empty() {
                            out.source_label.clone()
                        } else {
                            format!("{} ({})", out.source_label, s.name)
                        }
                    })
                    .unwrap_or_else(|| out.source_label.clone()),
                _ => out.source_label.clone(),
            };

            let cells = vec![
                format!("{} {}", connector_kind(show, &out.socket), out.socket.index),
                device_label(show, &out.socket.device),
                source,
                if out.tap == Tap::Unknown { String::new() } else { out.tap.as_str().into() },
            ];
            y = table_row(&mut page, ox0, y, &ocols, &cells, ROW_H, zebra);
            zebra = !zebra;
        }

        footer(&mut page, page_no, &footer_note(show));
        doc.add_page(page);
    }

    doc.finish()
}

/// What the connector is physically called on that device.
fn connector_kind(show: &Show, socket: &SocketRef) -> &'static str {
    match show.device(&socket.device).map(|d| &d.transport) {
        Some(Transport::Ultranet) => "Port",
        Some(Transport::Recorder) => "Track",
        Some(Transport::Card(_)) | Some(Transport::Dante) | Some(Transport::Madi) => "Ch",
        _ => match socket.dir {
            Direction::In => "In",
            Direction::Out => "Out",
        },
    }
}

fn footer_note(show: &Show) -> String {
    format!(
        "Generated by PatchFerret from {} · {} · gains and patching reflect the show file as \
         saved, not the console's live state",
        show.meta.source_format.to_uppercase(),
        show.meta.console
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn show_with(n: usize) -> Show {
        let mut show = Show::default();
        show.meta.name = "Test".into();
        show.meta.console = "Behringer X32".into();
        show.meta.source_format = "x32".into();
        show.devices.push(Device {
            id: "local".into(),
            label: "Console local I/O".into(),
            model: None,
            transport: Transport::Local,
            inputs: 32,
            outputs: 16,
        });
        for i in 1..=n as u16 {
            let mut s = Strip::new(StripId::new(StripKind::Input, i));
            s.name = format!("Ch {i}");
            s.source = SignalRef::InputSlot(i);
            show.strips.push(s);
            show.patch.inputs.push(InputPatch {
                slot: i,
                block_label: "IN1-8".into(),
                socket: Some(SocketRef::new("local", Direction::In, i)),
                strip: Some(StripId::new(StripKind::Input, i)),
            });
            show.head_amps.push(HeadAmp {
                socket: SocketRef::new("local", Direction::In, i),
                gain_db: Some(12.0),
                phantom: true,
                pad: false,
                polarity_invert: false,
            });
        }
        show
    }

    #[test]
    fn produces_a_valid_pdf() {
        let pdf = build(&show_with(8));
        assert!(pdf.starts_with(b"%PDF-1.4"));
        assert!(pdf.ends_with(b"%%EOF\n"));
    }

    #[test]
    fn paginates_when_rows_exceed_a_page() {
        let one = build(&show_with(8));
        let many = build(&show_with(200));
        assert!(many.len() > one.len());
        let s = String::from_utf8_lossy(&many).into_owned();
        let count: usize = s
            .split("/Count ")
            .nth(1)
            .and_then(|t| t.split_whitespace().next())
            .and_then(|n| n.parse().ok())
            .unwrap();
        assert!(count > 1, "200 rows should span more than one page, got {count}");
    }

    #[test]
    fn empty_show_still_produces_a_pdf() {
        let pdf = build(&Show::default());
        assert!(pdf.starts_with(b"%PDF-1.4"));
    }

    #[test]
    fn unpatched_slots_are_labelled_not_omitted() {
        let mut show = show_with(2);
        show.patch.inputs.push(InputPatch {
            slot: 3,
            block_label: "IN1-8".into(),
            socket: Some(SocketRef::new("local", Direction::In, 3)),
            strip: None,
        });
        let pdf = build(&show);
        let s = String::from_utf8_lossy(&pdf);
        assert!(s.contains("not patched"));
    }
}
