//! Yamaha DM3 / DM7 / TF scenes and presets.
//!
//! One adapter covers the whole modern range because the file format carries
//! its own schema — see [`mms`]. Nothing here is keyed to a model except the
//! device inventory, and an unknown model still parses.
//!
//! **CL and QL are not this format.** They are the previous architecture and
//! their editors ship no descriptors at all; this adapter will not recognise
//! their files and must not pretend to.
//!
//! # What is verified
//!
//! Channel names, categories, colours and icons decode correctly out of the
//! factory scenes shipped inside DM3 Editor and TF Editor — real content such
//! as "Pulpit / Omni Lav / Headset / Handhld1 / Leader / A.Gt / Kick / OH".
//!
//! # What is not
//!
//! The **patch source encoding**. On DM3 the word splits as an index plus a
//! type code (`0x0140`, `0x0160` seen), and the indices run sequentially across
//! the channels in every factory scene, which is consistent with the analog
//! inputs — but no reading has been checked against a console's patch screen.
//! Where the type code is unrecognised the socket is left unresolved and a
//! diagnostic is emitted rather than a guess.

pub mod mbdf;
pub mod mms;

use patchferret_model::*;

use crate::{AdapterError, Confidence, ShowAdapter, ShowInput};

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum YamahaError {
    #[error("not a Yamaha MBDF container")]
    NotMbdf,
    #[error("not an MMSXLIT payload")]
    NotMms,
    #[error("truncated: {0}")]
    Truncated(&'static str),
    #[error("unexpected record tag {0:?}")]
    BadRecord(String),
    #[error("bad schema: {0}")]
    BadSchema(&'static str),
}

pub struct YamahaAdapter;

impl ShowAdapter for YamahaAdapter {
    fn id(&self) -> &'static str {
        "yamaha-mms"
    }

    fn display_name(&self) -> &'static str {
        "Yamaha DM3 / DM7 / TF scene"
    }

    fn extensions(&self) -> &'static [&'static str] {
        &["dm3s", "dm7s", "tfs", "dm3p", "dm7p", "tfp"]
    }

    fn sniff(&self, input: &ShowInput) -> Confidence {
        let Some(file) = input.primary() else {
            return Confidence::No;
        };
        if !mbdf::Container::looks_like(&file.bytes) {
            return Confidence::No;
        }
        // The magic is specific enough to be conclusive on its own.
        Confidence::Strong
    }

    fn parse(&self, input: &ShowInput) -> Result<Show, AdapterError> {
        let file = input.primary().ok_or(AdapterError::Unrecognised)?;
        parse_container(&file.bytes)
            .map_err(|e| AdapterError::Parse { adapter: "yamaha-mms", message: e.to_string() })
    }
}

/// Top-level strip collections, and the PFX kind each maps to.
///
/// Names are shared across the range; a model lacking one simply has no such
/// collection, so an unknown model degrades to whatever it does have.
const STRIP_TABLES: &[(&str, StripKind)] = &[
    ("InputChannel", StripKind::Input),
    ("StInChannel", StripKind::AuxIn),
    ("FxRtnChannel", StripKind::FxReturn),
    ("Mix", StripKind::Bus),
    ("MixOutput", StripKind::Bus),
    ("Matrix", StripKind::Matrix),
    ("MatrixOutput", StripKind::Matrix),
    ("Stereo", StripKind::Main),
    ("StereoOutput", StripKind::Main),
    ("Mono", StripKind::Mono),
    ("DCA", StripKind::Dca),
    ("DCAGroup", StripKind::Dca),
];

/// The label sub-collection holding a strip's name.
const LABEL: &str = "Label";

/// Names a model's input patch collection may go by.
const PATCH_NAMES: &[&str] = &["Patch", "InPatch"];

/// Decode a patch word into a device id and 1-based connector index.
///
/// The low half is an index and the third/fourth bytes a type code. Only codes
/// actually observed are mapped; anything else returns `None` so the caller can
/// record a diagnostic instead of inventing a connector.
fn decode_patch(word: u64) -> Option<(&'static str, u16)> {
    let index = (word & 0xFFFF) as u16;
    let port_type = ((word >> 16) & 0xFFFF) as u16;
    let device = match port_type {
        0x0140 => "local",
        0x0160 => "stereo-in",
        _ => return None,
    };
    Some((device, index + 1))
}

