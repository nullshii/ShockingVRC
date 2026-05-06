use rustyline_async::{Readline, ReadlineError, ReadlineEvent};
use std::io::Write;

use crate::engine::command_registry::CommandRegistry;

pub struct CliEngine<'a> {
    registry: &'a CommandRegistry,
}

impl<'a> CliEngine<'a> {
    pub fn new(registry: &'a CommandRegistry) -> Self {
        CliEngine { registry }
    }

    pub async fn run(&self) -> Result<(), CliError> {
        let (mut rl, mut writer) = Readline::new("DG-LAB CLI> ".to_string())?;

        loop {
            tokio::select! {
                event = rl.readline() => {
                    match event {
                        Ok(ReadlineEvent::Line(line)) => {
                            let line = line.trim();
                            if line.is_empty() { continue; }

                            rl.add_history_entry(line.to_string());

                            let parts: Vec<&str> = line.split_whitespace().collect();
                            if let Some((cmd_name, args)) = parts.split_first() {
                                let string_args: Vec<String> = args.iter().map(|s| s.to_string()).collect();

                                // TODO: Make async
                                self.registry.run(cmd_name, string_args, &mut writer)?;
                            }
                        }
                        Ok(ReadlineEvent::Eof) => break, // Ctrl+D
                        Ok(ReadlineEvent::Interrupted) => break, // Ctrl+C
                        Err(e) => {
                            writeln!(writer, "Error: {}", e)?;
                            break;
                        }
                    }
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug)]
pub enum CliError {
    IoError(std::io::Error),
    ReadlineError(ReadlineError),
}

impl From<ReadlineError> for CliError {
    fn from(value: ReadlineError) -> Self {
        Self::ReadlineError(value)
    }
}

impl From<std::io::Error> for CliError {
    fn from(value: std::io::Error) -> Self {
        Self::IoError(value)
    }
}
