use shocking_vrc_core::cli::{AggregationMode, ContactMode};
use shocking_vrc_core::modulation::config::{ModulationFunction, ModulationSource};

use super::{Action, AlarmField, Channel, ModKind, NormField, UkfField};

pub fn mod_function_list() -> Vec<ModulationFunction> {
    use ModulationFunction::*;
    vec![
        None, Sin, Cos, Tan, SinCos, Sin2, Cos2, SinPlusCos,
        Sinh, Cosh, Tanh,
        Square, Cube, Pow4, Sqrt, Cbrt, Abs, Sign,
        Exp, ExpNeg, Pow2x, Pow10x,
        Ln, Log2, Log10,
        Triangle, Saw, ReverseSaw, SquareWave, Pulse, Bounce,
        Sigmoid, SmoothStep, SmootherStep, Logistic, SoftSign,
        Perlin, Simplex, Fractal, ValueNoise,
        SinPlusNoise, SinTimesNoise, TrianglePlusSin, SquareTimesSigmoid,
    ]
}

pub fn mod_function_list_for(current: &ModulationFunction) -> Vec<ModulationFunction> {
    let mut list = mod_function_list();
    if !list.iter().any(|f| f == current) {
        list.insert(1, current.clone());
    }
    list
}

const MOD_SOURCES: [ModulationSource; 4] = [
    ModulationSource::Depth,
    ModulationSource::Speed,
    ModulationSource::Acc,
    ModulationSource::Recoil,
];

pub fn mod_source_list() -> &'static [ModulationSource; 4] {
    &MOD_SOURCES
}

pub fn cycle_mode(m: ContactMode) -> ContactMode {
    match m {
        ContactMode::Depth => ContactMode::Speed,
        ContactMode::Speed => ContactMode::Acc,
        ContactMode::Acc => ContactMode::Recoil,
        ContactMode::Recoil => ContactMode::Depth,
    }
}

pub fn cycle_aggregation(m: &AggregationMode) -> AggregationMode {
    match m {
        AggregationMode::Max => AggregationMode::Sum,
        AggregationMode::Sum => AggregationMode::Average,
        AggregationMode::Average => AggregationMode::Max,
    }
}

pub fn agg_name(m: &AggregationMode) -> &'static str {
    match m {
        AggregationMode::Max => "max",
        AggregationMode::Sum => "sum",
        AggregationMode::Average => "avg",
    }
}

pub fn aggregation_modes() -> &'static [AggregationMode; 3] {
    &[
        AggregationMode::Max,
        AggregationMode::Sum,
        AggregationMode::Average,
    ]
}

