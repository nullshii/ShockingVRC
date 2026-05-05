use crate::engine::command::{Command, CommandData};

pub struct HelpCommand;

impl Command for HelpCommand {
    fn names(&self) -> &[&str] {
        &["help", "h", "?"]
    }

    fn description(&self) -> &str {
        "Print list of commands."
    }

    fn execute(&self, _args: Vec<String>, data: CommandData) {
        let commands = data.registry.get_commands();

        println!("List of commands: ");
        for command in commands {
            println!("{} - {}", command.names().join(", "), command.description());
        }
    }
}
