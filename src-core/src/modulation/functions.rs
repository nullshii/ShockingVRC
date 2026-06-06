use super::config::ModulationFunction;
use super::noise;

use std::f32::consts::PI;

pub fn eval(func: &ModulationFunction, x: f32) -> f32 {
    match func {
        ModulationFunction::None => 0.0,

        ModulationFunction::Sin => x.sin(),
        ModulationFunction::Cos => x.cos(),
        ModulationFunction::Tan => x.tan().clamp(-1.0, 1.0),
        ModulationFunction::Asin => x.clamp(-1.0, 1.0).asin() / (PI / 2.0),
        ModulationFunction::Acos => x.clamp(-1.0, 1.0).acos() / PI * 2.0 - 1.0,
        ModulationFunction::Atan => x.atan() / (PI / 2.0),
        ModulationFunction::SinCos => x.sin() * x.cos(),
        ModulationFunction::Sin2 => {
            let s = x.sin();
            s * s
        }
        ModulationFunction::Cos2 => {
            let c = x.cos();
            c * c
        }
        ModulationFunction::SinPlusCos => (x.sin() + x.cos()) * 0.7071,
        ModulationFunction::SinPow(n) => x.sin().abs().powf(*n) * x.sin().signum(),
        ModulationFunction::CosPow(n) => x.cos().abs().powf(*n) * x.cos().signum(),

        ModulationFunction::Sinh => x.sinh().clamp(-1.0, 1.0),
        ModulationFunction::Cosh => (x.cosh() - 1.0).clamp(0.0, 1.0),
        ModulationFunction::Tanh => x.tanh(),

        ModulationFunction::Square => x * x * x.signum(),
        ModulationFunction::Cube => x * x * x,
        ModulationFunction::Pow4 => {
            let x2 = x * x;
            x2 * x2 * x.signum()
        }
        ModulationFunction::Sqrt => x.abs().sqrt() * x.signum(),
        ModulationFunction::Cbrt => x.cbrt(),
        ModulationFunction::Abs => x.abs(),
        ModulationFunction::Sign => x.signum(),

        ModulationFunction::Exp => (x.exp() / (1.0 + x.exp())) * 2.0 - 1.0,
        ModulationFunction::ExpNeg => ((-x).exp() / (1.0 + (-x).exp())) * 2.0 - 1.0,
        ModulationFunction::Pow2x => (2.0f32.powf(x) / (1.0 + 2.0f32.powf(x))) * 2.0 - 1.0,
        ModulationFunction::Pow10x => (10.0f32.powf(x.clamp(-1.0, 1.0)) - 1.0) / 9.0,

        ModulationFunction::Ln => (x.abs() + 1.0).ln() * x.signum(),
        ModulationFunction::Log2 => (x.abs() + 1.0).log2() * x.signum(),
        ModulationFunction::Log10 => (x.abs() + 1.0).log10() * x.signum() * 3.32,

        ModulationFunction::Triangle => {
            let phase = x / (2.0 * PI);
            let t = phase - phase.floor();
            2.0 * (2.0 * t - 1.0).abs() - 1.0
        }
        ModulationFunction::Saw => {
            let phase = x / (2.0 * PI);
            let t = phase - phase.floor();
            2.0 * t - 1.0
        }
        ModulationFunction::ReverseSaw => {
            let phase = x / (2.0 * PI);
            let t = phase - phase.floor();
            1.0 - 2.0 * t
        }
        ModulationFunction::SquareWave => {
            if x.sin() >= 0.0 { 1.0 } else { -1.0 }
        }
        ModulationFunction::Pulse => {
            let phase = x / (2.0 * PI);
            let t = phase - phase.floor();
            if t < 0.25 { 1.0 } else { -1.0 }
        }
        ModulationFunction::Bounce => {
            let phase = x / (2.0 * PI);
            let t = phase - phase.floor();
            let b = (2.0 * t - 1.0).abs();
            b * b
        }

        ModulationFunction::Sigmoid => {
            1.0 / (1.0 + (-x).exp()) * 2.0 - 1.0
        }
        ModulationFunction::SmoothStep => {
            let t = x.clamp(0.0, 1.0);
            t * t * (3.0 - 2.0 * t) * 2.0 - 1.0
        }
        ModulationFunction::SmootherStep => {
            let t = x.clamp(0.0, 1.0);
            t * t * t * (t * (t * 6.0 - 15.0) + 10.0) * 2.0 - 1.0
        }
        ModulationFunction::Logistic => {
            let k = 5.0;
            1.0 / (1.0 + (-k * x).exp()) * 2.0 - 1.0
        }
        ModulationFunction::SoftSign => x / (1.0 + x.abs()),

        ModulationFunction::Perlin => noise::perlin_1d(x),
        ModulationFunction::Simplex => noise::simplex_1d(x),
        ModulationFunction::Fractal => noise::fractal_1d(x),
        ModulationFunction::ValueNoise => noise::value_noise_1d(x),

        ModulationFunction::SinPlusNoise => {
            (x.sin() + noise::perlin_1d(x)) * 0.5
        }
        ModulationFunction::SinTimesNoise => {
            x.sin() * noise::perlin_1d(x * 3.0)
        }
        ModulationFunction::TrianglePlusSin => {
            let tri = eval(&ModulationFunction::Triangle, x);
            (tri + x.sin()) * 0.5
        }
        ModulationFunction::SquareTimesSigmoid => {
            let sq = eval(&ModulationFunction::SquareWave, x);
            let sig = eval(&ModulationFunction::Sigmoid, x);
            sq * sig.abs()
        }

        ModulationFunction::Compose { outer, inner } => {
            let inner_val = eval(inner, x);
            eval(outer, inner_val)
        }
    }
}