pub fn mod_kind_name(k: ModKind) -> &'static str {
    match k {
        ModKind::Freq => "freq",
        ModKind::Intensity => "int",
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChannelControl {
    Freq(Channel, usize),
    Intensity(Channel, usize),
    LimitMin(Channel),
    LimitMax(Channel),
    Aggregation(Channel),
    Save,
    Load,
}

impl ChannelControl {
    pub fn adjust_action(self, d: i32) -> Option<Action> {
        match self {
            ChannelControl::Freq(ch, seg) => Some(Action::StepFreq(ch, seg, d)),
            ChannelControl::Intensity(ch, seg) => Some(Action::StepIntensity(ch, seg, d)),
            ChannelControl::LimitMin(ch) => Some(Action::StepLimitMin(ch, d)),
            ChannelControl::LimitMax(ch) => Some(Action::StepLimitMax(ch, d)),
            ChannelControl::Aggregation(ch) => Some(Action::CycleAggregation(ch)),
            _ => None,
        }
    }

    pub fn activate_action(self) -> Option<Action> {
        match self {
            ChannelControl::Aggregation(ch) => Some(Action::CycleAggregation(ch)),
            ChannelControl::Save => Some(Action::SaveConfig),
            ChannelControl::Load => Some(Action::LoadConfig),
            _ => None,
        }
    }
}

pub fn channel_controls() -> Vec<ChannelControl> {
    let mut v = Vec::new();
    for ch in [Channel::A, Channel::B] {
        for seg in 0..4 {
            v.push(ChannelControl::Freq(ch, seg));
        }
        for seg in 0..4 {
            v.push(ChannelControl::Intensity(ch, seg));
        }
        v.push(ChannelControl::LimitMin(ch));
        v.push(ChannelControl::LimitMax(ch));
        v.push(ChannelControl::Aggregation(ch));
    }
    v.push(ChannelControl::Save);
    v.push(ChannelControl::Load);
    v
}

pub fn channel_control_row(ctrl: ChannelControl) -> Option<u16> {
    match ctrl {
        ChannelControl::Freq(_, seg) => Some(1 + seg as u16),
        ChannelControl::Intensity(_, seg) => Some(6 + seg as u16),
        ChannelControl::LimitMin(_) => Some(11),
        ChannelControl::LimitMax(_) => Some(12),
        ChannelControl::Aggregation(_) => Some(13),
        ChannelControl::Save | ChannelControl::Load => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TuningControl {
    Ukf(UkfField),
    UkfReset,
    Norm(NormField),
    NormReset,
}

impl TuningControl {
    pub fn adjust_action(self, d: i32) -> Option<Action> {
        match self {
            TuningControl::Ukf(f) => Some(Action::StepUkf(f, d)),
            TuningControl::Norm(f) => Some(Action::StepNorm(f, d)),
            _ => None,
        }
    }

    pub fn activate_action(self) -> Option<Action> {
        match self {
            TuningControl::UkfReset => Some(Action::ResetUkf),
            TuningControl::NormReset => Some(Action::ResetNorms),
            _ => None,
        }
    }
}

pub fn tuning_controls() -> Vec<TuningControl> {
    vec![
        TuningControl::Ukf(UkfField::Q),
        TuningControl::Ukf(UkfField::R),
        TuningControl::Ukf(UkfField::Alpha),
        TuningControl::Ukf(UkfField::Beta),
        TuningControl::Ukf(UkfField::Kappa),
        TuningControl::UkfReset,
        TuningControl::Norm(NormField::Speed),
        TuningControl::Norm(NormField::Acc),
        TuningControl::Norm(NormField::Recoil),
        TuningControl::NormReset,
    ]
}

pub fn tuning_control_row(ctrl: TuningControl) -> u16 {
    match ctrl {
        TuningControl::Ukf(UkfField::Q) => 0,
        TuningControl::Ukf(UkfField::R) => 1,
        TuningControl::Ukf(UkfField::Alpha) => 2,
        TuningControl::Ukf(UkfField::Beta) => 3,
        TuningControl::Ukf(UkfField::Kappa) => 4,
        TuningControl::UkfReset => 5,
        TuningControl::Norm(NormField::Speed) => 0,
        TuningControl::Norm(NormField::Acc) => 1,
        TuningControl::Norm(NormField::Recoil) => 2,
        TuningControl::NormReset => 3,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetupControl {
    AutoSave,
    AlarmTab,
    Tutorial,
}

impl SetupControl {
    pub const ALL: [SetupControl; 3] = [
        SetupControl::AutoSave,
        SetupControl::AlarmTab,
        SetupControl::Tutorial,
    ];

    pub fn adjust_action(self, d: i32) -> Option<Action> {
        match self {
            SetupControl::AutoSave => Some(Action::SetAutoSave(d > 0)),
            SetupControl::AlarmTab => Some(Action::SetAlarmTabVisible(d > 0)),
            SetupControl::Tutorial => None,
        }
    }

    pub fn activate_action(self) -> Option<Action> {
        match self {
            SetupControl::AutoSave => Some(Action::ToggleAutoSave),
            SetupControl::AlarmTab => Some(Action::ToggleAlarmTabVisible),
            SetupControl::Tutorial => Some(Action::TutorialStart),
        }
    }
}

pub fn setup_controls() -> &'static [SetupControl; 3] {
    &SetupControl::ALL
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlarmControl {
    Enabled,
    Field(AlarmField),
    Channels,
    Test,
    Stop,
    Snooze,
}

impl AlarmControl {
    pub fn adjust_action(self, d: i32) -> Option<Action> {
        match self {
            AlarmControl::Enabled => Some(Action::SetAlarmEnabled(d > 0)),
            AlarmControl::Field(f) => Some(Action::StepAlarmField(f, d)),
            AlarmControl::Channels => Some(Action::CycleAlarmChannels(d)),
            _ => None,
        }
    }

    pub fn activate_action(self) -> Option<Action> {
        match self {
            AlarmControl::Enabled => Some(Action::ToggleAlarmEnabled),
            AlarmControl::Channels => Some(Action::CycleAlarmChannels(1)),
            AlarmControl::Test => Some(Action::AlarmTest),
            AlarmControl::Stop => Some(Action::AlarmStop),
            AlarmControl::Snooze => Some(Action::AlarmSnooze),
            AlarmControl::Field(_) => None,
        }
    }
}

pub const ALARM_SCHEDULE_FIELDS: [AlarmField; 2] = [AlarmField::Hour, AlarmField::Minute];
pub const ALARM_PATTERN_FIELDS: [AlarmField; 7] = [
    AlarmField::StartStrength,
    AlarmField::PeakStrength,
    AlarmField::Ramp,
    AlarmField::MaxDuration,
    AlarmField::Repeats,
    AlarmField::PulseOn,
    AlarmField::PulseOff,
];

pub fn alarm_controls() -> Vec<AlarmControl> {
    let mut v = vec![AlarmControl::Enabled];
    v.extend(ALARM_SCHEDULE_FIELDS.map(AlarmControl::Field));
    v.push(AlarmControl::Channels);
    v.extend(ALARM_PATTERN_FIELDS.map(AlarmControl::Field));
    v.push(AlarmControl::Field(AlarmField::Snooze));
    v.push(AlarmControl::Test);
    v.push(AlarmControl::Stop);
    v.push(AlarmControl::Snooze);
    v
}

pub fn alarm_control_row(ctrl: AlarmControl) -> Option<u16> {
    match ctrl {
        AlarmControl::Enabled => Some(0),
        AlarmControl::Channels => Some(3),
        AlarmControl::Field(f) => {
            if let Some(i) = ALARM_SCHEDULE_FIELDS.iter().position(|x| *x == f) {
                return Some(1 + i as u16);
            }
            if let Some(i) = ALARM_PATTERN_FIELDS.iter().position(|x| *x == f) {
                return Some(i as u16);
            }
            Some(ALARM_PATTERN_FIELDS.len() as u16)
        }
        AlarmControl::Test | AlarmControl::Stop | AlarmControl::Snooze => None,
    }
}

pub fn mod_controls_len() -> usize {
    use super::ModParam;
    2 + ModParam::ALL.len() + 3
}

pub fn mod_slot_index(ch: Channel, kind: ModKind, seg: usize) -> u16 {
    let ch_off = match ch {
        Channel::A => 0,
        Channel::B => 8,
    };
    let kind_off = match kind {
        ModKind::Freq => 0,
        ModKind::Intensity => 4,
    };
    ch_off + kind_off + seg as u16
}
