use std::io::Write;
use std::sync::Arc;

use rustyline_async::{Readline, ReadlineError, ReadlineEvent};

use crate::app_state::AppState;
use crate::engine::command_registry::CommandRegistry;

pub struct CliEngine {
    registry: Arc<CommandRegistry>,
    state: Arc<AppState>,
}

impl CliEngine {
    pub fn new(registry: Arc<CommandRegistry>, state: Arc<AppState>) -> Self {
        CliEngine { registry, state }
    }

    pub async fn run(&self) -> Result<(), CliError> {
        let (mut rl, writer) = Readline::new("ShockingVRC> ".to_string())?;

        loop {
            tokio::select! {
                event = rl.readline() => {
                    match event {
                        Ok(ReadlineEvent::Line(line)) => {
                            let line = line.trim().to_lowercase();
                            if line.is_empty() { continue; }

                            rl.add_history_entry(line.to_string());

                            let parts: Vec<&str> = line.split_whitespace().collect();
                            if let Some((cmd_name, args)) = parts.split_first() {
                                let string_args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
                                CommandRegistry::run(
                                    Arc::clone(&self.registry),
                                    cmd_name,
                                    string_args,
                                    writer.clone(),
                                    Arc::clone(&self.state),
                                ).await?;
                            }
                        }
                        Ok(ReadlineEvent::Eof) | Ok(ReadlineEvent::Interrupted) => break,
                        Err(e) => {
                            let mut w = writer.clone();
                            writeln!(w, "Error: {}", e)?;
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
