use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use rustyline_async::SharedWriter;
use tokio::io;

use crate::app_state::AppState;
use crate::engine::command_registry::CommandRegistry;

pub type CommandFuture = Pin<Box<dyn Future<Output = io::Result<()>> + Send>>;

pub trait Command: Send + Sync {
    fn names(&self) -> &[&str];
    fn description(&self) -> &str;
    fn execute(&self, cmd_name: String, args: Vec<String>, data: CommandData) -> CommandFuture;
}

pub struct CommandData {
    pub registry: Arc<CommandRegistry>,
    pub writer: SharedWriter,
    pub state: Arc<AppState>,
}
