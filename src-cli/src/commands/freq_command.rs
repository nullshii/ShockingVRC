use std::io::Write;

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
