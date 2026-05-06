use std::io::Write;
use tokio::io;

use crate::engine::command::{Command, CommandData};

pub struct HelpCommand;

impl Command for HelpCommand {
    fn names(&self) -> &[&str] {
        &["help", "h", "?"]
    }

    fn description(&self) -> &str {
        "Print list of commands."
    }

    fn execute(&self, _args: Vec<String>, data: CommandData) -> io::Result<()> {
        let commands = data.registry.get_commands();

        writeln!(data.writer, "List of commands: ")?;
        for command in commands {
            writeln!(data.writer, "{} - {}", command.names().join(", "), command.description())?;
        }

        Ok(())
    }
}
