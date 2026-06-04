use std::io::{Error, ErrorKind};

use crate::engine::command::{Command, CommandData, CommandFuture};

pub struct ClearCommand;

impl Command for ClearCommand {
    fn names(&self) -> &[&str] {
        &["clear", "cls"]
    }

    fn description(&self) -> &str {
        "Clear console screen."
    }

    fn execute(&self, _cmd_name: String, _args: Vec<String>, _data: CommandData) -> CommandFuture {
        Box::pin(async move {
            clearscreen::clear().map_err(|e| Error::new(ErrorKind::Other, e))?;
            Ok(())
        })
    }
}
