use std::io::Write;

use crate::commands::add_zone_command::parse_zone_id;
use crate::engine::command::{Command, CommandData, CommandFuture};

pub struct RemoveZoneCommand;

impl Command for RemoveZoneCommand {
    fn names(&self) -> &[&str] {
        &["rm-a", "rm-b"]
    }

    fn description(&self) -> &str {
        "Remove zone from channel. Usage: rm-a <type> <name>"
    }

    fn execute(&self, cmd_name: String, args: Vec<String>, data: CommandData) -> CommandFuture {
        Box::pin(async move {
            let mut w = data.writer;
            let channel = if cmd_name == "rm-a" { "A" } else { "B" };

            if args.len() < 2 {
                writeln!(w, "Usage: {cmd_name} <type> <name>  (* for wildcard name)")?;
                return Ok(());
            }

            let id = parse_zone_id(&args[0], &args[1]);
            if channel == "A" {
                data.state.engine.remove_zone_a(&id).await;
            } else {
                data.state.engine.remove_zone_b(&id).await;
            }
            writeln!(w, "[ch-{channel}] Zone removed: {id}")?;
            Ok(())
        })
    }
}
