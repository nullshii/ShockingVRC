use std::io::{Error, ErrorKind};

use crate::engine::command::Command;

pub struct ClearCommand;

impl Command for ClearCommand {
    fn names(&self) -> &[&str] {
        &["Clear", "CLS"]
    }

    fn description(&self) -> &str {
        "Clear console screen."
    }

    fn execute(&self, _args: Vec<String>, _data: crate::engine::command::CommandData) -> tokio::io::Result<()> {
        clearscreen::clear().map_err(|e| Error::new(ErrorKind::Other, e))?;
        Ok(())
    }
}
