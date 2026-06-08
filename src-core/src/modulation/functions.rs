use super::config::ModulationFunction;
use super::noise;

use std::f32::consts::PI;

fn unit_phase(x: f32) -> f32 {
    let phase = x / (2.0 * PI);
    phase - phase.floor()
}

fn centered_phase(x: f32) -> f32 {
    2.0 * unit_phase(x) - 1.0
}

pub fn eval(func: &ModulationFunction, x: f32) -> f32 {
    match func {
        ModulationFunction::None => 0.0,

        ModulationFunction::Sin => x.sin(),
        ModulationFunction::Cos => x.cos(),
        ModulationFunction::Tan => x.tan().clamp(-1.0, 1.0),
        ModulationFunction::Asin => centered_phase(x).clamp(-1.0, 1.0).asin() / (PI / 2.0),
        ModulationFunction::Acos => centered_phase(x).clamp(-1.0, 1.0).acos() / PI * 2.0 - 1.0,
        ModulationFunction::Atan => centered_phase(x).atan() / (PI / 2.0),
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

        // Wrapped: constant source advances phase linearly; without folding these saturate once.
        ModulationFunction::Sinh => (centered_phase(x) * 3.0).sinh().clamp(-1.0, 1.0),
        ModulationFunction::Cosh => ((centered_phase(x) * 3.0).cosh() - 1.0).clamp(0.0, 1.0),
        ModulationFunction::Tanh => (centered_phase(x) * 3.0).tanh(),

        ModulationFunction::Square => {
            let c = centered_phase(x);
            c * c * c.signum()
        }
        ModulationFunction::Cube => {
            let c = centered_phase(x);
            c * c * c
        }
        ModulationFunction::Pow4 => {
            let c = centered_phase(x);
            let c2 = c * c;
            c2 * c2 * c.signum()
        }
        ModulationFunction::Sqrt => unit_phase(x).sqrt() * 2.0 - 1.0,
        ModulationFunction::Cbrt => centered_phase(x).cbrt(),
        ModulationFunction::Abs => centered_phase(x).abs(),
        ModulationFunction::Sign => centered_phase(x).signum(),

        ModulationFunction::Exp => {
            let s = centered_phase(x) * 6.0;
            (s.exp() / (1.0 + s.exp())) * 2.0 - 1.0
        }
        ModulationFunction::ExpNeg => {
            let s = centered_phase(x) * 6.0;
            ((-s).exp() / (1.0 + (-s).exp())) * 2.0 - 1.0
        }
        ModulationFunction::Pow2x => {
            let s = centered_phase(x) * 4.0;
            (2.0f32.powf(s) / (1.0 + 2.0f32.powf(s))) * 2.0 - 1.0
        }
        ModulationFunction::Pow10x => {
            let s = centered_phase(x);
            (10.0f32.powf(s) - 1.0) / 9.0
        }

        ModulationFunction::Ln => {
            let t = unit_phase(x) * 0.99 + 0.01;
            (t.ln() / 0.01_f32.ln()).clamp(0.0, 1.0) * 2.0 - 1.0
        }
        ModulationFunction::Log2 => {
            let t = unit_phase(x) * 0.99 + 0.01;
            (t.log2() / 0.01_f32.log2()).clamp(0.0, 1.0) * 2.0 - 1.0
        }
        ModulationFunction::Log10 => {
            let t = unit_phase(x) * 0.99 + 0.01;
            (t.log10() / 0.01_f32.log10()).clamp(0.0, 1.0) * 2.0 - 1.0
        }

        ModulationFunction::Triangle => {
            let t = unit_phase(x);
            2.0 * (2.0 * t - 1.0).abs() - 1.0
        }
        ModulationFunction::Saw => {
            let t = unit_phase(x);
            2.0 * t - 1.0
        }
        ModulationFunction::ReverseSaw => {
            let t = unit_phase(x);
            1.0 - 2.0 * t
        }
        ModulationFunction::SquareWave => {
            if x.sin() >= 0.0 { 1.0 } else { -1.0 }
        }
        ModulationFunction::Pulse => {
            let t = unit_phase(x);
            if t < 0.25 { 1.0 } else { -1.0 }
        }
        ModulationFunction::Bounce => {
            let t = unit_phase(x);
            let b = (2.0 * t - 1.0).abs();
            b * b
        }

        ModulationFunction::Sigmoid => {
            let s = centered_phase(x) * 6.0;
            1.0 / (1.0 + (-s).exp()) * 2.0 - 1.0
        }
        ModulationFunction::SmoothStep => {
            let t = unit_phase(x);
            t * t * (3.0 - 2.0 * t) * 2.0 - 1.0
        }
        ModulationFunction::SmootherStep => {
            let t = unit_phase(x);
            t * t * t * (t * (t * 6.0 - 15.0) + 10.0) * 2.0 - 1.0
        }
        ModulationFunction::Logistic => {
            let s = centered_phase(x) * 5.0;
            1.0 / (1.0 + (-s).exp()) * 2.0 - 1.0
        }
        ModulationFunction::SoftSign => {
            let s = centered_phase(x) * 5.0;
            s / (1.0 + s.abs())
        }

        ModulationFunction::Perlin => noise::perlin_1d(x),
        ModulationFunction::Simplex => noise::simplex_1d(x),
        ModulationFunction::Fractal => noise::fractal_1d(x),
        ModulationFunction::ValueNoise => noise::value_noise_1d(x),

        ModulationFunction::SinPlusNoise => (x.sin() + noise::perlin_1d(x)) * 0.5,
        ModulationFunction::SinTimesNoise => x.sin() * noise::perlin_1d(x * 3.0),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrapped_functions_repeat_over_phase() {
        let funcs = [
            ModulationFunction::Sinh,
            ModulationFunction::Cosh,
            ModulationFunction::Tanh,
            ModulationFunction::Square,
            ModulationFunction::Cube,
            ModulationFunction::Pow4,
            ModulationFunction::Sqrt,
            ModulationFunction::Cbrt,
            ModulationFunction::Abs,
            ModulationFunction::Exp,
            ModulationFunction::ExpNeg,
            ModulationFunction::Pow2x,
            ModulationFunction::Pow10x,
            ModulationFunction::Ln,
            ModulationFunction::Log2,
            ModulationFunction::Log10,
            ModulationFunction::Sigmoid,
            ModulationFunction::SmoothStep,
            ModulationFunction::SmootherStep,
            ModulationFunction::Logistic,
            ModulationFunction::SoftSign,
        ];

        for func in funcs {
            let v0 = eval(&func, 0.0);
            let v1 = eval(&func, 2.0 * PI);
            let v_mid = eval(&func, PI);
            assert!(
                (v0 - v1).abs() < 1e-4,
                "{func:?} should repeat every 2π: v(0)={v0}, v(2π)={v1}"
            );
            assert!(
                (v0 - v_mid).abs() > 1e-4,
                "{func:?} should vary within a cycle: v(0)={v0}, v(π)={v_mid}"
            );
        }
    }
}
