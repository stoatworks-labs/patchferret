//! PDF documentation generated from the PFX model.
//!
//! Everything here reads [`Show`] and nothing else — no adapter, no file
//! format, no I/O. That is what lets the same code produce a report in the
//! browser via WASM and on the command line, and what will let a report cover
//! a console PatchFerret cannot yet parse the moment an adapter for it lands.

pub mod layout;
pub mod patch_list;
pub mod pdf;
pub mod spec;
pub mod topology;

use patchferret_model::Show;

/// A generated document.
pub struct Report {
    /// Suggested file name, without a directory.
    pub file_name: String,
    /// Human title for the UI.
    pub title: String,
    pub bytes: Vec<u8>,
}

/// Slugify a show name into something safe for a file name on any platform.
fn slug(s: &str) -> String {
    let mut out: String = s
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' })
        .collect();
    while out.contains("--") {
        out = out.replace("--", "-");
    }
    let out = out.trim_matches('-').to_string();
    if out.is_empty() {
        "show".into()
    } else {
        out.chars().take(48).collect()
    }
}

/// Build every report for a show.
pub fn build_all(show: &Show) -> Vec<Report> {
    let base = slug(&show.meta.name);
    vec![
        Report {
            file_name: format!("{base}-patch-list.pdf"),
            title: "Patch list".into(),
            bytes: patch_list::build(show),
        },
        Report {
            file_name: format!("{base}-spec.pdf"),
            title: "Show file specification".into(),
            bytes: spec::build(show),
        },
        Report {
            file_name: format!("{base}-topology.pdf"),
            title: "Wiring topology".into(),
            bytes: topology::build(show),
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugs_are_filesystem_safe() {
        assert_eq!(slug("General 1.2.2"), "general-1-2-2");
        assert_eq!(slug("  ../etc/passwd  "), "etc-passwd");
        assert_eq!(slug(""), "show");
        assert_eq!(slug("!!!"), "show");
        assert!(!slug("a/b\\c:d").contains(['/', '\\', ':']));
    }

    #[test]
    fn slugs_are_length_capped() {
        assert!(slug(&"x".repeat(500)).len() <= 48);
    }

    #[test]
    fn builds_three_reports() {
        let reports = build_all(&Show::default());
        assert_eq!(reports.len(), 3);
        for r in reports {
            assert!(r.bytes.starts_with(b"%PDF-1.4"), "{} is not a PDF", r.file_name);
            assert!(r.file_name.ends_with(".pdf"));
        }
    }
}
