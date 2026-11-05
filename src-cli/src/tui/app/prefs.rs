use super::Action;

pub(super) use shocking_vrc_app::prefs::{
    CONFIG_FILE, load_ui_prefs, save_alarm_tab_pref, save_config, save_ui_prefs, save_ui_prefs_full,
};

pub(super) fn should_persist_config(action: &Action) -> bool {
    matches!(
        action,
        Action::RemoveZone(_, _)
            | Action::CycleMode(_, _)
            | Action::StepZoneScale(_, _, _)
            | Action::SetZoneScale(_, _, _)
            | Action::SetZoneThresholdMin(_, _, _)
            | Action::SetZoneThresholdMax(_, _, _)
            | Action::AddAvatarZone(_, _)
            | Action::AddAllZones(_)
            | Action::ApplyZonePreset
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
