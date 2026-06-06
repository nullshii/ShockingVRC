use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum ModulationSource {
    #[default]
    Depth,
    Speed,
    Acc,
    Recoil,
}

impl fmt::Display for ModulationSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Depth => write!(f, "depth"),
            Self::Speed => write!(f, "speed"),
            Self::Acc => write!(f, "acc"),
            Self::Recoil => write!(f, "recoil"),
        }
    }
}

impl FromStr for ModulationSource {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "depth" | "d" | "position" | "pos" => Ok(Self::Depth),
            "speed" | "s" | "vel" | "velocity" => Ok(Self::Speed),
            "acc" | "a" | "accel" | "acceleration" => Ok(Self::Acc),
            "recoil" | "r" | "jerk" => Ok(Self::Recoil),
            _ => Err(format!("'{s}' is not a valid ModulationSource")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub enum ModulationFunction {
    #[default]
    None,

    Sin,
    Cos,
    Tan,
    Asin,
    Acos,
    Atan,
    SinCos,
    Sin2,
    Cos2,
    SinPlusCos,
    SinPow(f32),
    CosPow(f32),

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

    Compose {
        outer: Box<ModulationFunction>,
        inner: Box<ModulationFunction>,
    },
}

impl fmt::Display for ModulationFunction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::None => write!(f, "none"),
            Self::Sin => write!(f, "sin"),
            Self::Cos => write!(f, "cos"),
            Self::Tan => write!(f, "tan"),
            Self::Asin => write!(f, "asin"),
            Self::Acos => write!(f, "acos"),
            Self::Atan => write!(f, "atan"),
            Self::SinCos => write!(f, "sincos"),
            Self::Sin2 => write!(f, "sin2"),
            Self::Cos2 => write!(f, "cos2"),
            Self::SinPlusCos => write!(f, "sin+cos"),
            Self::SinPow(n) => write!(f, "sin^{n}"),
            Self::CosPow(n) => write!(f, "cos^{n}"),
            Self::Sinh => write!(f, "sinh"),
            Self::Cosh => write!(f, "cosh"),
            Self::Tanh => write!(f, "tanh"),
            Self::Square => write!(f, "x2"),
            Self::Cube => write!(f, "x3"),
            Self::Pow4 => write!(f, "x4"),
            Self::Sqrt => write!(f, "sqrt"),
            Self::Cbrt => write!(f, "cbrt"),
            Self::Abs => write!(f, "abs"),
            Self::Sign => write!(f, "sign"),
            Self::Exp => write!(f, "exp"),
            Self::ExpNeg => write!(f, "exp-"),
            Self::Pow2x => write!(f, "2^x"),
            Self::Pow10x => write!(f, "10^x"),
            Self::Ln => write!(f, "ln"),
            Self::Log2 => write!(f, "log2"),
            Self::Log10 => write!(f, "log10"),
            Self::Triangle => write!(f, "triangle"),
            Self::Saw => write!(f, "saw"),
            Self::ReverseSaw => write!(f, "rsaw"),
            Self::SquareWave => write!(f, "square"),
            Self::Pulse => write!(f, "pulse"),
            Self::Bounce => write!(f, "bounce"),
            Self::Sigmoid => write!(f, "sigmoid"),
            Self::SmoothStep => write!(f, "smoothstep"),
            Self::SmootherStep => write!(f, "smootherstep"),
            Self::Logistic => write!(f, "logistic"),
            Self::SoftSign => write!(f, "softsign"),
            Self::Perlin => write!(f, "perlin"),
            Self::Simplex => write!(f, "simplex"),
            Self::Fractal => write!(f, "fractal"),
            Self::ValueNoise => write!(f, "valuenoise"),
            Self::SinPlusNoise => write!(f, "sin+noise"),
            Self::SinTimesNoise => write!(f, "sin*noise"),
            Self::TrianglePlusSin => write!(f, "triangle+sin"),
            Self::SquareTimesSigmoid => write!(f, "square*sigmoid"),
            Self::Compose { outer, inner } => write!(f, "{outer}({inner})"),
        }
    }
}

impl FromStr for ModulationFunction {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let lower = s.to_ascii_lowercase();

        if let Some(rest) = lower.strip_prefix("sin^") {
            let n: f32 = rest.parse().map_err(|_| format!("invalid power in '{s}'"))?;
            return Ok(Self::SinPow(n));
        }
        if let Some(rest) = lower.strip_prefix("cos^") {
            let n: f32 = rest.parse().map_err(|_| format!("invalid power in '{s}'"))?;
            return Ok(Self::CosPow(n));
        }

