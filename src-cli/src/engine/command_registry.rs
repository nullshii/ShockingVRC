use std::collections::HashMap;
use std::io::Write;
use std::sync::Arc;

use rustyline_async::SharedWriter;
use tokio::io;

use crate::app_state::AppState;
use crate::engine::command::{Command, CommandData};

pub struct CommandRegistry {
    commands: Vec<Box<dyn Command>>,
    lookup: HashMap<String, usize>,
}

pub struct RegistryBuilder {
    commands: Vec<Box<dyn Command>>,
    lookup: HashMap<String, usize>,
}

impl RegistryBuilder {
    pub fn new() -> Self {
        Self {
            commands: Vec::new(),
            lookup: HashMap::new(),
        }
    }

    pub fn add_command(mut self, cmd: Box<dyn Command>) -> Self {
        let index = self.commands.len();
        for name in cmd.names() {
            self.lookup.insert(name.to_string().to_lowercase(), index);
        }
        self.commands.push(cmd);
        self
    }

    pub fn build(self) -> Arc<CommandRegistry> {
        Arc::new(CommandRegistry {
            commands: self.commands,
            lookup: self.lookup,
        })
    }
}

impl CommandRegistry {
    pub fn new() -> RegistryBuilder {
        RegistryBuilder::new()
    }

    pub fn get_commands(&self) -> &[Box<dyn Command>] {
        &self.commands
    }

    pub async fn run(
        this: Arc<Self>,
        input: &str,
        args: Vec<String>,
        writer: SharedWriter,
        state: Arc<AppState>,
    ) -> io::Result<()> {
        if let Some(&idx) = this.lookup.get(input) {
            this.commands[idx]
                .execute(
                    input.to_string(),
                    args,
                    CommandData {
                        registry: Arc::clone(&this),
                        writer,
                        state,
                    },
                )
                .await
        } else {
            let mut w = writer;
            writeln!(w, "Unknown command '{input}'. Type 'help' for a list.")?;
            Ok(())
        }
    }
}
