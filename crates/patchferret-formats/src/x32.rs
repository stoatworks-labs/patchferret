//! Behringer X32 / Midas M32 / X-Air `.scn` scene files.
//!
//! The format is line-oriented ASCII: an OSC-like path followed by
//! whitespace-separated tokens, with quoted strings for names.
//!
//! ```text
//! #2.7# "General 1.2.2" "" %000000000 1
//! /config/routing/IN A1-8 A9-16 A17-24 B1-8 AUX1-4
//! /ch/01/config "Diazno" 1 CY 1
//! /headamp/000 +0.0 OFF
//! /outputs/main/01 4 PRE OFF
//! ```
//!
//! # The patch chain
//!
//! Getting from an XLR to a fader takes three hops on this console, and all
//! three have to be composed or the patch list is wrong:
//!
//! 1. `/config/routing/IN` maps blocks of 8 **physical connectors** onto the
//!    32 **input slots**. A block named `A1-8` means slots 1–8 are fed by
//!    AES50-A connectors 1–8.
//! 2. `/ch/NN/config`'s last token selects which **input slot** feeds channel
//!    `NN`. This is a free mapping — channel 7 may well take slot 22.
//! 3. `/headamp/NNN` holds the gain for a **connector**, indexed in a flat
//!    space across local and both AES50 links.
//!
//! Skipping step 1 and assuming channel N is fed by XLR N is the single most
//! common way to produce a confident, wrong patch list.
//!
//! # Provenance of the enumerations
//!
//! The signal-source numbering below is derived from community documentation
//! and corroborated against real scene files, *not* from a running console.
//! Two internal checks support it: no value in the corroborating file decodes
//! to a matrix above 6 (the X32 has exactly 6), and `/outputs/p16/01 26 <-EQ`
//! decodes to "direct out of channel 1", which is what an Ultranet port 1 is
//! conventionally fed with. Anything outside the mapped ranges is reported as
//! a [`Diagnostic`] rather than guessed.

use patchferret_model::*;

use crate::{AdapterError, Confidence, ShowAdapter, ShowInput};

pub struct X32Adapter;

impl ShowAdapter for X32Adapter {
    fn id(&self) -> &'static str {
        "x32"
    }

    fn display_name(&self) -> &'static str {
        "Behringer X32 / Midas M32 scene"
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["scn"]
    }

    fn sniff(&self, input: &ShowInput) -> Confidence {
        let Some(file) = input.primary() else {
            return Confidence::No;
        };
        // Only look at the head — these files reach hundreds of kilobytes.
        let head: String = String::from_utf8_lossy(&file.bytes[..file.bytes.len().min(4096)])
            .chars()
            .take(2048)
            .collect();

        let has_header = head.starts_with('#');
        let has_paths = head.lines().filter(|l| l.starts_with('/')).count() >= 3;
        let x32_paths = head.contains("/config/") || head.contains("/ch/");

        match (has_header && has_paths && x32_paths, file.extension() == "scn") {
            (true, _) => Confidence::Strong,
            (false, true) if has_paths => Confidence::Weak,
            _ => Confidence::No,
        }
    }

    fn parse(&self, input: &ShowInput) -> Result<Show, AdapterError> {
        let file = input.primary().ok_or(AdapterError::Unrecognised)?;
        parse_scn(&file.text(), &input.name)
    }
}

/// Split a scene line into its path and tokens, honouring quoted strings.
fn tokenise(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_quotes = false;
    let mut has_token = false;

    for c in line.chars() {
        match c {
            '"' => {
                in_quotes = !in_quotes;
                has_token = true;
            }
            c if c.is_whitespace() && !in_quotes => {
                if has_token {
                    out.push(std::mem::take(&mut cur));
                    has_token = false;
                }
            }
            c => {
                cur.push(c);
                has_token = true;
            }
        }
    }
    if has_token {
        out.push(cur);
    }
    out
}

/// A physical input group named by a routing block, e.g. `A1-8`.
struct BlockTarget {
    device: &'static str,
    /// 1-based connector index of the first socket in the block.
    start: u16,
}