        match lower.as_str() {
            "none" | "off" | "disable" => Ok(Self::None),
            "sin" => Ok(Self::Sin),
            "cos" => Ok(Self::Cos),
            "tan" => Ok(Self::Tan),
            "asin" => Ok(Self::Asin),
            "acos" => Ok(Self::Acos),
            "atan" => Ok(Self::Atan),
            "sincos" | "sin*cos" => Ok(Self::SinCos),
            "sin2" | "sin²" => Ok(Self::Sin2),
            "cos2" | "cos²" => Ok(Self::Cos2),
            "sin+cos" => Ok(Self::SinPlusCos),
            "sinh" => Ok(Self::Sinh),
            "cosh" => Ok(Self::Cosh),
            "tanh" => Ok(Self::Tanh),
            "x2" | "x²" | "square" | "sq" => Ok(Self::Square),
            "x3" | "x³" | "cube" => Ok(Self::Cube),
            "x4" | "x⁴" => Ok(Self::Pow4),
            "sqrt" => Ok(Self::Sqrt),
            "cbrt" => Ok(Self::Cbrt),
            "abs" => Ok(Self::Abs),
            "sign" | "sgn" => Ok(Self::Sign),
            "exp" => Ok(Self::Exp),
            "exp-" | "expneg" | "exp_neg" => Ok(Self::ExpNeg),
            "2^x" | "pow2" => Ok(Self::Pow2x),
            "10^x" | "pow10" => Ok(Self::Pow10x),
            "ln" => Ok(Self::Ln),
            "log2" => Ok(Self::Log2),
            "log10" => Ok(Self::Log10),
            "triangle" | "tri" => Ok(Self::Triangle),
            "saw" | "sawtooth" => Ok(Self::Saw),
            "rsaw" | "reversesaw" | "reverse_saw" => Ok(Self::ReverseSaw),
            "squarewave" | "sqwave" => Ok(Self::SquareWave),
            "pulse" => Ok(Self::Pulse),
            "bounce" => Ok(Self::Bounce),
            "sigmoid" | "sig" => Ok(Self::Sigmoid),
            "smoothstep" | "sstep" => Ok(Self::SmoothStep),
            "smootherstep" | "sstep2" => Ok(Self::SmootherStep),
            "logistic" => Ok(Self::Logistic),
            "softsign" => Ok(Self::SoftSign),
            "perlin" => Ok(Self::Perlin),
            "simplex" => Ok(Self::Simplex),
            "fractal" | "fbm" => Ok(Self::Fractal),
            "valuenoise" | "vnoise" => Ok(Self::ValueNoise),
            "sin+noise" => Ok(Self::SinPlusNoise),
            "sin*noise" => Ok(Self::SinTimesNoise),
            "triangle+sin" | "tri+sin" => Ok(Self::TrianglePlusSin),
            "square*sigmoid" | "sq*sig" => Ok(Self::SquareTimesSigmoid),
            _ => Err(format!("'{s}' is not a valid ModulationFunction")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModulationConfig {
    pub source: ModulationSource,
    pub function: ModulationFunction,
    #[serde(default = "default_base_speed")]
    pub base_speed: f32,
    pub sensitivity: f32,
    pub max_deviation: f32,
    pub phase: f32,
    pub frequency_multiplier: f32,
    pub offset: f32,
    pub power: f32,
    pub clamp_min: f32,
    pub clamp_max: f32,
}

fn default_base_speed() -> f32 { 1.0 }

impl Default for ModulationConfig {
    fn default() -> Self {
        Self {
            source: ModulationSource::Depth,
            function: ModulationFunction::Sin,
            base_speed: 1.0,
            sensitivity: 1.0,
            max_deviation: 20.0,
            phase: 0.0,
            frequency_multiplier: 1.0,
            offset: 0.0,
            power: 1.0,
            clamp_min: 0.0,
            clamp_max: 255.0,
        }
    }
}

impl fmt::Display for ModulationConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "src={} fn={} bspd={:.2} sens={:.2} dev={:.1} phase={:.2} fmul={:.2} off={:.1} pow={:.1} [{:.0}..{:.0}]",
            self.source,
            self.function,
            self.base_speed,
            self.sensitivity,
            self.max_deviation,
            self.phase,
            self.frequency_multiplier,
            self.offset,
            self.power,
            self.clamp_min,
            self.clamp_max,
        )
    }
}
