//! WASM entry point for the browser build.
//!
//! Uses a plain C ABI rather than `wasm-bindgen`. The interface is one call
//! with bytes in and bytes out, so the generated glue would be doing almost
//! nothing, and avoiding it means the browser build needs no `wasm-pack`, no
//! `wasm-bindgen-cli` and no version handshake between the CLI and the crate —
//! `cargo build --target wasm32-unknown-unknown` is the whole toolchain.
//!
//! # Result encoding
//!
//! [`pf_process`] returns a pointer to a buffer laid out as:
//!
//! ```text
//! u32  total length of everything after this field
//! records, each:
//!   u32  kind   (see `kind` constants)
//!   u32  name length, then that many bytes of UTF-8
//!   u32  body length, then that many bytes
//! ```
//!
//! All integers are little-endian, which is the only byte order WASM uses.

use std::alloc::{alloc, dealloc, Layout};

use patchferret_formats::{detect, ShowFile, ShowInput};
use patchferret_model::{xml, JobInfo, Logo, Show};
use patchferret_report::build_all_with;

/// A human-readable error; no other records will follow.
pub const KIND_ERROR: u32 = 0;
/// A JSON summary for the page to display.
pub const KIND_SUMMARY: u32 = 1;
/// The PFX interchange XML.
pub const KIND_XML: u32 = 2;
/// A generated PDF.
pub const KIND_PDF: u32 = 3;
/// Something went wrong but the run still produced output.
pub const KIND_WARNING: u32 = 4;

/// Allocate `len` bytes for the caller to write input into.
///
/// # Safety
/// The caller must pass the returned pointer and the same `len` to
/// [`pf_free`], or hand it to [`pf_process`], which takes ownership.
#[no_mangle]
pub unsafe extern "C" fn pf_alloc(len: usize) -> *mut u8 {
    if len == 0 {
        return std::ptr::null_mut();
    }
    match Layout::from_size_align(len, 1) {
        Ok(layout) => alloc(layout),
        Err(_) => std::ptr::null_mut(),
    }
}

/// Release a buffer obtained from [`pf_alloc`] or [`pf_process`].
///
/// # Safety
/// `ptr` must have come from this module with the same `len`.
#[no_mangle]
pub unsafe extern "C" fn pf_free(ptr: *mut u8, len: usize) {
    if ptr.is_null() || len == 0 {
        return;
    }
    if let Ok(layout) = Layout::from_size_align(len, 1) {
        dealloc(ptr, layout);
    }
}

/// Total length of a result buffer, including its own 4-byte length prefix.
///
/// # Safety
/// `ptr` must be a buffer returned by [`pf_process`].
#[no_mangle]
pub unsafe extern "C" fn pf_result_len(ptr: *const u8) -> usize {
    if ptr.is_null() {
        return 0;
    }
    let mut n = [0u8; 4];
    std::ptr::copy_nonoverlapping(ptr, n.as_mut_ptr(), 4);
    u32::from_le_bytes(n) as usize + 4
}

struct Writer(Vec<u8>);

impl Writer {
    fn new() -> Self {
        // Reserve the length prefix; filled in by `finish`.
        Self(vec![0, 0, 0, 0])
    }

    fn record(&mut self, kind: u32, name: &str, body: &[u8]) {
        self.0.extend_from_slice(&kind.to_le_bytes());
        self.0.extend_from_slice(&(name.len() as u32).to_le_bytes());
        self.0.extend_from_slice(name.as_bytes());
        self.0.extend_from_slice(&(body.len() as u32).to_le_bytes());
        self.0.extend_from_slice(body);
    }

    /// Write the length prefix and hand the buffer to the caller.
    fn finish(mut self) -> *mut u8 {
        let payload = (self.0.len() - 4) as u32;
        self.0[..4].copy_from_slice(&payload.to_le_bytes());
        let mut boxed = self.0.into_boxed_slice();
        let ptr = boxed.as_mut_ptr();
        std::mem::forget(boxed);
        ptr
    }
}

fn error(message: &str) -> *mut u8 {
    let mut w = Writer::new();
    w.record(KIND_ERROR, "error", message.as_bytes());
    w.finish()
}

/// Escape a string for embedding in JSON.
fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

