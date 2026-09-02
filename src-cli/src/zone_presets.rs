use std::path::{Path, PathBuf};

use shocking_vrc_core::cli::CliConfig;
use shocking_vrc_core::presets::{parse_zone_preset, ZonePreset, ZonePresetEntry};

use crate::presets::slugify;

const ZONE_PRESETS_DIR: &str = "presets/zones";

pub fn zone_preset_dir() -> PathBuf {
    PathBuf::from(ZONE_PRESETS_DIR)
}

pub fn load_zone_presets() -> Vec<ZonePresetEntry> {
    load_zone_presets_from(&zone_preset_dir())
}

pub fn save_zone_preset(name: &str, config: &CliConfig) -> Result<ZonePresetEntry, String> {
    save_zone_preset_in(&zone_preset_dir(), name, config)
}

pub fn delete_zone_preset(id: &str) -> Result<(), String> {
    delete_zone_preset_in(&zone_preset_dir(), id)
}

pub fn sort_zone_preset_entries(entries: &mut [ZonePresetEntry]) {
    entries.sort_by_key(|e| e.name().to_lowercase());
}

pub fn default_zone_set_name() -> String {
    use rand::RngExt;
    let n: u32 = rand::rng().random_range(1_000..9_999);
    format!("zones-{n}")
}

fn load_zone_presets_from(dir: &Path) -> Vec<ZonePresetEntry> {
    let Ok(read) = std::fs::read_dir(dir) else {
        return Vec::new();
    };

    let mut entries = Vec::new();
    for item in read.flatten() {
        let path = item.path();
        if path.is_dir() || path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let Some(id) = path.file_stem().and_then(|s| s.to_str()).map(str::to_string) else {
            continue;
        };
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        match parse_zone_preset(&text) {
            Ok(preset) => entries.push(ZonePresetEntry { id, preset }),
            Err(e) => log::warn!("[zone-sets] Skipping {}: {e}", path.display()),
        }
    }

    sort_zone_preset_entries(&mut entries);
    entries
}

fn save_zone_preset_in(
    dir: &Path,
    name: &str,
    config: &CliConfig,
) -> Result<ZonePresetEntry, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("Zone set name cannot be empty".into());
    }

    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;

    let id = slugify(name);
    let preset = ZonePreset::from_config(name, config);
    let json = serde_json::to_string_pretty(&preset).map_err(|e| e.to_string())?;
    let path = dir.join(format!("{id}.json"));
    std::fs::write(&path, json).map_err(|e| format!("{}: {e}", path.display()))?;

    Ok(ZonePresetEntry { id, preset })
}

fn delete_zone_preset_in(dir: &Path, id: &str) -> Result<(), String> {
    let path = dir.join(format!("{id}.json"));
    if !path.exists() {
        return Err(format!("zone set file '{}' not found", path.display()));
    }
    std::fs::remove_file(&path).map_err(|e| format!("{}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use shocking_vrc_core::cli::{ContactMode, ZoneEntry, ZoneId};
    use shocking_vrc_core::OldZoneType;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("shockingvrc-zone-sets-{tag}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    fn config_with_zones() -> CliConfig {
        let mut cfg = CliConfig::default();
        let mut front = ZoneEntry::with_default_mode(ZoneId::new(OldZoneType::DGB, "FrontR"));
        front.mode = ContactMode::Speed;
        front.scale = 70;
        cfg.channel_a.zones = vec![front];
        cfg.channel_b.zones = vec![ZoneEntry::with_default_mode(ZoneId::new(
            OldZoneType::Touch,
            "Chest",
        ))];
        cfg
    }

    #[test]
    fn save_load_delete_round_trip() {
        let dir = temp_dir("round-trip");
        let cfg = config_with_zones();

        let saved = save_zone_preset_in(&dir, "My Avatar", &cfg).expect("save");
        assert_eq!(saved.id, "my-avatar");
        assert!(dir.join("my-avatar.json").exists());

        let loaded = load_zone_presets_from(&dir);
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].name(), "My Avatar");
        assert_eq!(loaded[0].preset.counts(), (1, 1));
        assert_eq!(loaded[0].preset.channel_a[0].mode, ContactMode::Speed);
        assert_eq!(loaded[0].preset.channel_a[0].scale, 70);

        delete_zone_preset_in(&dir, "my-avatar").expect("delete");
        assert!(load_zone_presets_from(&dir).is_empty());
    }

    #[test]
    fn saving_the_same_name_overwrites_one_file() {
        let dir = temp_dir("overwrite");
        save_zone_preset_in(&dir, "set", &config_with_zones()).expect("save");
        save_zone_preset_in(&dir, "set", &CliConfig::default()).expect("re-save");

        let loaded = load_zone_presets_from(&dir);
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].preset.counts(), (0, 0));
    }

    #[test]
    fn listing_sorts_by_name_and_skips_junk() {
        let dir = temp_dir("sorting");
        save_zone_preset_in(&dir, "zebra", &CliConfig::default()).expect("save");
        save_zone_preset_in(&dir, "Alpha", &CliConfig::default()).expect("save");
        std::fs::write(dir.join("broken.json"), "{ not json").expect("write junk");
        std::fs::write(dir.join("notes.txt"), "ignored").expect("write txt");

        let names: Vec<_> = load_zone_presets_from(&dir)
            .iter()
            .map(|e| e.name().to_string())
            .collect();
        assert_eq!(names, vec!["Alpha", "zebra"]);
    }

    #[test]
    fn empty_name_is_rejected_and_generated_names_are_unique_enough() {
        let dir = temp_dir("names");
        assert!(save_zone_preset_in(&dir, "   ", &CliConfig::default()).is_err());
        assert!(default_zone_set_name().starts_with("zones-"));
    }

    #[test]
    fn deleting_a_missing_set_reports_an_error() {
        let dir = temp_dir("missing");
        assert!(delete_zone_preset_in(&dir, "nope").is_err());
    }

    #[test]
    fn missing_directory_lists_nothing() {
        let dir = std::env::temp_dir().join("shockingvrc-zone-sets-does-not-exist");
        let _ = std::fs::remove_dir_all(&dir);
        assert!(load_zone_presets_from(&dir).is_empty());
    }
}
