use crate::engine::command_registry::CommandRegistry;

pub trait Command {
    fn names(&self) -> &[&str];
    fn description(&self) -> &str;
    fn execute(&self, args: Vec<String>, data: CommandData);
}

pub struct CommandData<'a> {
    pub registry: &'a CommandRegistry,
}
