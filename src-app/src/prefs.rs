use shocking_vrc_core::cli::{CliConfig, EngineMode};
use shocking_vrc_core::graph::safety::DEFAULT_POWER_CAP;

pub const CONFIG_FILE: &str = "cli_config.json";
pub const PREFS_FILE: &str = "shockingvrc_prefs.json";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GraphicsPreference {
    #[default]
    Auto,
    Full,
    Lite,
}

impl GraphicsPreference {
    fn is_auto(&self) -> bool {
        matches!(self, Self::Auto)
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UiPrefs {
    #[serde(default = "default_auto_save")]
    pub auto_save: bool,
    #[serde(default)]
    pub has_seen_tutorial: bool,
    #[serde(default)]
    pub nickname: String,
    #[serde(default)]
    pub skipped_update_version: Option<String>,
    #[serde(default)]
    pub alarm_tab: bool,
    #[serde(default)]
    pub theme: Option<String>,
    #[serde(default)]
    pub backdrop_speed: Option<f32>,
    #[serde(default)]
    pub backdrop_density: Option<f32>,
    #[serde(default = "default_spoof_guard")]
    pub spoof_guard: bool,
    #[serde(default)]
    pub mode: EngineMode,
    #[serde(default = "default_graph_power_cap")]
    pub graph_power_cap: [u8; 2],
    #[serde(default, skip_serializing_if = "is_false")]
    pub remote_enabled: bool,
    #[serde(default, skip_serializing_if = "GraphicsPreference::is_auto")]
    pub graphics: GraphicsPreference,
}

fn is_false(v: &bool) -> bool {
    !*v
}

fn default_graph_power_cap() -> [u8; 2] {
    [DEFAULT_POWER_CAP; 2]
}

fn default_spoof_guard() -> bool {
    true
}

fn default_auto_save() -> bool {
    true
}

impl Default for UiPrefs {
    fn default() -> Self {
        Self {
            auto_save: default_auto_save(),
            has_seen_tutorial: false,
            nickname: String::new(),
            skipped_update_version: None,
            alarm_tab: false,
            theme: None,
            backdrop_speed: None,
            backdrop_density: None,
            spoof_guard: default_spoof_guard(),
            mode: EngineMode::default(),
            graph_power_cap: default_graph_power_cap(),
            remote_enabled: false,
            graphics: GraphicsPreference::Auto,
        }
    }
}

pub fn load_ui_prefs() -> UiPrefs {
    std::fs::read_to_string(PREFS_FILE)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save_ui_prefs_full(prefs: &UiPrefs) -> Result<(), Box<dyn std::error::Error>> {
    let json = serde_json::to_string_pretty(prefs)?;
    std::fs::write(PREFS_FILE, json)?;
    Ok(())
}

pub fn save_ui_prefs(auto_save: bool) -> Result<(), Box<dyn std::error::Error>> {
    let mut prefs = load_ui_prefs();
    prefs.auto_save = auto_save;
    save_ui_prefs_full(&prefs)
}

pub fn save_alarm_tab_pref(visible: bool) -> Result<(), Box<dyn std::error::Error>> {
    let mut prefs = load_ui_prefs();
    prefs.alarm_tab = visible;
    save_ui_prefs_full(&prefs)
}

pub fn update_ui_prefs<F: FnOnce(&mut UiPrefs)>(f: F) -> Result<(), Box<dyn std::error::Error>> {
    let mut prefs = load_ui_prefs();
    f(&mut prefs);
    save_ui_prefs_full(&prefs)
}

pub fn save_config(path: &str, config: &CliConfig) -> Result<(), Box<dyn std::error::Error>> {
    let json = serde_json::to_string_pretty(config)?;
    std::fs::write(path, json)?;
    Ok(())
}

pub fn load_config_from_file(path: &str) -> Result<CliConfig, Box<dyn std::error::Error>> {
    let json = std::fs::read_to_string(path)?;
    Ok(serde_json::from_str(&json)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_prefs_files_default_to_classic_with_a_conservative_cap() {
        let p: UiPrefs = serde_json::from_str(r#"{"auto_save":false,"nickname":"x"}"#).unwrap();
        assert_eq!(p.mode, EngineMode::Classic);
        assert_eq!(p.graph_power_cap, [DEFAULT_POWER_CAP; 2]);
        assert!(!p.auto_save);
    }

    #[test]
    fn settings_are_unchanged_while_remote_is_off() {
        let json = serde_json::to_value(UiPrefs::default()).unwrap();
        let mut keys: Vec<&str> = json.as_object().unwrap().keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            [
                "alarm_tab",
                "auto_save",
                "backdrop_density",
                "backdrop_speed",
                "graph_power_cap",
                "has_seen_tutorial",
                "mode",
                "nickname",
                "skipped_update_version",
                "spoof_guard",
                "theme"
            ],
            "the prefs file has exactly the pre-remote keys"
        );
        let old: UiPrefs = serde_json::from_str(r#"{"auto_save":true}"#).unwrap();
        assert!(!old.remote_enabled, "remote is off unless turned on");
        let on = UiPrefs {
            remote_enabled: true,
            ..UiPrefs::default()
        };
        let back: UiPrefs = serde_json::from_str(&serde_json::to_string(&on).unwrap()).unwrap();
        assert!(back.remote_enabled);
    }

    #[test]
    fn mode_round_trips() {
        let p = UiPrefs {
            mode: EngineMode::Graph,
            ..UiPrefs::default()
        };
        let back: UiPrefs = serde_json::from_str(&serde_json::to_string(&p).unwrap()).unwrap();
        assert_eq!(back.mode, EngineMode::Graph);
    }

    #[test]
    fn graphics_preference_round_trips() {
        for pref in [
            GraphicsPreference::Auto,
            GraphicsPreference::Full,
            GraphicsPreference::Lite,
        ] {
            let p = UiPrefs {
                graphics: pref,
                ..UiPrefs::default()
            };
            let back: UiPrefs = serde_json::from_str(&serde_json::to_string(&p).unwrap()).unwrap();
            assert_eq!(back.graphics, pref);
        }
        let explicit_auto: UiPrefs = serde_json::from_str(r#"{"graphics":"auto"}"#).unwrap();
        assert_eq!(explicit_auto.graphics, GraphicsPreference::Auto);
    }
}
