use crate::display::print_full_status;
use crate::engine::command::{Command, CommandData, CommandFuture};

pub struct StatusCommand;

impl Command for StatusCommand {
    fn names(&self) -> &[&str] {
        &["status"]
    }

    fn description(&self) -> &str {
        "Show current channel levels, strength and active zones."
    }

    fn execute(&self, _cmd_name: String, _args: Vec<String>, data: CommandData) -> CommandFuture {
        Box::pin(async move {
            let mut w = data.writer;
            let status = data.state.engine.current_status().await;
            print_full_status(&status, &mut w)?;
            Ok(())
        })
    }
}
