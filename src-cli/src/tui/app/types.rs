use ratatui::layout::Rect;
use shocking_vrc_core::cli::{AggregationMode, AlarmChannels, AlarmConfig};
use shocking_vrc_core::modulation::config::{ModulationConfig, ModulationSource};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Channel {
    A,
    B,
}

impl Channel {
    pub fn label(self) -> &'static str {
        match self {
            Channel::A => "A",
            Channel::B => "B",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Status,
    Zones,
    Channels,
    Tuning,
    Modulation,
    Log,
    Presets,
    Setup,
    Alarm,
}

impl Tab {
    pub const ALL: [Tab; 9] = [
        Tab::Status,
        Tab::Zones,
        Tab::Channels,
        Tab::Tuning,
        Tab::Modulation,
        Tab::Log,
        Tab::Presets,
        Tab::Setup,
        Tab::Alarm,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Tab::Status => "Status",
            Tab::Zones => "Zones",
            Tab::Channels => "Channels",
            Tab::Tuning => "Tuning",
            Tab::Modulation => "Modulation",
            Tab::Log => "Log",
            Tab::Presets => "Presets",
            Tab::Setup => "Setup",
            Tab::Alarm => "Alarm",
        }
    }

    pub fn index(self) -> usize {
        Tab::ALL.iter().position(|t| *t == self).unwrap_or(0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZonesPane {
    ConfiguredA,
    ConfiguredB,
    Avatar,
}

impl ZonesPane {
    pub fn next(self) -> Self {
        match self {
            ZonesPane::ConfiguredA => ZonesPane::ConfiguredB,
            ZonesPane::ConfiguredB => ZonesPane::Avatar,
            ZonesPane::Avatar => ZonesPane::ConfiguredA,
        }
    }

    pub fn prev(self) -> Self {
        match self {
            ZonesPane::ConfiguredA => ZonesPane::Avatar,
            ZonesPane::ConfiguredB => ZonesPane::ConfiguredA,
            ZonesPane::Avatar => ZonesPane::ConfiguredB,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModKind {
    Freq,
    Intensity,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UkfField {
    Q,
    R,
    Alpha,
    Beta,
    Kappa,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NormField {
    Speed,
    Acc,
    Recoil,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresetSaveField {
    Name,
    Nickname,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlarmField {
    Hour,
    Minute,
    StartStrength,
    PeakStrength,
    Ramp,
    MaxDuration,
    Repeats,
    PulseOn,
    PulseOff,
    Snooze,
}

impl AlarmField {
    pub fn label(self) -> &'static str {
        match self {
            AlarmField::Hour => "Hour",
            AlarmField::Minute => "Minute",
            AlarmField::StartStrength => "Start power",
            AlarmField::PeakStrength => "Peak power",
            AlarmField::Ramp => "Ramp to peak",
            AlarmField::MaxDuration => "Try for",
            AlarmField::Repeats => "Repeats",
            AlarmField::PulseOn => "Pulse on",
            AlarmField::PulseOff => "Pulse off",
            AlarmField::Snooze => "Snooze",
        }
    }

    fn step(self) -> i32 {
        match self {
            AlarmField::Hour
            | AlarmField::Minute
            | AlarmField::Snooze
            | AlarmField::Repeats => 1,
            AlarmField::StartStrength | AlarmField::PeakStrength => 1,
            AlarmField::Ramp => 10,
            AlarmField::MaxDuration => 30,
            AlarmField::PulseOn | AlarmField::PulseOff => 100,
        }
    }

    fn range(self) -> (i32, i32) {
        match self {
            AlarmField::Hour => (0, 23),
            AlarmField::Minute => (0, 59),
            AlarmField::StartStrength | AlarmField::PeakStrength => {
                (0, AlarmConfig::MAX_STRENGTH as i32)
            }
            AlarmField::Ramp => (0, AlarmConfig::RAMP_MAX_SECS as i32),
            AlarmField::MaxDuration => (
                AlarmConfig::DURATION_MIN_SECS as i32,
                AlarmConfig::DURATION_MAX_SECS as i32,
            ),
            AlarmField::Repeats => (1, AlarmConfig::MAX_REPEATS as i32),
            AlarmField::PulseOn => (
                AlarmConfig::PULSE_MIN_MS as i32,
                AlarmConfig::PULSE_MAX_MS as i32,
            ),
            AlarmField::PulseOff => (0, AlarmConfig::PULSE_MAX_MS as i32),
            AlarmField::Snooze => (1, AlarmConfig::SNOOZE_MAX_MINS as i32),
        }
    }

    fn wraps(self) -> bool {
        matches!(self, AlarmField::Hour | AlarmField::Minute)
    }

    pub fn get(self, cfg: &AlarmConfig) -> i32 {
        match self {
            AlarmField::Hour => cfg.hour as i32,
            AlarmField::Minute => cfg.minute as i32,
            AlarmField::StartStrength => cfg.start_strength as i32,
            AlarmField::PeakStrength => cfg.peak_strength as i32,
            AlarmField::Ramp => cfg.ramp_secs as i32,
            AlarmField::MaxDuration => cfg.max_duration_secs as i32,
            AlarmField::Repeats => cfg.repeats as i32,
            AlarmField::PulseOn => cfg.pulse_on_ms as i32,
            AlarmField::PulseOff => cfg.pulse_off_ms as i32,
            AlarmField::Snooze => cfg.snooze_mins as i32,
        }
    }

    fn set(self, cfg: &mut AlarmConfig, v: i32) {
        match self {
            AlarmField::Hour => cfg.hour = v as u8,
            AlarmField::Minute => cfg.minute = v as u8,
            AlarmField::StartStrength => cfg.start_strength = v as u8,
            AlarmField::PeakStrength => cfg.peak_strength = v as u8,
            AlarmField::Ramp => cfg.ramp_secs = v as u16,
            AlarmField::MaxDuration => cfg.max_duration_secs = v as u16,
            AlarmField::Repeats => cfg.repeats = v as u8,
            AlarmField::PulseOn => cfg.pulse_on_ms = v as u16,
            AlarmField::PulseOff => cfg.pulse_off_ms = v as u16,
            AlarmField::Snooze => cfg.snooze_mins = v as u8,
        }
    }

    pub fn apply_step(self, cfg: &mut AlarmConfig, delta: i32) {
        let (min, max) = self.range();
        let next = self.get(cfg) + delta * self.step();
        let next = if self.wraps() {
            let span = max - min + 1;
            min + (next - min).rem_euclid(span)
        } else {
            next.clamp(min, max)
        };
        self.set(cfg, next);
        match self {
            AlarmField::StartStrength => {
                cfg.peak_strength = cfg.peak_strength.max(cfg.start_strength)
            }
            AlarmField::PeakStrength => {
                cfg.start_strength = cfg.start_strength.min(cfg.peak_strength)
            }
            _ => {}
        }
    }

    pub fn value_label(self, cfg: &AlarmConfig) -> String {
        let v = self.get(cfg);
        match self {
            AlarmField::Hour | AlarmField::Minute => format!("{v:02}"),
            AlarmField::StartStrength | AlarmField::PeakStrength => v.to_string(),
            AlarmField::Ramp | AlarmField::MaxDuration => format_secs(v),
            AlarmField::Repeats => {
                if v == 1 {
                    "once".to_string()
                } else {
                    format!("{v} ×")
                }
            }
            AlarmField::PulseOn | AlarmField::PulseOff => {
                if v == 0 {
                    "off (steady)".to_string()
                } else {
                    format!("{:.1} s", v as f32 / 1000.0)
                }
            }
            AlarmField::Snooze => format!("{v} min"),
        }
    }
}

pub fn format_secs(secs: i32) -> String {
    if secs < 60 {
        format!("{secs} s")
    } else if secs % 60 == 0 {
        format!("{} min", secs / 60)
    } else {
        format!("{}:{:02} min", secs / 60, secs % 60)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModParam {
    BaseSpeed,
    Sensitivity,
    MaxDeviation,
    Phase,
    FreqMul,
    Offset,
    Power,
    ClampMin,
    ClampMax,
}

impl ModParam {
    pub const ALL: [ModParam; 9] = [
        ModParam::BaseSpeed,
        ModParam::Sensitivity,
        ModParam::MaxDeviation,
        ModParam::Phase,
        ModParam::FreqMul,
        ModParam::Offset,
        ModParam::Power,
        ModParam::ClampMin,
        ModParam::ClampMax,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ModParam::BaseSpeed => "Speed",
            ModParam::Sensitivity => "Sensitivity",
            ModParam::MaxDeviation => "Deviation",
            ModParam::Phase => "Phase",
            ModParam::FreqMul => "Freq mul",
            ModParam::Offset => "Offset",
            ModParam::Power => "Power",
            ModParam::ClampMin => "Clamp min",
            ModParam::ClampMax => "Clamp max",
        }
    }

    pub fn step(self) -> f32 {
        match self {
            ModParam::BaseSpeed
            | ModParam::Sensitivity
            | ModParam::Phase
            | ModParam::FreqMul
            | ModParam::Power => 0.1,
            ModParam::MaxDeviation
            | ModParam::Offset
            | ModParam::ClampMin
            | ModParam::ClampMax => 1.0,
        }
    }

    pub fn get(self, c: &ModulationConfig) -> f32 {
        match self {
            ModParam::BaseSpeed => c.base_speed,
            ModParam::Sensitivity => c.sensitivity,
            ModParam::MaxDeviation => c.max_deviation,
            ModParam::Phase => c.phase,
            ModParam::FreqMul => c.frequency_multiplier,
            ModParam::Offset => c.offset,
            ModParam::Power => c.power,
            ModParam::ClampMin => c.clamp_min,
            ModParam::ClampMax => c.clamp_max,
        }
    }

    pub fn set(self, c: &mut ModulationConfig, v: f32) {
        match self {
            ModParam::BaseSpeed => c.base_speed = v,
            ModParam::Sensitivity => c.sensitivity = v,
            ModParam::MaxDeviation => c.max_deviation = v,
            ModParam::Phase => c.phase = v,
            ModParam::FreqMul => c.frequency_multiplier = v,
            ModParam::Offset => c.offset = v,
            ModParam::Power => c.power = v,
            ModParam::ClampMin => c.clamp_min = v,
            ModParam::ClampMax => c.clamp_max = v,
        }
    }

    pub fn clamp_value(self, v: f32, kind: ModKind) -> f32 {
        let intensity = kind == ModKind::Intensity;
        let (lo_limit, hi_limit) = ModulationConfig::output_bounds(intensity);
        match self {
            ModParam::Offset => v.clamp(-hi_limit, hi_limit),
            ModParam::ClampMin | ModParam::ClampMax => v.clamp(lo_limit, hi_limit),
            ModParam::BaseSpeed
            | ModParam::Sensitivity
            | ModParam::MaxDeviation
            | ModParam::Power => v.max(0.0),
            ModParam::Phase => v,
            ModParam::FreqMul => v.max(0.001),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum SliderKind {
    Freq(Channel, usize),
    Intensity(Channel, usize),
    LimitMin(Channel),
    LimitMax(Channel),
    ZoneScale(Channel, usize),
    ZoneThresholdMin(Channel, usize),
    ZoneThresholdMax(Channel, usize),
}

impl SliderKind {
    pub fn range(self) -> (i32, i32) {
        match self {
            SliderKind::Freq(..) => (10, 255),
            SliderKind::Intensity(..) => (0, 100),
            SliderKind::LimitMin(..) | SliderKind::LimitMax(..) => (0, 200),
            SliderKind::ZoneScale(..) => (0, 100),
            SliderKind::ZoneThresholdMin(..) | SliderKind::ZoneThresholdMax(..) => (1, 100),
        }
    }

    pub fn is_inverted(self) -> bool {
        matches!(self, SliderKind::Freq(..))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Quit,
    SwitchTab(Tab),

    FocusZonesPane(ZonesPane),
    SelectConfigured(Channel, usize),
    SelectAvatar(usize),
    RemoveZone(Channel, usize),
    CycleMode(Channel, usize),
    StepZoneScale(Channel, usize, i32),
    SetZoneScale(Channel, usize, u8),
    SetZoneThresholdMin(Channel, usize, u8),
    SetZoneThresholdMax(Channel, usize, u8),
    SwitchDevice(usize),
    CycleDevice(i32),
    AddAvatarZone(Channel, usize),
    AddAllZones(Channel),

    SetFreq(Channel, usize, u8),
    StepFreq(Channel, usize, i32),
    SetIntensity(Channel, usize, u8),
    StepIntensity(Channel, usize, i32),
    SetLimitMin(Channel, u8),
    SetLimitMax(Channel, u8),
    StepLimitMin(Channel, i32),
    StepLimitMax(Channel, i32),
    CycleAggregation(Channel),
    SetAggregation(Channel, AggregationMode),
    SaveConfig,
    LoadConfig,
    SetAutoSave(bool),
    ToggleAutoSave,
    SetAlarmTabVisible(bool),
    ToggleAlarmTabVisible,

    StepUkf(UkfField, i32),
    ResetUkf,
    StepNorm(NormField, i32),
    ResetNorms,

    SelectModSlot(Channel, ModKind, usize),
    OpenModFunctionPicker,
    PickModFunction(usize),
    CloseModFunctionPicker,
    SetModSource(ModulationSource),
    CycleModFunction(i32),
    CycleModSource(i32),
    StepModParam(ModParam, i32),
    ApplyMod,
    ClearMod,
    ClearAllMod(Channel),

    SelectPreset(usize),
    ApplyPreset(Channel),
    StartSavePreset(Channel),
    CommitSavePreset,
    CancelSavePreset,
    RequestDeletePreset(usize),
    ConfirmDeletePreset,
    CancelDeletePreset,
    RefreshPresets,

    SetAlarmEnabled(bool),
    ToggleAlarmEnabled,
    CycleAlarmChannels(i32),
    StepAlarmField(AlarmField, i32),
    SetAlarmChannels(AlarmChannels),
    AlarmTest,
    AlarmStop,
    AlarmSnooze,

    ConfirmUpdate,
    DismissUpdate,
    SkipUpdateVersion,

    TutorialStart,
    TutorialNext,
    TutorialPrev,
    TutorialClose,

    FocusNext,
    FocusPrev,
    AdjustFocused(i32),
    ActivateFocused,
}

pub struct Clickable {
    pub rect: Rect,
    pub kind: ClickKind,
}

pub enum ClickKind {
    Act(Action),
    Slider(SliderKind),
}