/// Resolve an input routing block name to the connectors it represents.
///
/// Names are `<prefix><first>-<last>`: `IN1-8` local, `A`/`B` the AES50 links,
/// `CARD` the expansion slot, `AUX` the rear auxiliary inputs.
fn resolve_input_block(token: &str) -> Option<BlockTarget> {
    let split = token.find(|c: char| c.is_ascii_digit())?;
    let (prefix, range) = token.split_at(split);
    let first: u16 = range.split('-').next()?.parse().ok()?;

    let device = match prefix.to_ascii_uppercase().as_str() {
        "IN" => "local",
        "A" => "aes50a",
        "B" => "aes50b",
        "CARD" => "card",
        "AUX" => "aux",
        _ => return None,
    };
    Some(BlockTarget { device, start: first })
}

/// Decode a strip's source token from the console's input-signal enumeration.
fn decode_input_signal(n: u16) -> SignalRef {
    match n {
        0 => SignalRef::Off,
        1..=32 => SignalRef::InputSlot(n),
        33..=40 => SignalRef::Named(format!("Aux In {}", n - 32)),
        41..=48 => SignalRef::Named(format!("FX Return {}", n - 40)),
        _ => SignalRef::Named(format!("source {n}")),
    }
}

/// Decode an output's source token from the console's output-signal
/// enumeration. Returns the reference plus a human label for the report.
fn decode_output_signal(n: u16) -> (SignalRef, String) {
    match n {
        0 => (SignalRef::Off, "—".into()),
        1 => (SignalRef::Strip(StripId::new(StripKind::Main, 1)), "Main L".into()),
        2 => (SignalRef::Strip(StripId::new(StripKind::Main, 1)), "Main R".into()),
        3 => (SignalRef::Strip(StripId::new(StripKind::Mono, 1)), "Main M/C".into()),
        4..=19 => {
            let b = n - 3;
            (SignalRef::Strip(StripId::new(StripKind::Bus, b)), format!("Bus {b}"))
        }
        20..=25 => {
            let m = n - 19;
            (SignalRef::Strip(StripId::new(StripKind::Matrix, m)), format!("Matrix {m}"))
        }
        26..=57 => {
            let c = n - 25;
            (SignalRef::Strip(StripId::new(StripKind::Input, c)), format!("Ch {c} direct"))
        }
        _ => (SignalRef::Named(format!("source {n}")), format!("source {n}")),
    }
}

fn decode_tap(token: &str) -> Tap {
    match token {
        "IN" | "IN/LC" => Tap::PreEq,
        "<-EQ" => Tap::PreEq,
        "EQ->" => Tap::PostEq,
        "PRE" => Tap::PreFader,
        "POST" => Tap::PostFader,
        _ => Tap::Unknown,
    }
}

/// Map a flat head-amp index onto the connector it controls.
///
/// 0–31 are the local XLRs, 32–79 AES50-A, 80–127 AES50-B.
fn headamp_socket(idx: u16) -> Option<SocketRef> {
    match idx {
        0..=31 => Some(SocketRef::new("local", Direction::In, idx + 1)),
        32..=79 => Some(SocketRef::new("aes50a", Direction::In, idx - 31)),
        80..=127 => Some(SocketRef::new("aes50b", Direction::In, idx - 79)),
        _ => None,
    }
}

fn devices() -> Vec<Device> {
    vec![
        Device {
            id: "local".into(),
            label: "Console local I/O".into(),
            model: None,
            transport: Transport::Local,
            inputs: 32,
            outputs: 16,
        },
        Device {
            id: "aes50a".into(),
            label: "AES50 A".into(),
            model: None,
            transport: Transport::Aes50A,
            inputs: 48,
            outputs: 48,
        },
        Device {
            id: "aes50b".into(),
            label: "AES50 B".into(),
            model: None,
            transport: Transport::Aes50B,
            inputs: 48,
            outputs: 48,
        },
        Device {
            id: "card".into(),
            label: "Expansion card".into(),
            model: None,
            transport: Transport::Card("slot".into()),
            inputs: 32,
            outputs: 32,
        },
        Device {
            id: "aux".into(),
            label: "Aux in/out (rear)".into(),
            model: None,
            transport: Transport::Local,
            inputs: 6,
            outputs: 6,
        },
        Device {
            id: "p16".into(),
            label: "Ultranet P16".into(),
            model: None,
            transport: Transport::Ultranet,
            inputs: 0,
            outputs: 16,
        },
        Device {
            id: "aes".into(),
            label: "AES/EBU".into(),
            model: None,
            transport: Transport::Other("aes3".into()),
            inputs: 2,
            outputs: 2,
        },
        Device {
            id: "rec".into(),
            label: "USB recorder".into(),
            model: None,
            transport: Transport::Recorder,
            inputs: 2,
            outputs: 2,
        },
    ]
}

