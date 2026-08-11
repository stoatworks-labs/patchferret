//! Run the SQ and CL/QL adapters against the files the controlled diffs
//! produced.
//!
//! Those files came out of the manufacturers' own offline editors and carry
//! their default data, so they are not committed. Point the environment at them
//! to get the stronger check; the tests **skip** otherwise, which keeps CI green
//! on a machine that has never run MixPad or QL Editor.
//!
//! ```text
//! PF_SQ_NVDATA=…/CurrentShow/NVDATA.DAT   the state before the patch change
//! PF_SQ_NVDATA_MOD=…                      after Ip3 was moved to Local 10
//! PF_CLF_BASE=~/Documents/pfql_base.CLF   QL5 default
//! PF_CLF_MOD=~/Documents/pfql_mod3.CLF    after CH4→DANTE1 and CH12→DANTE2
//! PF_CLF_NAME=~/Documents/pfql_name.CLF   after CH8 was renamed to ZZTOP
//! ```

use patchferret_formats::{parse_auto, ShowInput};
use patchferret_model::*;

fn load(var: &str) -> Option<(String, Vec<u8>)> {
    let path = std::env::var(var).ok()?;
    let bytes = std::fs::read(&path).ok()?;
    Some((path, bytes))
}

fn parse(var: &str) -> Option<Show> {
    let (path, bytes) = load(var)?;
    let name = path.rsplit('/').next().unwrap_or("show").to_string();
    match parse_auto(&ShowInput::single(name, bytes)) {
        Ok(s) => Some(s),
        Err(e) => panic!("{path}: {e}"),
    }
}

fn socket_of(show: &Show, slot: u16) -> Option<SocketRef> {
    show.patch.inputs.iter().find(|p| p.slot == slot)?.socket.clone()
}

#[test]
fn sq_reads_the_default_one_to_one_patch() {
    let Some(show) = parse("PF_SQ_NVDATA") else {
        eprintln!("skipping: PF_SQ_NVDATA not set");
        return;
    };
    assert_eq!(show.meta.source_format, "allen-heath-sq");
    assert!(!show.patch.inputs.is_empty());

    // A default SQ patch is 1:1, so channel n sits on socket n.
    for slot in 1..=8u16 {
        assert_eq!(
            socket_of(&show, slot),
            Some(SocketRef::new("input", Direction::In, slot)),
            "channel {slot} is not on its own socket in a default show"
        );
    }
    // And it must say it cannot name the socket class.
    assert!(show.diagnostics.iter().any(|d| d.message.contains("not which socket block")));
}

#[test]
fn sq_reflects_the_one_patch_point_that_was_moved() {
    let (Some(base), Some(modified)) = (parse("PF_SQ_NVDATA"), parse("PF_SQ_NVDATA_MOD"))
    else {
        eprintln!("skipping: PF_SQ_NVDATA / PF_SQ_NVDATA_MOD not set");
        return;
    };

    assert_eq!(socket_of(&base, 3), Some(SocketRef::new("input", Direction::In, 3)));
    assert_eq!(socket_of(&modified, 3), Some(SocketRef::new("input", Direction::In, 10)));

    // Nothing else moved — the diff that found this table changed one byte.
    let differing = base
        .patch
        .inputs
        .iter()
        .zip(&modified.patch.inputs)
        .filter(|(a, b)| a.socket != b.socket)
        .count();
    assert_eq!(differing, 1, "more than the one edited channel changed");
}

