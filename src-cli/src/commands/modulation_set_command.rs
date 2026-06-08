use std::io::Write;
use std::str::FromStr;

use shocking_vrc_core::modulation::config::{ModulationConfig, ModulationFunction, ModulationSource};

use crate::engine::command::{Command, CommandData, CommandFuture};

pub struct ModulationSetCommand;

impl Command for ModulationSetCommand {
    fn names(&self) -> &[&str] {
        &["mod-freq-a", "mod-freq-b", "mod-int-a", "mod-int-b"]
    }

    fn description(&self) -> &str {
        "Set modulation. Usage: mod-freq-a <seg 0-3> <function> <source> [key=val ...]"
    }

    fn execute(&self, cmd_name: String, args: Vec<String>, data: CommandData) -> CommandFuture {
        Box::pin(async move {
            let mut w = data.writer;

            let is_freq = cmd_name.contains("freq");
            let channel = if cmd_name.ends_with("-a") || cmd_name.ends_with("a") { "A" } else { "B" };

            if args.len() < 3 {
                writeln!(w, "Usage: {cmd_name} <segment 0-3> <function> <source> [key=val ...]")?;
                writeln!(w, "  Sources: depth, speed, acc, recoil")?;
                writeln!(w, "  Functions: sin, cos, tan, asin, acos, atan, sincos, sin2, cos2,")?;
                writeln!(w, "    sin+cos, sin^N, cos^N, sinh, cosh, tanh, x2, x3, x4, sqrt,")?;
                writeln!(w, "    cbrt, abs, sign, exp, exp-, 2^x, 10^x, ln, log2, log10,")?;
                writeln!(w, "    triangle, saw, rsaw, squarewave, pulse, bounce,")?;
                writeln!(w, "    sigmoid, smoothstep, smootherstep, logistic, softsign,")?;
                writeln!(w, "    perlin, simplex, fractal, valuenoise,")?;
                writeln!(w, "    sin+noise, sin*noise, triangle+sin, square*sigmoid")?;
                writeln!(w, "  Params: sens=1.0 dev=20.0 phase=0.0 fmul=1.0 off=0.0 pow=1.0 min=10.0 max=255.0 (freq) or min=0 max=100 (int)")?;
                return Ok(());
            }

            let seg: usize = match args[0].parse() {
                Ok(v) if v < 4 => v,
                _ => {
                    writeln!(w, "Segment must be 0, 1, 2, or 3.")?;
                    return Ok(());
                }
            };

            let function = match ModulationFunction::from_str(&args[1]) {
                Ok(f) => f,
                Err(e) => {
                    writeln!(w, "{e}")?;
                    return Ok(());
                }
            };

            let source = match ModulationSource::from_str(&args[2]) {
                Ok(s) => s,
                Err(e) => {
                    writeln!(w, "{e}")?;
                    return Ok(());
                }
            };

            let mut config = ModulationConfig {
                source,
                function,
                ..ModulationConfig::default()
            };

            if !is_freq {
                config.max_deviation = 10.0;
            }
            config.sanitise(!is_freq);

            for param in &args[3..] {
                if let Some((key, val)) = param.split_once('=') {
                    let v: f32 = match val.parse() {
                        Ok(v) => v,
                        Err(_) => {
                            writeln!(w, "Invalid value for '{key}': {val}")?;
                            return Ok(());
                        }
                    };
                    match key {
                        "sens" | "sensitivity" => config.sensitivity = v,
                        "bspd" | "base_speed" | "spd" => config.base_speed = v,
                        "dev" | "deviation" | "max_deviation" => config.max_deviation = v,
                        "phase" | "ph" => config.phase = v,
                        "fmul" | "freq_mul" | "frequency_multiplier" => config.frequency_multiplier = v,
                        "off" | "offset" => config.offset = v,
                        "pow" | "power" => config.power = v,
                        "min" | "clamp_min" => config.clamp_min = v,
                        "max" | "clamp_max" => config.clamp_max = v,
                        _ => {
                            writeln!(w, "Unknown param '{key}'. Valid: bspd, sens, dev, phase, fmul, off, pow, min, max")?;
                            return Ok(());
                        }
                    }
                }
            }

            config.sanitise(!is_freq);

            let mut cfg = data.state.engine.config().await;
            let ch_cfg = if channel == "A" { &mut cfg.channel_a } else { &mut cfg.channel_b };
            let target = if is_freq {
                &mut ch_cfg.freq_modulation
            } else {
                &mut ch_cfg.intensity_modulation
            };
            target[seg] = Some(config.clone());
            data.state.engine.set_config(cfg).await;

            let target_name = if is_freq { "freq" } else { "int" };
            writeln!(
                w,
                "[ch-{channel}] {target_name}[{seg}] modulation set: {config}"
            )?;
            Ok(())
        })
    }
}
