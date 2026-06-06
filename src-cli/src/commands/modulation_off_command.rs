use std::io::Write;

use crate::engine::command::{Command, CommandData, CommandFuture};

pub struct ModulationOffCommand;

impl Command for ModulationOffCommand {
    fn names(&self) -> &[&str] {
        &["mod-off-a", "mod-off-b"]
    }

    fn description(&self) -> &str {
        "Disable modulation. Usage: mod-off-a <seg 0-3 | all> [freq|int|both]"
    }

    fn execute(&self, cmd_name: String, args: Vec<String>, data: CommandData) -> CommandFuture {
        Box::pin(async move {
            let mut w = data.writer;
            let channel = if cmd_name.ends_with("-a") { "A" } else { "B" };

            if args.is_empty() {
                writeln!(w, "Usage: {cmd_name} <segment 0-3 | all> [freq|int|both]")?;
                return Ok(());
            }

            let target_type = args.get(1).map(|s| s.as_str()).unwrap_or("both");

            let mut cfg = data.state.engine.config().await;
            let ch_cfg = if channel == "A" { &mut cfg.channel_a } else { &mut cfg.channel_b };

            let clear = |idx: usize, ch: &mut shocking_vrc_core::cli::ChannelConfig, tt: &str| {
                if tt == "freq" || tt == "both" {
                    ch.freq_modulation[idx] = None;
                }
                if tt == "int" || tt == "intensity" || tt == "both" {
                    ch.intensity_modulation[idx] = None;
                }
            };

            if args[0] == "all" {
                for i in 0..4 {
                    clear(i, ch_cfg, target_type);
                }
                writeln!(w, "[ch-{channel}] All modulation disabled ({target_type})")?;
            } else {
                match args[0].parse::<usize>() {
                    Ok(seg) if seg < 4 => {
                        clear(seg, ch_cfg, target_type);
                        writeln!(w, "[ch-{channel}] Modulation seg[{seg}] disabled ({target_type})")?;
                    }
                    _ => {
                        writeln!(w, "Segment must be 0, 1, 2, 3 or 'all'.")?;
                        return Ok(());
                    }
                }
            }

            data.state.engine.set_config(cfg).await;
            Ok(())
        })
    }
}
