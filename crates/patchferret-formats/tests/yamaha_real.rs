//! Run the Yamaha adapter against the factory scenes shipped inside the
//! editor applications.
//!
//! Those files are vendor content in a licensed install, so they cannot be
//! committed as fixtures — the unit tests use synthetic containers instead.
//! This test uses the real thing when it is present and **skips** otherwise, so
//! CI stays green on a machine with no editors installed while a developer who
//! has them gets the stronger check.
//!
//! Set `YAMAHA_EDITOR_ROOT` to override the search location.

use std::path::{Path, PathBuf};

use patchferret_formats::{parse_auto, ShowInput};
use patchferret_model::*;

fn editor_roots() -> Vec<PathBuf> {
    if let Ok(dir) = std::env::var("YAMAHA_EDITOR_ROOT") {
        return vec![PathBuf::from(dir)];
    }
    ["DM3", "TF", "DM7"]
        .iter()
        .map(|m| PathBuf::from(format!("/Applications/{m} Editor.app/Contents/Resources")))
        .filter(|p| p.exists())
        .collect()
}

/// Every factory scene we can find, as (path, bytes).
fn scenes() -> Vec<(PathBuf, Vec<u8>)> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, out);
            } else if matches!(
                p.extension().and_then(|s| s.to_str()),
                Some("dm3s") | Some("tfs") | Some("dm7s")
            ) {
                out.push(p);
            }
        }
    }

    let mut paths = Vec::new();
    for root in editor_roots() {
        walk(&root, &mut paths);
    }
    paths.sort();
    paths.into_iter().filter_map(|p| std::fs::read(&p).ok().map(|b| (p, b))).collect()
}

#[test]
fn every_factory_scene_parses() {
    let scenes = scenes();
    if scenes.is_empty() {
        eprintln!("skipping: no Yamaha editors installed");
        return;
    }

    let mut checked = 0;
    for (path, bytes) in &scenes {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let show = parse_auto(&ShowInput::single(name.clone(), bytes.clone()))
            .unwrap_or_else(|e| panic!("{}: {e}", path.display()));

        assert!(show.meta.console.starts_with("Yamaha"), "{name}: {}", show.meta.console);

        let inputs = show.strips_of(StripKind::Input).count();
        assert!(inputs > 0, "{name}: no input channels");

        // Every input strip must have a patch row, and every row must point at
        // a strip. A silent mismatch here is how a patch list loses channels.
        assert_eq!(show.patch.inputs.len(), inputs, "{name}: patch/strip count mismatch");
        assert!(
            show.patch.inputs.iter().all(|p| p.strip.is_some()),
            "{name}: a patch row reaches no strip"
        );

        // The schema walk must have accounted for every declared byte.
        assert!(
            !show
                .diagnostics
                .iter()
                .any(|d| d.message.contains("do not fill")
                    || d.message.contains("not trustworthy")),
            "{name}: schema did not reconcile"
        );

        checked += 1;
    }
    eprintln!("checked {checked} factory scenes");
    assert!(checked >= 15, "expected at least the DM3 and TF scene banks, got {checked}");
}

#[test]
fn channel_names_decode_as_real_text() {
    let scenes = scenes();
    if scenes.is_empty() {
        eprintln!("skipping: no Yamaha editors installed");
        return;
    }

    // At least one scene must carry recognisable, human-authored channel names.
    // Decoding at the wrong offset yields empty strings or mojibake, both of
    // which this catches without pinning to any one factory template.
    let mut best = 0usize;
    for (_, bytes) in &scenes {
        let Ok(show) = parse_auto(&ShowInput::single("s.dm3s", bytes.clone())) else {
            continue;
        };
        let named = show
            .strips_of(StripKind::Input)
            .filter(|s| {
                let n = s.name.trim();
                !n.is_empty() && n.chars().all(|c| c.is_ascii_graphic() || c == ' ')
            })
            .count();
        best = best.max(named);
    }
    assert!(best >= 8, "no scene decoded 8+ clean channel names (best was {best})");
}

#[test]
fn round_trips_every_scene_through_pfx_xml() {
    let scenes = scenes();
    if scenes.is_empty() {
        eprintln!("skipping: no Yamaha editors installed");
        return;
    }
    for (path, bytes) in &scenes {
        let Ok(show) = parse_auto(&ShowInput::single("s", bytes.clone())) else { continue };
        let xml = patchferret_model::xml::to_xml(&show).expect("serialise");
        let back = patchferret_model::xml::from_xml(&xml).expect("parse");
        assert_eq!(show, back, "{}", path.display());
    }
}
