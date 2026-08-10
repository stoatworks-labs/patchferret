//! Allen & Heath Avantis and dLive shows.
//!
//! A show is a gzipped tar of per-subsystem files. The scene data is binary,
//! but self-describing in its own way: every parameter block carries a
//! human-readable label naming both the block type and the object it belongs
//! to, e.g. `"Parametric EQ, Stereo Group Channel 01 Left"` or
//! `"StageBox Port B Analogue Input, Slot 1, Number 07"`.
//!
//! # Two things that are easy to get wrong
//!
//! **The input patch is not scene-recallable.** It lives only in
//! `StageBoxScene65535` — 0xFFFF, the live/current state. The numbered scenes
//! do not carry it at all. Reading the patch from "the current scene" would
//! produce nothing on some shows and stale data on others.
//!
//! **Every scene tarball differs between two saves even when the contents are
//! identical**, because gzip records a timestamp. A file-level diff of two
//! shows reports sixty changed files and tells you nothing; the contents have
//! to be compared after decompression. That is how the patch table was found,
//! and it is worth knowing before anyone tries to repeat the exercise.
//!
//! # Provenance
//!
//! The patch table was located by a controlled diff, not by inference: store a
//! show from Avantis Director offline, change exactly one patch point, store
//! again, compare decompressed contents. Nine bytes differed, the first of them
//! sixteen bytes past a block labelled `Channel Mapper`. See
//! `docs/allen-heath.md` in the research repository.

pub mod archive;
pub mod sq;

use patchferret_model::*;

use crate::{AdapterError, Confidence, ShowAdapter, ShowInput};

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum AhError {
    #[error("not a gzip stream")]
    NotGzip,
    #[error("could not inflate")]
    Inflate,
    #[error("truncated: {0}")]
    Truncated(&'static str),
    #[error("unsupported: {0}")]
    Unsupported(&'static str),
    #[error("no Show directory — this does not look like an Allen & Heath show")]
    NotAShow,
}

/// The block whose payload is the input patch.
const CHANNEL_MAPPER: &[u8] = b"Channel Mapper";
/// Bytes between the end of the label and the first table entry.
const MAPPER_LEAD_IN: usize = 2;
/// Bytes per input channel: one type code, then a big-endian u16 index.
const MAPPER_STRIDE: usize = 3;

/// The live/current state scene. Numbered scenes do not carry the patch.
const LIVE_SCENE: &str = "StageBoxScene65535";

pub struct AllenHeathAdapter;

impl ShowAdapter for AllenHeathAdapter {
    fn id(&self) -> &'static str {
        "allen-heath"
    }

    fn display_name(&self) -> &'static str {
        "Allen & Heath Avantis / dLive show"
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["tar.gz", "gz"]
    }

    fn sniff(&self, input: &ShowInput) -> Confidence {
        let Some(file) = input.primary() else {
            return Confidence::No;
        };
        if !file.bytes.starts_with(&[0x1F, 0x8B]) {
            return Confidence::No;
        }
        // Gzip alone is not enough — plenty of things are gzipped. Only claim
        // it once a Show/ directory is actually present.
        match archive::open(&file.bytes) {
            Ok(entries) if entries.iter().any(|e| e.path.starts_with("Show/")) => {
                Confidence::Strong
            }
            _ => Confidence::No,
        }
    }

    fn parse(&self, input: &ShowInput) -> Result<Show, AdapterError> {
        let file = input.primary().ok_or(AdapterError::Unrecognised)?;
        parse_show(&file.bytes, &input.name)
            .map_err(|e| AdapterError::Parse { adapter: "allen-heath", message: e.to_string() })
    }
}

/// Decode a source type code into a device id.
///
/// Only codes confirmed by controlled diff are mapped. Avantis also offers I/O
/// Port 1, I/O Port 2 and USB as input sources; their codes are unknown, and
/// returning `None` lets the caller diagnose rather than invent a connector.
fn source_device(type_code: u8) -> Option<&'static str> {
    match type_code {
        0x00 => Some("local"),
        0x03 => Some("slink"),
        _ => None,
    }
}

fn devices() -> Vec<Device> {
    vec![
        Device {
            id: "local".into(),
            label: "Console local inputs".into(),
            model: None,
            transport: Transport::Local,
            inputs: 12,
            outputs: 12,
        },
        Device {
            id: "slink".into(),
            label: "SLink (stage box)".into(),
            model: None,
            transport: Transport::SLink,
            inputs: 128,
            outputs: 128,
        },
    ]
}

