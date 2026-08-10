//! Yamaha CL / QL — the `.CLF` console file.
//!
//! **Not the MMS/MBDF format** used by DM3, DM7 and TF (see [`super::mms`]).
//! CL and QL are the previous architecture: a flat, uncompressed binary with no
//! self-description and no descriptors shipped alongside. Everything below came
//! from controlled diffs through QL Editor V5.8.1 running offline.
//!
//! QL Editor's save panel offers "Console File (\*.CLF)" — the same extension CL
//! Editor writes — so one adapter is expected to cover both. **No CL-written
//! file has been examined**, so that is an expectation rather than a finding,
//! and the header check below is deliberately loose enough to let one through
//! and report what it is.
//!
//! # Provenance
//!
//! Two diffs, each changing exactly one patch point:
//!
//! ```text
//! CH4  INPUT4  -> DANTE1   0x00d74e: 0x44 -> 0x01
//! CH12 INPUT12 -> DANTE2   0x00d756: 0xc4 -> 0x02
//! ```
//!
//! An 8-byte gap for an 8-channel gap: one byte per channel, contiguous. See
//! `docs/yamaha-clf.md` in the research repository.

use patchferret_model::*;

use crate::{AdapterError, Confidence, ShowAdapter, ShowInput};

/// Offset of input channel 1's patch byte.
///
/// Observed in a QL5 file written by QL Editor V5.8.1. It is an absolute offset
/// into a flat file, which is fragile across frame sizes and firmware — so it is
/// never used without checking that what it points at actually decodes, and the
/// adapter reports rather than guesses when it does not.
const PATCH_TABLE: usize = 0x00d74b;

/// Input channels read. A QL5's default table runs exactly this many entries
/// before the values stop decoding, and it is the only frame size seen.
const CHANNELS: usize = 64;

/// Where the product string sits in the header.
const PRODUCT_AT: usize = 0x10;

pub struct ClfAdapter;

impl ShowAdapter for ClfAdapter {
    fn id(&self) -> &'static str {
        "yamaha-clf"
    }

    fn display_name(&self) -> &'static str {
        "Yamaha CL / QL console file"
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["clf"]
    }

    fn sniff(&self, input: &ShowInput) -> Confidence {
        let Some(f) = input.primary() else {
            return Confidence::No;
        };
        match product(&f.bytes) {
            Some(_) => Confidence::Strong,
            None => Confidence::No,
        }
    }

    fn parse(&self, input: &ShowInput) -> Result<Show, AdapterError> {
        let f = input.primary().ok_or(AdapterError::Unrecognised)?;
        parse_clf(&f.bytes, &input.name)
            .map_err(|e| AdapterError::Parse { adapter: "yamaha-clf", message: e.to_string() })
    }
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum ClfError {
    #[error("not a CL/QL console file")]
    NotClf,
}

/// The product string, e.g. `QL [OSX, 5.8.1.27]`.
///
/// This is the format's only reliable signature: the leading bytes are a
/// version word with nothing distinctive in it.
fn product(d: &[u8]) -> Option<String> {
    let s = d.get(PRODUCT_AT..PRODUCT_AT + 32)?;
    let end = s.iter().position(|&b| b == 0).unwrap_or(s.len());
    let text = std::str::from_utf8(&s[..end]).ok()?.trim().to_string();
    if text.starts_with("QL ") || text.starts_with("CL ") {
        Some(text)
    } else {
        None
    }
}

/// Decode a patch byte into a source.
///
/// ```text
/// 0x00        not patched
/// 0x01..0x40  DANTE 1..64
/// 0x41..0x48  INPUT 1..8
/// 0xC1..0xD8  INPUT 9..32
/// ```
///
/// The split in the INPUT range is odd but deterministic — `0x40 + n` below
/// nine and `0xC0 + (n - 8)` above — and both halves are confirmed by a diff
/// (INPUT4 = `0x44`, INPUT12 = `0xC4`). It must not be "simplified" to
/// `0x40 + n`, which is right for eight channels and wrong for twenty-four.
///
/// The picker also offers `SLOT`, `FX`, `PB OUT` and `NONE` sources whose codes
/// have never been observed; those return `None` so the caller can say so.
fn decode_source(v: u8) -> Option<(&'static str, u16)> {
    match v {
        0x00 => None,
        0x01..=0x40 => Some(("dante", v as u16)),
        0x41..=0x48 => Some(("local", (v - 0x40) as u16)),
        0xC1..=0xD8 => Some(("local", (v - 0xC0) as u16 + 8)),
        _ => None,
    }
}

fn devices(product: &str) -> Vec<Device> {
    vec![
        Device {
            id: "local".into(),
            label: format!("{product} local inputs"),
            model: None,
            transport: Transport::Local,
            inputs: 32,
            outputs: 16,
        },
        Device {
            id: "dante".into(),
            label: "Dante".into(),
            model: None,
            transport: Transport::Dante,
            inputs: 64,
            outputs: 64,
        },
    ]
}