/// Build the JSON the page renders its summary from.
fn summary_json(show: &Show, format: &str) -> String {
    let routed = show.patch.inputs.iter().filter(|p| p.socket.is_some()).count();
    let reaching = show.patch.inputs.iter().filter(|p| p.strip.is_some()).count();

    let mut diags = String::new();
    for (i, d) in show.diagnostics.iter().enumerate() {
        if i > 0 {
            diags.push(',');
        }
        diags.push_str(&format!(
            r#"{{"severity":"{}","locus":"{}","message":"{}"}}"#,
            d.severity.as_str(),
            json_escape(&d.locus),
            json_escape(&d.message)
        ));
    }

    format!(
        r#"{{"name":"{}","console":"{}","format":"{}","version":"{}","strips":{},"devices":{},"slots":{},"routed":{},"reaching":{},"outputs":{},"headAmps":{},"diagnostics":[{}]}}"#,
        json_escape(&show.meta.name),
        json_escape(&show.meta.console),
        json_escape(format),
        json_escape(show.meta.format_version.as_deref().unwrap_or("")),
        show.strips.len(),
        show.devices.len(),
        show.patch.inputs.len(),
        routed,
        reaching,
        show.patch.outputs.len(),
        show.head_amps.len(),
        diags
    )
}

/// Parse a show file and generate every output.
///
/// Takes ownership of both input buffers; the caller must not free them.
/// The returned buffer must be released with [`pf_free`] using the length from
/// [`pf_result_len`].
///
/// # Safety
/// Both pointers must be valid for their stated lengths.
#[no_mangle]
pub unsafe extern "C" fn pf_process(
    name_ptr: *const u8,
    name_len: usize,
    data_ptr: *const u8,
    data_len: usize,
    job_ptr: *const u8,
    job_len: usize,
    logo_ptr: *const u8,
    logo_len: usize,
) -> *mut u8 {
    if data_ptr.is_null() || data_len == 0 {
        return error("empty file");
    }

    let name = if name_ptr.is_null() || name_len == 0 {
        "show".to_string()
    } else {
        String::from_utf8_lossy(std::slice::from_raw_parts(name_ptr, name_len)).into_owned()
    };
    let data = std::slice::from_raw_parts(data_ptr, data_len).to_vec();

    let input = ShowInput { name: name.clone(), files: vec![ShowFile::new(name, data)] };

    let Some((adapter, _confidence)) = detect(&input) else {
        return error(
            "This file was not recognised as a show file PatchFerret can read. \
             Supported formats are listed below.",
        );
    };

    let show = match adapter.parse(&input) {
        Ok(s) => s,
        Err(e) => return error(&e.to_string()),
    };

    // Job metadata arrives as the same `key: value` sheet the CLI reads, so
    // the two front ends cannot drift apart on which keys are understood.
    let mut job = if job_ptr.is_null() || job_len == 0 {
        JobInfo::default()
    } else {
        let text = String::from_utf8_lossy(std::slice::from_raw_parts(job_ptr, job_len));
        JobInfo::parse_sidecar(&text).0
    };
    if !logo_ptr.is_null() && logo_len > 0 {
        job.logo = Some(Logo::new(std::slice::from_raw_parts(logo_ptr, logo_len).to_vec()));
    }

    let mut w = Writer::new();
    w.record(KIND_SUMMARY, "summary", summary_json(&show, adapter.display_name()).as_bytes());

    match xml::to_xml(&show) {
        Ok(x) => {
            let base = show.meta.name.replace(|c: char| !c.is_ascii_alphanumeric(), "-");
            w.record(KIND_XML, &format!("{}.pfx.xml", base.trim_matches('-')), x.as_bytes());
        }
        Err(e) => w.record(KIND_ERROR, "xml", e.to_string().as_bytes()),
    }

    let (reports, logo_error) = build_all_with(&show, &job);
    if let Some(e) = logo_error {
        // Not fatal: the reports are built without the logo, and the page says
        // why rather than silently dropping it.
        w.record(KIND_WARNING, "logo", e.as_bytes());
    }
    for r in reports {
        w.record(KIND_PDF, &r.file_name, &r.bytes);
    }

    w.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Decode a result buffer the way the JS side does.
    unsafe fn decode(ptr: *mut u8) -> Vec<(u32, String, Vec<u8>)> {
        let total = pf_result_len(ptr);
        let buf = std::slice::from_raw_parts(ptr, total);
        let mut out = Vec::new();
        let mut i = 4usize;
        let u32_at = |b: &[u8], i: usize| {
            u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]) as usize
        };
        while i < total {
            let kind = u32_at(buf, i) as u32;
            i += 4;
            let nl = u32_at(buf, i);
            i += 4;
            let name = String::from_utf8_lossy(&buf[i..i + nl]).into_owned();
            i += nl;
            let bl = u32_at(buf, i);
            i += 4;
            let body = buf[i..i + bl].to_vec();
            i += bl;
            out.push((kind, name, body));
        }
        out
    }

    fn process(name: &str, data: &[u8]) -> Vec<(u32, String, Vec<u8>)> {
        unsafe {
            let ptr = pf_process(
                name.as_ptr(),
                name.len(),
                data.as_ptr(),
                data.len(),
                std::ptr::null(),
                0,
                std::ptr::null(),
                0,
            );
            let records = decode(ptr);
            pf_free(ptr, pf_result_len(ptr));
            records
        }
    }

    #[test]
    fn processes_a_real_scene_into_xml_and_three_pdfs() {
        let scene = include_bytes!("../../../tests/fixtures/x32-soundboard.scn");
        let records = process("x32-soundboard.scn", scene);

        assert_eq!(records.iter().filter(|(k, _, _)| *k == KIND_SUMMARY).count(), 1);
        assert_eq!(records.iter().filter(|(k, _, _)| *k == KIND_XML).count(), 1);
        assert_eq!(records.iter().filter(|(k, _, _)| *k == KIND_PDF).count(), 3);
        assert_eq!(records.iter().filter(|(k, _, _)| *k == KIND_ERROR).count(), 0);

        for (kind, name, body) in &records {
            if *kind == KIND_PDF {
                assert!(body.starts_with(b"%PDF-1.4"), "{name} is not a PDF");
                assert!(name.ends_with(".pdf"));
            }
        }
    }

    #[test]
    fn summary_json_is_well_formed() {
        let scene = include_bytes!("../../../tests/fixtures/x32-soundboard.scn");
        let records = process("x32-soundboard.scn", scene);
        let (_, _, body) =
            records.iter().find(|(k, _, _)| *k == KIND_SUMMARY).expect("summary");
        let json = String::from_utf8(body.clone()).expect("utf8");

        assert!(json.starts_with('{') && json.ends_with('}'));
        assert!(json.contains(r#""name":"General 1.2.2""#));
        assert!(json.contains(r#""slots":32"#));
        assert_eq!(json.matches('{').count(), json.matches('}').count());
        assert_eq!(json.matches('[').count(), json.matches(']').count());
    }

    #[test]
    fn json_escaping_survives_hostile_names() {
        assert_eq!(json_escape(r#"a"b\c"#), r#"a\"b\\c"#);
        assert_eq!(json_escape("line\nbreak"), "line\\nbreak");
    }

    #[test]
    fn unrecognised_input_returns_a_single_error_record() {
        let records = process("junk.bin", &[0xff, 0x00, 0xfe]);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].0, KIND_ERROR);
    }

    #[test]
    fn empty_input_returns_an_error_not_a_crash() {
        unsafe {
            let ptr = pf_process(
                std::ptr::null(),
                0,
                std::ptr::null(),
                0,
                std::ptr::null(),
                0,
                std::ptr::null(),
                0,
            );
            let records = decode(ptr);
            assert_eq!(records[0].0, KIND_ERROR);
            pf_free(ptr, pf_result_len(ptr));
        }
    }

    #[test]
    fn alloc_and_free_round_trip() {
        unsafe {
            let p = pf_alloc(128);
            assert!(!p.is_null());
            std::ptr::write_bytes(p, 0xAB, 128);
            assert_eq!(*p, 0xAB);
            pf_free(p, 128);
            // Zero-length allocation is legal and yields null.
            assert!(pf_alloc(0).is_null());
            pf_free(std::ptr::null_mut(), 0);
        }
    }
}
