//! Parse a real X32 scene file end to end.
//!
//! The unit tests use minimal synthetic scenes; this one asserts against a
//! 2,104-line scene saved by an actual console, which is the only way to catch
//! the things a hand-written fixture never contains.

use patchferret_formats::{parse_auto, Confidence, ShowInput};
use patchferret_model::*;

fn load() -> Show {
    let bytes = include_bytes!("../../../tests/fixtures/x32-soundboard.scn").to_vec();
    parse_auto(&ShowInput::single("x32-soundboard.scn", bytes)).expect("parse real scene")
}

#[test]
fn detects_the_format_with_strong_confidence() {
    let bytes = include_bytes!("../../../tests/fixtures/x32-soundboard.scn").to_vec();
    let input = ShowInput::single("x32-soundboard.scn", bytes);
    let (adapter, confidence) = patchferret_formats::detect(&input).expect("detected");
    assert_eq!(adapter.id(), "x32");
    assert_eq!(confidence, Confidence::Strong);
}

#[test]
fn reads_show_identity() {
    let show = load();
    assert_eq!(show.meta.name, "General 1.2.2");
    assert_eq!(show.meta.format_version.as_deref(), Some("2.7"));
    assert_eq!(show.meta.source_format, "x32");
}

#[test]
fn reads_all_thirty_two_input_channels() {
    let show = load();
    assert_eq!(show.strips_of(StripKind::Input).count(), 32);
    assert_eq!(show.strips_of(StripKind::Bus).count(), 16);
    assert_eq!(show.strips_of(StripKind::Matrix).count(), 6);
    assert_eq!(show.strips_of(StripKind::Dca).count(), 8);
    assert_eq!(show.strips_of(StripKind::AuxIn).count(), 8);
}

#[test]
fn reads_channel_names_verbatim() {
    let show = load();
    let ch1 = show.strip(StripId::new(StripKind::Input, 1)).unwrap();
    assert_eq!(ch1.name, "Diazno");
    let ch5 = show.strip(StripId::new(StripKind::Input, 5)).unwrap();
    assert_eq!(ch5.name, "Acc Diazno");
    // Unnamed channels keep an empty name and fall back only for display.
    let ch3 = show.strip(StripId::new(StripKind::Input, 3)).unwrap();
    assert_eq!(ch3.name, "");
    assert_eq!(ch3.display_name(), "input 3");
}

#[test]
fn input_patch_covers_all_thirty_two_slots() {
    let show = load();
    assert_eq!(show.patch.inputs.len(), 32);
    let slots: Vec<u16> = show.patch.inputs.iter().map(|p| p.slot).collect();
    assert_eq!(slots, (1..=32).collect::<Vec<_>>());
}

#[test]
fn resolves_connectors_through_the_real_routing_blocks() {
    // The file routes: A1-8 A9-16 A17-24 B1-8 AUX1-4
    let show = load();
    let by_slot = |n: u16| {
        show.patch.inputs.iter().find(|p| p.slot == n).unwrap().socket.clone().unwrap()
    };
    assert_eq!(by_slot(1), SocketRef::new("aes50a", Direction::In, 1));
    assert_eq!(by_slot(8), SocketRef::new("aes50a", Direction::In, 8));
    assert_eq!(by_slot(9), SocketRef::new("aes50a", Direction::In, 9));
    assert_eq!(by_slot(24), SocketRef::new("aes50a", Direction::In, 24));
    // Block four crosses to the other AES50 link.
    assert_eq!(by_slot(25), SocketRef::new("aes50b", Direction::In, 1));
    assert_eq!(by_slot(32), SocketRef::new("aes50b", Direction::In, 8));
}

#[test]
fn head_amps_are_kept_only_for_patched_connectors() {
    let show = load();
    // The console stores 128; this show routes 32 connectors.
    assert_eq!(show.head_amps.len(), 32);
    assert!(show.head_amps.iter().all(|h| h.socket.dir == Direction::In));
    assert!(show
        .head_amps
        .iter()
        .all(|h| h.socket.device == "aes50a" || h.socket.device == "aes50b"));
}

#[test]
fn reads_sixteen_local_outputs_plus_the_other_sections() {
    let show = load();
    let local_outs = show.patch.outputs.iter().filter(|o| o.socket.device == "local").count();
    assert_eq!(local_outs, 16);
    let p16 = show.patch.outputs.iter().filter(|o| o.socket.device == "p16").count();
    assert_eq!(p16, 16);
}

#[test]
fn decodes_a_known_output_row() {
    let show = load();
    // /outputs/main/01 4 PRE OFF  -> Bus 1, pre-fader, on local XLR out 1.
    let out1 = show
        .patch
        .outputs
        .iter()
        .find(|o| o.socket == SocketRef::new("local", Direction::Out, 1))
        .unwrap();
    assert_eq!(out1.source, SignalRef::Strip(StripId::new(StripKind::Bus, 1)));
    assert_eq!(out1.tap, Tap::PreFader);

    // /outputs/p16/01 26 <-EQ OFF -> channel 1 direct, pre-EQ.
    let p16_1 = show
        .patch
        .outputs
        .iter()
        .find(|o| o.socket == SocketRef::new("p16", Direction::Out, 1))
        .unwrap();
    assert_eq!(p16_1.source, SignalRef::Strip(StripId::new(StripKind::Input, 1)));
    assert_eq!(p16_1.tap, Tap::PreEq);
}

#[test]
fn no_output_row_decoded_to_an_unknown_source() {
    let show = load();
    let unknown: Vec<_> =
        show.patch.outputs.iter().filter(|o| matches!(o.source, SignalRef::Named(_))).collect();
    assert!(unknown.is_empty(), "undecoded output sources: {unknown:?}");
}

#[test]
fn round_trips_through_pfx_xml() {
    let show = load();
    let xml = patchferret_model::xml::to_xml(&show).expect("serialise");
    let back = patchferret_model::xml::from_xml(&xml).expect("parse");
    assert_eq!(show, back);
}
