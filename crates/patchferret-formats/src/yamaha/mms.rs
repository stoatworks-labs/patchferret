//! MMSXLIT — Yamaha's self-describing parameter payload.
//!
//! Each payload carries its own schema inline, then the packed values that
//! schema describes:
//!
//! ```text
//! "MMSXLIT\0" <function name>
//! schema:  COL record, 48 bytes   collection
//!          PR  record, 32 bytes   parameter
//! values:  packed structs laid out per the schema
//! ```
//!
//! **This is why PatchFerret needs no vendor files.** Yamaha ships the same
//! information as `mms_*.xml` inside each editor application, and the two agree
//! exactly — but the editors are Windows/macOS installs with a filesystem, and
//! the browser build has neither. Because the schema travels inside the file,
//! one decoder serves every model without a lookup table, and a console
//! released tomorrow parses without a code change.
//!
//! # Record layout
//!
//! ```text
//! COL, 48 bytes            PR, 32 bytes
//!  0  "COL"                 0  "PR "
//!  3  level byte            3  type code   0x00 string, 0x02 unsigned
//!  4  name, NUL-padded      4  u16le size (of one element)
//! 32  u32le (unused)        6  u16le arraysize
//! 36  u32le offset          8  name, NUL-padded
//! 40  u32le datasize
//! 44  u32le arraysize
//! ```
//!
//! Note the endianness split, which is genuinely how the format is: **schema
//! metadata is little-endian, and so are the packed values**. Reading the DM3
//! patch word big-endian also produces a tidy-looking sequence, which is the
//! trap — both readings give ascending indices, so "it looks right" proves
//! nothing here.
//!
//! # Validation
//!
//! Walking a real DM3 scene yields exactly the 207 collections and 494
//! parameters its `mms_Mixing.xml` declares, the value block begins exactly
//! where the walk ends, and the reconstructed tree sums to the declared root
//! `datasize` of 14,890 with nothing left over.

use super::YamahaError;

const COL_REC: usize = 48;
const PR_REC: usize = 32;

pub const TYPE_STRING: u8 = 0x00;

fn cstr(b: &[u8]) -> String {
    let end = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    String::from_utf8_lossy(&b[..end]).into_owned()
}

fn u32le(b: &[u8], at: usize) -> Option<u32> {
    b.get(at..at + 4).map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

fn u16le(b: &[u8], at: usize) -> Option<u16> {
    b.get(at..at + 2).map(|s| u16::from_le_bytes([s[0], s[1]]))
}

#[derive(Debug, Clone, PartialEq)]
pub enum NodeKind {
    Collection { children: Vec<Node> },
    Parameter { type_code: u8 },
}

/// One entry in the schema tree.
#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    pub name: String,
    /// Byte offset within the parent element.
    pub offset: u32,
    /// Size of a single element.
    pub datasize: u32,
    pub arraysize: u32,
    pub kind: NodeKind,
}

impl Node {
    pub fn is_collection(&self) -> bool {
        matches!(self.kind, NodeKind::Collection { .. })
    }

    pub fn children(&self) -> &[Node] {
        match &self.kind {
            NodeKind::Collection { children } => children,
            NodeKind::Parameter { .. } => &[],
        }
    }

    pub fn child(&self, name: &str) -> Option<&Node> {
        self.children().iter().find(|c| c.name == name)
    }

    /// Total bytes this node occupies in its parent, all array elements.
    pub fn span(&self) -> u32 {
        self.datasize.saturating_mul(self.arraysize)
    }
}

/// A decoded MMSXLIT payload: schema tree plus the value block it describes.
#[derive(Debug, Clone)]
pub struct Payload {
    /// Function name, e.g. `Mixing`.
    pub function: String,
    pub root: Node,
    /// Offset of the value block within the payload.
    pub values_at: usize,
    pub collections: usize,
    pub parameters: usize,
    data: Vec<u8>,
}