pub fn parse_clf(d: &[u8], name: &str) -> Result<Show, ClfError> {
    let product = product(d).ok_or(ClfError::NotClf)?;
    // "QL [OSX, 5.8.1.27]" -> model "QL", editor build for the version field.
    let model = product.split_whitespace().next().unwrap_or("CL/QL").to_string();

    let mut show = Show::default();
    show.meta.source_format = "yamaha-clf".into();
    show.meta.console = format!("Yamaha {model}");
    show.meta.format_version = Some(product.clone());
    show.meta.name = name
        .rsplit('/')
        .next()
        .unwrap_or(name)
        .trim_end_matches(".CLF")
        .trim_end_matches(".clf")
        .to_string();
    show.devices = devices(&model);

    let Some(table) = d.get(PATCH_TABLE..PATCH_TABLE + CHANNELS) else {
        show.diagnostics.push(Diagnostic {
            severity: Severity::Unknown,
            locus: format!("{PATCH_TABLE:#07x}"),
            message: "the file ends before the input patch table this format keeps at a fixed \
                      offset; no patch could be read"
                .into(),
        });
        return Ok(show);
    };

    // The offset is absolute and was established on one frame size and one
    // firmware. Check what it points at before trusting it: a table of real
    // patch values decodes almost entirely, and arbitrary bytes do not.
    let decoded = table.iter().filter(|&&v| decode_source(v).is_some()).count();
    if decoded * 4 < CHANNELS * 3 {
        show.diagnostics.push(Diagnostic {
            severity: Severity::Suspect,
            locus: format!("{PATCH_TABLE:#07x}"),
            message: format!(
                "only {decoded} of {CHANNELS} bytes at the expected patch-table offset decode to \
                 a known source, so this is probably not the table. The offset was established \
                 on a QL5 written by QL Editor V5.8.1 and is not derived from anything in the \
                 file, so another frame size or firmware may well move it. No patch is reported \
                 rather than a wrong one"
            ),
        });
        return Ok(show);
    }

    let mut unresolved = 0usize;
    for (i, &v) in table.iter().enumerate() {
        let ch = (i + 1) as u16;
        show.strips.push(Strip::new(StripId::new(StripKind::Input, ch)));

        let socket = decode_source(v).map(|(dev, idx)| SocketRef::new(dev, Direction::In, idx));
        if socket.is_none() && v != 0x00 {
            unresolved += 1;
        }
        show.patch.inputs.push(InputPatch {
            slot: ch,
            block_label: String::new(),
            socket,
            strip: Some(StripId::new(StripKind::Input, ch)),
        });
    }

    if unresolved > 0 {
        show.diagnostics.push(Diagnostic {
            severity: Severity::Unknown,
            locus: "input patch table".into(),
            message: format!(
                "{unresolved} channel(s) carry a source code outside the ranges confirmed by \
                 diff (Dante and the local inputs). CL/QL can also patch from a slot card, an \
                 effect return or the playback bus, and those codes have never been observed, \
                 so those connectors are left blank rather than guessed"
            ),
        });
    }

    show.diagnostics.push(Diagnostic {
        severity: Severity::Unmodelled,
        locus: name.to_string(),
        message: "channel names, head-amp gain and phantom, bus sends and processing are all in \
                  this file and none of them are decoded yet — the format carries no schema, so \
                  each has to be located by its own controlled diff. The file also carries a \
                  checksum, which does not matter for reading but would have to be solved before \
                  anything could be written back"
            .into(),
    });

    Ok(show)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A synthetic CLF with the given patch table.
    fn clf(product_str: &str, table: &[u8]) -> Vec<u8> {
        let mut d = vec![0u8; PATCH_TABLE + CHANNELS + 64];
        d[..8].copy_from_slice(&[0x01, 0, 0, 0, 0, 0, 0, 0x28]);
        d[PRODUCT_AT..PRODUCT_AT + product_str.len()].copy_from_slice(product_str.as_bytes());
        d[PATCH_TABLE..PATCH_TABLE + table.len()].copy_from_slice(table);
        d
    }

    /// The QL5 default: INPUT 1-32 then DANTE 1-32.
    fn default_table() -> Vec<u8> {
        let mut t = Vec::new();
        for n in 1..=32u8 {
            t.push(if n <= 8 { 0x40 + n } else { 0xC0 + (n - 8) });
        }
        t.extend(1..=32u8);
        t
    }

    #[test]
    fn decodes_the_real_default_table() {
        let show = parse_clf(&clf("QL [OSX, 5.8.1.27]", &default_table()), "show.CLF").unwrap();
        assert_eq!(show.meta.console, "Yamaha QL");
        assert_eq!(show.patch.inputs.len(), 64);

        // Channels 1-32 on the local inputs, in order.
        assert_eq!(
            show.patch.inputs[0].socket,
            Some(SocketRef::new("local", Direction::In, 1))
        );
        assert_eq!(
            show.patch.inputs[7].socket,
            Some(SocketRef::new("local", Direction::In, 8))
        );
        // The bank boundary: channel 9 is 0xC1, not 0x49.
        assert_eq!(
            show.patch.inputs[8].socket,
            Some(SocketRef::new("local", Direction::In, 9))
        );
        assert_eq!(
            show.patch.inputs[31].socket,
            Some(SocketRef::new("local", Direction::In, 32))
        );
        // Channels 33-64 on Dante.
        assert_eq!(
            show.patch.inputs[32].socket,
            Some(SocketRef::new("dante", Direction::In, 1))
        );
        assert_eq!(
            show.patch.inputs[63].socket,
            Some(SocketRef::new("dante", Direction::In, 32))
        );
    }

    #[test]
    fn the_input_bank_split_is_not_a_linear_formula() {
        // The whole point of the odd encoding: 0x40 + n is right for eight
        // channels and wrong for the next twenty-four.
        assert_eq!(decode_source(0x44), Some(("local", 4))); // INPUT4, from a diff
        assert_eq!(decode_source(0xC4), Some(("local", 12))); // INPUT12, from a diff
        assert_eq!(decode_source(0x4C), None); // what "0x40 + 12" would have been
    }

    #[test]
    fn dante_and_unpatched_decode_as_expected() {
        assert_eq!(decode_source(0x01), Some(("dante", 1)));
        assert_eq!(decode_source(0x40), Some(("dante", 64)));
        assert_eq!(decode_source(0x00), None);
    }

    #[test]
    fn reads_the_two_changes_the_diffs_were_built_on() {
        let mut t = default_table();
        t[3] = 0x01; // CH4  -> DANTE1
        t[11] = 0x02; // CH12 -> DANTE2
        let show = parse_clf(&clf("QL [OSX, 5.8.1.27]", &t), "x.CLF").unwrap();
        assert_eq!(
            show.patch.inputs[3].socket,
            Some(SocketRef::new("dante", Direction::In, 1))
        );
        assert_eq!(
            show.patch.inputs[11].socket,
            Some(SocketRef::new("dante", Direction::In, 2))
        );
    }

    #[test]
    fn refuses_to_report_a_patch_when_the_offset_points_at_nonsense() {
        // The offset is absolute and unverified across frame sizes, so the
        // adapter has to notice when it is wrong instead of printing garbage.
        let table = vec![0x7Fu8; CHANNELS];
        let show = parse_clf(&clf("QL [OSX, 5.8.1.27]", &table), "x.CLF").unwrap();
        assert!(show.patch.inputs.is_empty());
        assert!(show
            .diagnostics
            .iter()
            .any(|d| d.severity == Severity::Suspect
                && d.message.contains("probably not the table")));
    }

    #[test]
    fn a_mostly_valid_table_with_a_few_unknowns_is_still_read() {
        let mut t = default_table();
        t[0] = 0x90; // a source class never observed
        let show = parse_clf(&clf("QL [OSX, 5.8.1.27]", &t), "x.CLF").unwrap();
        assert_eq!(show.patch.inputs.len(), 64);
        assert_eq!(show.patch.inputs[0].socket, None);
        assert!(show
            .diagnostics
            .iter()
            .any(|d| d.message.contains("outside the ranges confirmed")));
    }

    #[test]
    fn accepts_a_cl_file_and_names_it() {
        let show = parse_clf(&clf("CL [OSX, 5.8.1.27]", &default_table()), "x.CLF").unwrap();
        assert_eq!(show.meta.console, "Yamaha CL");
    }

    #[test]
    fn rejects_other_formats() {
        assert_eq!(parse_clf(b"", "x").unwrap_err(), ClfError::NotClf);
        assert_eq!(parse_clf(&vec![0u8; 200_000], "x").unwrap_err(), ClfError::NotClf);
        // An MBDF container must not be claimed by this adapter.
        let mut mbdf = vec![0u8; 200];
        mbdf[..18].copy_from_slice(b"#YAMAHA MBDFScene\0");
        assert_eq!(parse_clf(&mbdf, "x").unwrap_err(), ClfError::NotClf);
    }

    #[test]
    fn a_truncated_file_reports_rather_than_panicking() {
        let full = clf("QL [OSX, 5.8.1.27]", &default_table());
        let show = parse_clf(&full[..PATCH_TABLE + 4], "x.CLF").unwrap();
        assert!(show.patch.inputs.is_empty());
        assert!(show.diagnostics.iter().any(|d| d.message.contains("ends before")));
        for cut in (0..full.len()).step_by(4096) {
            let _ = parse_clf(&full[..cut], "x");
        }
    }
}
