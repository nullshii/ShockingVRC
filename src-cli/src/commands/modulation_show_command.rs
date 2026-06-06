use std::io::Write;

use crate::engine::command::{Command, CommandData, CommandFuture};

pub struct ModulationShowCommand;

impl Command for ModulationShowCommand {
    fn names(&self) -> &[&str] {
        &["mod-show-a", "mod-show-b", "mod-show"]
    }

    fn description(&self) -> &str {
        "Show current modulation config for channel."
    }

    fn execute(&self, cmd_name: String, _args: Vec<String>, data: CommandData) -> CommandFuture {
        Box::pin(async move {
            let mut w = data.writer;
            let cfg = data.state.engine.config().await;

            let show_channel = |w: &mut dyn Write, label: &str, ch: &shocking_vrc_core::cli::ChannelConfig| -> std::io::Result<()> {
                writeln!(w, "\n[{label}] Frequency modulation:")?;
                for (i, m) in ch.freq_modulation.iter().enumerate() {
                    match m {
                        Some(c) => writeln!(w, "  seg[{i}]: {c}")?,
                        None => writeln!(w, "  seg[{i}]: off")?,
                    }
                }
                writeln!(w, "[{label}] Intensity modulation:")?;
                for (i, m) in ch.intensity_modulation.iter().enumerate() {
                    match m {
                        Some(c) => writeln!(w, "  seg[{i}]: {c}")?,
                        None => writeln!(w, "  seg[{i}]: off")?,
                    }
                }
                Ok(())
            };

            if cmd_name == "mod-show" || cmd_name == "mod-show-a" {
                show_channel(&mut w, "ch-A", &cfg.channel_a)?;
            }
            if cmd_name == "mod-show" || cmd_name == "mod-show-b" {
                show_channel(&mut w, "ch-B", &cfg.channel_b)?;
            }
            Ok(())
        })
    }
}
