use ratatui::crossterm::event::{KeyEvent, KeyModifiers};
use ratatui::layout::Rect;
use shocking_vrc_core::cli::{ChannelStatus, CliStatus, MotionNorms, UkfConfig};

use super::{Action, NormField, SliderKind, UkfField};

pub(super) fn default_status() -> CliStatus {
    let empty = || ChannelStatus {
        raw_level: 0.0,
        strength: 0,
        frequency: [0; 4],
        active_zones: Vec::new(),
        kinematics: Default::default(),
    };
    CliStatus {
        channel_a: empty(),
        channel_b: empty(),
        device_connected: false,
    }
}

pub(super) enum HitResult {
    Action(Action),
    Slider(SliderKind, Rect),
}

pub(super) fn point_in(r: Rect, col: u16, row: u16) -> bool {
    col >= r.x && col < r.x + r.width && row >= r.y && row < r.y + r.height
}

pub(super) fn slider_value_action(kind: SliderKind, rect: Rect, col: u16) -> Action {
    let (min, max) = kind.range();
    let w = rect.width.max(1);
    let rel = col.saturating_sub(rect.x) as f32;
    let denom = (w.saturating_sub(1)).max(1) as f32;
    let mut frac = (rel / denom).clamp(0.0, 1.0);
    if kind.is_inverted() {
        frac = 1.0 - frac;
    }
    let val = (min as f32 + frac * (max - min) as f32).round() as i32;
    match kind {
        SliderKind::Freq(ch, seg) => Action::SetFreq(ch, seg, val.clamp(min, max) as u8),
        SliderKind::Intensity(ch, seg) => Action::SetIntensity(ch, seg, val.clamp(min, max) as u8),
        SliderKind::LimitMin(ch) => Action::SetLimitMin(ch, val.clamp(min, max) as u8),
        SliderKind::LimitMax(ch) => Action::SetLimitMax(ch, val.clamp(min, max) as u8),
        SliderKind::ZoneScale(ch, i) => Action::SetZoneScale(ch, i, val.clamp(min, max) as u8),
        SliderKind::ZoneThresholdMin(ch, i) => {
            Action::SetZoneThresholdMin(ch, i, val.clamp(min, max) as u8)
        }
        SliderKind::ZoneThresholdMax(ch, i) => {
            Action::SetZoneThresholdMax(ch, i, val.clamp(min, max) as u8)
        }
    }
}

pub(super) fn is_zone_preset_action(action: &Action) -> bool {
    matches!(
        action,
        Action::SelectZonePreset(_)
            | Action::ApplyZonePreset
            | Action::StartSaveZonePreset
            | Action::CommitSaveZonePreset
            | Action::CancelSaveZonePreset
            | Action::RequestDeleteZonePreset(_)
            | Action::ConfirmDeleteZonePreset
            | Action::CancelDeleteZonePreset
            | Action::CloseZonePresets
    )
}

pub(super) fn step_index(cur: usize, delta: i32, len: usize) -> usize {
    if len == 0 {
        return 0;
    }
    (cur as i32 + delta).rem_euclid(len as i32) as usize
}

pub(super) fn step_u8(cur: u8, delta: i32, min: u8, max: u8) -> u8 {
    let v = cur as i32 + delta;
    v.clamp(min as i32, max as i32) as u8
}

pub(super) fn step_multiplier(key: &KeyEvent) -> i32 {
    if key.modifiers.contains(KeyModifiers::SHIFT) {
        10
    } else {
        1
    }
}

pub(super) fn apply_ukf_step(p: &mut UkfConfig, field: UkfField, d: i32) {
    let f = d as f32;
    match field {
        UkfField::Q => p.q = (p.q + f * 0.001).max(0.0001),
        UkfField::R => p.r = (p.r + f * 0.001).max(0.0001),
        UkfField::Alpha => p.alpha = (p.alpha + f * 0.05).max(0.001),
        UkfField::Beta => p.beta = (p.beta + f * 0.1).max(0.0),
        UkfField::Kappa => p.kappa += f * 0.1,
    }
    p.q = round3(p.q);
    p.r = round3(p.r);
    p.alpha = round3(p.alpha);
    p.beta = round3(p.beta);
    p.kappa = round3(p.kappa);
}

pub(super) fn apply_norm_step(n: &mut MotionNorms, field: NormField, d: i32) {
    let f = d as f32;
    match field {
        NormField::Speed => n.speed = (n.speed + f * 0.5).max(0.001),
        NormField::Acc => n.acc = (n.acc + f * 1.0).max(0.001),
        NormField::Recoil => n.recoil = (n.recoil + f * 5.0).max(0.001),
    }
    n.speed = round3(n.speed);
    n.acc = round3(n.acc);
    n.recoil = round3(n.recoil);
}

pub(super) fn round3(v: f32) -> f32 {
    (v * 1000.0).round() / 1000.0
}
