use std::borrow::Cow;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::OldZoneType;
use crate::cli::config::{MotionNorms, UkfConfig, ZoneId};
use crate::modulation::config::ModulationFunction;

use super::curve::Curve;
use super::device::{self, ChannelParams, DeviceBinding};
use super::model::Graph;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PinType {
    Float,
    Bool,
}

pub fn as_bool(v: f32) -> bool {
    v > 0.5
}

pub fn from_bool(b: bool) -> f32 {
    if b { 1.0 } else { 0.0 }
}

#[derive(Debug, Clone, PartialEq)]
pub struct PinSpec {
    pub key: Cow<'static, str>,
    pub label: Cow<'static, str>,
    pub ty: PinType,
    pub default: f32,
    pub range: (f32, f32),
}

fn fl(key: &'static str, label: &'static str, default: f32, range: (f32, f32)) -> PinSpec {
    PinSpec {
        key: Cow::Borrowed(key),
        label: Cow::Borrowed(label),
        ty: PinType::Float,
        default,
        range,
    }
}

fn bl(key: &'static str, label: &'static str, default: f32) -> PinSpec {
    PinSpec {
        key: Cow::Borrowed(key),
        label: Cow::Borrowed(label),
        ty: PinType::Bool,
        default,
        range: (0.0, 1.0),
    }
}

fn unit(key: &'static str, label: &'static str) -> PinSpec {
    fl(key, label, 0.0, (0.0, 1.0))
}

