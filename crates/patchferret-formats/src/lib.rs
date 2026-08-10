//! Format adapters: native console show files in, [`Show`] out.
//!
//! Adding a console means adding one module that implements [`ShowAdapter`] and
//! one line in [`adapters`]. Nothing else in PatchFerret changes — the reports,
//! the XML, the WASM surface and the CLI all work from the model alone.
//!
//! # Why the input is a bundle
//!
//! [`ShowInput`] carries a *list* of files, not one file, because show files
//! are not consistently single files. A Behringer `.scn` is one text file, but
//! an Allen & Heath SQ show is a folder of `SHOW.DAT` + `NVDATA.DAT`, and
//! DiGiCo sessions are directory trees. Modelling the single-file case only
//! would force a redesign at exactly the point the format work gets hard.

pub mod x32;

use patchferret_model::Show;

#[derive(Debug, thiserror::Error)]
pub enum AdapterError {
    #[error("no adapter recognised this show file")]
    Unrecognised,
    #[error("{adapter}: {message}")]
    Parse { adapter: &'static str, message: String },
    #[error("expected file {0} not present in the show bundle")]
    MissingFile(String),
}

/// One file within a show bundle.
#[derive(Debug, Clone)]
pub struct ShowFile {
    /// Path relative to the bundle root, e.g. `SHOW.DAT` or `mix.scn`.
    pub path: String,
    pub bytes: Vec<u8>,
}

impl ShowFile {
    pub fn new(path: impl Into<String>, bytes: Vec<u8>) -> Self {
        Self { path: path.into(), bytes }
    }

    /// Lossy UTF-8 view. Console show files are frequently not clean UTF-8
    /// (latin-1 channel names are common), and refusing to parse the whole
    /// show because one channel is called "Café" would be useless behaviour.
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.bytes).into_owned()
    }

    pub fn extension(&self) -> String {
        self.path.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default()
    }

    pub fn file_name(&self) -> &str {
        self.path.rsplit('/').next().unwrap_or(&self.path)
    }
}

/// A show as presented to the adapters.
#[derive(Debug, Clone)]
pub struct ShowInput {
    /// Display name — the uploaded file or folder name.
    pub name: String,
    pub files: Vec<ShowFile>,
}

impl ShowInput {
    /// A single-file show.
    pub fn single(name: impl Into<String>, bytes: Vec<u8>) -> Self {
        let name = name.into();
        Self { files: vec![ShowFile::new(name.clone(), bytes)], name }
    }

    pub fn bundle(name: impl Into<String>, files: Vec<ShowFile>) -> Self {
        Self { name: name.into(), files }
    }

    /// Find a file by case-insensitive base name.
    pub fn find(&self, file_name: &str) -> Option<&ShowFile> {
        self.files.iter().find(|f| f.file_name().eq_ignore_ascii_case(file_name))
    }

    /// The first file, which is the whole show for single-file formats.
    pub fn primary(&self) -> Option<&ShowFile> {
        self.files.first()
    }
}

/// How sure an adapter is that it can read an input.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Confidence {
    /// Definitely not this format.
    No,
    /// Extension or name matches, but the content was not checked or is odd.
    Weak,
    /// Content carries this format's signature.
    Strong,
}

/// A console show file format.
pub trait ShowAdapter {
    /// Stable slug, used as `source-format` in PFX and in the CLI.
    fn id(&self) -> &'static str;

    /// Human name for the UI.
    fn display_name(&self) -> &'static str;

    /// Extensions this adapter claims, lowercase and without the dot.
    fn extensions(&self) -> &'static [&'static str];

    /// Cheap content check. Must not panic on arbitrary bytes.
    fn sniff(&self, input: &ShowInput) -> Confidence;

    fn parse(&self, input: &ShowInput) -> Result<Show, AdapterError>;
}

/// Every adapter compiled into this build.
pub fn adapters() -> Vec<Box<dyn ShowAdapter>> {
    vec![Box::new(x32::X32Adapter)]
}

/// Pick the adapter most confident about this input.
pub fn detect(input: &ShowInput) -> Option<(Box<dyn ShowAdapter>, Confidence)> {
    let mut best: Option<(Box<dyn ShowAdapter>, Confidence)> = None;
    for a in adapters() {
        let c = a.sniff(input);
        if c == Confidence::No {
            continue;
        }
        if best.as_ref().map(|(_, bc)| c > *bc).unwrap_or(true) {
            best = Some((a, c));
        }
    }
    best
}

/// Detect the format and parse in one step.
pub fn parse_auto(input: &ShowInput) -> Result<Show, AdapterError> {
    let (adapter, _) = detect(input).ok_or(AdapterError::Unrecognised)?;
    adapter.parse(input)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unrecognised_input_is_an_error_not_a_panic() {
        let input = ShowInput::single("random.bin", vec![0xff, 0x00, 0xfe, 0x7f]);
        assert!(matches!(parse_auto(&input), Err(AdapterError::Unrecognised)));
    }

    #[test]
    fn sniffing_empty_input_does_not_panic() {
        let input = ShowInput::bundle("empty", vec![]);
        for a in adapters() {
            assert_eq!(a.sniff(&input), Confidence::No, "{} sniffed an empty bundle", a.id());
        }
    }

    #[test]
    fn adapter_ids_are_unique() {
        let mut ids: Vec<_> = adapters().iter().map(|a| a.id()).collect();
        ids.sort_unstable();
        let before = ids.len();
        ids.dedup();
        assert_eq!(before, ids.len(), "duplicate adapter id");
    }
}