/// Strip kinds keyed by their scene-file path segment, with how many exist.
const STRIP_SECTIONS: &[(&str, StripKind, u16)] = &[
    ("ch", StripKind::Input, 32),
    ("auxin", StripKind::AuxIn, 8),
    ("fxrtn", StripKind::FxReturn, 8),
    ("bus", StripKind::Bus, 16),
    ("mtx", StripKind::Matrix, 6),
    ("dca", StripKind::Dca, 8),
];

/// Output sections keyed by path segment, with the device they land on.
const OUTPUT_SECTIONS: &[(&str, &str)] =
    &[("main", "local"), ("aux", "aux"), ("p16", "p16"), ("aes", "aes"), ("rec", "rec")];

pub fn parse_scn(text: &str, name: &str) -> Result<Show, AdapterError> {
    let mut show = Show::default();
    show.meta.source_format = "x32".into();
    show.meta.console = "Behringer X32 / Midas M32".into();
    show.devices = devices();

    let mut input_blocks: Vec<String> = Vec::new();
    // Counts lines whose path this adapter understood, so that "not a scene
    // file" can be distinguished from "a scene file with nothing patched".
    let mut recognised = 0usize;

    for (lineno, raw) in text.lines().enumerate() {
        let line = raw.trim_end();
        if line.is_empty() {
            continue;
        }

        // Header: #2.7# "Scene name" "note" %bitmap n
        if let Some(rest) = line.strip_prefix('#') {
            let toks = tokenise(rest);
            if let Some((ver, tail)) = toks.split_first() {
                show.meta.format_version = Some(ver.trim_matches('#').to_string());
                show.meta.name = tail
                    .first()
                    .cloned()
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| name.trim_end_matches(".scn").to_string());
                show.meta.note = tail.get(1).cloned().filter(|s| !s.is_empty());
            }
            continue;
        }

        if !line.starts_with('/') {
            continue;
        }

        let toks = tokenise(line);
        let Some(path) = toks.first() else { continue };
        let args = &toks[1..];
        let segs: Vec<&str> = path.trim_start_matches('/').split('/').collect();

        recognised += 1;
        match segs.as_slice() {
            // --- input routing blocks ---
            ["config", "routing", "IN"] => {
                input_blocks = args.to_vec();
            }

            // --- output routing blocks on the serial links ---
            ["config", "routing", link @ ("AES50A" | "AES50B" | "CARD" | "OUT")] => {
                show.diagnostics.push(Diagnostic {
                    severity: Severity::Unmodelled,
                    locus: format!("line {}: {path}", lineno + 1),
                    message: format!(
                        "{link} output routing blocks [{}] are recorded but not expanded into \
                         per-connector rows; the output patch below covers the console's own \
                         output definitions only",
                        args.join(" ")
                    ),
                });
            }

            // --- main L/R and mono ---
            // These must precede the generic `[section, idx, "config"]` arm
            // below, which would otherwise match them as a strip section named
            // "main" with index "st" and silently drop both.
            ["main", "st", "config"] => {
                let mut strip = Strip::new(StripId::new(StripKind::Main, 1));
                strip.name = args
                    .first()
                    .cloned()
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| "Main LR".into());
                strip.colour = args.get(1).cloned();
                show.strips.push(strip);
            }
            ["main", "m", "config"] => {
                let mut strip = Strip::new(StripId::new(StripKind::Mono, 1));
                strip.name = args
                    .first()
                    .cloned()
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| "Main M/C".into());
                strip.colour = args.get(1).cloned();
                show.strips.push(strip);
            }

            // --- strips ---
            [section, idx, "config"] => {
                let Some((_, kind, count)) =
                    STRIP_SECTIONS.iter().find(|(s, _, _)| s == section)
                else {
                    continue;
                };
                let Ok(index) = idx.parse::<u16>() else { continue };
                if index == 0 || index > *count {
                    show.diagnostics.push(Diagnostic {
                        severity: Severity::Suspect,
                        locus: format!("line {}: {path}", lineno + 1),
                        message: format!(
                            "{section} {index} is outside the X32's range of {count}"
                        ),
                    });
                    continue;
                }

                let mut strip = Strip::new(StripId::new(*kind, index));
                strip.name = args.first().cloned().unwrap_or_default();
                strip.colour = args.get(1).cloned();
                strip.icon = args.get(2).cloned();
                // Only input-bearing strips carry a source token.
                if matches!(kind, StripKind::Input | StripKind::AuxIn) {
                    if let Some(src) = args.get(3).and_then(|t| t.parse::<u16>().ok()) {
                        strip.source = decode_input_signal(src);
                    }
                }
                show.strips.push(strip);
            }

            // --- fader / mute ---
            [section, idx, "mix"] => {
                let Some((_, kind, _)) = STRIP_SECTIONS.iter().find(|(s, _, _)| s == section)
                else {
                    continue;
                };
                let Ok(index) = idx.parse::<u16>() else { continue };
                let id = StripId::new(*kind, index);
                if let Some(strip) = show.strips.iter_mut().find(|s| s.id == id) {
                    strip.muted = args.first().map(|t| t == "OFF").unwrap_or(false);
                    strip.fader_db = args.get(1).and_then(|t| t.parse::<f32>().ok());
                }
            }

            // --- head amps ---
            ["headamp", idx] => {
                let Ok(n) = idx.parse::<u16>() else { continue };
                let Some(socket) = headamp_socket(n) else {
                    show.diagnostics.push(Diagnostic {
                        severity: Severity::Suspect,
                        locus: format!("line {}: {path}", lineno + 1),
                        message: format!("head amp {n} is outside the 0–127 range"),
                    });
                    continue;
                };
                show.head_amps.push(HeadAmp {
                    socket,
                    gain_db: args.first().and_then(|t| t.parse::<f32>().ok()),
                    phantom: args.get(1).map(|t| t == "ON").unwrap_or(false),
                    pad: false,
                    polarity_invert: false,
                });
            }

            // --- outputs ---
            [_out @ "outputs", section, idx] => {
                let Some((_, device)) = OUTPUT_SECTIONS.iter().find(|(s, _)| s == section)
                else {
                    continue;
                };
                let Ok(index) = idx.parse::<u16>() else { continue };
                let Some(src) = args.first().and_then(|t| t.parse::<u16>().ok()) else {
                    continue;
                };
                let (source, label) = decode_output_signal(src);
                if matches!(source, SignalRef::Named(_)) {
                    show.diagnostics.push(Diagnostic {
                        severity: Severity::Unknown,
                        locus: format!("line {}: {path}", lineno + 1),
                        message: format!(
                            "output source {src} is outside the decoded range 0–57 and was kept \
                             as an opaque label"
                        ),
                    });
                }
                show.patch.outputs.push(OutputPatch {
                    socket: SocketRef::new(*device, Direction::Out, index),
                    source,
                    tap: args.get(1).map(|t| decode_tap(t)).unwrap_or(Tap::Unknown),
                    source_label: label,
                });
            }

            _ => recognised -= 1,
        }
    }

    build_input_patch(&mut show, &input_blocks);
    resolve_head_amp_sockets(&mut show);

    if recognised == 0 {
        return Err(AdapterError::Parse {
            adapter: "x32",
            message: "no recognised scene paths found — this does not look like a scene file"
                .into(),
        });
    }

    show.strips.sort_by_key(|s| s.id);
    show.patch.outputs.sort_by(|a, b| a.socket.cmp(&b.socket));
    Ok(show)
}

