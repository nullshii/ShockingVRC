use super::config::{ModulationConfig, ModulationSource};
use super::functions;

use std::f32::consts::TAU;

#[derive(Debug, Clone, Copy, Default)]
pub struct KinematicsInput {
    pub depth: f32,
    pub speed: f32,
    pub acc: f32,
    pub recoil: f32,
}

fn source_value(config: &ModulationConfig, input: &KinematicsInput) -> f32 {
    match config.source {
        ModulationSource::Depth => input.depth,
        ModulationSource::Speed => input.speed,
        ModulationSource::Acc => input.acc,
        ModulationSource::Recoil => input.recoil,
    }
}

pub fn advance_accumulator(
    config: &ModulationConfig,
    accum: &mut f32,
    input: &KinematicsInput,
    dt: f32,
) {
    let src = source_value(config, input);
    let speed_hz = config.base_speed + src * config.sensitivity;
    *accum += speed_hz * dt;

    const WRAP: f32 = 1000.0;
    if accum.abs() > WRAP {
        *accum -= (*accum / WRAP).trunc() * WRAP;
    }
}

pub fn evaluate_with_accumulator(config: &ModulationConfig, base_value: f32, accum: f32) -> f32 {
    let cycles = accum * config.frequency_multiplier + config.phase;
    let func_input = cycles * TAU;

    let mut modulation = functions::eval(&config.function, func_input);

    if config.power != 1.0 {
        modulation = modulation.abs().powf(config.power) * modulation.signum();
    }

    (base_value + modulation * config.max_deviation + config.offset)
        .clamp(config.clamp_min, config.clamp_max)
}


pub fn advance_and_evaluate_segments(
    configs: &[Option<ModulationConfig>; 4],
    accums: &mut [f32; 4],
    base_values: [f32; 4],
    input: &KinematicsInput,
    dt: f32,
) -> [f32; 4] {
    let mut result = base_values;
    for i in 0..4 {
        if let Some(cfg) = &configs[i] {
            advance_accumulator(cfg, &mut accums[i], input, dt);
            result[i] = evaluate_with_accumulator(cfg, base_values[i], accums[i]);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modulation::config::ModulationFunction;

    fn sin_config() -> ModulationConfig {
        ModulationConfig {
            source: ModulationSource::Depth,
            function: ModulationFunction::Sin,
            base_speed: 1.0,
            sensitivity: 0.0,
            max_deviation: 1.0,
            phase: 0.0,
            frequency_multiplier: 1.0,
            offset: 0.0,
            power: 1.0,
            clamp_min: -1000.0,
            clamp_max: 1000.0,
        }
    }

    #[test]
    fn base_speed_is_in_hertz() {
        let cfg = sin_config();
        let kin = KinematicsInput::default();
        let mut accum = 0.0;
        let steps = 1000;
        let dt = 1.0 / steps as f32;
        for _ in 0..steps {
            advance_accumulator(&cfg, &mut accum, &kin, dt);
        }
        assert!((accum - 1.0).abs() < 1e-4, "1 Hz over 1 s should be 1 cycle, got {accum}");
        let v = evaluate_with_accumulator(&cfg, 0.0, accum);
        assert!(v.abs() < 1e-2, "sin at a whole cycle should be ~0, got {v}");
    }

    #[test]
    fn phase_is_in_cycles() {
        let mut cfg = sin_config();
        cfg.base_speed = 0.0;
        cfg.phase = 0.25;
        let v = evaluate_with_accumulator(&cfg, 0.0, 0.0);
        assert!((v - 1.0).abs() < 1e-4, "phase 0.25 cycle should give sin(π/2)=1, got {v}");
    }

    #[test]
    fn wrap_is_continuous_for_periodic_functions() {
        let kin = KinematicsInput::default();
        for fmul in [0.25, 0.5, 1.0, 1.5, 2.75, 0.137] {
            let mut cfg = sin_config();
            cfg.frequency_multiplier = fmul;
            let mut accum = 999.9_f32;
            let before = evaluate_with_accumulator(&cfg, 0.0, accum);
            for _ in 0..20 {
                advance_accumulator(&cfg, &mut accum, &kin, 0.01);
            }
            let after = evaluate_with_accumulator(&cfg, 0.0, accum);
            assert!(accum.abs() <= 1000.0, "accumulator not wrapped: {accum}");
            assert!(before.is_finite() && after.is_finite());
        }
    }

    #[test]
    fn wrap_preserves_phase_exactly() {
        let mut cfg = sin_config();
        cfg.frequency_multiplier = 1.0;
        cfg.phase = 0.3;
        let a = 12.3456_f32;
        let unwrapped = evaluate_with_accumulator(&cfg, 0.0, a + 1000.0);
        let wrapped = evaluate_with_accumulator(&cfg, 0.0, a);
        assert!((unwrapped - wrapped).abs() < 1e-3, "wrap changed phase: {unwrapped} vs {wrapped}");
    }
}
