use serde::{Deserialize, Serialize};

use crate::cli::{CliConfig, ZoneEntry};

pub const ZONE_PRESET_VERSION: u32 = 1;

fn default_version() -> u32 {
    ZONE_PRESET_VERSION
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ZonePreset {
    #[serde(default = "default_version")]
    pub version: u32,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub channel_a: Vec<ZoneEntry>,
    #[serde(default)]
    pub channel_b: Vec<ZoneEntry>,
}

impl ZonePreset {
    pub fn from_config(name: impl Into<String>, config: &CliConfig) -> Self {
        Self {
            version: ZONE_PRESET_VERSION,
            name: name.into(),
            description: String::new(),
            channel_a: config.channel_a.zones.clone(),
            channel_b: config.channel_b.zones.clone(),
        }
    }

    pub fn apply_to(&self, config: &mut CliConfig) {
        config.channel_a.zones = self.channel_a.clone();
        config.channel_b.zones = self.channel_b.clone();
    }

    pub fn counts(&self) -> (usize, usize) {
        (self.channel_a.len(), self.channel_b.len())
    }

    pub fn is_empty(&self) -> bool {
        self.channel_a.is_empty() && self.channel_b.is_empty()
    }
}

#[derive(Debug, Clone)]
pub struct ZonePresetEntry {
    pub id: String,
    pub preset: ZonePreset,
}

impl ZonePresetEntry {
    pub fn name(&self) -> &str {
        if self.preset.name.is_empty() {
            &self.id
        } else {
            &self.preset.name
        }
    }
}

pub fn parse_zone_preset(json: &str) -> Result<ZonePreset, String> {
    serde_json::from_str(json).map_err(|e| format!("invalid zone set: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::OldZoneType;
    use crate::cli::{AggregationMode, ContactMode, PowerLimits, ZoneId};

    fn zone(name: &str) -> ZoneEntry {
        ZoneEntry::with_default_mode(ZoneId::new(OldZoneType::DGB, name))
    }

    fn config_with(a: Vec<ZoneEntry>, b: Vec<ZoneEntry>) -> CliConfig {
        let mut cfg = CliConfig::default();
        cfg.channel_a.zones = a;
        cfg.channel_b.zones = b;
        cfg
    }

    #[test]
    fn snapshot_captures_both_channels() {
        let cfg = config_with(vec![zone("FrontR"), zone("FrontL")], vec![zone("Back")]);
        let preset = ZonePreset::from_config("my avatar", &cfg);
        assert_eq!(preset.counts(), (2, 1));
        assert_eq!(preset.name, "my avatar");
    }

    #[test]
    fn apply_replaces_zones_and_keeps_the_waveform() {
        let mut cfg = config_with(vec![zone("Old")], vec![]);
        cfg.channel_a.frequency = [12, 34, 56, 78];
        cfg.channel_a.intensity = [11, 22, 33, 44];
        cfg.channel_a.limits = PowerLimits::new(5, 42);
        cfg.channel_a.aggregation = AggregationMode::Sum;

        let preset = ZonePreset::from_config("set", &config_with(vec![zone("New")], vec![zone("B")]));
        preset.apply_to(&mut cfg);

        assert_eq!(cfg.channel_a.zones.len(), 1);
        assert_eq!(cfg.channel_a.zones[0].id.name, "New");
        assert_eq!(cfg.channel_b.zones.len(), 1);
        assert_eq!(cfg.channel_a.frequency, [12, 34, 56, 78]);
        assert_eq!(cfg.channel_a.intensity, [11, 22, 33, 44]);
        assert_eq!(cfg.channel_a.limits.max, 42);
        assert_eq!(cfg.channel_a.aggregation, AggregationMode::Sum);
    }

    #[test]
    fn apply_can_clear_a_channel() {
        let mut cfg = config_with(vec![zone("A1")], vec![zone("B1")]);
        let empty = ZonePreset::from_config("none", &CliConfig::default());
        assert!(empty.is_empty());
        empty.apply_to(&mut cfg);
        assert!(cfg.channel_a.zones.is_empty());
        assert!(cfg.channel_b.zones.is_empty());
    }

    #[test]
    fn per_zone_tuning_round_trips_through_json() {
        let mut e = zone("FrontR");
        e.mode = ContactMode::Speed;
        e.scale = 60;
        e.min_threshold = 15;
        e.max_threshold = 90;
        let preset = ZonePreset::from_config("tuned", &config_with(vec![e.clone()], vec![]));

        let json = serde_json::to_string_pretty(&preset).unwrap();
        let back = parse_zone_preset(&json).unwrap();
        assert_eq!(back, preset);
        assert_eq!(back.channel_a[0], e);
    }

    #[test]
    fn version_defaults_when_missing() {
        let json = r#"{"name":"legacy","channel_a":[],"channel_b":[]}"#;
        let preset = parse_zone_preset(json).unwrap();
        assert_eq!(preset.version, ZONE_PRESET_VERSION);
    }

    #[test]
    fn rejects_malformed_json() {
        assert!(parse_zone_preset("not json").is_err());
    }
}
