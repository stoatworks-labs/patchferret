//! gzip and tar, enough to open an Allen & Heath show.
//!
//! An A&H show is a gzipped tar whose `Scenes/` entries are themselves gzipped
//! tars, so both layers are needed and the tar reader has to be re-entrant.
//!
//! The gzip *header* and the tar format are hand-parsed — both are small and
//! entirely specified. Only the DEFLATE stream is delegated, to `miniz_oxide`:
//! that one is neither cheap to hand-roll nor bloaty to depend on, and unlike
//! the C-backed zlib bindings it compiles to `wasm32-unknown-unknown`, which
//! the browser build requires.

use super::AhError;

const TAR_BLOCK: usize = 512;

/// Decompress a gzip member and return the inflated bytes.
pub fn gunzip(data: &[u8]) -> Result<Vec<u8>, AhError> {
    if data.len() < 18 || data[0] != 0x1F || data[1] != 0x8B {
        return Err(AhError::NotGzip);
    }
    if data[2] != 8 {
        return Err(AhError::Unsupported("gzip compression method is not DEFLATE"));
    }

    let flags = data[3];
    let mut at = 10usize;

    // FEXTRA
    if flags & 0x04 != 0 {
        let len = data
            .get(at..at + 2)
            .map(|b| u16::from_le_bytes([b[0], b[1]]) as usize)
            .ok_or(AhError::Truncated("gzip FEXTRA"))?;
        at = at.checked_add(2 + len).ok_or(AhError::Truncated("gzip FEXTRA"))?;
    }
    // FNAME and FCOMMENT are NUL-terminated
    for flag in [0x08u8, 0x10] {
        if flags & flag != 0 {
            let end = data
                .get(at..)
                .and_then(|s| s.iter().position(|&b| b == 0))
                .ok_or(AhError::Truncated("gzip header string"))?;
            at += end + 1;
        }
    }
    // FHCRC
    if flags & 0x02 != 0 {
        at = at.checked_add(2).ok_or(AhError::Truncated("gzip FHCRC"))?;
    }

    // The trailer is CRC32 + ISIZE; the deflate stream ends before it.
    let end = data.len().checked_sub(8).ok_or(AhError::Truncated("gzip trailer"))?;
    let body = data.get(at..end).ok_or(AhError::Truncated("gzip body"))?;

    miniz_oxide::inflate::decompress_to_vec(body).map_err(|_| AhError::Inflate)
}

/// One file inside a tar.
#[derive(Debug, Clone)]
pub struct Entry {
    pub path: String,
    pub data: Vec<u8>,
}

fn octal(field: &[u8]) -> Option<u64> {
    let s = field
        .iter()
        .take_while(|&&b| b != 0 && b != b' ')
        .map(|&b| b as char)
        .collect::<String>();
    if s.is_empty() {
        return Some(0);
    }
    u64::from_str_radix(s.trim(), 8).ok()
}

/// Read a tar archive. Directories and metadata entries are skipped.
pub fn untar(data: &[u8]) -> Result<Vec<Entry>, AhError> {
    let mut out = Vec::new();
    let mut at = 0usize;

    while at + TAR_BLOCK <= data.len() {
        let header = &data[at..at + TAR_BLOCK];
        // Two consecutive zero blocks terminate the archive; one is enough to
        // stop on, since trailing padding is all zeros anyway.
        if header.iter().all(|&b| b == 0) {
            break;
        }

        let name = header[..100]
            .iter()
            .take_while(|&&b| b != 0)
            .map(|&b| b as char)
            .collect::<String>();
        let size =
            octal(&header[124..136]).ok_or(AhError::Truncated("tar size field"))? as usize;
        let typeflag = header[156];

        let start = at + TAR_BLOCK;
        let end = start.checked_add(size).ok_or(AhError::Truncated("tar entry"))?;
        if end > data.len() {
            return Err(AhError::Truncated("tar entry runs past end of archive"));
        }

        // '0' and NUL are regular files; everything else (dirs, links, PAX
        // headers) is not something a show needs.
        if (typeflag == b'0' || typeflag == 0) && !name.ends_with('/') {
            out.push(Entry { path: name, data: data[start..end].to_vec() });
        }

        // Entry data is padded to a block boundary.
        at = start + size.div_ceil(TAR_BLOCK) * TAR_BLOCK;
    }

    Ok(out)
}

