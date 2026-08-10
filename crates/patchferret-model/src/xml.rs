//! PFX XML: reading and writing the interchange format.
//!
//! Hand-written rather than derived. This format is a published spec that other
//! tools are meant to consume, so the element and attribute names are chosen
//! deliberately and must not drift when a Rust field is renamed — which is
//! exactly what a derive macro would let happen silently.

use std::io::Cursor;

use quick_xml::events::{BytesDecl, BytesEnd, BytesStart, BytesText, Event};
use quick_xml::{Reader, Writer};

use crate::*;

pub const NAMESPACE: &str = "https://patchferret.stoatworks.dev/schema/1";
pub const SCHEMA_VERSION: &str = "1";

#[derive(Debug, thiserror::Error)]
pub enum XmlError {
    #[error("XML error: {0}")]
    Xml(#[from] quick_xml::Error),
    #[error("malformed PFX: {0}")]
    Malformed(String),
    #[error("unsupported PFX schema version {0}, this build understands {SCHEMA_VERSION}")]
    Version(String),
}

type Result<T> = std::result::Result<T, XmlError>;

fn fnum(v: f32) -> String {
    format!("{v:.1}")
}

/// Serialise a show to PFX XML.
pub fn to_xml(show: &Show) -> Result<String> {
    let mut w = Writer::new_with_indent(Cursor::new(Vec::new()), b' ', 2);
    w.write_event(Event::Decl(BytesDecl::new("1.0", Some("UTF-8"), None)))?;

    let mut root = BytesStart::new("pfx");
    root.push_attribute(("xmlns", NAMESPACE));
    root.push_attribute(("version", SCHEMA_VERSION));
    w.write_event(Event::Start(root))?;

    // --- meta ---
    let mut meta = BytesStart::new("show");
    meta.push_attribute(("name", show.meta.name.as_str()));
    meta.push_attribute(("console", show.meta.console.as_str()));
    meta.push_attribute(("source-format", show.meta.source_format.as_str()));
    if let Some(v) = &show.meta.format_version {
        meta.push_attribute(("format-version", v.as_str()));
    }
    if let Some(n) = &show.meta.note {
        meta.push_attribute(("note", n.as_str()));
    }
    w.write_event(Event::Empty(meta))?;

    // --- devices ---
    w.write_event(Event::Start(BytesStart::new("devices")))?;
    for d in &show.devices {
        let mut e = BytesStart::new("device");
        e.push_attribute(("id", d.id.as_str()));
        e.push_attribute(("label", d.label.as_str()));
        e.push_attribute(("transport", d.transport.as_str().as_str()));
        e.push_attribute(("inputs", d.inputs.to_string().as_str()));
        e.push_attribute(("outputs", d.outputs.to_string().as_str()));
        if let Some(m) = &d.model {
            e.push_attribute(("model", m.as_str()));
        }
        w.write_event(Event::Empty(e))?;
    }
    w.write_event(Event::End(BytesEnd::new("devices")))?;

    // --- head amps ---
    w.write_event(Event::Start(BytesStart::new("headamps")))?;
    for h in &show.head_amps {
        let mut e = BytesStart::new("headamp");
        e.push_attribute(("socket", h.socket.to_string().as_str()));
        if let Some(g) = h.gain_db {
            e.push_attribute(("gain-db", fnum(g).as_str()));
        }
        e.push_attribute(("phantom", if h.phantom { "true" } else { "false" }));
        if h.pad {
            e.push_attribute(("pad", "true"));
        }
        if h.polarity_invert {
            e.push_attribute(("polarity-invert", "true"));
        }
        w.write_event(Event::Empty(e))?;
    }
    w.write_event(Event::End(BytesEnd::new("headamps")))?;

    // --- strips ---
    w.write_event(Event::Start(BytesStart::new("strips")))?;
    for s in &show.strips {
        let mut e = BytesStart::new("strip");
        e.push_attribute(("id", s.id.to_string().as_str()));
        e.push_attribute(("name", s.name.as_str()));
        e.push_attribute(("source", s.source.as_str().as_str()));
        if s.muted {
            e.push_attribute(("muted", "true"));
        }
        if let Some(f) = s.fader_db {
            e.push_attribute(("fader-db", fnum(f).as_str()));
        }
        if let Some(c) = &s.colour {
            e.push_attribute(("colour", c.as_str()));
        }
        if let Some(i) = &s.icon {
            e.push_attribute(("icon", i.as_str()));
        }
        if let Some(l) = s.linked_to {
            e.push_attribute(("linked-to", l.to_string().as_str()));
        }
        w.write_event(Event::Empty(e))?;
    }
    w.write_event(Event::End(BytesEnd::new("strips")))?;

    // --- patch ---
    w.write_event(Event::Start(BytesStart::new("patch")))?;

    w.write_event(Event::Start(BytesStart::new("inputs")))?;
    for p in &show.patch.inputs {
        let mut e = BytesStart::new("in");
        e.push_attribute(("slot", p.slot.to_string().as_str()));
        e.push_attribute(("block", p.block_label.as_str()));
        if let Some(s) = &p.socket {
            e.push_attribute(("socket", s.to_string().as_str()));
        }
        if let Some(s) = p.strip {
            e.push_attribute(("strip", s.to_string().as_str()));
        }
        w.write_event(Event::Empty(e))?;
    }
    w.write_event(Event::End(BytesEnd::new("inputs")))?;

    w.write_event(Event::Start(BytesStart::new("outputs")))?;
    for p in &show.patch.outputs {
        let mut e = BytesStart::new("out");
        e.push_attribute(("socket", p.socket.to_string().as_str()));
        e.push_attribute(("source", p.source.as_str().as_str()));
        e.push_attribute(("tap", p.tap.as_str()));
        if !p.source_label.is_empty() {
            e.push_attribute(("source-label", p.source_label.as_str()));
        }
        w.write_event(Event::Empty(e))?;
    }
    w.write_event(Event::End(BytesEnd::new("outputs")))?;

    w.write_event(Event::End(BytesEnd::new("patch")))?;

    // --- scenes ---
    w.write_event(Event::Start(BytesStart::new("scenes")))?;
    for s in &show.scenes {
        let mut e = BytesStart::new("scene");
        e.push_attribute(("index", s.index.to_string().as_str()));
        e.push_attribute(("name", s.name.as_str()));
        if let Some(n) = &s.note {
            e.push_attribute(("note", n.as_str()));
        }
        w.write_event(Event::Empty(e))?;
    }
    w.write_event(Event::End(BytesEnd::new("scenes")))?;

    // --- diagnostics ---
    w.write_event(Event::Start(BytesStart::new("diagnostics")))?;
    for d in &show.diagnostics {
        let mut e = BytesStart::new("diagnostic");
        e.push_attribute(("severity", d.severity.as_str()));
        e.push_attribute(("locus", d.locus.as_str()));
        w.write_event(Event::Start(e))?;
        w.write_event(Event::Text(BytesText::new(&d.message)))?;
        w.write_event(Event::End(BytesEnd::new("diagnostic")))?;
    }
    w.write_event(Event::End(BytesEnd::new("diagnostics")))?;

    w.write_event(Event::End(BytesEnd::new("pfx")))?;

    let bytes = w.into_inner().into_inner();
    String::from_utf8(bytes).map_err(|e| XmlError::Malformed(e.to_string()))
}

/// Attribute lookup helper. Returns `None` when absent.
fn attr(e: &BytesStart, name: &str) -> Option<String> {
    e.attributes()
        .flatten()
        .find(|a| a.key.as_ref() == name.as_bytes())
        .and_then(|a| a.unescape_value().ok().map(|v| v.into_owned()))
}

fn attr_or(e: &BytesStart, name: &str, default: &str) -> String {
    attr(e, name).unwrap_or_else(|| default.to_string())
}

fn attr_bool(e: &BytesStart, name: &str) -> bool {
    matches!(attr(e, name).as_deref(), Some("true") | Some("1"))
}

fn attr_f32(e: &BytesStart, name: &str) -> Option<f32> {
    attr(e, name).and_then(|v| v.trim().parse().ok())
}

fn attr_u16(e: &BytesStart, name: &str) -> Option<u16> {
    attr(e, name).and_then(|v| v.trim().parse().ok())
}

/// Parse PFX XML back into a show.
///
/// Round-trip fidelity with [`to_xml`] is asserted by the tests; that property
/// is what makes the XML a usable hand-off point for other tools rather than a
/// write-only report format.
pub fn from_xml(src: &str) -> Result<Show> {
    let mut reader = Reader::from_str(src);
    reader.config_mut().trim_text(true);

    let mut show = Show::default();
    let mut buf = Vec::new();
    // Which list we are currently inside, so `<in>`/`<out>` are unambiguous.
    let mut section: Vec<String> = Vec::new();
    let mut pending_diag: Option<Diagnostic> = None;
    let mut saw_root = false;

    loop {
        match reader.read_event_into(&mut buf) {
            Err(e) => return Err(XmlError::Xml(e)),
            Ok(Event::Eof) => break,

            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                let name = String::from_utf8_lossy(e.name().as_ref()).into_owned();
                match name.as_str() {
                    "pfx" => {
                        saw_root = true;
                        let v = attr_or(&e, "version", SCHEMA_VERSION);
                        if v != SCHEMA_VERSION {
                            return Err(XmlError::Version(v));
                        }
                    }
                    "show" => {
                        show.meta = ShowMeta {
                            name: attr_or(&e, "name", ""),
                            note: attr(&e, "note"),
                            source_format: attr_or(&e, "source-format", ""),
                            console: attr_or(&e, "console", ""),
                            format_version: attr(&e, "format-version"),
                        };
                    }
                    "device" => show.devices.push(Device {
                        id: attr_or(&e, "id", ""),
                        label: attr_or(&e, "label", ""),
                        model: attr(&e, "model"),
                        transport: Transport::parse(&attr_or(&e, "transport", "other")),
                        inputs: attr_u16(&e, "inputs").unwrap_or(0),
                        outputs: attr_u16(&e, "outputs").unwrap_or(0),
                    }),
                    "headamp" => {
                        let socket =
                            attr(&e, "socket").and_then(|s| SocketRef::parse(&s)).ok_or_else(
                                || XmlError::Malformed("headamp without valid socket".into()),
                            )?;
                        show.head_amps.push(HeadAmp {
                            socket,
                            gain_db: attr_f32(&e, "gain-db"),
                            phantom: attr_bool(&e, "phantom"),
                            pad: attr_bool(&e, "pad"),
                            polarity_invert: attr_bool(&e, "polarity-invert"),
                        });
                    }
                    "strip" => {
                        let id = attr(&e, "id").and_then(|s| StripId::parse(&s)).ok_or_else(
                            || XmlError::Malformed("strip without valid id".into()),
                        )?;
                        show.strips.push(Strip {
                            id,
                            name: attr_or(&e, "name", ""),
                            colour: attr(&e, "colour"),
                            icon: attr(&e, "icon"),
                            source: SignalRef::parse(&attr_or(&e, "source", "off")),
                            muted: attr_bool(&e, "muted"),
                            fader_db: attr_f32(&e, "fader-db"),
                            linked_to: attr(&e, "linked-to").and_then(|s| StripId::parse(&s)),
                        });
                    }
                    "in" => show.patch.inputs.push(InputPatch {
                        slot: attr_u16(&e, "slot").unwrap_or(0),
                        block_label: attr_or(&e, "block", ""),
                        socket: attr(&e, "socket").and_then(|s| SocketRef::parse(&s)),
                        strip: attr(&e, "strip").and_then(|s| StripId::parse(&s)),
                    }),
                    "out" => {
                        let socket =
                            attr(&e, "socket").and_then(|s| SocketRef::parse(&s)).ok_or_else(
                                || XmlError::Malformed("output without valid socket".into()),
                            )?;
                        show.patch.outputs.push(OutputPatch {
                            socket,
                            source: SignalRef::parse(&attr_or(&e, "source", "off")),
                            tap: Tap::parse(&attr_or(&e, "tap", "unknown")),
                            source_label: attr_or(&e, "source-label", ""),
                        });
                    }
                    "scene" => show.scenes.push(Scene {
                        index: attr_u16(&e, "index").unwrap_or(0),
                        name: attr_or(&e, "name", ""),
                        note: attr(&e, "note"),
                    }),
                    "diagnostic" => {
                        pending_diag = Some(Diagnostic {
                            severity: Severity::parse(&attr_or(&e, "severity", "unknown")),
                            locus: attr_or(&e, "locus", ""),
                            message: String::new(),
                        });
                    }
                    other => section.push(other.to_string()),
                }
            }

            Ok(Event::Text(t)) => {
                if let Some(d) = pending_diag.as_mut() {
                    d.message = t.unescape().map(|c| c.into_owned()).unwrap_or_default();
                }
            }

            Ok(Event::End(e)) if e.name().as_ref() == b"diagnostic" => {
                if let Some(d) = pending_diag.take() {
                    show.diagnostics.push(d);
                }
            }

            _ => {}
        }
        buf.clear();
    }

