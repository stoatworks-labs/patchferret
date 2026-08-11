//! Allen & Heath SQ — `NVDATA.DAT` and `SCENE*.DAT`.
//!
//! No container: flat, fixed-size 128 KiB NVRAM images. An SQ show is a folder
//! of them, and MixPad keeps a live one at
//! `~/Library/Application Support/Allen & Heath/SQ-MixPad …/CurrentShow/`.
//!
//! # The patch is in NVDATA, not the scene
//!
//! Changing a patch point moves bytes in `NVDATA.DAT` and leaves `SCENE001.DAT`
//! byte-identical. The input patch is global setup, exactly as it is on Avantis,
//! where it lives only in the live-state scene. An adapter that reads "the
//! current scene" finds nothing.
//!
//! # Provenance
//!
//! Located by controlled diff through MixPad V1.6.0 offline as an SQ-7: moving
//! Ip3 from Local socket 3 to Local socket 10 changed exactly five bytes — one
//! patch byte and a four-byte checksum at the end of the file. See
//! `docs/allen-heath-sq.md` in the research repository.

use patchferret_model::*;

use crate::{AdapterError, Confidence, ShowAdapter, ShowInput};

/// Every SQ NVRAM image is exactly this size.
const IMAGE_LEN: usize = 131_072;

/// Bytes between one input channel's record and the next.
const CHANNEL_STRIDE: usize = 336;

/// The bytes either side of the patch field, used to find the record run.
///
/// Anchoring on this rather than on the absolute offset the diff produced means
/// the scan survives a file whose records sit somewhere else — and it fails
/// loudly rather than reading whatever happens to be at a hardcoded address.
///
/// The byte between `SIG_AFTER_0` and `SIG_AFTER_2` is **not** part of the
/// signature: it is a patched/unpatched flag. Treating it as fixed at `0x01`
/// silently dropped every unpatched channel — on a default SQ-7 that lost
/// Ip33–Ip40 entirely, and the patch list simply had eight fewer rows than the
/// console does with no indication anything was missing.
const SIG_BEFORE: [u8; 3] = [0xFF, 0xFF, 0xFF];
const SIG_AFTER_0: u8 = 0x00;
const SIG_AFTER_2: u8 = 0xFE;

/// Input channels on the frame this was established on.
///
/// The record array runs to 122 entries and covers far more than the input
/// channels — stereo inputs and mix objects share the same 336-byte stride, and
/// nothing in the file marks where one kind ends and the next begins. An SQ-7's
/// Setup → Strip Assign page lists **Ip1–Ip40**, then ST1–ST3 and USB, and the
/// value pattern agrees: records 0–39 index the local sockets, and record 40
/// onward jumps to socket 49+. So the boundary is taken from the console rather
/// than derived, and the adapter says so.
const INPUT_CHANNELS: usize = 40;

pub struct SqAdapter;

impl ShowAdapter for SqAdapter {
    fn id(&self) -> &'static str {
        "allen-heath-sq"
    }

    fn display_name(&self) -> &'static str {
        "Allen & Heath SQ show (NVDATA.DAT)"
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["dat"]
    }

    fn sniff(&self, input: &ShowInput) -> Confidence {
        if nvdata(input).is_some() {
            Confidence::Strong
        } else {
            Confidence::No
        }
    }

    fn parse(&self, input: &ShowInput) -> Result<Show, AdapterError> {
        let data = nvdata(input).ok_or(AdapterError::Unrecognised)?;
        parse_nvdata(&data, &input.name).map_err(|e| AdapterError::Parse {
            adapter: "allen-heath-sq",
            message: e.to_string(),
        })
    }
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum SqError {
    #[error("no input channel records found — this does not look like SQ NVDATA")]
    NoRecords,
}

/// Does this look like an SQ NVRAM image?
///
/// Byte 0 is a type tag — `0xB5` on NVDATA and `0xA1` on a scene — so it is
/// deliberately not part of the check; everything from byte 1 is shared.
fn looks_like_image(d: &[u8]) -> bool {
    d.len() == IMAGE_LEN
        && d[1] == 0x00
        && d[2] == 0xFE
        && d[3..12].iter().all(|&b| b == 0xFF)
        && d[0x0C..0x10] == [0x01, 0x06, 0x00, 0x01]
}

