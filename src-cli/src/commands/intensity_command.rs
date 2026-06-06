use std::io::Write;

use crate::engine::command::{Command, CommandData, CommandFuture};

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
