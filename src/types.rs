use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use uuid::Uuid;

/// Room identifier
pub type RoomId = Uuid;

/// Device identifier
pub type DeviceId = String;

/// Position identifier (speaker position in room)
pub type PositionId = String;

/// Gain value in decibels
pub type GainValue = f64;

/// Mute state
pub type MuteState = bool;

// RoomDocument is now merged into Room struct - this type is kept for backward compatibility
// but not used internally anymore

/// Device information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Device {
    pub name: String,

    /// Product tags (e.g., "8c", "subwoofer")
    #[serde(default)]
    pub tags: Vec<String>,

    /// Licensed features
    #[serde(default)]
    pub licenses: Vec<String>,
}

/// Gain data structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GainData {
    /// Global gain value in dB
    pub global: f64,

    /// Gain limits
    #[serde(default)]
    pub limits: GainLimits,
}

impl GainData {
    /// Get the minimum allowed gain value
    pub fn min(&self) -> f64 {
        self.limits.min
    }

    /// Get the maximum allowed gain value
    pub fn max(&self) -> f64 {
        self.limits.max
    }

    /// Get the gain adjustment step size
    pub fn step(&self) -> f64 {
        self.limits.step
    }
}

/// Gain limits
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GainLimits {
    #[serde(default = "default_min")]
    pub min: f64,
    #[serde(default)]
    pub max: f64,
    #[serde(default = "default_step")]
    pub step: f64,
}

fn default_min() -> f64 {
    -80.0
}

fn default_step() -> f64 {
    0.5
}

/// Mute data structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MuteData {
    /// Global mute state
    pub global: bool,

    /// Per-position mute states
    #[serde(flatten)]
    pub positions: BTreeMap<String, bool>,
}

impl MuteData {
    /// Get the mute state for a specific position
    pub fn position(&self, position_id: &str) -> Option<bool> {
        self.positions.get(position_id).copied()
    }

    /// Get all position IDs that have mute state
    pub fn position_ids(&self) -> Vec<String> {
        self.positions.keys().cloned().collect()
    }

    /// Check if any position is muted (regardless of global state)
    pub fn any_position_muted(&self) -> bool {
        self.positions.values().any(|&muted| muted)
    }
}

/// Voicing profile
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoicingProfile {
    pub name: String,
    pub sub: f64,
    pub bass: f64,
    pub treble: f64,
    #[serde(default)]
    #[serde(rename = "paramEQ")]
    pub param_eq: BTreeMap<String, serde_json::Value>,
}

/// Tone control settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToneSettings {
    /// Subwoofer gain adjustment
    pub sub: f64,

    /// Midrange gain adjustment
    pub mid: f64,

    /// Treble gain adjustment
    pub treble: f64,
}

/// Preset configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Preset {
    pub name: String,
    #[serde(default)]
    pub description: String,

    /// Preset settings
    #[serde(default)]
    pub settings: BTreeMap<String, serde_json::Value>,

    /// Whether this is a read-only preset
    #[serde(default)]
    pub readonly: bool,
}

/// Channel mapping configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelMapping {
    /// Mapping from input channels to output gains
    #[serde(flatten)]
    pub channels: BTreeMap<String, ChannelGains>,
}

/// Gains for left and right channels
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelGains {
    pub left: f64,
    pub right: f64,
}

/// Streaming API method definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamingApiMethod {
    /// Method arguments (parameter types/names)
    #[serde(default)]
    pub arguments: Vec<serde_json::Value>,

    /// Return value types
    #[serde(default)]
    #[serde(rename = "returnValues")]
    pub return_values: Vec<serde_json::Value>,

    /// Whether this method can be called
    #[serde(default)]
    pub callable: bool,
}

/// Streaming API definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamingApi {
    /// Available methods (play, pause, next, previous, etc.)
    #[serde(default)]
    pub methods: BTreeMap<String, StreamingApiMethod>,
}

/// Streaming information for now playing content
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(from = "StreamingInfoRaw")]
pub struct StreamingInfo {
    /// Service name (e.g., "Roon", "AirPlay", "Spotify Connect")
    pub service_name: String,

    /// Display lines (contains track info like title, artist, album)
    pub display: Vec<String>,

    /// Whether currently playing
    pub is_playing: bool,

    /// Track length in seconds (converted from milliseconds)
    pub track_length: f64,

    /// Current track position in seconds (converted from nanoseconds)
    pub track_position: f64,

    /// Repeat mode
    pub repeat: String,

    /// Shuffle state
    pub shuffle: bool,

    /// Available API methods
    pub api: Option<StreamingApi>,
}

/// Raw streaming info from API (before conversion)
#[derive(Debug, Clone, Deserialize)]
struct StreamingInfoRaw {
    #[serde(default)]
    #[serde(rename = "serviceName")]
    service_name: String,

    #[serde(default)]
    display: Vec<String>,

    #[serde(default)]
    is_playing: bool,

    #[serde(default)]
    track_length: f64,

    #[serde(default)]
    track_position: f64,

    #[serde(default)]
    repeat: String,

    #[serde(default)]
    shuffle: bool,

    #[serde(default)]
    api: Option<StreamingApi>,
}

impl From<StreamingInfoRaw> for StreamingInfo {
    fn from(raw: StreamingInfoRaw) -> Self {
        Self {
            service_name: raw.service_name,
            display: raw.display,
            is_playing: raw.is_playing,
            // Convert from milliseconds to seconds
            track_length: raw.track_length / 1000.0,
            // Convert from nanoseconds to seconds
            track_position: raw.track_position / 1_000_000_000.0,
            repeat: raw.repeat,
            shuffle: raw.shuffle,
            api: raw.api,
        }
    }
}