fn devices_for(model: &str, inputs: u16) -> Vec<Device> {
    vec![
        Device {
            id: "local".into(),
            label: format!("{model} local inputs"),
            model: Some(model.to_string()),
            transport: Transport::Local,
            inputs,
            outputs: 0,
        },
        Device {
            id: "stereo-in".into(),
            label: "Stereo / playback inputs".into(),
            model: None,
            transport: Transport::Local,
            inputs: 8,
            outputs: 0,
        },
    ]
}

pub fn parse_container(bytes: &[u8]) -> Result<Show, YamahaError> {
    let container = mbdf::Container::parse(bytes)?;
    let mut show = Show::default();
    show.meta.source_format = "yamaha-mms".into();
    show.meta.console = if container.model.is_empty() {
        "Yamaha".into()
    } else {
        format!("Yamaha {}", container.model)
    };
    show.meta.format_version =
        Some(container.version.iter().map(|b| format!("{b:02x}")).collect::<String>());

    // Scene identity lives in its own record: SceneInfo/Info carries Title,
    // Comment and OwnerName, and a TimeStamp collection alongside them. Several
    // of these feed the report header directly.
    if let Some(scene) = container.record("Scene").or_else(|| container.record("Preset")) {
        if let Ok(info) = mms::Payload::parse(&scene.payload) {
            let get = |field: &str| {
                info.string(&["Info", field], 0)
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
            };
            if let Some(title) = get("Title") {
                show.meta.name = title;
            }
            show.meta.note = get("Comment");
            // ProductSubName distinguishes frame sizes where the model string
            // alone does not (a DM3 and a DM3S both report "DM3").
            if let Some(sub) = get("ProductSubName") {
                if sub != container.model {
                    show.meta.console = format!("Yamaha {sub}");
                }
            }
        }
    }

    // A preset targets a single channel and carries no console-wide state; say
    // so plainly rather than emitting a show with one lonely strip.
    if container.subtype == "Preset" {
        show.diagnostics.push(Diagnostic {
            severity: Severity::Unmodelled,
            locus: format!("#YAMAHA MBDF{}", container.subtype),
            message:
                "this is a channel preset, not a scene — it carries one strip's processing \
                      and no patch, routing or head-amp state, so no patch list can be built \
                      from it"
                    .into(),
        });
    }

    let Some(record) = container.record("Mixing") else {
        show.diagnostics.push(Diagnostic {
            severity: Severity::Unknown,
            locus: "#MMS FIELD".into(),
            message: format!(
                "no Mixing record in this {} container; records present: {}",
                container.subtype,
                container
                    .records
                    .iter()
                    .map(|r| r.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        });
        return Ok(show);
    };

    let payload = mms::Payload::parse(&record.payload)?;

    // Cross-check: the reconstructed tree must account for exactly the bytes
    // the root claims. A mismatch means the walk drifted, and every offset
    // below it would be quietly wrong.
    let summed: u32 = payload.root.children().iter().map(|c| c.span()).sum();
    if summed != payload.declared_size() {
        show.diagnostics.push(Diagnostic {
            severity: Severity::Suspect,
            locus: format!("MMSXLIT/{}", payload.function),
            message: format!(
                "schema children sum to {summed} bytes but the root declares {}; \
                 decoded values below this point are not trustworthy",
                payload.declared_size()
            ),
        });
    }

    let mut input_count = 0u16;

    for (table, kind) in STRIP_TABLES {
        let Some(count) = payload.strip_table(table) else {
            continue;
        };
        if *kind == StripKind::Input {
            input_count = count as u16;
        }

        for i in 0..count {
            let mut strip = Strip::new(StripId::new(*kind, (i + 1) as u16));
            strip.name = payload
                .string(&[table, LABEL, "Name"], i)
                .unwrap_or_default()
                .trim()
                .to_string();
            strip.colour =
                payload.string(&[table, LABEL, "Color"], i).filter(|s| !s.is_empty());
            strip.icon = payload.string(&[table, LABEL, "Icon"], i).filter(|s| !s.is_empty());

            if *kind == StripKind::Input {
                strip.source = SignalRef::InputSlot((i + 1) as u16);
            }
            show.strips.push(strip);
        }
    }

    // Input patch: connector -> slot -> strip.
    //
    // The patch collection and the parameter inside it are BOTH model-specific
    // — DM3 has Patch/Source as 4 bytes, TF has Patch/Select as 1. Hardcoding
    // DM3's names made TF resolve zero connectors and emit no diagnostic at
    // all, which is the exact failure this project exists to avoid. So: find
    // the collection by any of its known names, and take whatever parameter it
    // actually contains.
    let patch = payload.root.child("InputChannel").and_then(|ic| {
        PATCH_NAMES.iter().find_map(|n| ic.child(n).map(|c| (*n, c))).and_then(|(cname, c)| {
            c.children()
                .iter()
                .find(|p| !p.is_collection())
                .map(|p| (cname, p.name.clone(), p.datasize))
        })
    });

    let mut unresolved = 0usize;
    match &patch {
        Some((cname, pname, width)) => {
            for i in 0..input_count as u32 {
                let word = payload.uint(&["InputChannel", cname, pname], i);
                // Only the wide form carries a port type. A narrow field names
                // a source *selector* whose connector lives in a separate
                // collection we have not decoded, so it cannot yield a socket.
                let socket = if *width >= 4 {
                    word.and_then(decode_patch)
                        .map(|(dev, idx)| SocketRef::new(dev, Direction::In, idx))
                } else {
                    None
                };
                if socket.is_none() {
                    unresolved += 1;
                }
                show.patch.inputs.push(InputPatch {
                    slot: (i + 1) as u16,
                    block_label: String::new(),
                    socket,
                    strip: Some(StripId::new(StripKind::Input, (i + 1) as u16)),
                });
            }

            if unresolved > 0 {
                let detail = if *width >= 4 {
                    "their port-type code is not one of the values observed so far, so the \
                     connector is left blank rather than guessed"
                        .to_string()
                } else {
                    format!(
                        "this model's {cname}/{pname} field is only {width} byte(s) and carries a \
                         source selector, not a port; the connector is chosen by a separate \
                         collection that has not been decoded"
                    )
                };
                show.diagnostics.push(Diagnostic {
                    severity: Severity::Unknown,
                    locus: format!("MMSXLIT/Mixing/InputChannel/{cname}/{pname}"),
                    message: format!(
                        "{unresolved} of {input_count} channels have no resolved connector: {detail}"
                    ),
                });
            }
        }
        None => {
            show.diagnostics.push(Diagnostic {
                severity: Severity::Unknown,
                locus: "MMSXLIT/Mixing/InputChannel".into(),
                message: format!(
                    "no input patch collection found (looked for {}); the connector column of \
                     the patch list is unknown for this model",
                    PATCH_NAMES.join(" or ")
                ),
            });
        }
    }

    show.diagnostics.push(Diagnostic {
        severity: Severity::Unmodelled,
        locus: format!("MMSXLIT/{}", payload.function),
        message: format!(
            "{} collections and {} parameters were read; PFX currently models names, colours, \
             icons and the input patch. Head-amp gain, bus sends, EQ and dynamics are present \
             in the file but not yet carried across",
            payload.collections, payload.parameters
        ),
    });

    show.devices = devices_for(&container.model, input_count.max(1));
    show.strips.sort_by_key(|s| s.id);
    Ok(show)
}

#[cfg(test)]
mod tests {
    use super::mbdf::build as cbuild;
    use super::mms::build::*;
    use super::*;

    /// A synthetic two-channel DM3-shaped scene.
    fn scene() -> Vec<u8> {
        let schema = vec![
            col("Mixing", 0, 24, 1),
            col("InputChannel", 0, 12, 2),
            col("Label", 0, 8, 1),
            pr("Name", mms::TYPE_STRING, 8, 1),
            col("Patch", 8, 4, 1),
            pr("Source", 0x02, 4, 1),
        ];
        let mut values = Vec::new();
        values.extend_from_slice(b"Kick\0\0\0\0");
        values.extend_from_slice(&0x0140_0000u32.to_le_bytes());
        values.extend_from_slice(b"Snare\0\0\0");
        values.extend_from_slice(&0x0140_0004u32.to_le_bytes());
        let p = payload("Mixing", schema, &values);
        cbuild::container("Scene", "DM3", &[("Mixing", b"", &p)])
    }

    #[test]
    fn parses_a_scene_into_strips_and_a_patch() {
        let show = parse_container(&scene()).expect("parse");
        assert_eq!(show.meta.console, "Yamaha DM3");
        assert_eq!(show.strips_of(StripKind::Input).count(), 2);
        assert_eq!(show.strip(StripId::new(StripKind::Input, 1)).unwrap().name, "Kick");
        assert_eq!(show.strip(StripId::new(StripKind::Input, 2)).unwrap().name, "Snare");
    }

    #[test]
    fn patch_resolves_the_connector_index_from_the_word() {
        let show = parse_container(&scene()).unwrap();
        let rows = &show.patch.inputs;
        assert_eq!(rows.len(), 2);
        // index 0 -> connector 1, index 4 -> connector 5 (1-based silkscreen)
        assert_eq!(rows[0].socket, Some(SocketRef::new("local", Direction::In, 1)));
        assert_eq!(rows[1].socket, Some(SocketRef::new("local", Direction::In, 5)));
        assert_eq!(rows[1].strip, Some(StripId::new(StripKind::Input, 2)));
    }

    #[test]
    fn unknown_port_type_is_diagnosed_not_guessed() {
        assert_eq!(decode_patch(0x0140_0000), Some(("local", 1)));
        assert_eq!(decode_patch(0x0160_0001), Some(("stereo-in", 2)));
        assert_eq!(decode_patch(0xDEAD_0000), None);

        let schema = vec![
            col("Mixing", 0, 12, 1),
            col("InputChannel", 0, 12, 1),
            col("Label", 0, 8, 1),
            pr("Name", mms::TYPE_STRING, 8, 1),
            col("Patch", 8, 4, 1),
            pr("Source", 0x02, 4, 1),
        ];
        let mut values = b"Odd\0\0\0\0\0".to_vec();
        values.extend_from_slice(&0xDEAD_0000u32.to_le_bytes());
        let raw = cbuild::container(
            "Scene",
            "DM3",
            &[("Mixing", b"", &payload("Mixing", schema, &values))],
        );

        let show = parse_container(&raw).unwrap();
        assert_eq!(show.patch.inputs[0].socket, None);
        assert!(show
            .diagnostics
            .iter()
            .any(|d| d.message.contains("port-type code is not one")));
    }

    #[test]
    fn finds_a_patch_parameter_under_a_different_name() {
        // Regression: TF calls it Patch/Select and makes it 1 byte, where DM3
        // has Patch/Source at 4. Hardcoding DM3's names made TF resolve zero
        // connectors AND emit no diagnostic — a silent wrong answer.
        let schema = vec![
            col("Mixing", 0, 9, 1),
            col("InputChannel", 0, 9, 1),
            col("Label", 0, 8, 1),
            pr("Name", mms::TYPE_STRING, 8, 1),
            col("Patch", 8, 1, 1),
            pr("Select", 0x02, 1, 1),
        ];
        let values = b"Vox\0\0\0\0\0\x00".to_vec();
        let raw = cbuild::container(
            "Scene",
            "TF",
            &[("Mixing", b"", &payload("Mixing", schema, &values))],
        );

        let show = parse_container(&raw).unwrap();
        assert_eq!(show.strip(StripId::new(StripKind::Input, 1)).unwrap().name, "Vox");
        // A 1-byte field cannot yield a connector, and that must be SAID.
        assert_eq!(show.patch.inputs[0].socket, None);
        let d = show
            .diagnostics
            .iter()
            .find(|d| d.locus.contains("Patch/Select"))
            .expect("no diagnostic for the unresolved narrow patch field");
        assert!(d.message.contains("1 byte"), "{}", d.message);
    }

    #[test]
    fn a_preset_says_it_is_not_a_scene() {
        let raw = cbuild::container("Preset", "TF", &[("Process", b"CH\0\0\0\0\0\x01", b"x")]);
        let show = parse_container(&raw).unwrap();
        assert!(show
            .diagnostics
            .iter()
            .any(|d| d.message.contains("channel preset, not a scene")));
    }

    #[test]
    fn a_container_without_mixing_is_reported_not_silently_empty() {
        let raw = cbuild::container("Scene", "DM3", &[("FX", b"", b"data")]);
        let show = parse_container(&raw).unwrap();
        assert!(show.strips.is_empty());
        assert!(show.diagnostics.iter().any(|d| d.message.contains("no Mixing record")));
    }

    #[test]
    fn round_trips_through_pfx_xml() {
        let show = parse_container(&scene()).unwrap();
        let xml = patchferret_model::xml::to_xml(&show).unwrap();
        assert_eq!(patchferret_model::xml::from_xml(&xml).unwrap(), show);
    }

    #[test]
    fn sniffing_rejects_other_formats() {
        let a = YamahaAdapter;
        assert_eq!(
            a.sniff(&ShowInput::single("x.scn", b"#2.7# \"S\"".to_vec())),
            Confidence::No
        );
        assert_eq!(a.sniff(&ShowInput::single("x.dm3s", scene())), Confidence::Strong);
    }
}