    if !saw_root {
        return Err(XmlError::Malformed("no <pfx> root element".into()));
    }
    Ok(show)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Show {
        let mut show = Show {
            meta: ShowMeta {
                name: "Round Trip".into(),
                note: Some("a note".into()),
                source_format: "x32".into(),
                console: "Behringer X32".into(),
                format_version: Some("2.7".into()),
            },
            ..Default::default()
        };
        show.devices.push(Device {
            id: "local".into(),
            label: "Console local I/O".into(),
            model: None,
            transport: Transport::Local,
            inputs: 32,
            outputs: 16,
        });
        show.devices.push(Device {
            id: "card".into(),
            label: "Card".into(),
            model: Some("X-UF".into()),
            transport: Transport::Card("X-UF".into()),
            inputs: 32,
            outputs: 32,
        });
        show.head_amps.push(HeadAmp {
            socket: SocketRef::new("local", Direction::In, 3),
            gain_db: Some(24.5),
            phantom: true,
            pad: false,
            polarity_invert: true,
        });
        let mut s = Strip::new(StripId::new(StripKind::Input, 1));
        s.name = "Kick".into();
        s.source = SignalRef::InputSlot(1);
        s.fader_db = Some(-3.5);
        s.muted = true;
        s.colour = Some("RD".into());
        s.linked_to = Some(StripId::new(StripKind::Input, 2));
        show.strips.push(s);
        show.patch.inputs.push(InputPatch {
            slot: 1,
            block_label: "A1-8".into(),
            socket: Some(SocketRef::new("aes50a", Direction::In, 1)),
            strip: Some(StripId::new(StripKind::Input, 1)),
        });
        show.patch.outputs.push(OutputPatch {
            socket: SocketRef::new("local", Direction::Out, 1),
            source: SignalRef::Strip(StripId::new(StripKind::Bus, 4)),
            tap: Tap::PreFader,
            source_label: "Bus 4".into(),
        });
        show.scenes.push(Scene { index: 0, name: "Opening".into(), note: None });
        show.diagnostics.push(Diagnostic {
            severity: Severity::Unmodelled,
            locus: "/config/talk/A".into(),
            message: "talkback routing is not represented in PFX".into(),
        });
        show
    }

