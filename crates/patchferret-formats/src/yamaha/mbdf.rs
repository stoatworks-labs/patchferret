//! The `#YAMAHA MBDF…` container.
//!
//! Yamaha wraps console data in one container across the modern range. Several
//! subtypes exist, distinguished by the magic:
//!
//! ```text
//! #YAMAHA MBDFScene        scenes      .dm3s .tfs
//! #YAMAHA MBDFPreset       presets     .tfp .dm7p
//! #YAMAHA MBDFArchive      firmware update packages
//! #YAMAHA MBDFBackup       list backups
//! #YAMAHA MBDFUserAccount  user accounts
//! ```
//!
//! Only Scene and Preset are handled here; the Archive subtype has a different
//! record layout and is a firmware concern, not a show-file one.
//!
//! Derived from Yamaha's published TF preset pack (a public download) and
//! validated unchanged against the factory scenes shipped inside DM3 Editor and
//! TF Editor — a different subtype *and* a different model, which is the
//! strongest evidence available that this is genuinely one container family.

use super::YamahaError;

const HEADER_LEN: usize = 0x48;
const REC_HEADER_LEN: usize = 36;
pub const MAGIC_PREFIX: &[u8] = b"#YAMAHA MBDF";

fn cstr(b: &[u8]) -> String {
    let end = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    String::from_utf8_lossy(&b[..end]).into_owned()
}

fn be_u32(b: &[u8], at: usize) -> Option<u32> {
    b.get(at..at + 4).map(|s| u32::from_be_bytes([s[0], s[1], s[2], s[3]]))
}

/// One `#MMS FIELD` record.
#[derive(Debug, Clone)]
pub struct Record {
    /// Field name — `Scene`, `Mixing`, `Process`, `FX`, …
    pub name: String,
    /// Target sub-header, e.g. `CH\0\0\0\0\0\x1f` on a per-channel preset.
    pub target: Vec<u8>,
    pub payload: Vec<u8>,
}

/// A parsed MBDF container.
#[derive(Debug, Clone)]
pub struct Container {
    /// Text after `#YAMAHA MBDF` — `Scene`, `Preset`, …
    pub subtype: String,
    /// Model string, e.g. `DM3`, `TF`, `DM7`.
    pub model: String,
    /// Version word, verbatim. Not decoded.
    pub version: [u8; 4],
    pub records: Vec<Record>,
}

impl Container {
    /// Cheap check used for format sniffing. Must not panic on arbitrary bytes.
    pub fn looks_like(data: &[u8]) -> bool {
        data.starts_with(MAGIC_PREFIX)
    }

    pub fn parse(data: &[u8]) -> Result<Self, YamahaError> {
        if !Self::looks_like(data) {
            return Err(YamahaError::NotMbdf);
        }
        if data.len() < HEADER_LEN {
            return Err(YamahaError::Truncated("header"));
        }

        let magic = cstr(&data[..0x18]);
        let subtype = magic.strip_prefix("#YAMAHA MBDF").unwrap_or_default().to_string();
        let model = cstr(&data[0x24..0x34]);
        let mut version = [0u8; 4];
        version.copy_from_slice(&data[0x34..0x38]);

        let mut records = Vec::new();
        let mut off = HEADER_LEN;
        let mut saw_end = false;

        // Only the 12-byte tag is needed to spot #END; requiring a full record
        // header here would miss a terminator sitting in the last few bytes.
        while off + 12 <= data.len() {
            let tag = cstr(&data[off..off + 12]);
            if tag == "#END" {
                saw_end = true;
                break;
            }
            if off + REC_HEADER_LEN > data.len() {
                return Err(YamahaError::Truncated("record header"));
            }
            if tag != "#MMS FIELD" {
                return Err(YamahaError::BadRecord(tag));
            }

            let name = cstr(&data[off + 12..off + 24]);
            let extra =
                be_u32(data, off + 24).ok_or(YamahaError::Truncated("record header"))? as usize;
            let plen =
                be_u32(data, off + 28).ok_or(YamahaError::Truncated("record header"))? as usize;

            let pstart = off
                .checked_add(REC_HEADER_LEN)
                .and_then(|v| v.checked_add(extra))
                .ok_or(YamahaError::Truncated("record"))?;
            let pend = pstart.checked_add(plen).ok_or(YamahaError::Truncated("record"))?;
            if pend > data.len() {
                return Err(YamahaError::Truncated("payload"));
            }

            records.push(Record {
                name,
                target: data[off + REC_HEADER_LEN..pstart].to_vec(),
                payload: data[pstart..pend].to_vec(),
            });

            // Records are padded to a 4-byte boundary.
            off = (pend + 3) & !3;
        }

        if !saw_end {
            return Err(YamahaError::Truncated("no #END record"));
        }
        Ok(Container { subtype, model, version, records })
    }

    pub fn record(&self, name: &str) -> Option<&Record> {
        self.records.iter().find(|r| r.name == name)
    }
}

