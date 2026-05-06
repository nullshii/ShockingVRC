use tokio::io;

use crate::engine::command::{Command, CommandData};

pub struct AddZoneCommand;

impl Command for AddZoneCommand {
    fn names(&self) -> &[&str] {
        &["add-zone", "new-zone", "az"]
    }

    fn description(&self) -> &str {
        "Add new zone to channel."
    }

    fn execute(&self, _args: Vec<String>, _data: CommandData) -> io::Result<()> {
        Ok(())
    }
}
