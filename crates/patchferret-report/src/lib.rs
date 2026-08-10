//! PDF documentation generated from the PFX model.
//!
//! Everything here reads [`Show`] and nothing else — no adapter, no file
//! format, no I/O. That is what lets the same code produce a report in the
//! browser via WASM and on the command line, and what will let a report cover
//! a console PatchFerret cannot yet parse the moment an adapter for it lands.

pub mod image;
pub mod layout;
pub mod patch_list;
pub mod pdf;
pub mod spec;
pub mod topology;

use patchferret_model::{JobInfo, Show};

use crate::image::Image;

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

/// Build every report for a show, with no job metadata.
pub fn build_all(show: &Show) -> Vec<Report> {
    build_all_with(show, &JobInfo::default()).0
}

/// Build every report, with job metadata for the cover header.
///
/// Returns the reports plus any problem with the supplied logo. A logo that
/// cannot be embedded must not fail the whole run — the patch list is the point
/// and the logo is decoration — so the reason comes back for the caller to
/// surface, and the reports are generated without it.
pub fn build_all_with(show: &Show, job: &JobInfo) -> (Vec<Report>, Option<String>) {
    let mut logo_error = None;
    let logo: Option<Image> = job.logo.as_ref().and_then(|l| match Image::parse(&l.bytes) {
        Ok(img) => Some(img),
        Err(e) => {
            logo_error = Some(e.to_string());
            None
        }
    });
    let logo = logo.as_ref();

    // Prefer the event name for the file name: that is what the folder of
    // paperwork gets filed under, not the scene name.
    let base = slug(job.event.as_deref().unwrap_or(&show.meta.name));
    let reports = vec![
        Report {
            file_name: format!("{base}-patch-list.pdf"),
            title: "Patch list".into(),
            bytes: patch_list::build(show, job, logo),
        },
        Report {
            file_name: format!("{base}-spec.pdf"),
            title: "Show file specification".into(),
            bytes: spec::build(show, job, logo),
        },
        Report {
            file_name: format!("{base}-topology.pdf"),
            title: "Wiring topology".into(),
            bytes: topology::build(show, job, logo),
        },
    ];
    (reports, logo_error)
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

    /// A 2x2 red JPEG, hand-assembled — enough for a real DCTDecode XObject.
    fn tiny_jpeg() -> Vec<u8> {
        let mut v = vec![0xFF, 0xD8, 0xFF, 0xC0, 0x00, 0x11, 0x08];
        v.extend_from_slice(&2u16.to_be_bytes()); // height
        v.extend_from_slice(&2u16.to_be_bytes()); // width
        v.push(3);
        for c in 1..=3u8 {
            v.extend_from_slice(&[c, 0x11, 0]);
        }
        v.extend_from_slice(&[0xFF, 0xD9]);
        v
    }

    #[test]
    fn job_metadata_reaches_the_pdf() {
        let job = JobInfo {
            event: Some("Summer Live 2026".into()),
            venue: Some("Old Granada Studios".into()),
            engineer: Some("A. Sargeant".into()),
            custom: vec![("Truck call".into(), "0600".into())],
            ..Default::default()
        };

        let (reports, err) = build_all_with(&Show::default(), &job);
        assert_eq!(err, None);
        for r in &reports {
            let s = String::from_utf8_lossy(&r.bytes);
            assert!(s.contains("Summer Live 2026"), "{} lost the event name", r.file_name);
            assert!(s.contains("Old Granada Studios"), "{} lost the venue", r.file_name);
            assert!(s.contains("Truck call"), "{} lost the custom field", r.file_name);
        }
        // The file name follows the event, which is how the job gets filed.
        assert!(reports[0].file_name.starts_with("summer-live-2026"));
    }

    #[test]
    fn a_logo_becomes_an_image_xobject() {
        let job = JobInfo {
            event: Some("With Logo".into()),
            logo: Some(patchferret_model::Logo::new(tiny_jpeg())),
            ..Default::default()
        };

        let (reports, err) = build_all_with(&Show::default(), &job);
        assert_eq!(err, None);
        let s = String::from_utf8_lossy(&reports[0].bytes);
        assert!(s.contains("/Subtype /Image"));
        assert!(s.contains("/DCTDecode"));
        assert!(s.contains("/XObject"));
        assert!(s.contains("/Im0 Do"));
    }

    #[test]
    fn an_unusable_logo_reports_why_and_still_builds() {
        // The patch list is the point; a bad logo must not lose it.
        let job = JobInfo {
            logo: Some(patchferret_model::Logo::new(b"GIF89a not an image".to_vec())),
            ..Default::default()
        };
        let (reports, err) = build_all_with(&Show::default(), &job);
        assert_eq!(reports.len(), 3);
        assert!(err.unwrap().contains("JPEG or PNG"));
        assert!(reports[0].bytes.starts_with(b"%PDF-1.4"));
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
