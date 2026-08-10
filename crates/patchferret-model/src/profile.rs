//! Console capability profiles.
//!
//! A [`ConsoleProfile`] describes what a desk *can do*, independent of any
//! particular show. It exists so that conforming a show from one console to
//! another is a data problem rather than an N×N matrix of bespoke converters:
//! the conform pass reads the source show and the target profile, and reports
//! what does not fit.
//!
//! Nothing in this module performs a conversion yet — the profiles are the
//! prerequisite, and are populated here so the shape is fixed before adapters
//! start depending on it.

use crate::Transport;

/// A physical I/O box or card a console can have attached.
#[derive(Debug, Clone, PartialEq)]
pub struct DeviceSpec {
    pub id: String,
    pub label: String,
    pub transport: Transport,
    pub inputs: u16,
    pub outputs: u16,
    /// Whether the preamps on this device are remotely controllable.
    pub remote_gain: bool,
}

/// What a console offers.
#[derive(Debug, Clone, PartialEq)]
pub struct ConsoleProfile {
    /// Slug matching the adapter id where one exists, e.g. `x32`.
    pub id: String,
    pub vendor: String,
    pub model: String,

    pub input_channels: u16,
    pub aux_ins: u16,
    pub fx_returns: u16,
    pub buses: u16,
    pub matrices: u16,
    pub mains: u16,
    pub dcas: u16,
    pub mute_groups: u16,

    /// Parametric EQ bands on an input channel.
    pub eq_bands_input: u8,
    /// Parametric EQ bands on a bus/matrix.
    pub eq_bands_bus: u8,
    pub has_gate: bool,
    pub has_compressor: bool,
    /// Total remotely-controllable preamps.
    pub head_amps: u16,

    pub devices: Vec<DeviceSpec>,
}

impl ConsoleProfile {
    /// Highest strip number available for a given kind.
    pub fn capacity(&self, kind: crate::StripKind) -> u16 {
        use crate::StripKind::*;
        match kind {
            Input => self.input_channels,
            AuxIn => self.aux_ins,
            FxReturn => self.fx_returns,
            Bus => self.buses,
            Matrix => self.matrices,
            Main => self.mains,
            Mono => 1,
            Dca => self.dcas,
            MuteGroup => self.mute_groups,
        }
    }
}

/// The Behringer X32 / Midas M32 full-size console.
///
/// Figures are from the published specifications, not measured from a file.
pub fn x32() -> ConsoleProfile {
    ConsoleProfile {
        id: "x32".into(),
        vendor: "Behringer".into(),
        model: "X32".into(),
        input_channels: 32,
        aux_ins: 8,
        fx_returns: 8,
        buses: 16,
        matrices: 6,
        mains: 1,
        dcas: 8,
        mute_groups: 6,
        eq_bands_input: 4,
        eq_bands_bus: 6,
        has_gate: true,
        has_compressor: true,
        head_amps: 128,
        devices: vec![
            DeviceSpec {
                id: "local".into(),
                label: "Console local I/O".into(),
                transport: Transport::Local,
                inputs: 32,
                outputs: 16,
                remote_gain: true,
            },
            DeviceSpec {
                id: "aes50a".into(),
                label: "AES50 A".into(),
                transport: Transport::Aes50A,
                inputs: 48,
                outputs: 48,
                remote_gain: true,
            },
            DeviceSpec {
                id: "aes50b".into(),
                label: "AES50 B".into(),
                transport: Transport::Aes50B,
                inputs: 48,
                outputs: 48,
                remote_gain: true,
            },
            DeviceSpec {
                id: "card".into(),
                label: "Expansion card".into(),
                transport: Transport::Card("X-UF".into()),
                inputs: 32,
                outputs: 32,
                remote_gain: false,
            },
        ],
    }
}

/// Every profile PatchFerret currently knows.
pub fn all() -> Vec<ConsoleProfile> {
    vec![x32()]
}

/// Look a profile up by id.
pub fn by_id(id: &str) -> Option<ConsoleProfile> {
    all().into_iter().find(|p| p.id == id)
}