#[cfg(test)]
pub(crate) mod build {
    //! Synthesises containers for tests.
    //!
    //! Yamaha's factory scenes cannot be committed — they are vendor content
    //! inside a licensed application — so the unit tests build their own
    //! containers to the documented layout. The real files are exercised by the
    //! integration test, which skips when the editors are not installed.

    use super::{HEADER_LEN, REC_HEADER_LEN};

    pub fn container(subtype: &str, model: &str, records: &[(&str, &[u8], &[u8])]) -> Vec<u8> {
        let mut out = vec![0u8; HEADER_LEN];
        let magic = format!("#YAMAHA MBDF{subtype}");
        out[..magic.len()].copy_from_slice(magic.as_bytes());
        out[0x18..0x1C].copy_from_slice(&0x24u32.to_be_bytes());
        out[0x24..0x24 + model.len()].copy_from_slice(model.as_bytes());
        out[0x34..0x38].copy_from_slice(&[0x56, 0x04, 0x02, 0x00]);

        for (name, target, payload) in records {
            let mut hdr = vec![0u8; REC_HEADER_LEN];
            hdr[..10].copy_from_slice(b"#MMS FIELD");
            hdr[12..12 + name.len()].copy_from_slice(name.as_bytes());
            hdr[24..28].copy_from_slice(&(target.len() as u32).to_be_bytes());
            hdr[28..32].copy_from_slice(&(payload.len() as u32).to_be_bytes());
            out.extend_from_slice(&hdr);
            out.extend_from_slice(target);
            out.extend_from_slice(payload);
            while out.len() % 4 != 0 {
                out.push(0);
            }
        }
        // Real files pad the terminator to the record header size.
        let mut end = vec![0u8; REC_HEADER_LEN];
        end[..4].copy_from_slice(b"#END");
        out.extend_from_slice(&end);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_two_record_container() {
        let raw = build::container(
            "Scene",
            "DM3",
            &[("Scene", b"", b"hello"), ("Mixing", b"CH\0\0\0\0\0\x01", b"world!!")],
        );
        let c = Container::parse(&raw).expect("parse");
        assert_eq!(c.subtype, "Scene");
        assert_eq!(c.model, "DM3");
        assert_eq!(c.records.len(), 2);
        assert_eq!(c.record("Scene").unwrap().payload, b"hello");
        let mixing = c.record("Mixing").unwrap();
        assert_eq!(mixing.payload, b"world!!");
        assert_eq!(mixing.target, b"CH\0\0\0\0\0\x01");
    }

    #[test]
    fn payload_padding_does_not_shift_later_records() {
        // A 1-byte payload forces three bytes of padding; the next record must
        // still be found. Getting this wrong is how a walk drifts off the rails
        // several records later rather than failing immediately.
        let raw = build::container(
            "Scene",
            "DM3",
            &[("A", b"", b"x"), ("B", b"", b"yy"), ("C", b"", b"zzz")],
        );
        let c = Container::parse(&raw).unwrap();
        assert_eq!(c.records.len(), 3);
        assert_eq!(c.record("C").unwrap().payload, b"zzz");
    }

    #[test]
    fn rejects_non_mbdf() {
        assert!(matches!(Container::parse(b"not yamaha"), Err(YamahaError::NotMbdf)));
        assert!(!Container::looks_like(b""));
    }

    #[test]
    fn rejects_truncated_container() {
        let raw = build::container("Scene", "DM3", &[("Mixing", b"", b"data")]);
        // Short of a complete terminator the file must be rejected. Cutting
        // into the terminator's trailing padding is NOT truncation — the tag is
        // still whole — so the last failing cut is where "#END" itself breaks.
        let end_tag_at = raw.len() - REC_HEADER_LEN;
        for cut in [0x10, 0x40, 0x50, end_tag_at, end_tag_at + 3] {
            assert!(Container::parse(&raw[..cut]).is_err(), "accepted a {cut}-byte prefix");
        }
        // ...and a cut that keeps the whole tag is fine.
        assert!(Container::parse(&raw[..end_tag_at + 12]).is_ok());
    }

    #[test]
    fn rejects_a_payload_length_past_the_end() {
        let mut raw = build::container("Scene", "DM3", &[("Mixing", b"", b"data")]);
        // Overwrite the payload length with something enormous.
        let at = 0x48 + 28;
        raw[at..at + 4].copy_from_slice(&0xFFFF_FFFFu32.to_be_bytes());
        assert!(matches!(Container::parse(&raw), Err(YamahaError::Truncated(_))));
    }

    #[test]
    fn arbitrary_bytes_do_not_panic() {
        for len in [0, 1, 0x47, 0x48, 0x60] {
            let mut v: Vec<u8> = MAGIC_PREFIX.to_vec();
            v.resize(len.max(MAGIC_PREFIX.len()), 0xAB);
            let _ = Container::parse(&v);
        }
    }
}
