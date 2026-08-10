//! PFX — the PatchFerret eXchange model.
//!
//! A console-neutral description of a mixing console show file. Every format
//! adapter parses its native show file into this model; every report and every
//! exporter reads only this model. Adapters never talk to each other, and
//! nothing downstream of here knows what a `.scn` file is.
//!
//! The model is deliberately *lossy in a declared way*: anything an adapter
//! understood but this model cannot express is recorded as a [`Diagnostic`]
//! rather than silently dropped. A show that round-trips with no diagnostics is
//! one we can claim to have fully understood; anything else is honest about the
//! gap. See `docs/01-model.md`.

pub mod profile;
pub mod xml;

use std::fmt;

pub use profile::{ConsoleProfile, DeviceSpec};

/// Signal direction, from the console's point of view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Direction {
    In,
    Out,
}

impl Direction {
    pub fn as_str(self) -> &'static str {
        match self {
            Direction::In => "in",
            Direction::Out => "out",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "in" => Some(Direction::In),
            "out" => Some(Direction::Out),
            _ => None,
        }
    }
}

/// How a device is attached to the console.
///
/// This is the physical/transport layer, not the connector type — an AES50
/// stagebox and a Dante card both carry XLR-originated signals, but a patch
/// list has to distinguish them because they fail differently and are cabled
/// differently.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Transport {
    /// Connectors on the console surface itself.
    Local,
    Aes50A,
    Aes50B,
    Aes50C,
    /// Expansion card slot (Dante, MADI, USB, …) — `detail` names the card.
    Card(String),
    Dante,
    Madi,
    /// Allen & Heath SLink / dSNAKE.
    SLink,
    /// Behringer Ultranet personal-monitor bus.
    Ultranet,
    /// USB / SD recording and playback.
    Recorder,
    /// Not a physical connector: internal FX slots, oscillator, talkback.
    Internal,
    Other(String),
}

impl Transport {
    pub fn as_str(&self) -> String {
        match self {
            Transport::Local => "local".into(),
            Transport::Aes50A => "aes50a".into(),
            Transport::Aes50B => "aes50b".into(),
            Transport::Aes50C => "aes50c".into(),
            Transport::Card(c) => format!("card:{c}"),
            Transport::Dante => "dante".into(),
            Transport::Madi => "madi".into(),
            Transport::SLink => "slink".into(),
            Transport::Ultranet => "ultranet".into(),
            Transport::Recorder => "recorder".into(),
            Transport::Internal => "internal".into(),
            Transport::Other(o) => format!("other:{o}"),
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "local" => Transport::Local,
            "aes50a" => Transport::Aes50A,
            "aes50b" => Transport::Aes50B,
            "aes50c" => Transport::Aes50C,
            "dante" => Transport::Dante,
            "madi" => Transport::Madi,
            "slink" => Transport::SLink,
            "ultranet" => Transport::Ultranet,
            "recorder" => Transport::Recorder,
            "internal" => Transport::Internal,
            other => match other.split_once(':') {
                Some(("card", c)) => Transport::Card(c.to_string()),
                Some(("other", o)) => Transport::Other(o.to_string()),
                _ => Transport::Other(other.to_string()),
            },
        }
    }
}

/// A physical I/O device: the console's local connectors, a stagebox, a card.
#[derive(Debug, Clone, PartialEq)]
pub struct Device {
    /// Stable slug used by [`SocketRef`] — e.g. `local`, `aes50a-sb1`.
    pub id: String,
    /// Human label for the patch list — e.g. "Stage Left S16".
    pub label: String,
    /// Model, where the show file names one.
    pub model: Option<String>,
    pub transport: Transport,
    pub inputs: u16,
    pub outputs: u16,
}

/// A physical connector on a [`Device`].
///
/// `index` is 1-based, matching what is silkscreened on the box rather than any
/// internal numbering — every adapter is responsible for that conversion.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SocketRef {
    pub device: String,
    pub dir: Direction,
    pub index: u16,
}

impl SocketRef {
    pub fn new(device: impl Into<String>, dir: Direction, index: u16) -> Self {
        Self { device: device.into(), dir, index }
    }
}

impl fmt::Display for SocketRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}/{}", self.device, self.dir.as_str(), self.index)
    }
}

impl SocketRef {
    /// Parse the `device/dir/index` form written by [`fmt::Display`].
    pub fn parse(s: &str) -> Option<Self> {
        let mut parts = s.rsplitn(3, '/');
        let index = parts.next()?.parse().ok()?;
        let dir = Direction::parse(parts.next()?)?;
        let device = parts.next()?.to_string();
        Some(Self { device, dir, index })
    }
}

/// What kind of mix strip this is.
///
/// Kept coarse on purpose. Consoles disagree wildly on the group/bus/aux
/// distinction, so PFX records the *role* and leaves vendor naming to `label`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum StripKind {
    /// Mic/line input channel.
    Input,
    /// Stereo/aux return fed from playback or an external source.
    AuxIn,
    /// Return from an internal effect.
    FxReturn,
    /// Mix bus — aux send or subgroup.
    Bus,
    /// Matrix.
    Matrix,
    /// Main L/R.
    Main,
    /// Main mono / centre.
    Mono,
    /// DCA / VCA.
    Dca,
    MuteGroup,
}

