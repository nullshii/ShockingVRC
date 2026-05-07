use std::process::exit;

use tokio::io;

use crate::engine::command::{Command, CommandData};

pub struct QuitCommand;

impl Command for QuitCommand {
    fn names(&self) -> &[&str] {
        &["Quit", "Exit", "Q"]
    }

    fn description(&self) -> &str {
        "Exit app."
    }

    fn execute(&self, _args: Vec<String>, _data: CommandData) -> io::Result<()> {
        exit(0);
    }
}