/// Compose routing blocks and channel sources into the input patch.
fn build_input_patch(show: &mut Show, blocks: &[String]) {
    if blocks.is_empty() {
        show.diagnostics.push(Diagnostic {
            severity: Severity::Unknown,
            locus: "/config/routing/IN".into(),
            message:
                "no input routing found; the connector column of the patch list is unknown"
                    .into(),
        });
        return;
    }

    // Blocks 1–4 cover the 32 input slots, eight at a time. Any further block
    // addresses the rear aux inputs, which sit outside the slot space.
    for (bi, block) in blocks.iter().enumerate().take(4) {
        let target = resolve_input_block(block);
        if target.is_none() {
            show.diagnostics.push(Diagnostic {
                severity: Severity::Unknown,
                locus: "/config/routing/IN".into(),
                message: format!("unrecognised input routing block '{block}'"),
            });
        }
        for offset in 0..8u16 {
            let slot = (bi as u16) * 8 + offset + 1;
            let socket = target
                .as_ref()
                .map(|t| SocketRef::new(t.device, Direction::In, t.start + offset));
            let strip = show
                .strips
                .iter()
                .find(|s| s.source == SignalRef::InputSlot(slot))
                .map(|s| s.id);
            show.patch.inputs.push(InputPatch {
                slot,
                block_label: block.clone(),
                socket,
                strip,
            });
        }
    }

    if let Some(aux_block) = blocks.get(4) {
        show.diagnostics.push(Diagnostic {
            severity: Severity::Unmodelled,
            locus: "/config/routing/IN".into(),
            message: format!(
                "aux input routing block '{aux_block}' feeds the rear aux inputs, which sit \
                 outside the 32 input slots and are not listed in the input patch"
            ),
        });
    }
}