impl StripKind {
    pub fn as_str(self) -> &'static str {
        match self {
            StripKind::Input => "input",
            StripKind::AuxIn => "auxin",
            StripKind::FxReturn => "fxreturn",
            StripKind::Bus => "bus",
            StripKind::Matrix => "matrix",
            StripKind::Main => "main",
            StripKind::Mono => "mono",
            StripKind::Dca => "dca",
            StripKind::MuteGroup => "mutegroup",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "input" => StripKind::Input,
            "auxin" => StripKind::AuxIn,
            "fxreturn" => StripKind::FxReturn,
            "bus" => StripKind::Bus,
            "matrix" => StripKind::Matrix,
            "main" => StripKind::Main,
            "mono" => StripKind::Mono,
            "dca" => StripKind::Dca,
            "mutegroup" => StripKind::MuteGroup,
            _ => return None,
        })
    }
}

/// Identity of a strip: its kind plus its 1-based number on the surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct StripId {
    pub kind: StripKind,
    pub index: u16,
}

impl StripId {
    pub fn new(kind: StripKind, index: u16) -> Self {
        Self { kind, index }
    }
}

impl fmt::Display for StripId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.kind.as_str(), self.index)
    }
}

impl StripId {
    pub fn parse(s: &str) -> Option<Self> {
        let (kind, index) = s.rsplit_once('/')?;
        Some(Self { kind: StripKind::parse(kind)?, index: index.parse().ok()? })
    }
}

/// Where a signal comes from.
///
/// `InputSlot` is the load-bearing variant: most consoles put a re-patchable
/// indirection between the physical connector and the channel, and collapsing
/// that away would make the patch list a guess. PatchFerret keeps both the slot
/// and the socket it currently resolves to.
#[derive(Debug, Clone, PartialEq)]
pub enum SignalRef {
    /// Nothing patched.
    Off,
    /// Console input slot number (1-based), before channel assignment.
    InputSlot(u16),
    /// A physical connector.
    Socket(SocketRef),
    /// A tap off another strip.
    Strip(StripId),
    /// Understood as a name but not resolvable to the above.
    Named(String),
}

impl SignalRef {
    pub fn as_str(&self) -> String {
        match self {
            SignalRef::Off => "off".into(),
            SignalRef::InputSlot(n) => format!("slot:{n}"),
            SignalRef::Socket(s) => format!("socket:{s}"),
            SignalRef::Strip(s) => format!("strip:{s}"),
            SignalRef::Named(n) => format!("named:{n}"),
        }
    }

    pub fn parse(s: &str) -> Self {
        match s.split_once(':') {
            Some(("slot", n)) => n.parse().map(SignalRef::InputSlot).unwrap_or(SignalRef::Off),
            Some(("socket", r)) => {
                SocketRef::parse(r).map(SignalRef::Socket).unwrap_or(SignalRef::Off)
            }
            Some(("strip", r)) => {
                StripId::parse(r).map(SignalRef::Strip).unwrap_or(SignalRef::Off)
            }
            Some(("named", n)) => SignalRef::Named(n.to_string()),
            _ => SignalRef::Off,
        }
    }
}

/// Where in a strip's signal path an output is tapped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tap {
    PreEq,
    PostEq,
    PreFader,
    PostFader,
    Unknown,
}

impl Tap {
    pub fn as_str(self) -> &'static str {
        match self {
            Tap::PreEq => "pre-eq",
            Tap::PostEq => "post-eq",
            Tap::PreFader => "pre-fader",
            Tap::PostFader => "post-fader",
            Tap::Unknown => "unknown",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "pre-eq" => Tap::PreEq,
            "post-eq" => Tap::PostEq,
            "pre-fader" => Tap::PreFader,
            "post-fader" => Tap::PostFader,
            _ => Tap::Unknown,
        }
    }
}

/// Preamp state.
///
/// Deliberately attached to the *socket*, not the channel. On a shared
/// stagebox the gain belongs to the connector and is shared by every console
/// listening to it — modelling it per-channel is the classic way to produce a
/// patch list that lies about gain sharing.
#[derive(Debug, Clone, PartialEq)]
pub struct HeadAmp {
    pub socket: SocketRef,
    pub gain_db: Option<f32>,
    pub phantom: bool,
    pub pad: bool,
    pub polarity_invert: bool,
}

/// A logical mix strip.
#[derive(Debug, Clone, PartialEq)]
pub struct Strip {
    pub id: StripId,
    pub name: String,
    /// Vendor colour name/index, verbatim.
    pub colour: Option<String>,
    /// Vendor icon identifier, verbatim.
    pub icon: Option<String>,
    /// What feeds this strip.
    pub source: SignalRef,
    pub muted: bool,
    pub fader_db: Option<f32>,
    /// Index of the strip this one is stereo-linked to, if any.
    pub linked_to: Option<StripId>,
}