/// Count how many objects of a given class the scene describes.
///
/// Block labels name their object, e.g. `", Input Channel 07"`, so counting
/// distinct trailing numbers gives the strip inventory without decoding any
/// values.
fn count_objects(scene: &[u8], class: &str) -> u16 {
    let needle = format!(", {class} ");
    let hay = needle.as_bytes();
    let mut highest = 0u16;
    let mut i = 0usize;
    while i + hay.len() < scene.len() {
        if &scene[i..i + hay.len()] == hay {
            let tail = &scene[i + hay.len()..(i + hay.len() + 4).min(scene.len())];
            let digits: String =
                tail.iter().take_while(|b| b.is_ascii_digit()).map(|&b| b as char).collect();
            if let Ok(n) = digits.parse::<u16>() {
                highest = highest.max(n);
            }
            i += hay.len();
        } else {
            i += 1;
        }
    }
    highest
}

/// Object classes present in a scene, and the PFX kind each maps to.
const STRIP_CLASSES: &[(&str, StripKind)] = &[
    ("Input Channel", StripKind::Input),
    ("Mono Aux Channel", StripKind::Bus),
    ("Mono Group Channel", StripKind::Bus),
    ("Mono Matrix Channel", StripKind::Matrix),
    ("FX Channel", StripKind::FxReturn),
];

