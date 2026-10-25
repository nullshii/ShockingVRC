use shocking_vrc_core::cli::{AlarmConfig, EngineMode, MotionNorms, UkfConfig};
use shocking_vrc_core::modulation::config::{
    ModulationConfig, ModulationFunction, ModulationSource,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Channel {
    A,
    B,
}

impl Channel {
    pub const ALL: [Channel; 2] = [Channel::A, Channel::B];

    pub fn label(self) -> &'static str {
        match self {
            Channel::A => "A",
            Channel::B => "B",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModKind {
    Freq,
    Intensity,
}

impl ModKind {
    pub const ALL: [ModKind; 2] = [ModKind::Freq, ModKind::Intensity];

    pub fn short(self) -> &'static str {
        match self {
            ModKind::Freq => "freq",
            ModKind::Intensity => "int",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            ModKind::Freq => "Frequency",
            ModKind::Intensity => "Intensity",
        }
    }

    pub fn is_intensity(self) -> bool {
        self == ModKind::Intensity
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    Status,
    Zones,
    Channels,
    Tuning,
    Modulation,
    Log,
    Presets,
    Setup,
    Alarm,
    Graph,
}

impl Section {
    pub const ALL: [Section; 10] = [
        Section::Status,
        Section::Graph,
        Section::Zones,
        Section::Channels,
        Section::Tuning,
        Section::Modulation,
        Section::Log,
        Section::Presets,
        Section::Setup,
        Section::Alarm,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Section::Status => "Status",
            Section::Zones => "Zones",
            Section::Channels => "Channels",
            Section::Tuning => "Tuning",
            Section::Modulation => "Modulation",
            Section::Log => "Log",
            Section::Presets => "Presets",
            Section::Setup => "Setup",
            Section::Alarm => "Alarm",
            Section::Graph => "Graph",
        }
    }

    pub fn classic_only(self) -> bool {
        matches!(
            self,
            Section::Zones
                | Section::Channels
                | Section::Tuning
                | Section::Modulation
                | Section::Presets
        )
    }

    pub fn in_mode(self, mode: EngineMode) -> bool {
        match self {
            Section::Graph => mode == EngineMode::Graph,
            s if s.classic_only() => mode == EngineMode::Classic,
            _ => true,
        }
    }

    pub fn for_mode(mode: EngineMode) -> impl Iterator<Item = Section> {
        Section::ALL.into_iter().filter(move |s| s.in_mode(mode))
    }

    pub fn key(self) -> char {
        match self {
            Section::Status => '1',
            Section::Zones => '2',
            Section::Channels => '3',
            Section::Tuning => '4',
            Section::Modulation => '5',
            Section::Log => '6',
            Section::Presets => '7',
            Section::Setup => '8',
            Section::Alarm => '9',
            Section::Graph => '2',
        }
    }

    pub fn blurb(self) -> &'static str {
        match self {
            Section::Status => "Live output for both channels, device health and OSC link.",
            Section::Zones => "Map avatar contacts onto channels and calibrate each one.",
            Section::Channels => "The four-segment waveform, power ceiling and zone mixing.",
            Section::Tuning => "Kalman smoothing and the motion scale factors.",
            Section::Modulation => "Give frequency and intensity movement over time.",
            Section::Log => "Everything the engine has reported this session.",
            Section::Presets => "Waveform presets from the catalogue and your own.",
            Section::Setup => "Connection, preferences and the guided tour.",
            Section::Alarm => "Wake-up alarm — drives every connected device.",
            Section::Graph => "Wire avatar inputs through nodes into each device.",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UkfField {
    Q,
    R,
    Alpha,
    Beta,
    Kappa,
}

impl UkfField {
    pub const ALL: [UkfField; 5] = [
        UkfField::Q,
        UkfField::R,
        UkfField::Alpha,
        UkfField::Beta,
        UkfField::Kappa,
    ];

    pub fn label(self) -> &'static str {
        match self {
            UkfField::Q => "Q (process)",
            UkfField::R => "R (measure)",
            UkfField::Alpha => "Alpha (σ spread)",
            UkfField::Beta => "Beta (prior)",
            UkfField::Kappa => "Kappa",
        }
    }

    pub fn hint(self) -> &'static str {
        match self {
            UkfField::Q => "Lower is smoother but slower to react",
            UkfField::R => "How noisy incoming contact data is assumed to be",
            UkfField::Alpha => "Spread of the sigma points around the mean",
            UkfField::Beta => "Prior knowledge of the distribution (2 = Gaussian)",
            UkfField::Kappa => "Secondary scaling parameter",
        }
    }

    pub fn get(self, p: &UkfConfig) -> f32 {
        match self {
            UkfField::Q => p.q,
            UkfField::R => p.r,
            UkfField::Alpha => p.alpha,
            UkfField::Beta => p.beta,
            UkfField::Kappa => p.kappa,
        }
    }

    pub fn step(self) -> f32 {
        match self {
            UkfField::Q | UkfField::R => 0.001,
            UkfField::Alpha => 0.05,
            UkfField::Beta | UkfField::Kappa => 0.1,
        }
    }

    pub fn range(self) -> (f32, f32) {
        match self {
            UkfField::Q | UkfField::R => (0.0001, 0.5),
            UkfField::Alpha => (0.001, 2.0),
            UkfField::Beta => (0.0, 5.0),
            UkfField::Kappa => (-3.0, 3.0),
        }
    }

    pub fn format(self, p: &UkfConfig) -> String {
        match self {
            UkfField::Q | UkfField::R => format!("{:.4}", self.get(p)),
            _ => format!("{:.3}", self.get(p)),
        }
    }

    pub fn set(self, p: &mut UkfConfig, v: f32) {
        let v = round3(v);
        match self {
            UkfField::Q => p.q = v.max(0.0001),
            UkfField::R => p.r = v.max(0.0001),
            UkfField::Alpha => p.alpha = v.max(0.001),
            UkfField::Beta => p.beta = v.max(0.0),
            UkfField::Kappa => p.kappa = v,
        }
    }

    pub fn nudge(self, p: &mut UkfConfig, delta: i32) {
        let v = self.get(p) + delta as f32 * self.step();
        self.set(p, v);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NormField {
    Speed,
    Acc,
    Recoil,
}

impl NormField {
    pub const ALL: [NormField; 3] = [NormField::Speed, NormField::Acc, NormField::Recoil];

    pub fn label(self) -> &'static str {
        match self {
            NormField::Speed => "Speed ÷",
            NormField::Acc => "Accel ÷",
            NormField::Recoil => "Recoil ÷",
        }
    }

    pub fn hint(self) -> &'static str {
        match self {
            NormField::Speed => "Divisor for speed mode — higher is less sensitive",
            NormField::Acc => "Divisor for acceleration mode",
            NormField::Recoil => "Divisor for recoil (pull-out) mode",
        }
    }

    pub fn get(self, n: &MotionNorms) -> f32 {
        match self {
            NormField::Speed => n.speed,
            NormField::Acc => n.acc,
            NormField::Recoil => n.recoil,
        }
    }

    pub fn step(self) -> f32 {
        match self {
            NormField::Speed => 0.5,
            NormField::Acc => 1.0,
            NormField::Recoil => 5.0,
        }
    }

    pub fn range(self) -> (f32, f32) {
        match self {
            NormField::Speed => (0.001, 40.0),
            NormField::Acc => (0.001, 200.0),
            NormField::Recoil => (0.001, 600.0),
        }
    }

    pub fn set(self, n: &mut MotionNorms, v: f32) {
        let v = round3(v).max(0.001);
        match self {
            NormField::Speed => n.speed = v,
            NormField::Acc => n.acc = v,
            NormField::Recoil => n.recoil = v,
        }
    }

    pub fn nudge(self, n: &mut MotionNorms, delta: i32) {
        let v = self.get(n) + delta as f32 * self.step();
        self.set(n, v);
    }
}

pub fn round3(v: f32) -> f32 {
    (v * 1000.0).round() / 1000.0
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
    pub const SCHEDULE: [AlarmField; 2] = [AlarmField::Hour, AlarmField::Minute];
    pub const PATTERN: [AlarmField; 7] = [
        AlarmField::StartStrength,
        AlarmField::PeakStrength,
        AlarmField::Ramp,
        AlarmField::MaxDuration,
        AlarmField::Repeats,
        AlarmField::PulseOn,
        AlarmField::PulseOff,
    ];

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

    pub fn hint(self) -> &'static str {
        match self {
            AlarmField::Hour | AlarmField::Minute => "When the alarm fires, local time",
            AlarmField::StartStrength => "Power the ring opens at",
            AlarmField::PeakStrength => "Power it climbs to — ignores the channel ceiling",
            AlarmField::Ramp => "How long the climb from start to peak takes",
            AlarmField::MaxDuration => "How long one attempt rings before giving up",
            AlarmField::Repeats => "How many attempts before it stops for good",
            AlarmField::PulseOn => "Length of each pulse",
            AlarmField::PulseOff => "Gap between pulses — 0 rings steadily",
            AlarmField::Snooze => "Gap before the next attempt",
        }
    }

    pub fn step(self) -> i32 {
        match self {
            AlarmField::Hour
            | AlarmField::Minute
            | AlarmField::Snooze
            | AlarmField::Repeats
            | AlarmField::StartStrength
            | AlarmField::PeakStrength => 1,
            AlarmField::Ramp => 10,
            AlarmField::MaxDuration => 30,
            AlarmField::PulseOn | AlarmField::PulseOff => 100,
        }
    }

    pub fn range(self) -> (i32, i32) {
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

    fn put(self, cfg: &mut AlarmConfig, v: i32) {
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

    pub fn set(self, cfg: &mut AlarmConfig, v: i32) {
        let (min, max) = self.range();
        let v = if self.wraps() {
            let span = max - min + 1;
            min + (v - min).rem_euclid(span)
        } else {
            v.clamp(min, max)
        };
        self.put(cfg, v);
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

    pub fn nudge(self, cfg: &mut AlarmConfig, delta: i32) {
        self.set(cfg, self.get(cfg) + delta * self.step());
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

pub fn format_mins(mins: u32) -> String {
    if mins == 0 {
        "less than a minute".to_string()
    } else if mins < 60 {
        format!("{mins} min")
    } else {
        let (h, m) = (mins / 60, mins % 60);
        if m == 0 {
            format!("{h} h")
        } else {
            format!("{h} h {m} min")
        }
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

    pub fn hint(self) -> &'static str {
        match self {
            ModParam::BaseSpeed => "Free-running rate, in cycles per second",
            ModParam::Sensitivity => "How much the source drives the rate on top of Speed",
            ModParam::MaxDeviation => "Peak swing away from the base value",
            ModParam::Phase => "Starting offset, in cycles",
            ModParam::FreqMul => "Multiplies the waveform's own frequency",
            ModParam::Offset => "Constant shift applied after the function",
            ModParam::Power => "Exponent applied to the function's output",
            ModParam::ClampMin => "Hard floor for the result",
            ModParam::ClampMax => "Hard ceiling for the result",
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

    pub fn range(self, kind: ModKind) -> (f32, f32) {
        let (lo, hi) = ModulationConfig::output_bounds(kind.is_intensity());
        match self {
            ModParam::BaseSpeed => (0.0, 10.0),
            ModParam::Sensitivity => (0.0, 10.0),
            ModParam::MaxDeviation => (0.0, if kind.is_intensity() { 100.0 } else { 245.0 }),
            ModParam::Phase => (0.0, 1.0),
            ModParam::FreqMul => (0.001, 8.0),
            ModParam::Offset => (-hi, hi),
            ModParam::Power => (0.001, 6.0),
            ModParam::ClampMin | ModParam::ClampMax => (lo, hi),
        }
    }

    pub fn clamp_value(self, v: f32, kind: ModKind) -> f32 {
        let (lo, hi) = ModulationConfig::output_bounds(kind.is_intensity());
        match self {
            ModParam::Offset => v.clamp(-hi, hi),
            ModParam::ClampMin | ModParam::ClampMax => v.clamp(lo, hi),
            ModParam::BaseSpeed
            | ModParam::Sensitivity
            | ModParam::MaxDeviation
            | ModParam::Power => v.max(0.0),
            ModParam::Phase => v,
            ModParam::FreqMul => v.max(0.001),
        }
    }

    pub fn format(self, c: &ModulationConfig) -> String {
        let v = self.get(c);
        match self {
            ModParam::BaseSpeed => format!("{v:.2} Hz"),
            ModParam::Phase => format!("{v:.2} cyc"),
            ModParam::MaxDeviation => format!("±{v:.1}"),
            ModParam::Offset | ModParam::ClampMin | ModParam::ClampMax => format!("{v:.1}"),
            _ => format!("{v:.3}"),
        }
    }
}

pub fn mod_function_list() -> Vec<ModulationFunction> {
    use ModulationFunction::*;
    vec![
        None,
        Sin,
        Cos,
        Tan,
        SinCos,
        Sin2,
        Cos2,
        SinPlusCos,
        Sinh,
        Cosh,
        Tanh,
        Square,
        Cube,
        Pow4,
        Sqrt,
        Cbrt,
        Abs,
        Sign,
        Exp,
        ExpNeg,
        Pow2x,
        Pow10x,
        Ln,
        Log2,
        Log10,
        Triangle,
        Saw,
        ReverseSaw,
        SquareWave,
        Pulse,
        Bounce,
        Sigmoid,
        SmoothStep,
        SmootherStep,
        Logistic,
        SoftSign,
        Perlin,
        Simplex,
        Fractal,
        ValueNoise,
        SinPlusNoise,
        SinTimesNoise,
        TrianglePlusSin,
        SquareTimesSigmoid,
    ]
}

pub fn mod_function_list_for(current: &ModulationFunction) -> Vec<ModulationFunction> {
    let mut list = mod_function_list();
    if !list.iter().any(|f| f == current) {
        list.insert(1, current.clone());
    }
    list
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FunctionGroup {
    Off,
    Trig,
    Hyperbolic,
    Power,
    Exponential,
    Logarithmic,
    Wave,
    Curve,
    Noise,
    Blend,
    Other,
}

impl FunctionGroup {
    pub const ALL: [FunctionGroup; 11] = [
        FunctionGroup::Off,
        FunctionGroup::Trig,
        FunctionGroup::Hyperbolic,
        FunctionGroup::Power,
        FunctionGroup::Exponential,
        FunctionGroup::Logarithmic,
        FunctionGroup::Wave,
        FunctionGroup::Curve,
        FunctionGroup::Noise,
        FunctionGroup::Blend,
        FunctionGroup::Other,
    ];

    pub fn label(self) -> &'static str {
        match self {
            FunctionGroup::Off => "Off",
            FunctionGroup::Trig => "Trigonometric",
            FunctionGroup::Hyperbolic => "Hyperbolic",
            FunctionGroup::Power => "Powers & roots",
            FunctionGroup::Exponential => "Exponential",
            FunctionGroup::Logarithmic => "Logarithmic",
            FunctionGroup::Wave => "Waveshapes",
            FunctionGroup::Curve => "Curves & steps",
            FunctionGroup::Noise => "Noise",
            FunctionGroup::Blend => "Blends",
            FunctionGroup::Other => "Other",
        }
    }
}

pub fn function_group(f: &ModulationFunction) -> FunctionGroup {
    use ModulationFunction as F;
    match f {
        F::None => FunctionGroup::Off,
        F::Sin | F::Cos | F::Tan | F::Asin | F::Acos | F::Atan | F::SinCos | F::Sin2 | F::Cos2
        | F::SinPlusCos | F::SinPow(_) | F::CosPow(_) => FunctionGroup::Trig,
        F::Sinh | F::Cosh | F::Tanh => FunctionGroup::Hyperbolic,
        F::Square | F::Cube | F::Pow4 | F::Sqrt | F::Cbrt | F::Abs | F::Sign => {
            FunctionGroup::Power
        }
        F::Exp | F::ExpNeg | F::Pow2x | F::Pow10x => FunctionGroup::Exponential,
        F::Ln | F::Log2 | F::Log10 => FunctionGroup::Logarithmic,
        F::Triangle | F::Saw | F::ReverseSaw | F::SquareWave | F::Pulse | F::Bounce => {
            FunctionGroup::Wave
        }
        F::Sigmoid | F::SmoothStep | F::SmootherStep | F::Logistic | F::SoftSign => {
            FunctionGroup::Curve
        }
        F::Perlin | F::Simplex | F::Fractal | F::ValueNoise => FunctionGroup::Noise,
        F::SinPlusNoise | F::SinTimesNoise | F::TrianglePlusSin | F::SquareTimesSigmoid => {
            FunctionGroup::Blend
        }
        F::Compose { .. } => FunctionGroup::Other,
    }
}

pub const MOD_SOURCES: [ModulationSource; 4] = [
    ModulationSource::Depth,
    ModulationSource::Speed,
    ModulationSource::Acc,
    ModulationSource::Recoil,
];

pub fn source_label(s: ModulationSource) -> &'static str {
    match s {
        ModulationSource::Depth => "depth",
        ModulationSource::Speed => "speed",
        ModulationSource::Acc => "acc",
        ModulationSource::Recoil => "recoil",
    }
}

pub fn default_mod_config(kind: ModKind) -> ModulationConfig {
    let mut c = ModulationConfig::default();
    if kind.is_intensity() {
        c.max_deviation = 10.0;
        c.clamp_min = 0.0;
        c.clamp_max = 100.0;
    }
    c.sanitise(kind.is_intensity());
    c
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alarm_hour_wraps_both_ways() {
        let mut cfg = AlarmConfig::default();
        AlarmField::Hour.set(&mut cfg, 23);
        AlarmField::Hour.nudge(&mut cfg, 1);
        assert_eq!(cfg.hour, 0, "23:00 + 1 h wraps to midnight");
        AlarmField::Hour.nudge(&mut cfg, -1);
        assert_eq!(cfg.hour, 23);
    }

    #[test]
    fn alarm_power_fields_stay_ordered() {
        let mut cfg = AlarmConfig::default();
        AlarmField::PeakStrength.set(&mut cfg, 40);
        AlarmField::StartStrength.set(&mut cfg, 90);
        assert!(
            cfg.peak_strength >= cfg.start_strength,
            "raising the start must carry the peak with it"
        );

        AlarmField::StartStrength.set(&mut cfg, 50);
        AlarmField::PeakStrength.set(&mut cfg, 10);
        assert!(
            cfg.start_strength <= cfg.peak_strength,
            "lowering the peak must pull the start down"
        );
    }

    #[test]
    fn alarm_non_wrapping_fields_clamp() {
        let mut cfg = AlarmConfig::default();
        AlarmField::Repeats.set(&mut cfg, 999);
        assert_eq!(cfg.repeats, AlarmConfig::MAX_REPEATS);
        AlarmField::Repeats.set(&mut cfg, -5);
        assert_eq!(cfg.repeats, 1);
    }

    #[test]
    fn ukf_nudges_match_the_terminal_ui_steps() {
        let mut p = UkfConfig::default();
        let q0 = p.q;
        UkfField::Q.nudge(&mut p, 1);
        assert!((p.q - round3(q0 + 0.001)).abs() < 1e-6);

        UkfField::Kappa.set(&mut p, -1.5);
        assert!((p.kappa + 1.5).abs() < 1e-6);
        UkfField::Q.set(&mut p, -1.0);
        assert!(p.q > 0.0, "Q is floored above zero");
    }

    #[test]
    fn norm_nudges_use_their_own_scale() {
        let mut n = MotionNorms::default();
        NormField::Recoil.nudge(&mut n, 1);
        assert!((n.recoil - 105.0).abs() < 1e-4, "recoil steps by 5");
        NormField::Speed.nudge(&mut n, -1);
        assert!((n.speed - 4.5).abs() < 1e-4, "speed steps by 0.5");
    }

    #[test]
    fn mod_param_ranges_cover_their_defaults() {
        for kind in ModKind::ALL {
            let c = default_mod_config(kind);
            for p in ModParam::ALL {
                let (lo, hi) = p.range(kind);
                let v = p.get(&c);
                assert!(
                    v >= lo - 1e-3 && v <= hi + 1e-3,
                    "{:?}/{:?}: default {v} outside [{lo}, {hi}]",
                    kind,
                    p
                );
            }
        }
    }

    #[test]
    fn function_list_always_contains_the_current_selection() {
        let composed = ModulationFunction::Compose {
            outer: Box::new(ModulationFunction::Sin),
            inner: Box::new(ModulationFunction::Triangle),
        };
        let list = mod_function_list_for(&composed);
        assert!(list.contains(&composed), "a loaded config stays selectable");
        assert_eq!(function_group(&composed), FunctionGroup::Other);
    }

    #[test]
    fn every_listed_function_lands_in_a_group() {
        for f in mod_function_list() {
            let g = function_group(&f);
            assert!(
                FunctionGroup::ALL.contains(&g),
                "{f} fell outside the group list"
            );
        }
    }

    #[test]
    fn section_keys_are_unique() {
        for mode in [EngineMode::Classic, EngineMode::Graph] {
            let sections: Vec<Section> = Section::for_mode(mode).collect();
            let mut keys: Vec<char> = sections.iter().map(|s| s.key()).collect();
            keys.sort_unstable();
            keys.dedup();
            assert_eq!(keys.len(), sections.len(), "{mode:?}");
        }
    }

    #[test]
    fn each_mode_shows_its_own_sections() {
        let classic: Vec<Section> = Section::for_mode(EngineMode::Classic).collect();
        let graph: Vec<Section> = Section::for_mode(EngineMode::Graph).collect();
        assert_eq!(classic.len(), 9, "Classic navigation is unchanged");
        assert!(!classic.contains(&Section::Graph));
        assert!(graph.contains(&Section::Graph));
        for s in [Section::Status, Section::Log, Section::Setup, Section::Alarm] {
            assert!(classic.contains(&s) && graph.contains(&s), "{s:?} is shared");
        }
        assert!(graph.iter().all(|s| !s.classic_only()));
    }

    #[test]
    fn duration_formatting_reads_naturally() {
        assert_eq!(format_secs(45), "45 s");
        assert_eq!(format_secs(120), "2 min");
        assert_eq!(format_secs(90), "1:30 min");
        assert_eq!(format_mins(0), "less than a minute");
        assert_eq!(format_mins(90), "1 h 30 min");
    }
}
