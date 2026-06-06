use std::io::Write;
use std::str::FromStr;

use shocking_vrc_core::cli::ContactMode;

use crate::commands::add_zone_command::parse_zone_id;
use crate::engine::command::{Command, CommandData, CommandFuture};

pub struct ZoneModeCommand;

impl Command for ZoneModeCommand {
    fn names(&self) -> &[&str] {
        &["mode-a", "mode-b"]
    }

    fn description(&self) -> &str {
        "Change zone mode on channel. Usage: mode-a <type> <name> <mode>"
    }

    fn execute(&self, cmd_name: String, args: Vec<String>, data: CommandData) -> CommandFuture {
        Box::pin(async move {
            let mut w = data.writer;
            let channel = if cmd_name == "mode-a" { "A" } else { "B" };

            if args.len() < 3 {
                writeln!(w, "Usage: {cmd_name} <type> <name> <mode>  (mode: depth|speed|acc|recoil)")?;
                return Ok(());
            }

            let id = parse_zone_id(&args[0], &args[1]);
            match ContactMode::from_str(&args[2]) {
                Ok(m) => {
                    let found = if channel == "A" {
                        data.state.engine.set_zone_mode_a(&id, m).await
                    } else {
                        data.state.engine.set_zone_mode_b(&id, m).await
                    };
                    if found {
                        writeln!(w, "[ch-{channel}] Mode for {id} set to {m}")?;
                    } else {
                        writeln!(w, "[ch-{channel}] Zone {id} not found in channel {channel}")?;
                    }
                }
                Err(e) => writeln!(w, "[ch-{channel}] {e}")?,
            }
            Ok(())
        })
    }
}
