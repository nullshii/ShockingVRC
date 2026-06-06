use std::io::Write;

use shocking_vrc_core::cli::AggregationMode;

use crate::engine::command::{Command, CommandData, CommandFuture};

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