    #[test]
    fn round_trips_without_loss() {
        let original = sample();
        let xml = to_xml(&original).expect("serialise");
        let parsed = from_xml(&xml).expect("parse");
        assert_eq!(original, parsed);
    }

    #[test]
    fn round_trips_twice_identically() {
        let xml1 = to_xml(&sample()).unwrap();
        let xml2 = to_xml(&from_xml(&xml1).unwrap()).unwrap();
        assert_eq!(xml1, xml2, "serialisation is not stable across a round trip");
    }

    #[test]
    fn rejects_unknown_schema_version() {
        let xml = to_xml(&sample()).unwrap().replace("version=\"1\"", "version=\"99\"");
        assert!(matches!(from_xml(&xml), Err(XmlError::Version(_))));
    }

    #[test]
    fn rejects_non_pfx_document() {
        assert!(from_xml("<html><body/></html>").is_err());
    }

    #[test]
    fn socket_ref_round_trips_through_string() {
        let s = SocketRef::new("aes50a", Direction::In, 17);
        assert_eq!(SocketRef::parse(&s.to_string()), Some(s));
    }

    #[test]
    fn escapes_hostile_names() {
        let mut show = Show::default();
        show.meta.name = r#"Bad & "quoted" <name>"#.into();
        let xml = to_xml(&show).unwrap();
        assert_eq!(from_xml(&xml).unwrap().meta.name, r#"Bad & "quoted" <name>"#);
    }
}
