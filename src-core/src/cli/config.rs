use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

use crate::dsp::UkfParams;
use crate::{OldZoneType, osc::types::ZoneEvent};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct UkfConfig {
    pub q: f32,
    pub r: f32,
    pub alpha: f32,
    pub beta: f32,
    pub kappa: f32,
}

impl Default for UkfConfig {
    fn default() -> Self {
        let p = UkfParams::default();
        Self {
            q: p.q,
            r: p.r,
            alpha: p.alpha,
            beta: p.beta,
            kappa: p.kappa,
        }
    }
}

impl From<UkfConfig> for UkfParams {
    fn from(c: UkfConfig) -> Self {
        UkfParams {
            q: c.q,
            r: c.r,
            alpha: c.alpha,
            beta: c.beta,
            kappa: c.kappa,
        }
    }
}

impl From<UkfParams> for UkfConfig {
    fn from(p: UkfParams) -> Self {
        UkfConfig {
            q: p.q,
            r: p.r,
            alpha: p.alpha,
            beta: p.beta,
            kappa: p.kappa,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, Default)]
pub enum ContactMode {
    #[default]
    Depth,
    Speed,
    Acc,
    Recoil,
}

impl fmt::Display for ContactMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ContactMode::Depth => write!(f, "depth"),
            ContactMode::Speed => write!(f, "speed"),
            ContactMode::Acc => write!(f, "acc"),
            ContactMode::Recoil => write!(f, "recoil"),
        }
    }
}

impl FromStr for ContactMode {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "depth" | "d" | "raw" | "level" => Ok(ContactMode::Depth),
            "speed" | "s" | "vel" | "velocity" => Ok(ContactMode::Speed),
            "acc" | "a" | "accel" | "acceleration" => Ok(ContactMode::Acc),
            "recoil" | "r" | "pullout" | "retract" => Ok(ContactMode::Recoil),
            _ => Err(format!("'{s}' is not a valid ContactMode (depth|speed|acc|recoil)")),
        }
    }
}

/// Identifies an OSC zone by its type (Pen/Orf/Touch/DGB) and name.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct ZoneId {
    pub zone_type: OldZoneType,
    pub name: String,
}

impl ZoneId {
    pub fn new(zone_type: OldZoneType, name: impl Into<String>) -> Self {
        Self {
            zone_type: zone_type,
            name: name.into(),
        }
    }

    pub fn from_event(event: &ZoneEvent) -> Self {
        Self {
            zone_type: event.zone_type,
            name: event.id.clone(),
        }
    }

    pub fn is_wildcard(&self) -> bool {
        self.zone_type == OldZoneType::Any || self.name == "*"
    }

    pub fn matches(&self, other: &ZoneId) -> bool {
        let type_ok = self.zone_type == OldZoneType::Any || self.zone_type == other.zone_type;
        let name_ok = self.name == "*" || self.name == other.name;
        type_ok && name_ok
    }

    pub fn matches_event(&self, event: &ZoneEvent) -> bool {
        self.matches(&ZoneId::from_event(event))
    }
}

impl fmt::Display for ZoneId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.zone_type, self.name)
    }
}

/// Minimum and maximum strength values (0–200) for a channel.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PowerLimits {
    pub min: u8,
    pub max: u8,
}

impl Default for PowerLimits {
    fn default() -> Self {
        Self { min: 0, max: 100 }
    }
}

impl PowerLimits {
    pub fn new(min: u8, max: u8) -> Self {
        Self {
            min: min.min(200),
            max: max.min(200).max(min.min(200)),
        }
    }

    pub fn scale(&self, level: f32) -> u8 {
        if level <= 0.0 {
            return 0;
        }
        let level = level.clamp(0.0, 1.0);
        let range = self.max.saturating_sub(self.min) as f32;
        (self.min as f32 + level * range).round() as u8
    }
}

/// Aggregation strategy when multiple zones are assigned to one channel.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub enum AggregationMode {
    #[default]
    Max,
    Sum,
    Average,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ZoneEntry {
    #[serde(flatten)]
    pub id: ZoneId,
    #[serde(default)]
    pub mode: ContactMode,
}

impl ZoneEntry {
    pub fn new(id: ZoneId, mode: ContactMode) -> Self {
        Self { id, mode }
    }

    pub fn with_default_mode(id: ZoneId) -> Self {
        Self {
            id,
            mode: ContactMode::default(),
        }
    }
}

impl fmt::Display for ZoneEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}[{}]", self.id, self.mode)
    }
}

/// Configuration for a single output channel (A or B).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelConfig {
    pub zones: Vec<ZoneEntry>,
    pub frequency: [u8; 4],
    pub intensity: [u8; 4],
    pub limits: PowerLimits,
    pub aggregation: AggregationMode,
}

impl Default for ChannelConfig {
    fn default() -> Self {
        Self {
            zones: Vec::new(),
            frequency: [100; 4],
            intensity: [100; 4],
            limits: PowerLimits::default(),
            aggregation: AggregationMode::default(),
        }
    }
}

impl ChannelConfig {
    /// Aggregate multiple zone levels into a single [0.0, 1.0] value.
    pub fn aggregate(&self, levels: &[f32]) -> f32 {
        if levels.is_empty() {
            return 0.0;
        }
        match self.aggregation {
            AggregationMode::Max => levels.iter().cloned().fold(0.0f32, f32::max),
            AggregationMode::Sum => levels.iter().sum::<f32>().min(1.0),
            AggregationMode::Average => {
                let sum: f32 = levels.iter().sum();
                (sum / levels.len() as f32).min(1.0)
            }
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct MotionNorms {
    pub speed: f32,
    pub acc: f32,
    pub recoil: f32,
}

impl Default for MotionNorms {
    fn default() -> Self {
        Self {
            speed: 5.0,
            acc: 30.0,
            recoil: 100.0,
        }
    }
}

impl MotionNorms {
    pub fn sanitised(self) -> Self {
        const MIN: f32 = 1e-3;
        Self {
            speed: self.speed.max(MIN),
            acc: self.acc.max(MIN),
            recoil: self.recoil.max(MIN),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CliConfig {
    pub channel_a: ChannelConfig,
    pub channel_b: ChannelConfig,
    #[serde(default)]
    pub ukf: UkfConfig,
    #[serde(default)]
    pub norms: MotionNorms,
}

impl Default for CliConfig {
    fn default() -> Self {
        Self {
            channel_a: ChannelConfig::default(),
            channel_b: ChannelConfig::default(),
            ukf: UkfConfig::default(),
            norms: MotionNorms::default(),
        }
    }
}
