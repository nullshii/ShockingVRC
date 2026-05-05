use std::collections::HashMap;

use log::warn;

use crate::engine::command::{Command, CommandData};

pub struct CommandRegistry {
    commands: Vec<Box<dyn Command>>,
    lookup: HashMap<String, usize>,
}

impl CommandRegistry {
    pub fn new() -> Self {
        Self {
            commands: Vec::new(),
            lookup: HashMap::new(),
        }
    }

    pub fn add_command(&mut self, cmd: Box<dyn Command>) -> &mut Self {
        let index = self.commands.len();
        for name in cmd.names() {
            self.lookup.insert(name.to_string(), index);
        }
        self.commands.push(cmd);

        self
    }

    pub fn get_commands(&self) -> &[Box<dyn Command>] {
        &self.commands
    }

    pub fn run(&self, input: &str, args: Vec<String>) {
        if let Some(&idx) = self.lookup.get(input) {
            self.commands[idx].execute(args, CommandData { registry: &self });
        } else {
            warn!("Command not found, use help command to see list of commands.");
        }
    }
}