fn owned(key: String, label: String, default: f32, range: (f32, f32)) -> PinSpec {
    PinSpec {
        key: Cow::Owned(key),
        label: Cow::Owned(label),
        ty: PinType::Float,
        default,
        range,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Category {
    Input,
    Conditioning,
    Math,
    Generator,
    Logic,
    Safety,
    Output,
    Layout,
}

impl Category {
    pub const ALL: [Category; 8] = [
        Category::Input,
        Category::Conditioning,
        Category::Math,
        Category::Generator,
        Category::Logic,
        Category::Safety,
        Category::Output,
        Category::Layout,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Category::Input => "Inputs",
            Category::Conditioning => "Conditioning",
            Category::Math => "Math & curves",
            Category::Generator => "Generators & time",
            Category::Logic => "Logic & routing",
            Category::Safety => "Safety",
            Category::Output => "Output",
            Category::Layout => "Layout",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct NoParams {}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct OscFloatParams {
    pub address: String,
    pub stale_ms: u32,
}

impl Default for OscFloatParams {
    fn default() -> Self {
        Self {
            address: "/avatar/parameters/".into(),
            stale_ms: 5000,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ContactParams {
    pub zone: ZoneId,
    pub stale_ms: u32,
    pub spoof_guard: bool,
}

impl Default for ContactParams {
    fn default() -> Self {
        Self {
            zone: ZoneId::new(OldZoneType::Pen, "*"),
            stale_ms: 500,
            spoof_guard: true,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ConstantParams {
    pub value: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SliderParams {
    pub value: f32,
    pub min: f32,
    pub max: f32,
    pub step: f32,
    pub key_up: String,
    pub key_down: String,
}

impl Default for SliderParams {
    fn default() -> Self {
        Self {
            value: 0.0,
            min: 0.0,
            max: 1.0,
            step: 0.05,
            key_up: String::new(),
            key_down: String::new(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ToggleParams {
    pub on: bool,
    pub momentary: bool,
    pub key: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct UkfNodeParams {
    pub ukf: UkfConfig,
    pub norms: MotionNorms,
}

pub fn sanitised_ukf(c: UkfConfig) -> UkfConfig {
    let fin = |v: f32, d: f32| if v.is_finite() { v } else { d };
    let d = UkfConfig::default();
    UkfConfig {
        q: fin(c.q, d.q).max(0.0001),
        r: fin(c.r, d.r).max(0.0001),
        alpha: fin(c.alpha, d.alpha).max(0.001),
        beta: fin(c.beta, d.beta).max(0.0),
        kappa: fin(c.kappa, d.kappa),
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct IntegrateParams {
    pub leak: f32,
    pub min: f32,
    pub max: f32,
}

impl Default for IntegrateParams {
    fn default() -> Self {
        Self {
            leak: 0.0,
            min: 0.0,
            max: 1.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SmoothParams {
    pub attack_ms: f32,
    pub release_ms: f32,
}

impl Default for SmoothParams {
    fn default() -> Self {
        Self {
            attack_ms: 80.0,
            release_ms: 250.0,
        }
    }
}

pub const TICK_MS: f32 = 25.0;
pub const MAX_BUFFER: usize = 400;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MovingAverageParams {
    pub window_ms: f32,
}

impl Default for MovingAverageParams {
    fn default() -> Self {
        Self { window_ms: 500.0 }
    }
}

impl MovingAverageParams {
    pub fn samples(&self) -> usize {
        ((self.window_ms / TICK_MS).round() as usize).clamp(1, MAX_BUFFER)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct NormalizeParams {
    pub divisor: f32,
    pub abs: bool,
    pub clamp: bool,
}

impl Default for NormalizeParams {
    fn default() -> Self {
        Self {
            divisor: 5.0,
            abs: true,
            clamp: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SpoofGuardParams {
    pub jump_limit: f32,
    pub release_level: f32,
}

impl Default for SpoofGuardParams {
    fn default() -> Self {
        Self {
            jump_limit: 0.6,
            release_level: 0.02,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum MathOp {
    #[default]
    Add,
    Subtract,
    Multiply,
    Divide,
    Min,
    Max,
    Pow,
    Mod,
    Abs,
    Negate,
    Invert,
    Sqrt,
    Round,
    Floor,
    Ceil,
}

impl MathOp {
    pub const ALL: [MathOp; 15] = [
        MathOp::Add,
        MathOp::Subtract,
        MathOp::Multiply,
        MathOp::Divide,
        MathOp::Min,
        MathOp::Max,
        MathOp::Pow,
        MathOp::Mod,
        MathOp::Abs,
        MathOp::Negate,
        MathOp::Invert,
        MathOp::Sqrt,
        MathOp::Round,
        MathOp::Floor,
        MathOp::Ceil,
    ];

    pub fn label(self) -> &'static str {
        match self {
            MathOp::Add => "Add",
            MathOp::Subtract => "Subtract",
            MathOp::Multiply => "Multiply",
            MathOp::Divide => "Divide",
            MathOp::Min => "Min",
            MathOp::Max => "Max",
            MathOp::Pow => "Power",
            MathOp::Mod => "Modulo",
            MathOp::Abs => "Abs",
            MathOp::Negate => "Negate",
            MathOp::Invert => "Invert (1−x)",
            MathOp::Sqrt => "Sqrt",
            MathOp::Round => "Round",
            MathOp::Floor => "Floor",
            MathOp::Ceil => "Ceil",
        }
    }

    pub fn is_unary(self) -> bool {
        matches!(
            self,
            MathOp::Abs
                | MathOp::Negate
                | MathOp::Invert
                | MathOp::Sqrt
                | MathOp::Round
                | MathOp::Floor
                | MathOp::Ceil
        )
    }

    fn b_default(self) -> f32 {
        match self {
            MathOp::Multiply | MathOp::Divide | MathOp::Pow | MathOp::Mod => 1.0,
            _ => 0.0,
        }
    }

    pub fn apply(self, a: f32, b: f32) -> f32 {
        match self {
            MathOp::Add => a + b,
            MathOp::Subtract => a - b,
            MathOp::Multiply => a * b,
            MathOp::Divide => {
                if b.abs() < 1e-9 {
                    0.0
                } else {
                    a / b
                }
            }
            MathOp::Min => a.min(b),
            MathOp::Max => a.max(b),
            MathOp::Pow => {
                if a < 0.0 && b.fract() != 0.0 {
                    -(-a).powf(b)
                } else {
                    a.powf(b)
                }
            }
            MathOp::Mod => {
                if b.abs() < 1e-9 {
                    0.0
                } else {
                    a.rem_euclid(b)
                }
            }
            MathOp::Abs => a.abs(),
            MathOp::Negate => -a,
            MathOp::Invert => 1.0 - a,
            MathOp::Sqrt => a.max(0.0).sqrt(),
            MathOp::Round => a.round(),
            MathOp::Floor => a.floor(),
            MathOp::Ceil => a.ceil(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MathParams {
    pub op: MathOp,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RemapParams {
    pub clamp: bool,
}

impl Default for RemapParams {
    fn default() -> Self {
        Self { clamp: true }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CurveParams {
    pub curve: Curve,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FunctionParams {
    pub function: ModulationFunction,
    pub phase: f32,
    pub frequency_multiplier: f32,
    pub power: f32,
    pub offset: f32,
    pub max_deviation: f32,
    pub clamp_min: f32,
    pub clamp_max: f32,
}

impl Default for FunctionParams {
    fn default() -> Self {
        Self {
            function: ModulationFunction::Sin,
            phase: 0.0,
            frequency_multiplier: 1.0,
            power: 1.0,
            offset: 0.5,
            max_deviation: 0.5,
            clamp_min: 0.0,
            clamp_max: 1.0,
        }
    }
}

impl FunctionParams {
    pub fn to_modulation(&self) -> crate::modulation::ModulationConfig {
        crate::modulation::ModulationConfig {
            source: crate::modulation::ModulationSource::Depth,
            function: self.function.clone(),
            base_speed: 0.0,
            sensitivity: 0.0,
            max_deviation: self.max_deviation,
            phase: self.phase,
            frequency_multiplier: self.frequency_multiplier,
            offset: self.offset,
            power: self.power,
            clamp_min: self.clamp_min.min(self.clamp_max),
            clamp_max: self.clamp_max.max(self.clamp_min),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum CombineMode {
    #[default]
    Max,
    Sum,
    Average,
    Weighted,
}

impl CombineMode {
    pub const ALL: [CombineMode; 4] = [
        CombineMode::Max,
        CombineMode::Sum,
        CombineMode::Average,
        CombineMode::Weighted,
    ];

    pub fn label(self) -> &'static str {
        match self {
            CombineMode::Max => "max",
            CombineMode::Sum => "sum",
            CombineMode::Average => "average",
            CombineMode::Weighted => "weighted",
        }
    }
}

pub const MAX_FAN: u8 = 8;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CombineParams {
    pub mode: CombineMode,
    pub count: u8,
    pub weights: Vec<f32>,
}

impl Default for CombineParams {
    fn default() -> Self {
        Self {
            mode: CombineMode::Max,
            count: 2,
            weights: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum LfoShape {
    #[default]
    Sine,
    Triangle,
    Saw,
    Square,
    Pulse,
    Noise,
    SampleHold,
}

impl LfoShape {
    pub const ALL: [LfoShape; 7] = [
        LfoShape::Sine,
        LfoShape::Triangle,
        LfoShape::Saw,
        LfoShape::Square,
        LfoShape::Pulse,
        LfoShape::Noise,
        LfoShape::SampleHold,
    ];

    pub fn label(self) -> &'static str {
        match self {
            LfoShape::Sine => "sine",
            LfoShape::Triangle => "triangle",
            LfoShape::Saw => "saw",
            LfoShape::Square => "square",
            LfoShape::Pulse => "pulse",
            LfoShape::Noise => "noise",
            LfoShape::SampleHold => "S&H",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LfoParams {
    pub shape: LfoShape,
    pub phase: f32,
    pub one_shot: bool,
    pub seed: u32,
}

impl Default for LfoParams {
    fn default() -> Self {
        Self {
            shape: LfoShape::Sine,
            phase: 0.0,
            one_shot: false,
            seed: 1,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum NoiseKind {
    #[default]
    Perlin,
    Simplex,
    Value,
    Fractal,
}

impl NoiseKind {
    pub const ALL: [NoiseKind; 4] = [
        NoiseKind::Perlin,
        NoiseKind::Simplex,
        NoiseKind::Value,
        NoiseKind::Fractal,
    ];

    pub fn label(self) -> &'static str {
        match self {
            NoiseKind::Perlin => "perlin",
            NoiseKind::Simplex => "simplex",
            NoiseKind::Value => "value",
            NoiseKind::Fractal => "fractal",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct NoiseParams {
    pub kind: NoiseKind,
    pub seed: u32,
}

impl Default for NoiseParams {
    fn default() -> Self {
        Self {
            kind: NoiseKind::Perlin,
            seed: 1,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum EnvMode {
    #[default]
    Adsr,
    Ar,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EnvelopeParams {
    pub mode: EnvMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum DelayMode {
    #[default]
    Ticks,
    Ms,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DelayParams {
    pub mode: DelayMode,
    pub amount: u32,
}

impl Default for DelayParams {
    fn default() -> Self {
        Self {
            mode: DelayMode::Ticks,
            amount: 1,
        }
    }
}

impl DelayParams {
    pub fn ticks(&self) -> usize {
        let t = match self.mode {
            DelayMode::Ticks => self.amount as usize,
            DelayMode::Ms => (self.amount as f32 / TICK_MS).round() as usize,
        };
        t.clamp(1, MAX_BUFFER)
    }
}

pub const MAX_STEPS: usize = 16;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SequencerParams {
    pub steps: Vec<f32>,
}

impl Default for SequencerParams {
    fn default() -> Self {
        Self {
            steps: vec![0.25, 0.5, 0.75, 1.0],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum CompareOp {
    #[default]
    Greater,
    Less,
    GreaterEq,
    LessEq,
    Equal,
}

impl CompareOp {
    pub const ALL: [CompareOp; 5] = [
        CompareOp::Greater,
        CompareOp::Less,
        CompareOp::GreaterEq,
        CompareOp::LessEq,
        CompareOp::Equal,
    ];

    pub fn label(self) -> &'static str {
        match self {
            CompareOp::Greater => ">",
            CompareOp::Less => "<",
            CompareOp::GreaterEq => "≥",
            CompareOp::LessEq => "≤",
            CompareOp::Equal => "=",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CompareParams {
    pub op: CompareOp,
    pub hysteresis: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum LogicOp {
    #[default]
    And,
    Or,
    Xor,
    Not,
}

impl LogicOp {
    pub const ALL: [LogicOp; 4] = [LogicOp::And, LogicOp::Or, LogicOp::Xor, LogicOp::Not];

    pub fn label(self) -> &'static str {
        match self {
            LogicOp::And => "And",
            LogicOp::Or => "Or",
            LogicOp::Xor => "Xor",
            LogicOp::Not => "Not",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LogicParams {
    pub op: LogicOp,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CountParams {
    pub count: u8,
}

impl Default for CountParams {
    fn default() -> Self {
        Self { count: 2 }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CrossfadeParams {
    pub equal_power: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FrameParams {
    pub size: [f32; 2],
    pub text: String,
}

impl Default for FrameParams {
    fn default() -> Self {
        Self {
            size: [360.0, 220.0],
            text: String::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GroupPort {
    pub key: String,
    pub label: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PortsParams {
    pub ports: Vec<GroupPort>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GroupParams {
    pub name: String,
    pub graph: Box<Graph>,
}

impl Default for GroupParams {
    fn default() -> Self {
        Self {
            name: "Group".into(),
            graph: Box::default(),
        }
    }
}

impl GroupParams {
    fn ports(&self, input: bool) -> Vec<GroupPort> {
        self.graph
            .nodes
            .iter()
            .find_map(|n| match (&n.kind, input) {
                (NodeKind::GroupInput(p), true) | (NodeKind::GroupOutput(p), false) => {
                    Some(p.ports.clone())
                }
                _ => None,
            })
            .unwrap_or_default()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PanicParams {
    pub latched: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ScopeParams {
    pub count: u8,
    pub window_s: f32,
}

impl Default for ScopeParams {
    fn default() -> Self {
        Self {
            count: 2,
            window_s: 4.0,
        }
    }
}

impl ScopeParams {
    pub fn samples(&self) -> usize {
        ((self.window_s * 1000.0 / TICK_MS).round() as usize).clamp(8, MAX_BUFFER)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DeviceParams {
    pub profile: String,
    pub binding: DeviceBinding,
    pub channels: Vec<ChannelParams>,
}

impl Default for DeviceParams {
    fn default() -> Self {
        let p = device::profile(device::COYOTE_V3.id);
        Self {
            profile: p.id.into(),
            binding: DeviceBinding::Unbound,
            channels: vec![ChannelParams::default(); p.channels.len()],
        }
    }
}

impl DeviceParams {
    pub fn bound_to(mac: &str) -> Self {
        Self {
            binding: DeviceBinding::Mac(mac.to_uppercase()),
            ..Self::default()
        }
    }

    pub fn profile(&self) -> &'static device::DeviceProfile {
        device::profile(&self.profile)
    }

    pub fn channel(&self, i: usize) -> ChannelParams {
        self.channels.get(i).cloned().unwrap_or_default()
    }

    pub fn pin_key(channel: &str, group: &str, seg: Option<usize>) -> String {
        match seg {
            Some(s) => format!("{channel}.{group}{}", s + 1),
            None => format!("{channel}.{group}"),
        }
    }

    pub fn pins(&self) -> Vec<PinSpec> {
        let p = self.profile();
        let mut out = Vec::new();
        for (ci, ch) in p.channels.iter().enumerate() {
            let params = self.channel(ci);
            for g in ch.groups {
                if g.expandable && params.is_expanded(g.key) {
                    for s in 0..device::SEGMENTS {
                        out.push(owned(
                            Self::pin_key(ch.key, g.key, Some(s)),
                            format!("{} {}", g.label, s + 1),
                            g.default,
                            g.range,
                        ));
                    }
                } else {
                    out.push(owned(
                        Self::pin_key(ch.key, g.key, None),
                        g.label.to_string(),
                        g.default,
                        g.range,
                    ));
                }
            }
        }
        out
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct MissingParams {
    pub type_name: String,
    pub raw: serde_json::Value,
}


#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum NodeKind {
    OscFloat(OscFloatParams),
    Contact(ContactParams),
    Constant(ConstantParams),
    Time(NoParams),
    Slider(SliderParams),
    Toggle(ToggleParams),
    Alarm(NoParams),
    Ukf(UkfNodeParams),
    Derivative(NoParams),
    Integrate(IntegrateParams),
    Smooth(SmoothParams),
    Slew(NoParams),
    MovingAverage(MovingAverageParams),
    PeakHold(NoParams),
    Normalize(NormalizeParams),
    SpoofGuard(SpoofGuardParams),
    Math(MathParams),
    Clamp(NoParams),
    Remap(RemapParams),
    Mix(NoParams),
    Threshold(NoParams),
    Curve(CurveParams),
    Function(FunctionParams),
    Combine(CombineParams),
    Lfo(LfoParams),
    Noise(NoiseParams),
    Envelope(EnvelopeParams),
    SampleHold(NoParams),
    Schmitt(NoParams),
    Metronome(NoParams),
    Delay(DelayParams),
    Timer(NoParams),
    Sequencer(SequencerParams),
    Compare(CompareParams),
    Logic(LogicParams),
    Switch(CountParams),
    Crossfade(CrossfadeParams),
    Router(CountParams),
    Reroute(NoParams),
    Frame(FrameParams),
    Group(GroupParams),
    GroupInput(PortsParams),
    GroupOutput(PortsParams),
    HardLimit(NoParams),
    RampLimiter(NoParams),
    Deadman(NoParams),
    Panic(PanicParams),
    Scope(ScopeParams),
    Device(DeviceParams),
    #[serde(skip)]
    Missing(MissingParams),
}

pub fn serialize_kind<S: Serializer>(kind: &NodeKind, s: S) -> Result<S::Ok, S::Error> {
    match kind {
        NodeKind::Missing(m) => m.raw.serialize(s),
        other => other.serialize(s),
    }
}

pub fn deserialize_kind<'de, D: Deserializer<'de>>(d: D) -> Result<NodeKind, D::Error> {
    let raw = serde_json::Value::deserialize(d)?;
    match NodeKind::deserialize(&raw) {
        Ok(k) => Ok(k),
        Err(_) => {
            let type_name = raw
                .get("type")
                .and_then(|t| t.as_str())
                .unwrap_or("?")
                .to_string();
            Ok(NodeKind::Missing(MissingParams { type_name, raw }))
        }
    }
}

impl NodeKind {
    pub fn type_name(&self) -> &str {
        match self {
            NodeKind::OscFloat(_) => "OscFloat",
            NodeKind::Contact(_) => "Contact",
            NodeKind::Constant(_) => "Constant",
            NodeKind::Time(_) => "Time",
            NodeKind::Slider(_) => "Slider",
            NodeKind::Toggle(_) => "Toggle",
            NodeKind::Alarm(_) => "Alarm",
            NodeKind::Ukf(_) => "Ukf",
            NodeKind::Derivative(_) => "Derivative",
            NodeKind::Integrate(_) => "Integrate",
            NodeKind::Smooth(_) => "Smooth",
            NodeKind::Slew(_) => "Slew",
            NodeKind::MovingAverage(_) => "MovingAverage",
            NodeKind::PeakHold(_) => "PeakHold",
            NodeKind::Normalize(_) => "Normalize",
            NodeKind::SpoofGuard(_) => "SpoofGuard",
            NodeKind::Math(_) => "Math",
            NodeKind::Clamp(_) => "Clamp",
            NodeKind::Remap(_) => "Remap",
            NodeKind::Mix(_) => "Mix",
            NodeKind::Threshold(_) => "Threshold",
            NodeKind::Curve(_) => "Curve",
            NodeKind::Function(_) => "Function",
            NodeKind::Combine(_) => "Combine",
            NodeKind::Lfo(_) => "Lfo",
            NodeKind::Noise(_) => "Noise",
            NodeKind::Envelope(_) => "Envelope",
            NodeKind::SampleHold(_) => "SampleHold",
            NodeKind::Schmitt(_) => "Schmitt",
            NodeKind::Metronome(_) => "Metronome",
            NodeKind::Delay(_) => "Delay",
            NodeKind::Timer(_) => "Timer",
            NodeKind::Sequencer(_) => "Sequencer",
            NodeKind::Compare(_) => "Compare",
            NodeKind::Logic(_) => "Logic",
            NodeKind::Switch(_) => "Switch",
            NodeKind::Crossfade(_) => "Crossfade",
            NodeKind::Router(_) => "Router",
            NodeKind::Reroute(_) => "Reroute",
            NodeKind::Frame(_) => "Frame",
            NodeKind::Group(_) => "Group",
            NodeKind::GroupInput(_) => "GroupInput",
            NodeKind::GroupOutput(_) => "GroupOutput",
            NodeKind::HardLimit(_) => "HardLimit",
            NodeKind::RampLimiter(_) => "RampLimiter",
            NodeKind::Deadman(_) => "Deadman",
            NodeKind::Panic(_) => "Panic",
            NodeKind::Scope(_) => "Scope",
            NodeKind::Device(_) => "Device",
            NodeKind::Missing(m) => &m.type_name,
        }
    }

    pub fn label(&self) -> String {
        match self {
            NodeKind::OscFloat(p) => {
                let short = p.address.rsplit('/').next().unwrap_or("");
                if short.is_empty() {
                    "OSC Float".into()
                } else {
                    format!("OSC {short}")
                }
            }
            NodeKind::Contact(p) => format!("Contact {}", p.zone),
            NodeKind::Constant(_) => "Constant".into(),
            NodeKind::Time(_) => "Time".into(),
            NodeKind::Slider(_) => "Slider".into(),
            NodeKind::Toggle(p) => if p.momentary { "Button" } else { "Toggle" }.into(),
            NodeKind::Alarm(_) => "Alarm".into(),
            NodeKind::Ukf(_) => "UKF Filter".into(),
            NodeKind::Derivative(_) => "Derivative".into(),
            NodeKind::Integrate(_) => "Integrate".into(),
            NodeKind::Smooth(_) => "Smooth".into(),
            NodeKind::Slew(_) => "Slew Limiter".into(),
            NodeKind::MovingAverage(_) => "Moving Average".into(),
            NodeKind::PeakHold(_) => "Peak Hold".into(),
            NodeKind::Normalize(_) => "Normalize".into(),
            NodeKind::SpoofGuard(_) => "Spoof Guard".into(),
            NodeKind::Math(p) => p.op.label().into(),
            NodeKind::Clamp(_) => "Clamp".into(),
            NodeKind::Remap(_) => "Remap".into(),
            NodeKind::Mix(_) => "Mix".into(),
            NodeKind::Threshold(_) => "Threshold Window".into(),
            NodeKind::Curve(_) => "Curve".into(),
            NodeKind::Function(p) => format!("Function {}", p.function),
            NodeKind::Combine(p) => format!("Combine {}", p.mode.label()),
            NodeKind::Lfo(p) => format!("LFO {}", p.shape.label()),
            NodeKind::Noise(p) => format!("Noise {}", p.kind.label()),
            NodeKind::Envelope(p) => match p.mode {
                EnvMode::Adsr => "Envelope ADSR".into(),
                EnvMode::Ar => "Envelope AR".into(),
            },
            NodeKind::SampleHold(_) => "Sample & Hold".into(),
            NodeKind::Schmitt(_) => "Schmitt Trigger".into(),
            NodeKind::Metronome(_) => "Metronome".into(),
            NodeKind::Delay(_) => "Delay".into(),
            NodeKind::Timer(_) => "Ramp Timer".into(),
            NodeKind::Sequencer(_) => "Sequencer".into(),
            NodeKind::Compare(p) => format!("Compare {}", p.op.label()),
            NodeKind::Logic(p) => p.op.label().into(),
            NodeKind::Switch(_) => "Switch".into(),
            NodeKind::Crossfade(_) => "Crossfade".into(),
            NodeKind::Router(_) => "Router".into(),
            NodeKind::Reroute(_) => "Reroute".into(),
            NodeKind::Frame(_) => "Frame".into(),
            NodeKind::Group(p) => p.name.clone(),
            NodeKind::GroupInput(_) => "Group In".into(),
            NodeKind::GroupOutput(_) => "Group Out".into(),
            NodeKind::HardLimit(_) => "Hard Limit".into(),
            NodeKind::RampLimiter(_) => "Ramp Limiter".into(),
            NodeKind::Deadman(_) => "Deadman".into(),
            NodeKind::Panic(_) => "Panic".into(),
            NodeKind::Scope(_) => "Scope".into(),
            NodeKind::Device(_) => "Device".into(),
            NodeKind::Missing(m) => format!("Missing: {}", m.type_name),
        }
    }

    pub fn category(&self) -> Category {
        match self {
            NodeKind::OscFloat(_)
            | NodeKind::Contact(_)
            | NodeKind::Constant(_)
            | NodeKind::Time(_)
            | NodeKind::Slider(_)
            | NodeKind::Toggle(_)
            | NodeKind::Alarm(_) => Category::Input,
            NodeKind::Ukf(_)
            | NodeKind::Derivative(_)
            | NodeKind::Integrate(_)
            | NodeKind::Smooth(_)
            | NodeKind::Slew(_)
            | NodeKind::MovingAverage(_)
            | NodeKind::PeakHold(_)
            | NodeKind::Normalize(_)
            | NodeKind::SpoofGuard(_) => Category::Conditioning,
            NodeKind::Math(_)
            | NodeKind::Clamp(_)
            | NodeKind::Remap(_)
            | NodeKind::Mix(_)
            | NodeKind::Threshold(_)
            | NodeKind::Curve(_)
            | NodeKind::Function(_)
            | NodeKind::Combine(_) => Category::Math,
            NodeKind::Lfo(_)
            | NodeKind::Noise(_)
            | NodeKind::Envelope(_)
            | NodeKind::SampleHold(_)
            | NodeKind::Schmitt(_)
            | NodeKind::Metronome(_)
            | NodeKind::Delay(_)
            | NodeKind::Timer(_)
            | NodeKind::Sequencer(_) => Category::Generator,
            NodeKind::Compare(_)
            | NodeKind::Logic(_)
            | NodeKind::Switch(_)
            | NodeKind::Crossfade(_)
            | NodeKind::Router(_)
            | NodeKind::Reroute(_)
            | NodeKind::Group(_)
            | NodeKind::GroupInput(_)
            | NodeKind::GroupOutput(_) => Category::Logic,
            NodeKind::HardLimit(_)
            | NodeKind::RampLimiter(_)
            | NodeKind::Deadman(_)
            | NodeKind::Panic(_) => Category::Safety,
            NodeKind::Scope(_) | NodeKind::Device(_) => Category::Output,
            NodeKind::Frame(_) | NodeKind::Missing(_) => Category::Layout,
        }
    }

    pub fn is_delay(&self) -> bool {
        matches!(self, NodeKind::Delay(_))
    }

    pub fn is_missing(&self) -> bool {
        matches!(self, NodeKind::Missing(_))
    }

    pub fn is_device(&self) -> bool {
        matches!(self, NodeKind::Device(_))
    }

    pub fn inputs(&self) -> Vec<PinSpec> {
        let x = || fl("in", "In", 0.0, (0.0, 1.0));
        match self {
            NodeKind::OscFloat(_)
            | NodeKind::Contact(_)
            | NodeKind::Constant(_)
            | NodeKind::Time(_)
            | NodeKind::Slider(_)
            | NodeKind::Toggle(_)
            | NodeKind::Alarm(_)
            | NodeKind::Frame(_)
            | NodeKind::GroupInput(_)
            | NodeKind::Panic(_)
            | NodeKind::Missing(_) => Vec::new(),
            NodeKind::Ukf(_)
            | NodeKind::Derivative(_)
            | NodeKind::Smooth(_)
            | NodeKind::MovingAverage(_)
            | NodeKind::Normalize(_)
            | NodeKind::SpoofGuard(_)
            | NodeKind::Curve(_)
            | NodeKind::Delay(_)
            | NodeKind::Reroute(_) => vec![x()],
            NodeKind::Integrate(_) => vec![x(), bl("reset", "Reset", 0.0)],
            NodeKind::Slew(_) => vec![
                x(),
                fl("rise", "Rise /s", 1.0, (0.0, 20.0)),
                fl("fall", "Fall /s", 1.0, (0.0, 20.0)),
            ],
            NodeKind::PeakHold(_) => vec![
                x(),
                fl("hold", "Hold s", 0.5, (0.0, 10.0)),
                fl("decay", "Decay /s", 1.0, (0.0, 20.0)),
            ],
            NodeKind::Math(p) => {
                if p.op.is_unary() {
                    vec![fl("a", "x", 0.0, (0.0, 1.0))]
                } else {
                    vec![
                        fl("a", "a", 0.0, (0.0, 1.0)),
                        fl("b", "b", p.op.b_default(), (0.0, 1.0)),
                    ]
                }
            }
            NodeKind::Clamp(_) => vec![
                x(),
                fl("min", "Min", 0.0, (-1.0, 1.0)),
                fl("max", "Max", 1.0, (0.0, 2.0)),
            ],
            NodeKind::Remap(_) => vec![
                x(),
                fl("in_min", "In min", 0.0, (-1.0, 1.0)),
                fl("in_max", "In max", 1.0, (0.0, 2.0)),
                fl("out_min", "Out min", 0.0, (-1.0, 1.0)),
                fl("out_max", "Out max", 1.0, (0.0, 2.0)),
            ],
            NodeKind::Mix(_) | NodeKind::Crossfade(_) => vec![
                fl("a", "A", 0.0, (0.0, 1.0)),
                fl("b", "B", 1.0, (0.0, 1.0)),
                fl("t", "Mix", 0.5, (0.0, 1.0)),
            ],
            NodeKind::Threshold(_) => vec![
                x(),
                fl("min", "Min", 0.01, (0.0, 1.0)),
                fl("max", "Max", 1.0, (0.0, 1.0)),
            ],
            NodeKind::Function(_) => vec![
                fl("cycles", "Cycles", 0.0, (0.0, 4.0)),
                fl("base", "Base", 0.0, (0.0, 1.0)),
            ],
            NodeKind::Combine(p) => (0..p.count.clamp(2, MAX_FAN))
                .map(|i| owned(format!("in{}", i + 1), format!("In {}", i + 1), 0.0, (0.0, 1.0)))
                .collect(),
            NodeKind::Lfo(_) => vec![
                fl("hz", "Hz", 1.0, (0.0, 10.0)),
                fl("amp", "Amp", 0.5, (0.0, 1.0)),
                fl("bias", "Bias", 0.5, (0.0, 1.0)),
                fl("width", "Width", 0.5, (0.0, 1.0)),
                bl("reset", "Reset", 0.0),
            ],
            NodeKind::Noise(_) => vec![fl("rate", "Rate Hz", 1.0, (0.0, 10.0))],
            NodeKind::Envelope(_) => vec![
                bl("gate", "Gate", 0.0),
                fl("attack", "Attack s", 0.05, (0.0, 5.0)),
                fl("decay", "Decay s", 0.2, (0.0, 5.0)),
                fl("sustain", "Sustain", 0.7, (0.0, 1.0)),
                fl("release", "Release s", 0.3, (0.0, 5.0)),
            ],
            NodeKind::SampleHold(_) => vec![x(), bl("trig", "Trigger", 0.0)],
            NodeKind::Schmitt(_) => vec![
                x(),
                fl("high", "High", 0.6, (0.0, 1.0)),
                fl("low", "Low", 0.4, (0.0, 1.0)),
            ],
            NodeKind::Metronome(_) => vec![
                fl("hz", "Hz", 2.0, (0.0, 10.0)),
                fl("width", "Width", 0.5, (0.0, 1.0)),
            ],
            NodeKind::Timer(_) => vec![
                bl("gate", "Gate", 0.0),
                fl("hold", "Hold s", 1.0, (0.0, 30.0)),
                fl("ramp", "Ramp s", 2.0, (0.0, 30.0)),
            ],
            NodeKind::Sequencer(_) => vec![
                fl("rate", "Rate Hz", 2.0, (0.0, 10.0)),
                bl("reset", "Reset", 0.0),
            ],
            NodeKind::Compare(_) => vec![
                fl("a", "a", 0.0, (0.0, 1.0)),
                fl("b", "b", 0.5, (0.0, 1.0)),
            ],
            NodeKind::Logic(p) => {
                if p.op == LogicOp::Not {
                    vec![bl("a", "a", 0.0)]
                } else {
                    vec![bl("a", "a", 0.0), bl("b", "b", 0.0)]
                }
            }
            NodeKind::Switch(p) => {
                let mut v = vec![fl("index", "Index", 0.0, (0.0, (MAX_FAN - 1) as f32))];
                v.extend((0..p.count.clamp(2, MAX_FAN)).map(|i| {
                    owned(format!("in{}", i + 1), format!("In {}", i + 1), 0.0, (0.0, 1.0))
                }));
                v
            }
            NodeKind::Router(_) => vec![
                x(),
                fl("index", "Index", 0.0, (0.0, (MAX_FAN - 1) as f32)),
            ],
            NodeKind::Group(p) => p
                .ports(true)
                .into_iter()
                .map(|port| owned(port.key, port.label, 0.0, (0.0, 1.0)))
                .collect(),
            NodeKind::GroupOutput(p) => p
                .ports
                .iter()
                .map(|port| owned(port.key.clone(), port.label.clone(), 0.0, (0.0, 1.0)))
                .collect(),
            NodeKind::HardLimit(_) => vec![x(), fl("max", "Max", 1.0, (0.0, 1.0))],
            NodeKind::RampLimiter(_) => vec![x(), fl("rise", "Rise /s", 1.0, (0.0, 20.0))],
            NodeKind::Deadman(_) => vec![
                x(),
                bl("alive", "Alive", 1.0),
                fl("timeout", "Timeout s", 1.0, (0.05, 30.0)),
            ],
            NodeKind::Scope(p) => (0..p.count.clamp(1, 4))
                .map(|i| owned(format!("in{}", i + 1), format!("In {}", i + 1), 0.0, (0.0, 1.0)))
                .collect(),
            NodeKind::Device(p) => p.pins(),
        }
    }

    pub fn outputs(&self) -> Vec<PinSpec> {
        let out = || unit("out", "Out");
        match self {
            NodeKind::OscFloat(_) => vec![unit("value", "Value"), bl("fresh", "Fresh", 0.0)],
            NodeKind::Contact(_) => vec![unit("level", "Level"), bl("active", "Active", 0.0)],
            NodeKind::Constant(_) | NodeKind::Slider(_) => vec![out()],
            NodeKind::Toggle(_) => vec![bl("out", "Out", 0.0)],
            NodeKind::Time(_) => vec![unit("t", "Seconds"), unit("dt", "Δt")],
            NodeKind::Alarm(_) => vec![unit("strength", "Strength"), bl("active", "Active", 0.0)],
            NodeKind::Ukf(_) => vec![
                unit("level", "Level"),
                unit("velocity", "Velocity"),
                unit("acceleration", "Accel"),
                unit("recoil", "Recoil"),
                unit("speed_n", "Speed n"),
                unit("acc_n", "Accel n"),
                unit("recoil_n", "Recoil n"),
            ],
            NodeKind::SpoofGuard(_) => vec![out(), bl("blocked", "Blocked", 0.0)],
            NodeKind::Lfo(_) => vec![out(), unit("phase", "Phase"), bl("done", "Done", 0.0)],
            NodeKind::Noise(_) => vec![out(), unit("uni", "0–1")],
            NodeKind::Schmitt(_) | NodeKind::Compare(_) | NodeKind::Logic(_) => {
                vec![bl("out", "Out", 0.0)]
            }
            NodeKind::Metronome(_) => vec![bl("gate", "Gate", 0.0), bl("beat", "Beat", 0.0)],
            NodeKind::Timer(_) => vec![out(), unit("elapsed", "Elapsed s")],
            NodeKind::Sequencer(_) => vec![out(), unit("index", "Step")],
            NodeKind::Router(p) => (0..p.count.clamp(2, MAX_FAN))
                .map(|i| owned(format!("out{}", i + 1), format!("Out {}", i + 1), 0.0, (0.0, 1.0)))
                .collect(),
            NodeKind::Panic(_) => vec![bl("out", "Latched", 0.0)],
            NodeKind::Group(p) => p
                .ports(false)
                .into_iter()
                .map(|port| owned(port.key, port.label, 0.0, (0.0, 1.0)))
                .collect(),
            NodeKind::GroupInput(p) => p
                .ports
                .iter()
                .map(|port| owned(port.key.clone(), port.label.clone(), 0.0, (0.0, 1.0)))
                .collect(),
            NodeKind::Frame(_)
            | NodeKind::GroupOutput(_)
            | NodeKind::Scope(_)
            | NodeKind::Device(_)
            | NodeKind::Missing(_) => Vec::new(),
            _ => vec![out()],
        }
    }

    pub fn sanitize(&mut self) {
        let fin = |v: &mut f32, d: f32| {
            if !v.is_finite() {
                *v = d;
            }
        };
        match self {
            NodeKind::Contact(p) => p.stale_ms = p.stale_ms.clamp(50, 60_000),
            NodeKind::OscFloat(p) => p.stale_ms = p.stale_ms.clamp(50, 600_000),
            NodeKind::Constant(p) => fin(&mut p.value, 0.0),
            NodeKind::Slider(p) => {
                fin(&mut p.min, 0.0);
                fin(&mut p.max, 1.0);
                fin(&mut p.step, 0.05);
                fin(&mut p.value, p.min);
                if p.max < p.min {
                    std::mem::swap(&mut p.min, &mut p.max);
                }
                p.value = p.value.clamp(p.min, p.max);
                p.step = p.step.abs().max(1e-4);
            }
            NodeKind::Ukf(p) => {
                p.ukf = sanitised_ukf(p.ukf);
                let d = MotionNorms::default();
                fin(&mut p.norms.speed, d.speed);
                fin(&mut p.norms.acc, d.acc);
                fin(&mut p.norms.recoil, d.recoil);
                p.norms = p.norms.sanitised();
            }
            NodeKind::Integrate(p) => {
                fin(&mut p.leak, 0.0);
                fin(&mut p.min, 0.0);
                fin(&mut p.max, 1.0);
                p.leak = p.leak.max(0.0);
                if p.max < p.min {
                    std::mem::swap(&mut p.min, &mut p.max);
                }
            }
            NodeKind::Smooth(p) => {
                fin(&mut p.attack_ms, 80.0);
                fin(&mut p.release_ms, 250.0);
                p.attack_ms = p.attack_ms.clamp(0.0, 60_000.0);
                p.release_ms = p.release_ms.clamp(0.0, 60_000.0);
            }
            NodeKind::MovingAverage(p) => {
                fin(&mut p.window_ms, 500.0);
                p.window_ms = p.window_ms.clamp(TICK_MS, TICK_MS * MAX_BUFFER as f32);
            }
            NodeKind::Normalize(p) => {
                fin(&mut p.divisor, 1.0);
                if p.divisor.abs() < 1e-3 {
                    p.divisor = 1e-3;
                }
            }
            NodeKind::SpoofGuard(p) => {
                fin(&mut p.jump_limit, 0.6);
                fin(&mut p.release_level, 0.02);
                p.jump_limit = p.jump_limit.clamp(0.0, 1.0);
                p.release_level = p.release_level.clamp(0.0, 1.0);
            }
            NodeKind::Curve(p) => p.curve.sanitize(),
            NodeKind::Function(p) => {
                fin(&mut p.phase, 0.0);
                fin(&mut p.frequency_multiplier, 1.0);
                fin(&mut p.power, 1.0);
                fin(&mut p.offset, 0.0);
                fin(&mut p.max_deviation, 1.0);
                fin(&mut p.clamp_min, 0.0);
                fin(&mut p.clamp_max, 1.0);
                p.frequency_multiplier = p.frequency_multiplier.max(0.001);
                p.power = p.power.max(0.001);
                if p.clamp_max < p.clamp_min {
                    std::mem::swap(&mut p.clamp_min, &mut p.clamp_max);
                }
            }
            NodeKind::Combine(p) => {
                p.count = p.count.clamp(2, MAX_FAN);
                p.weights.truncate(MAX_FAN as usize);
                for w in &mut p.weights {
                    fin(w, 1.0);
                }
            }
            NodeKind::Lfo(p) => fin(&mut p.phase, 0.0),
            NodeKind::Delay(p) => p.amount = p.amount.clamp(1, 10_000),
            NodeKind::Sequencer(p) => {
                p.steps.truncate(MAX_STEPS);
                for s in &mut p.steps {
                    fin(s, 0.0);
                }
                if p.steps.is_empty() {
                    p.steps.push(0.0);
                }
            }
            NodeKind::Compare(p) => {
                fin(&mut p.hysteresis, 0.0);
                p.hysteresis = p.hysteresis.max(0.0);
            }
            NodeKind::Switch(p) | NodeKind::Router(p) => p.count = p.count.clamp(2, MAX_FAN),
            NodeKind::Scope(p) => {
                p.count = p.count.clamp(1, 4);
                fin(&mut p.window_s, 4.0);
                p.window_s = p.window_s.clamp(0.25, 10.0);
            }
            NodeKind::Frame(p) => {
                fin(&mut p.size[0], 360.0);
                fin(&mut p.size[1], 220.0);
                p.size[0] = p.size[0].clamp(80.0, 6000.0);
                p.size[1] = p.size[1].clamp(60.0, 6000.0);
            }
            NodeKind::Group(p) => p.graph.sanitize(),
            NodeKind::Device(p) => {
                let n = p.profile().channels.len();
                p.channels.resize(n, ChannelParams::default());
                for c in &mut p.channels {
                    c.sanitize();
                }
                if let DeviceBinding::Mac(m) = &mut p.binding {
                    *m = m.to_uppercase();
                }
            }
            _ => {}
        }
    }
}

#[derive(Clone)]
pub struct CatalogEntry {
    pub label: &'static str,
    pub category: Category,
    pub keywords: &'static str,
    pub make: fn() -> NodeKind,
}

macro_rules! entry {
    ($label:expr, $cat:ident, $kw:expr, $make:expr) => {
        CatalogEntry {
            label: $label,
            category: Category::$cat,
            keywords: $kw,
            make: $make,
        }
    };
}

pub fn catalog() -> Vec<CatalogEntry> {
    let mut v = vec![
        entry!("OSC Float", Input, "osc parameter avatar slider float int bool", || {
            NodeKind::OscFloat(OscFloatParams::default())
        }),
        entry!("Contact (raw)", Input, "zone pen orf touch tps dgb depth contact", || {
            NodeKind::Contact(ContactParams::default())
        }),
        entry!("Constant", Input, "value number", || {
            NodeKind::Constant(ConstantParams { value: 0.5 })
        }),
        entry!("Time", Input, "seconds clock dt", || NodeKind::Time(NoParams {})),
        entry!("Slider", Input, "knob ui control", || {
            NodeKind::Slider(SliderParams::default())
        }),
        entry!("Toggle", Input, "switch ui bool", || {
            NodeKind::Toggle(ToggleParams::default())
        }),
        entry!("Button", Input, "momentary ui press", || {
            NodeKind::Toggle(ToggleParams {
                momentary: true,
                ..ToggleParams::default()
            })
        }),
        entry!("Alarm", Input, "wake alarm strength", || NodeKind::Alarm(NoParams {})),
        entry!("UKF Filter", Conditioning, "kalman ukf velocity speed acceleration recoil", || {
            NodeKind::Ukf(UkfNodeParams::default())
        }),
        entry!("Derivative", Conditioning, "rate change slope", || {
            NodeKind::Derivative(NoParams {})
        }),
        entry!("Integrate", Conditioning, "accumulate sum", || {
            NodeKind::Integrate(IntegrateParams::default())
        }),
        entry!("Smooth / Lag", Conditioning, "one pole lowpass attack release", || {
            NodeKind::Smooth(SmoothParams::default())
        }),
        entry!("Slew Limiter", Conditioning, "rate limit", || NodeKind::Slew(NoParams {})),
        entry!("Moving Average", Conditioning, "mean window", || {
            NodeKind::MovingAverage(MovingAverageParams::default())
        }),
        entry!("Peak Hold", Conditioning, "decay peak", || NodeKind::PeakHold(NoParams {})),
        entry!("Normalize", Conditioning, "divide norm scale", || {
            NodeKind::Normalize(NormalizeParams::default())
        }),
        entry!("Spoof Guard", Conditioning, "spoof jump guard", || {
            NodeKind::SpoofGuard(SpoofGuardParams::default())
        }),
    ];
    for op in MathOp::ALL {
        v.push(CatalogEntry {
            label: op.label(),
            category: Category::Math,
            keywords: "math arithmetic",
            make: match op {
                MathOp::Add => || NodeKind::Math(MathParams { op: MathOp::Add }),
                MathOp::Subtract => || NodeKind::Math(MathParams { op: MathOp::Subtract }),
                MathOp::Multiply => || NodeKind::Math(MathParams { op: MathOp::Multiply }),
                MathOp::Divide => || NodeKind::Math(MathParams { op: MathOp::Divide }),
                MathOp::Min => || NodeKind::Math(MathParams { op: MathOp::Min }),
                MathOp::Max => || NodeKind::Math(MathParams { op: MathOp::Max }),
                MathOp::Pow => || NodeKind::Math(MathParams { op: MathOp::Pow }),
                MathOp::Mod => || NodeKind::Math(MathParams { op: MathOp::Mod }),
                MathOp::Abs => || NodeKind::Math(MathParams { op: MathOp::Abs }),
                MathOp::Negate => || NodeKind::Math(MathParams { op: MathOp::Negate }),
                MathOp::Invert => || NodeKind::Math(MathParams { op: MathOp::Invert }),
                MathOp::Sqrt => || NodeKind::Math(MathParams { op: MathOp::Sqrt }),
                MathOp::Round => || NodeKind::Math(MathParams { op: MathOp::Round }),
                MathOp::Floor => || NodeKind::Math(MathParams { op: MathOp::Floor }),
                MathOp::Ceil => || NodeKind::Math(MathParams { op: MathOp::Ceil }),
            },
        });
    }
    v.extend([
        entry!("Clamp", Math, "limit range", || NodeKind::Clamp(NoParams {})),
        entry!("Remap", Math, "range map scale", || NodeKind::Remap(RemapParams::default())),
        entry!("Mix / Lerp", Math, "lerp blend", || NodeKind::Mix(NoParams {})),
        entry!("Threshold Window", Math, "threshold min max window gate", || {
            NodeKind::Threshold(NoParams {})
        }),
        entry!("Curve", Math, "response curve ease", || {
            NodeKind::Curve(CurveParams::default())
        }),
        entry!("Function", Math, "modulation sin triangle saw perlin compose", || {
            NodeKind::Function(FunctionParams::default())
        }),
        entry!("Combine", Math, "max sum average weighted aggregate", || {
            NodeKind::Combine(CombineParams::default())
        }),
        entry!("LFO / Oscillator", Generator, "lfo oscillator sine wave", || {
            NodeKind::Lfo(LfoParams::default())
        }),
        entry!("Noise", Generator, "perlin simplex fractal random", || {
            NodeKind::Noise(NoiseParams::default())
        }),
        entry!("Envelope", Generator, "adsr ar gate", || {
            NodeKind::Envelope(EnvelopeParams::default())
        }),
        entry!("Sample & Hold", Generator, "sample hold latch", || {
            NodeKind::SampleHold(NoParams {})
        }),
        entry!("Schmitt Trigger", Generator, "threshold trigger hysteresis", || {
            NodeKind::Schmitt(NoParams {})
        }),
        entry!("Metronome", Generator, "pulse train clock beat", || {
            NodeKind::Metronome(NoParams {})
        }),
        entry!("Delay / Memory", Generator, "delay memory feedback tick", || {
            NodeKind::Delay(DelayParams::default())
        }),
        entry!("Ramp Timer", Generator, "timer hold ramp", || NodeKind::Timer(NoParams {})),
        entry!("Sequencer", Generator, "step sequence pattern", || {
            NodeKind::Sequencer(SequencerParams::default())
        }),
        entry!("Compare", Logic, "greater less equal hysteresis", || {
            NodeKind::Compare(CompareParams::default())
        }),
        entry!("And", Logic, "logic bool", || {
            NodeKind::Logic(LogicParams { op: LogicOp::And })
        }),
        entry!("Or", Logic, "logic bool", || NodeKind::Logic(LogicParams { op: LogicOp::Or })),
        entry!("Xor", Logic, "logic bool", || {
            NodeKind::Logic(LogicParams { op: LogicOp::Xor })
        }),
        entry!("Not", Logic, "logic bool invert", || {
            NodeKind::Logic(LogicParams { op: LogicOp::Not })
        }),
        entry!("Switch / Select", Logic, "select index choose", || {
            NodeKind::Switch(CountParams::default())
        }),
        entry!("Crossfade", Logic, "fade blend", || {
            NodeKind::Crossfade(CrossfadeParams::default())
        }),
        entry!("Router", Logic, "route split demux", || {
            NodeKind::Router(CountParams::default())
        }),
        entry!("Reroute", Logic, "dot wire bend", || NodeKind::Reroute(NoParams {})),
        entry!("Hard Limit", Safety, "limit max cap", || NodeKind::HardLimit(NoParams {})),
        entry!("Ramp Limiter", Safety, "soft start rise", || {
            NodeKind::RampLimiter(NoParams {})
        }),
        entry!("Deadman / Timeout", Safety, "deadman timeout stale", || {
            NodeKind::Deadman(NoParams {})
        }),
        entry!("Panic", Safety, "panic stop emergency", || {
            NodeKind::Panic(PanicParams::default())
        }),
        entry!("Scope / Monitor", Output, "scope plot monitor", || {
            NodeKind::Scope(ScopeParams::default())
        }),
        entry!("Device (Coyote)", Output, "device coyote output", || {
            NodeKind::Device(DeviceParams::default())
        }),
        entry!("Frame / Comment", Layout, "frame comment note box", || {
            NodeKind::Frame(FrameParams::default())
        }),
    ]);
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_catalog_entry_round_trips_through_json() {
        for e in catalog() {
            let kind = (e.make)();
            let json = serde_json::to_value(&kind).expect("serialize");
            let back: NodeKind = serde_json::from_value(json).expect("deserialize");
            assert_eq!(back, kind, "{}", e.label);
        }
    }

    #[test]
    fn pin_keys_are_unique_per_node() {
        for e in catalog() {
            let kind = (e.make)();
            for pins in [kind.inputs(), kind.outputs()] {
                let mut keys: Vec<_> = pins.iter().map(|p| p.key.to_string()).collect();
                keys.sort();
                let n = keys.len();
                keys.dedup();
                assert_eq!(keys.len(), n, "{} has duplicate pin keys", e.label);
            }
        }
    }

    #[test]
    fn expanding_a_device_group_splits_its_pin() {
        let mut d = DeviceParams::default();
        let before = d.pins().len();
        d.channels[0].set_expanded("freq", true);
        let pins = d.pins();
        assert_eq!(pins.len(), before + 3);
        assert!(pins.iter().any(|p| p.key == "a.freq4"));
        assert!(!pins.iter().any(|p| p.key == "a.freq"));
        assert!(pins.iter().any(|p| p.key == "b.freq"));
    }

    #[test]
    fn safe_math_never_produces_non_finite_values() {
        for op in MathOp::ALL {
            for (a, b) in [(1.0, 0.0), (-2.0, 0.5), (0.0, -1.0), (f32::MAX, 2.0)] {
                let v = op.apply(a, b);
                assert!(!v.is_nan(), "{op:?}({a}, {b}) = NaN");
            }
        }
    }

    #[test]
    fn ukf_sanitising_matches_the_tuning_floors() {
        let c = sanitised_ukf(UkfConfig {
            q: -1.0,
            r: f32::NAN,
            alpha: 0.0,
            beta: -3.0,
            kappa: 1.5,
        });
        assert_eq!(c.q, 0.0001);
        assert_eq!(c.r, UkfConfig::default().r);
        assert_eq!(c.alpha, 0.001);
        assert_eq!(c.beta, 0.0);
        assert_eq!(c.kappa, 1.5);
    }
}
