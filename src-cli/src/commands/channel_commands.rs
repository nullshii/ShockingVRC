use std::io::Write;

use shocking_vrc_core::cli::{AggregationMode, PowerLimits};
use shocking_vrc_core::{hz_to_raw, raw_to_hz};

use crate::engine::command::{Command, CommandData, CommandFuture};

pub struct FreqCommand;

impl Command for FreqCommand {
    fn names(&self) -> &[&str] {
        &["freq-a", "freq-b", "freq-a-hz", "freq-b-hz"]
    }

    fn description(&self) -> &str {
        "Set channel frequency. Usage: freq-a <v0..v3> | freq-a-hz <hz0..hz3>"
    }

    fn execute(&self, cmd_name: String, args: Vec<String>, data: CommandData) -> CommandFuture {
        Box::pin(async move {
            let mut w = data.writer;
            let is_hz = cmd_name.ends_with("-hz");
            let channel = if cmd_name.starts_with("freq-a") { "A" } else { "B" };

            if args.len() < 4 {
                if is_hz {
                    writeln!(w, "Usage: {cmd_name} <hz0> <hz1> <hz2> <hz3>  (1–100 Hz each)")?;
                } else {
                    writeln!(w, "Usage: {cmd_name} <v0> <v1> <v2> <v3>  (raw 10–255)")?;
                }
                return Ok(());
            }

            let parsed: Vec<Option<f32>> = args[..4].iter().map(|s| s.parse::<f32>().ok()).collect();
            if parsed.iter().any(|v| v.is_none()) {
                writeln!(w, "All four values must be numbers.")?;
                return Ok(());
            }
            let vals: Vec<f32> = parsed.into_iter().flatten().collect();

            let freq: [u8; 4] = if is_hz {
                let invalid = vals.iter().any(|&hz| !(1.0..=100.0).contains(&hz));
                if invalid {
                    writeln!(w, "Hz values must be in range 1–100.")?;
                    return Ok(());
                }
                [hz_to_raw(vals[0]), hz_to_raw(vals[1]), hz_to_raw(vals[2]), hz_to_raw(vals[3])]
            } else {
                let as_u8: Option<Vec<u8>> = vals.iter().map(|&v| {
                    let u = v as u8;
                    if (10..=255).contains(&u) { Some(u) } else { None }
                }).collect();
                match as_u8 {
                    Some(v) => [v[0], v[1], v[2], v[3]],
                    None => {
                        writeln!(w, "Raw frequency values must be in range 10–255.")?;
                        return Ok(());
                    }
                }
            };

            if channel == "A" {
                data.state.engine.set_frequency_a(freq).await;
            } else {
                data.state.engine.set_frequency_b(freq).await;
            }

            if is_hz {
                let hz = freq.map(raw_to_hz);
                writeln!(
                    w,
                    "[ch-{channel}] Frequency set: {:.1}Hz {:.1}Hz {:.1}Hz {:.1}Hz (raw {:?})",
                    hz[0], hz[1], hz[2], hz[3], freq
                )?;
            } else {
                writeln!(w, "[ch-{channel}] Frequency set: {:?}", freq)?;
            }
            Ok(())
        })
    }
}

pub struct IntensityCommand;

impl Command for IntensityCommand {
    fn names(&self) -> &[&str] {
        &["int-a", "int-b"]
    }

    fn description(&self) -> &str {
        "Set channel intensity segments. Usage: int-a <v0..v3>  (0–100 each)"
    }

    fn execute(&self, cmd_name: String, args: Vec<String>, data: CommandData) -> CommandFuture {
        Box::pin(async move {
            let mut w = data.writer;
            let channel = if cmd_name == "int-a" { "A" } else { "B" };

            if args.len() < 4 {
                writeln!(w, "Usage: {cmd_name} <v0> <v1> <v2> <v3>  (values 0–100)")?;
                return Ok(());
            }

            let parsed: Option<Vec<u8>> = args[..4]
                .iter()
                .map(|s| s.parse::<u8>().ok().filter(|&v| v <= 100))
                .collect();

            match parsed {
                Some(v) => {
                    let intensity = [v[0], v[1], v[2], v[3]];
                    if channel == "A" {
                        data.state.engine.set_intensity_a(intensity).await;
                    } else {
                        data.state.engine.set_intensity_b(intensity).await;
                    }
                    writeln!(w, "[ch-{channel}] Intensity set: {:?}", intensity)?;
                }
                None => writeln!(w, "Usage: {cmd_name} <v0> <v1> <v2> <v3>  (values 0–100)")?,
            }
            Ok(())
        })
    }
}

pub struct LimitsCommand;

impl Command for LimitsCommand {
    fn names(&self) -> &[&str] {
        &["lim-a", "lim-b"]
    }

    fn description(&self) -> &str {
        "Set channel power limits. Usage: lim-a <min> <max>  (0–200)"
    }

    fn execute(&self, cmd_name: String, args: Vec<String>, data: CommandData) -> CommandFuture {
        Box::pin(async move {
            let mut w = data.writer;
            let channel = if cmd_name == "lim-a" { "A" } else { "B" };

            if args.len() < 2 {
                writeln!(w, "Usage: {cmd_name} <min> <max>  (0–200)")?;
                return Ok(());
            }

            match (args[0].parse::<u8>(), args[1].parse::<u8>()) {
                (Ok(mn), Ok(mx)) => {
                    let limits = PowerLimits::new(mn, mx);
                    if channel == "A" {
                        data.state.engine.set_limits_a(limits).await;
                    } else {
                        data.state.engine.set_limits_b(limits).await;
                    }
                    writeln!(w, "[ch-{channel}] Limits set: {mn}–{mx}")?;
                }
                _ => writeln!(w, "Usage: {cmd_name} <min> <max>  (0–200)")?,
            }
            Ok(())
        })
    }
}

pub struct AggregationCommand;

impl Command for AggregationCommand {
    fn names(&self) -> &[&str] {
        &["agg-a", "agg-b"]
    }

    fn description(&self) -> &str {
        "Set aggregation mode for channel. Usage: agg-a max|sum|avg"
    }

    fn execute(&self, cmd_name: String, args: Vec<String>, data: CommandData) -> CommandFuture {
        Box::pin(async move {
            let mut w = data.writer;
            let channel = if cmd_name == "agg-a" { "A" } else { "B" };

            if args.is_empty() {
                writeln!(w, "Usage: {cmd_name} <max|sum|avg>")?;
                return Ok(());
            }

            let mode = match args[0].as_str() {
                "max" => Some(AggregationMode::Max),
                "sum" => Some(AggregationMode::Sum),
                "avg" | "average" => Some(AggregationMode::Average),
                _ => None,
            };

            match mode {
                Some(m) => {
                    let mut cfg = data.state.engine.config().await;
                    if channel == "A" {
                        cfg.channel_a.aggregation = m;
                    } else {
                        cfg.channel_b.aggregation = m;
                    }
                    data.state.engine.set_config(cfg).await;
                    writeln!(w, "[ch-{channel}] Aggregation set to {}", args[0])?;
                }
                None => writeln!(w, "Usage: {cmd_name} <max|sum|avg>")?,
            }
            Ok(())
        })
    }
}