/// The NVDATA image out of whatever was handed to us.
///
/// An SQ show is a folder, so prefer the file actually called `NVDATA.DAT`;
/// fall back to any image that carries the NVDATA type tag, which is what makes
/// dropping the single file work too.
fn nvdata(input: &ShowInput) -> Option<Vec<u8>> {
    if let Some(f) = input.find("NVDATA.DAT") {
        if looks_like_image(&f.bytes) {
            return Some(f.bytes.clone());
        }
    }
    input
        .files
        .iter()
        .find(|f| looks_like_image(&f.bytes) && f.bytes[0] == 0xB5)
        .map(|f| f.bytes.clone())
}

/// Offsets of every input channel's patch byte, plus how many other candidate
/// runs were seen.
///
/// Finds candidates by signature, then keeps the longest run spaced exactly one
/// record apart. A real image has several such runs — a default SQ-7 has one of
/// 32 and four short ones carrying other tables (stereo inputs and the like) —
/// so taking the longest is what separates the input channels from its
/// neighbours. The count of the others is returned so the caller can say the
/// file held more than it read.
fn patch_offsets(d: &[u8]) -> (Vec<usize>, usize) {
    let mut hits = Vec::new();
    for i in 3..d.len().saturating_sub(4) {
        if d[i - 3..i] == SIG_BEFORE && d[i + 1] == SIG_AFTER_0 && d[i + 3] == SIG_AFTER_2 {
            hits.push(i);
        }
    }

    let mut best: Vec<usize> = Vec::new();
    let mut run: Vec<usize> = Vec::new();
    for &h in &hits {
        match run.last() {
            Some(&prev) if h - prev == CHANNEL_STRIDE => run.push(h),
            _ => {
                if run.len() > best.len() {
                    best = std::mem::take(&mut run);
                } else {
                    run.clear();
                }
                run.push(h);
            }
        }
    }
    if run.len() > best.len() {
        best = run;
    }
    // Everything else that formed a run of its own.
    let mut others = 0usize;
    let mut run: Vec<usize> = Vec::new();
    for &h in &hits {
        match run.last() {
            Some(&prev) if h - prev == CHANNEL_STRIDE => run.push(h),
            _ => {
                if run.len() > 1 && run.first() != best.first() {
                    others += 1;
                }
                run.clear();
                run.push(h);
            }
        }
    }
    if run.len() > 1 && run.first() != best.first() {
        others += 1;
    }

    // Only the leading records are input channels; the rest of the run is other
    // object kinds at the same stride.
    let over = best.len().saturating_sub(INPUT_CHANNELS);
    best.truncate(INPUT_CHANNELS);
    (best, others + usize::from(over > 0))
}

fn devices() -> Vec<Device> {
    vec![Device {
        id: "input".into(),
        // Deliberately vague, because the class genuinely is not known — see the
        // diagnostic in `parse_nvdata`. Naming it "Local" would read as a fact.
        label: "Input socket (class not decoded)".into(),
        model: None,
        transport: Transport::Other("sq-input".into()),
        inputs: 48,
        outputs: 0,
    }]
}

