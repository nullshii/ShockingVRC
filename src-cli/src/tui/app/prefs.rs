use shocking_vrc_core::cli::CliConfig;

use super::Action;

pub(super) const CONFIG_FILE: &str = "cli_config.json";
const PREFS_FILE: &str = "shockingvrc_prefs.json";

#[derive(serde::Serialize, serde::Deserialize)]
pub(super) struct UiPrefs {
    #[serde(default = "default_auto_save")]
    pub auto_save: bool,
    #[serde(default)]
    pub has_seen_tutorial: bool,
    #[serde(default)]
    pub nickname: String,
    #[serde(default)]
    pub skipped_update_version: Option<String>,
}

fn default_auto_save() -> bool {
    true
}

pub(super) fn load_ui_prefs() -> UiPrefs {
    std::fs::read_to_string(PREFS_FILE)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(UiPrefs {
            auto_save: default_auto_save(),
            has_seen_tutorial: false,
            nickname: String::new(),
            skipped_update_version: None,
        })
}

pub(super) fn save_ui_prefs(auto_save: bool) -> Result<(), Box<dyn std::error::Error>> {
    let mut prefs = load_ui_prefs();
    prefs.auto_save = auto_save;
    save_ui_prefs_full(&prefs)
}

pub(super) fn save_ui_prefs_full(prefs: &UiPrefs) -> Result<(), Box<dyn std::error::Error>> {
    let json = serde_json::to_string_pretty(prefs)?;
    std::fs::write(PREFS_FILE, json)?;
    Ok(())
}

pub(super) fn save_config(path: &str, config: &CliConfig) -> Result<(), Box<dyn std::error::Error>> {
    let json = serde_json::to_string_pretty(config)?;
    std::fs::write(path, json)?;
    Ok(())
}

pub(super) fn should_persist_config(action: &Action) -> bool {
    matches!(
        action,
        Action::RemoveZone(_, _)
            | Action::CycleMode(_, _)
            | Action::AddAvatarZone(_, _)
            | Action::AddAllZones(_)
            | Action::SetFreq(_, _, _)
            | Action::StepFreq(_, _, _)
            | Action::SetIntensity(_, _, _)
            | Action::StepIntensity(_, _, _)
            | Action::SetLimitMin(_, _)
            | Action::SetLimitMax(_, _)
            | Action::StepLimitMin(_, _)
            | Action::StepLimitMax(_, _)
            | Action::CycleAggregation(_)
            | Action::SetAggregation(_, _)
            | Action::StepUkf(_, _)
            | Action::ResetUkf
            | Action::StepNorm(_, _)
            | Action::ResetNorms
            | Action::ApplyMod
            | Action::ClearMod
            | Action::ClearAllMod(_)
            | Action::ApplyPreset(_)
    )
}