impl Strip {
    pub fn new(id: StripId) -> Self {
        Self {
            id,
            name: String::new(),
            colour: None,
            icon: None,
            source: SignalRef::Off,
            muted: false,
            fader_db: None,
            linked_to: None,
        }
    }

    /// Name for display, falling back to the strip's number when unnamed.
    pub fn display_name(&self) -> String {
        if self.name.trim().is_empty() {
            format!("{} {}", self.id.kind.as_str(), self.id.index)
        } else {
            self.name.clone()
        }
    }
}

/// One entry in the input patch: connector → slot → strip.
#[derive(Debug, Clone, PartialEq)]
pub struct InputPatch {
    /// 1-based console input slot.
    pub slot: u16,
    /// The vendor's own label for the routing block, kept for traceability.
    pub block_label: String,
    /// The physical connector currently feeding this slot.
    pub socket: Option<SocketRef>,
    /// The strip currently taking this slot, if any.
    pub strip: Option<StripId>,
}

/// One entry in the output patch: strip/signal → connector.
#[derive(Debug, Clone, PartialEq)]
pub struct OutputPatch {
    pub socket: SocketRef,
    pub source: SignalRef,
    pub tap: Tap,
    /// Vendor's own name for the source, kept verbatim where we could not
    /// resolve it to a `SignalRef`.
    pub source_label: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Patch {
    pub inputs: Vec<InputPatch>,
    pub outputs: Vec<OutputPatch>,
}

/// A stored scene/snapshot within the show.
#[derive(Debug, Clone, PartialEq)]
pub struct Scene {
    pub index: u16,
    pub name: String,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    /// Understood, but PFX has no field for it — will not survive conversion.
    Unmodelled,
    /// Recognised but the value made no sense.
    Suspect,
    /// Not recognised at all.
    Unknown,
}

impl Severity {
    pub fn as_str(self) -> &'static str {
        match self {
            Severity::Unmodelled => "unmodelled",
            Severity::Suspect => "suspect",
            Severity::Unknown => "unknown",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "unmodelled" => Severity::Unmodelled,
            "suspect" => Severity::Suspect,
            _ => Severity::Unknown,
        }
    }
}

/// Something the adapter could not fully carry into the model.
///
/// These are a first-class output, not a debug aid: the fidelity section of
/// every report is built from them, so a user can see exactly what would be
/// lost before they trust a conversion.
#[derive(Debug, Clone, PartialEq)]
pub struct Diagnostic {
    pub severity: Severity,
    /// Where in the source file — a path, line number, or token.
    pub locus: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct ShowMeta {
    pub name: String,
    pub note: Option<String>,
    /// Adapter id that produced this model, e.g. `x32`.
    pub source_format: String,
    /// Vendor/model as identified from the file.
    pub console: String,
    /// Show file format version string, verbatim.
    pub format_version: Option<String>,
}

/// A whole show, console-neutral.
#[derive(Debug, Clone, PartialEq)]
pub struct Show {
    pub meta: ShowMeta,
    pub devices: Vec<Device>,
    pub head_amps: Vec<HeadAmp>,
    pub strips: Vec<Strip>,
    pub patch: Patch,
    pub scenes: Vec<Scene>,
    pub diagnostics: Vec<Diagnostic>,
}

impl Default for Show {
    fn default() -> Self {
        Self {
            meta: ShowMeta::default(),
            devices: Vec::new(),
            head_amps: Vec::new(),
            strips: Vec::new(),
            patch: Patch { inputs: Vec::new(), outputs: Vec::new() },
            scenes: Vec::new(),
            diagnostics: Vec::new(),
        }
    }
}

impl Show {
    pub fn strip(&self, id: StripId) -> Option<&Strip> {
        self.strips.iter().find(|s| s.id == id)
    }

    pub fn device(&self, id: &str) -> Option<&Device> {
        self.devices.iter().find(|d| d.id == id)
    }

    pub fn head_amp(&self, socket: &SocketRef) -> Option<&HeadAmp> {
        self.head_amps.iter().find(|h| &h.socket == socket)
    }

    pub fn strips_of(&self, kind: StripKind) -> impl Iterator<Item = &Strip> {
        self.strips.iter().filter(move |s| s.id.kind == kind)
    }

    /// Input patch rows that actually reach a strip, in slot order.
    ///
    /// This is the join the patch-list report is built from, and the reason the
    /// slot indirection is kept in the model rather than resolved at parse time.
    pub fn patched_inputs(&self) -> Vec<(&InputPatch, Option<&Strip>)> {
        let mut rows: Vec<_> = self
            .patch
            .inputs
            .iter()
            .map(|p| (p, p.strip.and_then(|id| self.strip(id))))
            .collect();
        rows.sort_by_key(|(p, _)| p.slot);
        rows
    }
}