pub fn parse_nvdata(d: &[u8], name: &str) -> Result<Show, SqError> {
    let (offsets, other_runs) = patch_offsets(d);
    if offsets.is_empty() {
        return Err(SqError::NoRecords);
    }

    let mut show = Show::default();
    show.meta.source_format = "allen-heath-sq".into();
    show.meta.console = "Allen & Heath SQ".into();
    show.meta.name = name
        .rsplit('/')
        .next()
        .unwrap_or(name)
        .trim_end_matches(".DAT")
        .trim_end_matches(".dat")
        .to_string();
    show.devices = devices();

    let mut unpatched = 0usize;
    for (i, &off) in offsets.iter().enumerate() {
        let ch = (i + 1) as u16;
        show.strips.push(Strip::new(StripId::new(StripKind::Input, ch)));

        // The byte two past the patch field says whether the channel is patched
        // at all. An unpatched channel still gets a row — a patch list that
        // silently omits it is how you discover on site that a channel has no
        // source.
        let patched = d.get(off + 2).copied() == Some(0x01);
        if !patched {
            unpatched += 1;
        }
        show.patch.inputs.push(InputPatch {
            slot: ch,
            block_label: String::new(),
            // The field is a 0-based socket index; connectors are numbered from 1.
            socket: patched.then(|| SocketRef::new("input", Direction::In, d[off] as u16 + 1)),
            strip: Some(StripId::new(StripKind::Input, ch)),
        });
    }

    // The one thing a reader must not take on trust.
    show.diagnostics.push(Diagnostic {
        severity: Severity::Unknown,
        locus: "NVDATA input channel records".into(),
        message: "the patch field gives a socket NUMBER but not which socket block it belongs \
                  to — an SQ can take inputs from Local, SLink, USB or an I/O port, and only a \
                  Local patch has been observed. Connector numbers below are right; whether they \
                  are Local sockets is an assumption this tool has not verified"
            .into(),
    });

    show.diagnostics.push(Diagnostic {
        severity: Severity::Suspect,
        locus: "NVDATA.DAT".into(),
        message: format!(
            "{} input channel records were read ({unpatched} of them unpatched), taken from the \
             start of the longest run at the {CHANNEL_STRIDE}-byte record stride{}. The run \
             itself is longer and continues into stereo inputs and mix objects, which share the \
             stride; nothing in the file marks the boundary, so the input-channel count is taken \
             from an SQ-7's Setup page rather than derived. A frame with a different count will \
             read wrong here",
            offsets.len(),
            if other_runs > 0 {
                format!(
                    ", with {other_runs} shorter run(s) elsewhere left alone as other tables"
                )
            } else {
                String::new()
            }
        ),
    });

    show.diagnostics.push(Diagnostic {
        severity: Severity::Unmodelled,
        locus: "NVDATA.DAT".into(),
        message:
            "PFX carries the input patch only; channel names, preamp gain and phantom, bus \
                  sends and processing are all inside the same record and none are decoded yet"
                .into(),
    });

    Ok(show)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build an NVRAM image whose channels carry `(socket, patched)`.
    fn image_flagged(tag: u8, patch: &[(u8, bool)]) -> Vec<u8> {
        let mut d = vec![0u8; IMAGE_LEN];
        d[0] = tag;
        d[1] = 0x00;
        d[2] = 0xFE;
        for b in d[3..12].iter_mut() {
            *b = 0xFF;
        }
        d[0x0C..0x10].copy_from_slice(&[0x01, 0x06, 0x00, 0x01]);
        let base = 0x38C;
        for (i, &(v, on)) in patch.iter().enumerate() {
            let at = base + i * CHANNEL_STRIDE;
            d[at - 3..at].copy_from_slice(&SIG_BEFORE);
            d[at] = v;
            d[at + 1] = SIG_AFTER_0;
            d[at + 2] = if on { 0x01 } else { 0x00 };
            d[at + 3] = SIG_AFTER_2;
        }
        d
    }

    /// Build an NVRAM image with `n` channel records carrying `patch[i]`.
    fn image(tag: u8, patch: &[u8]) -> Vec<u8> {
        let mut d = vec![0u8; IMAGE_LEN];
        d[0] = tag;
        d[1] = 0x00;
        d[2] = 0xFE;
        for b in d[3..12].iter_mut() {
            *b = 0xFF;
        }
        d[0x0C..0x10].copy_from_slice(&[0x01, 0x06, 0x00, 0x01]);

        let base = 0x38C;
        for (i, &v) in patch.iter().enumerate() {
            let at = base + i * CHANNEL_STRIDE;
            d[at - 3..at].copy_from_slice(&SIG_BEFORE);
            d[at] = v;
            d[at + 1] = SIG_AFTER_0;
            d[at + 2] = 0x01;
            d[at + 3] = SIG_AFTER_2;
        }
        d
    }

    fn show_of(patch: &[u8]) -> Show {
        parse_nvdata(&image(0xB5, patch), "NVDATA.DAT").expect("parse")
    }

    #[test]
    fn reads_a_default_one_to_one_patch() {
        let show = show_of(&[0, 1, 2, 3, 4, 5, 6, 7]);
        assert_eq!(show.patch.inputs.len(), 8);
        // 0-based in the file, 1-based on the connector.
        assert_eq!(
            show.patch.inputs[0].socket,
            Some(SocketRef::new("input", Direction::In, 1))
        );
        assert_eq!(
            show.patch.inputs[7].socket,
            Some(SocketRef::new("input", Direction::In, 8))
        );
        assert_eq!(show.strips_of(StripKind::Input).count(), 8);
    }

    #[test]
    fn reads_the_patch_change_the_diff_was_built_on() {
        // Ip3 moved from socket 3 to socket 10 — file value 0x02 becomes 0x09.
        let show = show_of(&[0, 1, 9, 3]);
        assert_eq!(
            show.patch.inputs[2].socket,
            Some(SocketRef::new("input", Direction::In, 10))
        );
        assert_eq!(show.patch.inputs[2].strip, Some(StripId::new(StripKind::Input, 3)));
    }

    #[test]
    fn an_unpatched_channel_still_gets_a_row() {
        // Regression: the byte after the patch is a patched/unpatched flag, and
        // treating it as fixed at 0x01 dropped every unpatched channel. On a
        // default SQ-7 that lost Ip33-Ip40 with nothing to show anything was
        // missing.
        let show = parse_nvdata(
            &image_flagged(0xB5, &[(0, true), (1, true), (0, false), (3, true)]),
            "NVDATA.DAT",
        )
        .unwrap();

        assert_eq!(show.patch.inputs.len(), 4, "an unpatched channel was dropped");
        assert_eq!(show.patch.inputs[2].socket, None, "unpatched channel invented a socket");
        // ...and it still reaches its strip, so the row appears in the report.
        assert_eq!(show.patch.inputs[2].strip, Some(StripId::new(StripKind::Input, 3)));
        assert_eq!(
            show.patch.inputs[3].socket,
            Some(SocketRef::new("input", Direction::In, 4))
        );
        assert!(show.diagnostics.iter().any(|d| d.message.contains("1 of them unpatched")));
    }

    #[test]
    fn stops_at_the_input_channels_rather_than_running_into_other_objects() {
        // The record array continues past the input channels into stereo inputs
        // and mix objects at the same stride, with nothing marking the boundary.
        let many: Vec<(u8, bool)> = (0..80u8).map(|i| (i, true)).collect();
        let show = parse_nvdata(&image_flagged(0xB5, &many), "NVDATA.DAT").unwrap();
        assert_eq!(show.patch.inputs.len(), INPUT_CHANNELS);
        assert!(show.diagnostics.iter().any(|d| d.message.contains("taken from an SQ-7")));
    }

    #[test]
    fn says_it_does_not_know_the_socket_class() {
        // The most misleading thing this adapter could do is present a bare
        // number as a Local socket, so the diagnostic is load-bearing.
        let show = show_of(&[0, 1, 2]);
        assert!(show.diagnostics.iter().any(|d| d.message.contains("not which socket block")));
    }

    #[test]
    fn ignores_an_isolated_signature_match() {
        // A lone match in 128 KiB of NVRAM is noise; only a run at the record
        // stride is a channel table.
        let mut d = image(0xB5, &[0, 1, 2, 3]);
        let stray = 0x1F000;
        d[stray - 3..stray].copy_from_slice(&SIG_BEFORE);
        d[stray] = 0x7F;
        d[stray + 1] = SIG_AFTER_0;
        d[stray + 2] = 0x01;
        d[stray + 3] = SIG_AFTER_2;

        let show = parse_nvdata(&d, "NVDATA.DAT").unwrap();
        assert_eq!(show.patch.inputs.len(), 4, "the stray match was counted as a channel");
    }

    #[test]
    fn rejects_anything_that_is_not_an_sq_image() {
        assert!(!looks_like_image(&[]));
        assert!(!looks_like_image(&vec![0u8; IMAGE_LEN]));
        // Right size and shape but truncated by one byte.
        let mut d = image(0xB5, &[0]);
        d.pop();
        assert!(!looks_like_image(&d));
    }

    #[test]
    fn a_scene_image_alone_is_not_a_source_of_patch() {
        // Scenes carry the same header shape but tag 0xA1 and no patch; the
        // adapter must not pick one up as if it were NVDATA.
        let scene = image(0xA1, &[]);
        let input = ShowInput::single("SCENE001.DAT", scene);
        assert_eq!(SqAdapter.sniff(&input), Confidence::No);
    }

    #[test]
    fn finds_nvdata_inside_a_show_folder() {
        let files = vec![
            crate::ShowFile::new("SCENE001.DAT", image(0xA1, &[])),
            crate::ShowFile::new("NVDATA.DAT", image(0xB5, &[0, 1, 2])),
        ];
        let input = ShowInput::bundle("MyShow", files);
        assert_eq!(SqAdapter.sniff(&input), Confidence::Strong);
        let show = SqAdapter.parse(&input).unwrap();
        assert_eq!(show.patch.inputs.len(), 3);
    }

    #[test]
    fn arbitrary_bytes_do_not_panic() {
        for len in [0usize, 1, 1024, IMAGE_LEN] {
            let _ = parse_nvdata(&vec![0xABu8; len], "x");
        }
    }
}
