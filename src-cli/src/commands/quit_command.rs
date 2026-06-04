use crate::engine::command::{Command, CommandData, CommandFuture};

pub struct QuitCommand;

impl Command for QuitCommand {
    fn names(&self) -> &[&str] {
        &["quit", "exit", "q"]
    }

    fn description(&self) -> &str {
        "Exit app."
    }

    fn execute(&self, _cmd_name: String, _args: Vec<String>, _data: CommandData) -> CommandFuture {
        Box::pin(async move {
            std::process::exit(0);
        })
    }
}
