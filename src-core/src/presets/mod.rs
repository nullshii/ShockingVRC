use serde::{Deserialize, Serialize};

use crate::cli::ChannelConfig;
use crate::modulation::ModulationConfig;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PresetManifest {
    pub version: u32,
    pub presets: Vec<PresetRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PresetRef {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub file: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelPreset {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub author: String,
    pub frequency: [u8; 4],
    pub intensity: [u8; 4],
    #[serde(default)]
    pub freq_modulation: [Option<ModulationConfig>; 4],
    #[serde(default)]
    pub intensity_modulation: [Option<ModulationConfig>; 4],
}

#[derive(Debug, Clone)]
pub struct PresetEntry {
    pub id: String,
    pub name: String,
    pub description: String,
    pub preset: ChannelPreset,
    pub user: bool,
}

impl ChannelPreset {
    pub fn from_channel(name: impl Into<String>, author: impl Into<String>, ch: &ChannelConfig) -> Self {
        Self {
            name: name.into(),
            description: String::new(),
            author: author.into(),
            frequency: ch.frequency,
            intensity: ch.intensity,
            freq_modulation: ch.freq_modulation.clone(),
            intensity_modulation: ch.intensity_modulation.clone(),
        }
    }

    pub fn apply_to(&self, target: &mut ChannelConfig) {
        target.frequency = self.frequency;
        target.intensity = self.intensity;
        target.freq_modulation = self.freq_modulation.clone();
        target.intensity_modulation = self.intensity_modulation.clone();
    }
}

pub fn parse_manifest(json: &str) -> Result<PresetManifest, String> {
    serde_json::from_str(json).map_err(|e| format!("invalid manifest: {e}"))
}

pub fn parse_preset(json: &str) -> Result<ChannelPreset, String> {
    serde_json::from_str(json).map_err(|e| format!("invalid preset: {e}"))
}

pub fn entry_from_ref(pref: &PresetRef, preset: ChannelPreset, user: bool) -> PresetEntry {
    let mut preset = preset;
    if preset.name.is_empty() {
        preset.name = pref.name.clone();
    }
    PresetEntry {
        id: pref.id.clone(),
        name: pref.name.clone(),
        description: if pref.description.is_empty() {
            preset.description.clone()
        } else {
            pref.description.clone()
        },
        preset,
        user,
    }
}

pub fn entry_from_file(id: String, preset: ChannelPreset, user: bool) -> PresetEntry {
    PresetEntry {
        id,
        name: preset.name.clone(),
        description: preset.description.clone(),
        preset,
        user,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::{AggregationMode, PowerLimits, ZoneEntry, ZoneId};
    use crate::OldZoneType;

    #[test]
    fn parse_sample_manifest() {
        let json = include_str!("../../../presets/manifest.json");
        let manifest = parse_manifest(json).expect("manifest");
        assert_eq!(manifest.presets.len(), 31);
    }

    #[test]
    fn every_manifest_preset_file_parses() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("workspace root")
            .join("presets");
        let manifest_txt =
            std::fs::read_to_string(dir.join("manifest.json")).expect("read manifest");
        let manifest = parse_manifest(&manifest_txt).expect("parse manifest");
        for pref in &manifest.presets {
            let txt = std::fs::read_to_string(dir.join(&pref.file))
                .unwrap_or_else(|e| panic!("missing preset file '{}': {e}", pref.file));
            parse_preset(&txt)
                .unwrap_or_else(|e| panic!("preset '{}' failed to parse: {e}", pref.file));
        }
    }

    #[test]
    fn parse_steady_hum_preset() {
        let json = include_str!("../../../presets/steady-hum.json");
        let preset = parse_preset(json).expect("preset");
        assert_eq!(preset.frequency, [180, 50, 160, 70]);
        assert_eq!(preset.intensity, [75, 85, 80, 70]);
    }

    #[test]
    fn apply_preset_keeps_limits() {
        let json = include_str!("../../../presets/steady-hum.json");
        let preset = parse_preset(json).expect("preset");
        let mut ch = ChannelConfig {
            zones: vec![ZoneEntry::with_default_mode(ZoneId::new(
                OldZoneType::Pen,
                "test",
            ))],
            limits: PowerLimits::new(0, 25),
            aggregation: AggregationMode::Sum,
            ..ChannelConfig::default()
        };
        preset.apply_to(&mut ch);
        assert_eq!(ch.frequency, preset.frequency);
        assert_eq!(ch.limits.max, 25);
        assert_eq!(ch.aggregation, AggregationMode::Sum);
        assert_eq!(ch.zones.len(), 1);
    }
}
