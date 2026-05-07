use tokio::io;

use crate::engine::command::{Command, CommandData};

pub struct AddZoneCommand;

impl Command for AddZoneCommand {
    fn names(&self) -> &[&str] {
        &["Add-Zone", "New-Zone", "AZ"]
    }

    fn description(&self) -> &str {
        "Add new zone to channel."
    }

    fn execute(&self, _args: Vec<String>, _data: CommandData) -> io::Result<()> {
        Ok(())
    }
}