/// Drop head amps for connectors that nothing is patched from.
///
/// The console stores all 128 regardless of use; listing every one would bury
/// the eight that matter. Only amps on a connector the show actually patches
/// are kept.
fn resolve_head_amp_sockets(show: &mut Show) {
    let used: Vec<SocketRef> =
        show.patch.inputs.iter().filter_map(|p| p.socket.clone()).collect();
    show.head_amps.retain(|h| used.contains(&h.socket));
    show.head_amps.sort_by(|a, b| a.socket.cmp(&b.socket));
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEADER: &str = "#2.7# \"Test Show\" \"\" %000000000 1";

    fn parse(body: &str) -> Show {
        parse_scn(&format!("{HEADER}\n{body}"), "test.scn").expect("parse")
    }

    #[test]
    fn tokenises_quoted_names_with_spaces() {
        let t = tokenise(r#"/ch/01/config "Lead Vox L" 1 CY 5"#);
        assert_eq!(t, vec!["/ch/01/config", "Lead Vox L", "1", "CY", "5"]);
    }

    #[test]
    fn tokenises_empty_quoted_string_as_a_token() {
        let t = tokenise(r#"/ch/03/config "" 1 WHi 3"#);
        assert_eq!(t, vec!["/ch/03/config", "", "1", "WHi", "3"]);
    }

    #[test]
    fn reads_header_name_and_version() {
        let show = parse("/ch/01/config \"Kick\" 1 CY 1");
        assert_eq!(show.meta.name, "Test Show");
        assert_eq!(show.meta.format_version.as_deref(), Some("2.7"));
    }

    #[test]
    fn resolves_routing_block_names() {
        let a = resolve_input_block("A9-16").unwrap();
        assert_eq!((a.device, a.start), ("aes50a", 9));
        let l = resolve_input_block("IN1-8").unwrap();
        assert_eq!((l.device, l.start), ("local", 1));
        let c = resolve_input_block("CARD25-32").unwrap();
        assert_eq!((c.device, c.start), ("card", 25));
        assert!(resolve_input_block("NONSENSE").is_none());
    }

    #[test]
    fn composes_connector_to_channel_through_the_slot_indirection() {
        // Slots 1-8 come from AES50-A 1-8. Channel 1 takes slot 5.
        let show = parse(
            "/config/routing/IN A1-8 A9-16 A17-24 B1-8 AUX1-4\n\
             /ch/01/config \"Kick\" 1 CY 5",
        );
        let row = show.patch.inputs.iter().find(|p| p.slot == 5).unwrap();
        assert_eq!(row.socket, Some(SocketRef::new("aes50a", Direction::In, 5)));
        assert_eq!(row.strip, Some(StripId::new(StripKind::Input, 1)));

        // The naive reading — channel 1 is fed by connector 1 — must not hold.
        let slot1 = show.patch.inputs.iter().find(|p| p.slot == 1).unwrap();
        assert_eq!(slot1.strip, None, "slot 1 should feed nothing in this show");
    }

    #[test]
    fn second_routing_block_offsets_connectors_correctly() {
        let show = parse(
            "/config/routing/IN A1-8 B17-24 A17-24 B1-8 AUX1-4\n\
             /ch/01/config \"X\" 1 CY 9",
        );
        // Slot 9 is the first of block 2, which starts at AES50-B connector 17.
        let row = show.patch.inputs.iter().find(|p| p.slot == 9).unwrap();
        assert_eq!(row.socket, Some(SocketRef::new("aes50b", Direction::In, 17)));
    }

    #[test]
    fn head_amp_indices_split_across_local_and_both_links() {
        assert_eq!(headamp_socket(0), Some(SocketRef::new("local", Direction::In, 1)));
        assert_eq!(headamp_socket(31), Some(SocketRef::new("local", Direction::In, 32)));
        assert_eq!(headamp_socket(32), Some(SocketRef::new("aes50a", Direction::In, 1)));
        assert_eq!(headamp_socket(79), Some(SocketRef::new("aes50a", Direction::In, 48)));
        assert_eq!(headamp_socket(80), Some(SocketRef::new("aes50b", Direction::In, 1)));
        assert_eq!(headamp_socket(127), Some(SocketRef::new("aes50b", Direction::In, 48)));
        assert_eq!(headamp_socket(128), None);
    }

    #[test]
    fn output_enumeration_never_exceeds_the_consoles_matrix_count() {
        // The property that validates the whole enum: 20..=25 are the six
        // matrices, so nothing in range may decode to matrix 7.
        for n in 0..=57u16 {
            if let (SignalRef::Strip(id), _) = decode_output_signal(n) {
                if id.kind == StripKind::Matrix {
                    assert!(
                        id.index >= 1 && id.index <= 6,
                        "{n} decoded to matrix {}",
                        id.index
                    );
                }
                if id.kind == StripKind::Bus {
                    assert!(id.index >= 1 && id.index <= 16, "{n} decoded to bus {}", id.index);
                }
                if id.kind == StripKind::Input {
                    assert!(id.index >= 1 && id.index <= 32, "{n} decoded to ch {}", id.index);
                }
            }
        }
    }

    #[test]
    fn ultranet_port_one_decodes_to_channel_one_direct() {
        let (source, label) = decode_output_signal(26);
        assert_eq!(source, SignalRef::Strip(StripId::new(StripKind::Input, 1)));
        assert_eq!(label, "Ch 1 direct");
    }

    #[test]
    fn unknown_output_source_is_diagnosed_not_guessed() {
        let show = parse("/outputs/main/01 200 PRE OFF");
        assert!(matches!(show.patch.outputs[0].source, SignalRef::Named(_)));
        assert!(show.diagnostics.iter().any(|d| d.severity == Severity::Unknown));
    }

    #[test]
    fn out_of_range_strip_index_is_diagnosed() {
        let show = parse("/ch/99/config \"Nope\" 1 CY 1");
        assert!(show.strips.is_empty());
        assert!(show.diagnostics.iter().any(|d| d.severity == Severity::Suspect));
    }

    #[test]
    fn missing_input_routing_is_diagnosed_rather_than_assumed() {
        let show = parse("/ch/01/config \"Kick\" 1 CY 1");
        assert!(show.patch.inputs.is_empty());
        assert!(show.diagnostics.iter().any(|d| d.message.contains("no input routing found")));
    }

    #[test]
    fn unused_head_amps_are_dropped() {
        let show = parse(
            "/config/routing/IN A1-8 A9-16 A17-24 B1-8 AUX1-4\n\
             /headamp/032 +12.0 ON\n\
             /headamp/000 +5.0 ON",
        );
        // Head amp 32 is AES50-A 1, which block 1 patches; head amp 0 is a
        // local XLR, which this show never routes.
        assert_eq!(show.head_amps.len(), 1);
        assert_eq!(show.head_amps[0].socket, SocketRef::new("aes50a", Direction::In, 1));
        assert_eq!(show.head_amps[0].gain_db, Some(12.0));
        assert!(show.head_amps[0].phantom);
    }

    #[test]
    fn empty_file_is_an_error() {
        assert!(parse_scn(HEADER, "empty.scn").is_err());
    }

    #[test]
    fn arbitrary_bytes_do_not_panic() {
        let junk: String = (0u8..=255).map(|b| b as char).collect();
        let _ = parse_scn(&junk, "junk.scn");
        let _ = parse_scn("/ch//config\n/////\n/headamp/\n#", "odd.scn");
    }
}
