use std::io::Write;

use shocking_vrc_core::cli::PowerLimits;

use crate::engine::command::{Command, CommandData, CommandFuture};

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