struct Cursor<'a> {
    data: &'a [u8],
    at: usize,
    cols: usize,
    prs: usize,
}

impl<'a> Cursor<'a> {
    /// Read one record and, if a collection, its children.
    ///
    /// Children are consumed until their spans sum to the parent's `datasize`,
    /// which is what makes the flat pre-order run reconstructable as a tree.
    fn node(&mut self) -> Result<Node, YamahaError> {
        let d = self.data;
        let o = self.at;
        let tag = d.get(o..o + 3).ok_or(YamahaError::Truncated("schema record"))?;

        if tag == b"COL" {
            let name = cstr(d.get(o + 4..o + 32).ok_or(YamahaError::Truncated("COL name"))?);
            let offset = u32le(d, o + 36).ok_or(YamahaError::Truncated("COL offset"))?;
            let datasize = u32le(d, o + 40).ok_or(YamahaError::Truncated("COL datasize"))?;
            let arraysize = u32le(d, o + 44).ok_or(YamahaError::Truncated("COL arraysize"))?;
            self.at += COL_REC;
            self.cols += 1;

            let mut children = Vec::new();
            let mut consumed: u32 = 0;
            // A zero-size collection has no children; without this guard a
            // malformed file would spin here forever.
            while consumed < datasize {
                let before = self.at;
                let child = self.node()?;
                if self.at == before {
                    return Err(YamahaError::BadSchema("schema made no progress"));
                }
                consumed = consumed.saturating_add(child.span());
                children.push(child);
                if children.len() > 4096 {
                    return Err(YamahaError::BadSchema("implausible child count"));
                }
            }

            Ok(Node {
                name,
                offset,
                datasize,
                arraysize,
                kind: NodeKind::Collection { children },
            })
        } else if tag == b"PR " {
            let type_code = d[o + 3];
            let size = u16le(d, o + 4).ok_or(YamahaError::Truncated("PR size"))? as u32;
            let arraysize =
                u16le(d, o + 6).ok_or(YamahaError::Truncated("PR arraysize"))? as u32;
            let name = cstr(d.get(o + 8..o + 32).ok_or(YamahaError::Truncated("PR name"))?);
            self.at += PR_REC;
            self.prs += 1;
            // Parameters carry no offset of their own; it is assigned by the
            // parent below, from the running sum of its children.
            Ok(Node {
                name,
                offset: 0,
                datasize: size,
                arraysize,
                kind: NodeKind::Parameter { type_code },
            })
        } else {
            Err(YamahaError::BadSchema("expected COL or PR record"))
        }
    }
}

/// Assign offsets to parameters, which do not carry their own.
///
/// Collections state their offset explicitly; parameters are laid out end to
/// end from the start of their parent.
fn assign_parameter_offsets(node: &mut Node) {
    if let NodeKind::Collection { children } = &mut node.kind {
        let mut run = 0u32;
        for c in children.iter_mut() {
            if c.is_collection() {
                run = c.offset.saturating_add(c.span());
                assign_parameter_offsets(c);
            } else {
                c.offset = run;
                run = run.saturating_add(c.span());
            }
        }
    }
}

impl Payload {
    pub fn parse(payload: &[u8]) -> Result<Self, YamahaError> {
        if !payload.starts_with(b"MMSXLIT") {
            return Err(YamahaError::NotMms);
        }
        let function = cstr(payload.get(8..40).ok_or(YamahaError::Truncated("function name"))?);

        let start = payload
            .windows(3)
            .position(|w| w == b"COL")
            .ok_or(YamahaError::BadSchema("no schema block"))?;

        let mut cur = Cursor { data: payload, at: start, cols: 0, prs: 0 };
        let mut root = cur.node()?;
        assign_parameter_offsets(&mut root);

        let values_at = cur.at;
        if values_at > payload.len() {
            return Err(YamahaError::Truncated("schema overruns payload"));
        }

        Ok(Payload {
            function,
            root,
            values_at,
            collections: cur.cols,
            parameters: cur.prs,
            data: payload.to_vec(),
        })
    }

