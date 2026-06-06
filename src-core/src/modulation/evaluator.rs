use super::config::{ModulationConfig, ModulationSource};
use super::functions;

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
    let speed = config.base_speed + src * config.sensitivity;
    *accum += speed * dt;

    const WRAP: f32 = 6283.185;
    if accum.abs() > WRAP {
        *accum -= (*accum / WRAP).trunc() * WRAP;
    }
}

pub fn evaluate_with_accumulator(config: &ModulationConfig, base_value: f32, accum: f32) -> f32 {
    let func_input = accum * config.frequency_multiplier + config.phase;

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
