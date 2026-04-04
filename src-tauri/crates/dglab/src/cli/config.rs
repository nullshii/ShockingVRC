use serde::{Deserialize, Serialize};
use std::fmt;

use crate::osc::types::ZoneEvent;

/// Identifies an OSC zone by its type (Pen/Orf/Touch/DGB) and name.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct ZoneId {
    pub zone_type: String,
    pub name: String,
}

impl ZoneId {
    pub fn new(zone_type: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            zone_type: zone_type.into(),
            name: name.into(),
        }
    }

    pub fn from_event(event: &ZoneEvent) -> Self {
        Self {
            zone_type: event.zone_type.to_string(),
            name: event.id.clone(),
        }
    }

    pub fn is_wildcard(&self) -> bool {
        self.zone_type == "*" || self.name == "*"
    }

    pub fn matches(&self, other: &ZoneId) -> bool {
        let type_ok = self.zone_type == "*" || self.zone_type == other.zone_type;
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

/// Configuration for a single output channel (A or B).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelConfig {
    pub zones: Vec<ZoneId>,
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

/// Top-level CLI configuration containing both output channels.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CliConfig {
    pub channel_a: ChannelConfig,
    pub channel_b: ChannelConfig,
}

impl Default for CliConfig {
    fn default() -> Self {
        Self {
            channel_a: ChannelConfig::default(),
            channel_b: ChannelConfig::default(),
        }
    }
}