#[test]
fn clf_reads_the_ql5_default_patch() {
    let Some(show) = parse("PF_CLF_BASE") else {
        eprintln!("skipping: PF_CLF_BASE not set");
        return;
    };
    assert_eq!(show.meta.console, "Yamaha QL");
    assert_eq!(show.patch.inputs.len(), 64);

    // Channels 1-32 on the local inputs, 33-64 on Dante — including the awkward
    // bank boundary at channel 9, where the encoding jumps from 0x48 to 0xC1.
    assert_eq!(socket_of(&show, 1), Some(SocketRef::new("local", Direction::In, 1)));
    assert_eq!(socket_of(&show, 8), Some(SocketRef::new("local", Direction::In, 8)));
    assert_eq!(socket_of(&show, 9), Some(SocketRef::new("local", Direction::In, 9)));
    assert_eq!(socket_of(&show, 32), Some(SocketRef::new("local", Direction::In, 32)));
    assert_eq!(socket_of(&show, 33), Some(SocketRef::new("dante", Direction::In, 1)));
    assert_eq!(socket_of(&show, 64), Some(SocketRef::new("dante", Direction::In, 32)));

    // Every channel resolved, so the offset check cannot have bailed out.
    assert!(show.patch.inputs.iter().all(|p| p.socket.is_some()));
}

#[test]
fn clf_reflects_both_patch_points_that_were_moved() {
    let (Some(base), Some(modified)) = (parse("PF_CLF_BASE"), parse("PF_CLF_MOD")) else {
        eprintln!("skipping: PF_CLF_BASE / PF_CLF_MOD not set");
        return;
    };

    assert_eq!(socket_of(&base, 4), Some(SocketRef::new("local", Direction::In, 4)));
    assert_eq!(socket_of(&modified, 4), Some(SocketRef::new("dante", Direction::In, 1)));
    assert_eq!(socket_of(&base, 12), Some(SocketRef::new("local", Direction::In, 12)));
    assert_eq!(socket_of(&modified, 12), Some(SocketRef::new("dante", Direction::In, 2)));

    let differing = base
        .patch
        .inputs
        .iter()
        .zip(&modified.patch.inputs)
        .filter(|(a, b)| a.socket != b.socket)
        .count();
    assert_eq!(differing, 2, "exactly two channels were edited");
}

#[test]
fn clf_reads_channel_names_out_of_the_split_table() {
    let Some(show) = parse("PF_CLF_NAME") else {
        eprintln!("skipping: PF_CLF_NAME not set");
        return;
    };
    let name = |ch: u16| show.strip(StripId::new(StripKind::Input, ch)).unwrap().name.clone();

    // The renamed channel, spanning both blocks of the split table.
    assert_eq!(name(8), "ZZTOP");
    // Its neighbours keep the four-character defaults, which is what proves the
    // reassembly is not just reading one block and getting lucky.
    assert_eq!(name(7), "ch 7");
    assert_eq!(name(9), "ch 9");
    assert_eq!(name(64), "ch64");
}

#[test]
fn clf_rename_moved_only_the_one_channel() {
    let (Some(base), Some(renamed)) = (parse("PF_CLF_BASE"), parse("PF_CLF_NAME")) else {
        eprintln!("skipping: PF_CLF_BASE / PF_CLF_NAME not set");
        return;
    };
    let differing =
        base.strips.iter().zip(&renamed.strips).filter(|(a, b)| a.name != b.name).count();
    assert_eq!(differing, 1, "more than the one renamed channel changed");
}

#[test]
fn sq_keeps_unpatched_channels_in_the_list() {
    let Some(show) = parse("PF_SQ_NVDATA") else {
        eprintln!("skipping: PF_SQ_NVDATA not set");
        return;
    };
    // An SQ-7 lists Ip1-Ip40; the default show patches the first 32 and leaves
    // the rest with no source. All forty must appear.
    assert_eq!(show.patch.inputs.len(), 40, "unpatched channels were dropped");
    let with = show.patch.inputs.iter().filter(|p| p.socket.is_some()).count();
    assert_eq!(with, 32);
    assert!(show.patch.inputs.iter().all(|p| p.strip.is_some()));
}

#[test]
fn both_round_trip_through_pfx_xml() {
    for var in ["PF_SQ_NVDATA", "PF_CLF_BASE"] {
        let Some(show) = parse(var) else { continue };
        let xml = patchferret_model::xml::to_xml(&show).expect("serialise");
        assert_eq!(patchferret_model::xml::from_xml(&xml).expect("parse"), show, "{var}");
    }
}