/// Decompress and untar in one step.
pub fn open(data: &[u8]) -> Result<Vec<Entry>, AhError> {
    untar(&gunzip(data)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a tar in memory, so the tests do not need a fixture.
    fn tar(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut out = Vec::new();
        for (name, body) in files {
            let mut h = [0u8; TAR_BLOCK];
            h[..name.len()].copy_from_slice(name.as_bytes());
            let size = format!("{:011o}\0", body.len());
            h[124..124 + size.len()].copy_from_slice(size.as_bytes());
            h[156] = b'0';
            h[257..262].copy_from_slice(b"ustar");
            // Checksum field must be spaces while computing, but nothing here
            // verifies it, so leaving it blank is fine and is what matters:
            // the reader must not depend on it.
            for b in h[148..156].iter_mut() {
                *b = b' ';
            }
            out.extend_from_slice(&h);
            out.extend_from_slice(body);
            while out.len() % TAR_BLOCK != 0 {
                out.push(0);
            }
        }
        out.extend_from_slice(&[0u8; TAR_BLOCK * 2]);
        out
    }

    fn gzip(data: &[u8]) -> Vec<u8> {
        let mut out = vec![0x1F, 0x8B, 8, 0, 0, 0, 0, 0, 0, 3];
        out.extend_from_slice(&miniz_oxide::deflate::compress_to_vec(data, 6));
        out.extend_from_slice(&0u32.to_le_bytes()); // CRC, not checked
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out
    }

    #[test]
    fn round_trips_a_gzipped_tar() {
        let raw = tar(&[("Show/A.dat", b"alpha"), ("Show/B.dat", b"bravo!!")]);
        let entries = open(&gzip(&raw)).expect("open");
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].path, "Show/A.dat");
        assert_eq!(entries[0].data, b"alpha");
        assert_eq!(entries[1].data, b"bravo!!");
    }

    #[test]
    fn handles_entries_that_do_not_fill_a_block() {
        // A 1-byte file forces 511 bytes of padding; the next header must still
        // be found, or the walk silently loses every later entry.
        let raw = tar(&[("a", b"x"), ("b", b"yy"), ("c", &[7u8; 600])]);
        let entries = untar(&raw).unwrap();
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[2].data.len(), 600);
    }

    #[test]
    fn skips_directory_entries() {
        let mut raw = tar(&[("Show/file.dat", b"data")]);
        // Prepend a directory header.
        let mut dir = [0u8; TAR_BLOCK];
        dir[..5].copy_from_slice(b"Show/");
        dir[124..135].copy_from_slice(b"00000000000");
        dir[156] = b'5';
        let mut combined = dir.to_vec();
        combined.append(&mut raw);
        let entries = untar(&combined).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].path, "Show/file.dat");
    }

    #[test]
    fn parses_a_gzip_header_with_a_file_name() {
        let mut gz = vec![0x1F, 0x8B, 8, 0x08, 0, 0, 0, 0, 0, 3];
        gz.extend_from_slice(b"original.tar\0");
        gz.extend_from_slice(&miniz_oxide::deflate::compress_to_vec(b"payload", 6));
        gz.extend_from_slice(&0u32.to_le_bytes());
        gz.extend_from_slice(&7u32.to_le_bytes());
        assert_eq!(gunzip(&gz).unwrap(), b"payload");
    }

    #[test]
    fn rejects_non_gzip() {
        assert!(matches!(gunzip(b"not gzip at all!!!!!"), Err(AhError::NotGzip)));
        assert!(matches!(gunzip(b""), Err(AhError::NotGzip)));
    }

    #[test]
    fn truncated_input_does_not_panic() {
        let gz = gzip(&tar(&[("a", b"data")]));
        for cut in 0..gz.len() {
            let _ = open(&gz[..cut]);
        }
    }

    #[test]
    fn a_tar_size_field_past_the_end_is_an_error() {
        let mut raw = tar(&[("a", b"data")]);
        raw[124..135].copy_from_slice(b"99999999999");
        assert!(matches!(untar(&raw), Err(AhError::Truncated(_))));
    }
}
