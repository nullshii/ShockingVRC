use ratatui::layout::Rect;
use shocking_vrc_core::cli::AggregationMode;
use shocking_vrc_core::modulation::config::{ModulationConfig, ModulationSource};
use shocking_vrc_core::DiscoveryMode;

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
}

impl Tab {
    pub const ALL: [Tab; 8] = [
        Tab::Status,
        Tab::Zones,
        Tab::Channels,
        Tab::Tuning,
        Tab::Modulation,
        Tab::Log,
        Tab::Presets,
        Tab::Setup,
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
    StepOscPort(i32),
    StartOscPortEdit,
    SetDiscoveryMode(DiscoveryMode),
    StartOscAvatarsDirEdit,
    ResetOscAvatarsDir,

    FocusZonesPane(ZonesPane),
    SelectConfigured(Channel, usize),
    SelectAvatar(usize),
    RemoveZone(Channel, usize),
    CycleMode(Channel, usize),
    StepZoneScale(Channel, usize, i32),
    SetZoneScale(Channel, usize, u8),
    StepZoneThresholdMin(Channel, usize, i32),
    SetZoneThresholdMin(Channel, usize, u8),
    StepZoneThresholdMax(Channel, usize, i32),
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