pub fn parse_show(bytes: &[u8], name: &str) -> Result<Show, AhError> {
    let entries = archive::open(bytes)?;
    if !entries.iter().any(|e| e.path.starts_with("Show/")) {
        return Err(AhError::NotAShow);
    }

    let mut show = Show::default();
    show.meta.source_format = "allen-heath".into();
    show.meta.console = "Allen & Heath".into();
    show.meta.name = name
        .trim_end_matches(".gz")
        .trim_end_matches(".tar")
        .rsplit('/')
        .next()
        .unwrap_or(name)
        .to_string();
    show.devices = devices();

    // The live scene is the only one carrying the patch.
    let live = entries
        .iter()
        .find(|e| e.path.contains(LIVE_SCENE))
        .ok_or(AhError::Truncated("no live scene in this show"))?;

    // Scenes are themselves gzipped tars.
    let inner = archive::open(&live.data)?;
    let scene = inner
        .first()
        .map(|e| e.data.clone())
        .ok_or(AhError::Truncated("live scene archive is empty"))?;

    // No console model. A numbered scene leads with its own NAME, and the
    // factory FOH show happens to call scene 002 "Avantis" — which looks
    // exactly like a model field and is not one. A user-saved show names its
    // scenes whatever the user did ("Reset Settings FOH"), and nothing else in
    // the archive states the model. Say so rather than guess.
    show.diagnostics.push(Diagnostic {
        severity: Severity::Unmodelled,
        locus: "Show/".into(),
        message: "the show file does not state which console model it came from; Avantis and \
                  dLive shows are structurally the same and cannot be told apart from the \
                  archive alone"
            .into(),
    });

    // Strips, from the object classes the labels name.
    let mut inputs = 0u16;
    for (class, kind) in STRIP_CLASSES {
        let n = count_objects(&scene, class);
        if *kind == StripKind::Input {
            inputs = n;
        }
        for i in 1..=n {
            show.strips.push(Strip::new(StripId::new(*kind, i)));
        }
    }

    // Input patch, from the Channel Mapper block.
    match scene.windows(CHANNEL_MAPPER.len()).position(|w| w == CHANNEL_MAPPER) {
        Some(at) => {
            let base = at + CHANNEL_MAPPER.len() + MAPPER_LEAD_IN;
            let mut unmapped: Vec<(u8, usize)> = Vec::new();
            for ch in 0..inputs {
                let o = base + ch as usize * MAPPER_STRIDE;
                let Some(entry) = scene.get(o..o + MAPPER_STRIDE) else {
                    break;
                };
                let index = u16::from_be_bytes([entry[1], entry[2]]);
                let socket = source_device(entry[0])
                    // The table is 0-based; sockets are numbered from 1.
                    .map(|dev| SocketRef::new(dev, Direction::In, index + 1));
                if socket.is_none() {
                    match unmapped.iter_mut().find(|(c, _)| *c == entry[0]) {
                        Some((_, n)) => *n += 1,
                        None => unmapped.push((entry[0], 1)),
                    }
                }
                show.patch.inputs.push(InputPatch {
                    slot: ch + 1,
                    block_label: String::new(),
                    socket,
                    strip: Some(StripId::new(StripKind::Input, ch + 1)),
                });
            }

            if !unmapped.is_empty() {
                unmapped.sort_by_key(|(c, _)| *c);
                let total: usize = unmapped.iter().map(|(_, n)| n).sum();
                let detail = unmapped
                    .iter()
                    .map(|(c, n)| format!("0x{c:02x} on {n}"))
                    .collect::<Vec<_>>()
                    .join(", ");
                show.diagnostics.push(Diagnostic {
                    severity: Severity::Unknown,
                    locus: "Channel Mapper".into(),
                    message: format!(
                        "{total} channel(s) carry a source type outside the two confirmed codes \
                         (0x00 local, 0x03 SLink): {detail}. Avantis also offers I/O Port 1, \
                         I/O Port 2, USB and an unpatched state, and which code is which has not \
                         been established, so those connectors are left blank rather than guessed"
                    ),
                });
            }
        }
        None => {
            show.diagnostics.push(Diagnostic {
                severity: Severity::Unknown,
                locus: LIVE_SCENE.into(),
                message:
                    "no Channel Mapper block found in the live scene; the connector column \
                          of the patch list is unknown for this show"
                        .into(),
            });
        }
    }

    show.diagnostics.push(Diagnostic {
        severity: Severity::Unmodelled,
        locus: "Show/".into(),
        message: format!(
            "{} files were read. PFX currently models the strip inventory and the input patch. \
             Channel names, preamp gain and phantom, bus sends, EQ and dynamics are all present \
             in the scene data but not yet carried across",
            entries.len()
        ),
    });

    show.strips.sort_by_key(|s| s.id);
    Ok(show)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a show archive around a synthetic scene blob.
    fn show_archive(scene: &[u8]) -> Vec<u8> {
        fn tar(files: &[(&str, &[u8])]) -> Vec<u8> {
            let mut out = Vec::new();
            for (name, body) in files {
                let mut h = [0u8; 512];
                h[..name.len()].copy_from_slice(name.as_bytes());
                let size = format!("{:011o}\0", body.len());
                h[124..124 + size.len()].copy_from_slice(size.as_bytes());
                h[156] = b'0';
                out.extend_from_slice(&h);
                out.extend_from_slice(body);
                while out.len() % 512 != 0 {
                    out.push(0);
                }
            }
            out.extend_from_slice(&[0u8; 1024]);
            out
        }
        fn gzip(data: &[u8]) -> Vec<u8> {
            let mut out = vec![0x1F, 0x8B, 8, 0, 0, 0, 0, 0, 0, 3];
            out.extend_from_slice(&miniz_oxide::deflate::compress_to_vec(data, 6));
            out.extend_from_slice(&0u32.to_le_bytes());
            out.extend_from_slice(&(data.len() as u32).to_le_bytes());
            out
        }

        let inner = gzip(&tar(&[("StageBoxScene65535.dat", scene)]));
        gzip(&tar(&[
            ("Show/InputConfig/InputConfig.dat", b"1\n0\n"),
            ("Show/Scenes/StageBoxScene65535.tar.gz", &inner),
        ]))
    }

    /// A scene naming four input channels, with a Channel Mapper table.
    fn scene(entries: &[(u8, u16)]) -> Vec<u8> {
        let mut v = Vec::new();
        v.extend_from_slice(b"\x01\x01Avantis\0\0\0\0");
        for i in 1..=entries.len() {
            v.extend_from_slice(format!("Parametric EQ, Input Channel {i:02}").as_bytes());
            v.push(0);
        }
        v.extend_from_slice(CHANNEL_MAPPER);
        v.extend_from_slice(&[0x00, 0x02]); // lead-in
        for (ty, idx) in entries {
            v.push(*ty);
            v.extend_from_slice(&idx.to_be_bytes());
        }
        v
    }

    #[test]
    fn parses_the_patch_out_of_the_live_scene() {
        // Channel 1 on local 1, the rest on SLink 2,3,4 — the shape the real
        // controlled diff produced.
        let raw = show_archive(&scene(&[(0x00, 0), (0x03, 1), (0x03, 2), (0x03, 3)]));
        let show = parse_show(&raw, "FOH.tar.gz").expect("parse");

        // The archive does not name the model, and the adapter must not invent one.
        assert_eq!(show.meta.console, "Allen & Heath");
        assert!(show
            .diagnostics
            .iter()
            .any(|d| d.message.contains("does not state which console")));
        assert_eq!(show.strips_of(StripKind::Input).count(), 4);
        assert_eq!(show.patch.inputs.len(), 4);

        // 0-based in the file, 1-based on the connector.
        assert_eq!(
            show.patch.inputs[0].socket,
            Some(SocketRef::new("local", Direction::In, 1))
        );
        assert_eq!(
            show.patch.inputs[1].socket,
            Some(SocketRef::new("slink", Direction::In, 2))
        );
        assert_eq!(
            show.patch.inputs[3].socket,
            Some(SocketRef::new("slink", Direction::In, 4))
        );
        assert_eq!(show.patch.inputs[3].strip, Some(StripId::new(StripKind::Input, 4)));
    }

    #[test]
    fn an_unconfirmed_source_type_is_diagnosed_not_guessed() {
        assert_eq!(source_device(0x00), Some("local"));
        assert_eq!(source_device(0x03), Some("slink"));
        assert_eq!(source_device(0x07), None);

        let raw = show_archive(&scene(&[(0x07, 5), (0x03, 1)]));
        let show = parse_show(&raw, "x.tar.gz").unwrap();
        assert_eq!(show.patch.inputs[0].socket, None);
        assert!(show
            .diagnostics
            .iter()
            .any(|d| d.message.contains("outside the two confirmed codes")));
    }

    #[test]
    fn a_show_without_a_channel_mapper_says_so() {
        let mut s = scene(&[(0x03, 0)]);
        // Blank the label out.
        let at = s.windows(CHANNEL_MAPPER.len()).position(|w| w == CHANNEL_MAPPER).unwrap();
        s[at..at + CHANNEL_MAPPER.len()].fill(b'.');
        let show = parse_show(&show_archive(&s), "x.tar.gz").unwrap();
        assert!(show.patch.inputs.is_empty());
        assert!(show.diagnostics.iter().any(|d| d.message.contains("no Channel Mapper")));
    }

    #[test]
    fn counts_strips_from_the_block_labels() {
        let mut s = scene(&[(0x03, 0)]);
        s.extend_from_slice(b"Compressor, Mono Aux Channel 06\0");
        s.extend_from_slice(b"Compressor, Mono Aux Channel 03\0");
        let show = parse_show(&show_archive(&s), "x.tar.gz").unwrap();
        // Highest number seen wins — labels are sparse, not a dense list.
        assert_eq!(show.strips_of(StripKind::Bus).count(), 6);
    }

    #[test]
    fn a_show_with_no_live_scene_is_an_error() {
        // The numbered scenes do not carry the patch, so a show without 65535
        // cannot produce one.
        let raw = {
            fn gzip(d: &[u8]) -> Vec<u8> {
                let mut o = vec![0x1F, 0x8B, 8, 0, 0, 0, 0, 0, 0, 3];
                o.extend_from_slice(&miniz_oxide::deflate::compress_to_vec(d, 6));
                o.extend_from_slice(&0u32.to_le_bytes());
                o.extend_from_slice(&(d.len() as u32).to_le_bytes());
                o
            }
            let mut h = [0u8; 512];
            h[.."Show/Other.dat".len()].copy_from_slice(b"Show/Other.dat");
            h[124..135].copy_from_slice(b"00000000004");
            h[156] = b'0';
            let mut t = h.to_vec();
            t.extend_from_slice(b"data");
            t.resize(1024, 0);
            t.extend_from_slice(&[0u8; 1024]);
            gzip(&t)
        };
        assert!(matches!(parse_show(&raw, "x"), Err(AhError::Truncated(_))));
    }

    #[test]
    fn sniffing_rejects_unrelated_gzip() {
        let a = AllenHeathAdapter;
        let plain_gz = {
            let mut o = vec![0x1F, 0x8B, 8, 0, 0, 0, 0, 0, 0, 3];
            o.extend_from_slice(&miniz_oxide::deflate::compress_to_vec(b"hello", 6));
            o.extend_from_slice(&0u32.to_le_bytes());
            o.extend_from_slice(&5u32.to_le_bytes());
            o
        };
        assert_eq!(a.sniff(&ShowInput::single("x.gz", plain_gz)), Confidence::No);
        assert_eq!(
            a.sniff(&ShowInput::single("FOH.tar.gz", show_archive(&scene(&[(0x03, 0)])))),
            Confidence::Strong
        );
    }

    #[test]
    fn arbitrary_bytes_do_not_panic() {
        for junk in [&b""[..], &b"\x1f\x8b"[..], &[0xFFu8; 64][..]] {
            let _ = parse_show(junk, "junk");
        }
    }
}