    /// Absolute offset of element `index` of a node reached by `path`.
    ///
    /// `path` names successive children from the root, e.g.
    /// `["InputChannel", "Label", "Name"]`. `index` selects which element of
    /// the *first* array encountered — enough for the strip tables, which are
    /// the only arrays PatchFerret reads.
    fn locate(&self, path: &[&str], index: u32) -> Option<(usize, &Node)> {
        let mut node = &self.root;
        let mut at = self.values_at as u64;
        let mut applied = false;

        for name in path {
            let child = node.child(name)?;
            at += child.offset as u64;
            if !applied && child.arraysize > 1 {
                if index >= child.arraysize {
                    return None;
                }
                at += (child.datasize as u64) * (index as u64);
                applied = true;
            }
            node = child;
        }
        Some((at as usize, node))
    }

    /// Read a string parameter.
    pub fn string(&self, path: &[&str], index: u32) -> Option<String> {
        let (at, node) = self.locate(path, index)?;
        let end = at.checked_add(node.datasize as usize)?;
        self.data.get(at..end).map(cstr)
    }

    /// Read an unsigned integer parameter of whatever width the schema declares.
    pub fn uint(&self, path: &[&str], index: u32) -> Option<u64> {
        let (at, node) = self.locate(path, index)?;
        let width = node.datasize as usize;
        if width == 0 || width > 8 {
            return None;
        }
        let bytes = self.data.get(at..at.checked_add(width)?)?;
        let mut v: u64 = 0;
        for (i, b) in bytes.iter().enumerate() {
            v |= (*b as u64) << (8 * i); // little-endian
        }
        Some(v)
    }

    /// A top-level strip collection and how many elements it has.
    pub fn strip_table(&self, name: &str) -> Option<u32> {
        self.root.child(name).map(|n| n.arraysize)
    }

    /// Total bytes the schema says the root occupies.
    pub fn declared_size(&self) -> u32 {
        self.root.datasize
    }

    /// Bytes actually available for values.
    pub fn value_bytes(&self) -> usize {
        self.data.len().saturating_sub(self.values_at)
    }
}

#[cfg(test)]
pub(crate) mod build {
    //! Synthesises MMSXLIT payloads for tests, to the documented layout.

    pub fn col(name: &str, offset: u32, datasize: u32, arraysize: u32) -> Vec<u8> {
        let mut r = vec![0u8; 48];
        r[..3].copy_from_slice(b"COL");
        r[3] = b'0';
        r[4..4 + name.len()].copy_from_slice(name.as_bytes());
        r[36..40].copy_from_slice(&offset.to_le_bytes());
        r[40..44].copy_from_slice(&datasize.to_le_bytes());
        r[44..48].copy_from_slice(&arraysize.to_le_bytes());
        r
    }

    pub fn pr(name: &str, type_code: u8, size: u16, arraysize: u16) -> Vec<u8> {
        let mut r = vec![0u8; 32];
        r[..3].copy_from_slice(b"PR ");
        r[3] = type_code;
        r[4..6].copy_from_slice(&size.to_le_bytes());
        r[6..8].copy_from_slice(&arraysize.to_le_bytes());
        r[8..8 + name.len()].copy_from_slice(name.as_bytes());
        r
    }

