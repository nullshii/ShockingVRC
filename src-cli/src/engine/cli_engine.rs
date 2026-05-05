use tokio::io::{self, AsyncBufReadExt, BufReader};

use crate::engine::command_registry::CommandRegistry;

pub struct CliEngine {
    registry: CommandRegistry,
}

impl CliEngine {
    pub fn new(registry: CommandRegistry) -> Self {
        CliEngine { registry }
    }

    pub async fn run(&self) -> io::Result<()> {
        loop {
            let user_input = get_user_input().await?;
            let user_input = match user_input {
                Some(i) => i.to_lowercase(),
                None => continue,
            };

            let user_input: Vec<&str> = user_input.split_whitespace().collect();

            if let Some((cmd_name, args)) = user_input.split_first() {
                let string_args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
                self.registry.run(cmd_name, string_args);
            }
        }
    }
}

async fn get_user_input() -> io::Result<Option<String>> {
    let stdin = io::stdin();
    let mut reader = BufReader::new(stdin).lines();

    reader.next_line().await
}