    pub fn payload(function: &str, schema: Vec<Vec<u8>>, values: &[u8]) -> Vec<u8> {
        let mut out = vec![0u8; 44];
        out[..7].copy_from_slice(b"MMSXLIT");
        out[8..8 + function.len()].copy_from_slice(function.as_bytes());
        for r in schema {
            out.extend_from_slice(&r);
        }
        out.extend_from_slice(values);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::build::*;
    use super::*;

    /// Two channels of {Name[8], Source u32}, i.e. datasize 12 each.
    fn sample() -> Vec<u8> {
        let schema = vec![
            col("Mixing", 0, 24, 1),
            col("InputChannel", 0, 12, 2),
            col("Label", 0, 8, 1),
            pr("Name", TYPE_STRING, 8, 1),
            col("Patch", 8, 4, 1),
            pr("Source", 0x02, 4, 1),
        ];
        let mut values = Vec::new();
        values.extend_from_slice(b"Kick\0\0\0\0");
        values.extend_from_slice(&0x0140_0000u32.to_le_bytes());
        values.extend_from_slice(b"Snare\0\0\0");
        values.extend_from_slice(&0x0140_0001u32.to_le_bytes());
        payload("Mixing", schema, &values)
    }

    #[test]
    fn rebuilds_the_tree_and_finds_the_value_block() {
        let p = Payload::parse(&sample()).expect("parse");
        assert_eq!(p.function, "Mixing");
        assert_eq!(p.collections, 4);
        assert_eq!(p.parameters, 2);
        assert_eq!(p.strip_table("InputChannel"), Some(2));
    }

    #[test]
    fn children_sum_to_the_declared_parent_size() {
        let p = Payload::parse(&sample()).unwrap();
        let ic = p.root.child("InputChannel").unwrap();
        let summed: u32 = ic.children().iter().map(|c| c.span()).sum();
        assert_eq!(summed, ic.datasize, "InputChannel children do not fill it");
    }

    #[test]
    fn reads_strings_and_integers_across_array_elements() {
        let p = Payload::parse(&sample()).unwrap();
        assert_eq!(p.string(&["InputChannel", "Label", "Name"], 0).as_deref(), Some("Kick"));
        assert_eq!(p.string(&["InputChannel", "Label", "Name"], 1).as_deref(), Some("Snare"));
        assert_eq!(p.uint(&["InputChannel", "Patch", "Source"], 0), Some(0x0140_0000));
        assert_eq!(p.uint(&["InputChannel", "Patch", "Source"], 1), Some(0x0140_0001));
    }

    #[test]
    fn out_of_range_index_returns_none_rather_than_neighbouring_data() {
        let p = Payload::parse(&sample()).unwrap();
        assert_eq!(p.string(&["InputChannel", "Label", "Name"], 2), None);
        assert_eq!(p.string(&["InputChannel", "Label", "Nope"], 0), None);
        assert_eq!(p.string(&["Nope"], 0), None);
    }

    #[test]
    fn rejects_a_non_mms_payload() {
        assert!(matches!(Payload::parse(b"not mms at all"), Err(YamahaError::NotMms)));
    }

    #[test]
    fn rejects_a_schema_that_never_terminates() {
        // A collection claiming more bytes than its children provide would loop
        // forever without the progress guard.
        let schema = vec![col("Mixing", 0, 9999, 1), pr("X", TYPE_STRING, 4, 1)];
        assert!(Payload::parse(&payload("Mixing", schema, b"aaaa")).is_err());
    }

    #[test]
    fn truncated_payloads_do_not_panic() {
        let full = sample();
        for cut in (0..full.len()).step_by(7) {
            let _ = Payload::parse(&full[..cut]);
        }
    }

    #[test]
    fn reading_past_the_value_block_returns_none() {
        // Schema promises two channels; only one channel of values supplied.
        let schema = vec![
            col("Mixing", 0, 24, 1),
            col("InputChannel", 0, 12, 2),
            col("Label", 0, 8, 1),
            pr("Name", TYPE_STRING, 8, 1),
            col("Patch", 8, 4, 1),
            pr("Source", 0x02, 4, 1),
        ];
        let p = Payload::parse(&payload("Mixing", schema, b"Kick\0\0\0\0\0\0\0\0")).unwrap();
        assert_eq!(p.string(&["InputChannel", "Label", "Name"], 0).as_deref(), Some("Kick"));
        assert_eq!(p.string(&["InputChannel", "Label", "Name"], 1), None);
    }
}
