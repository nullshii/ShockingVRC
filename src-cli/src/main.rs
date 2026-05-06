use shocking_vrc_cli::{
    commands::{add_zone_command::AddZoneCommand, help_command::HelpCommand, quit_command::QuitCommand},
    engine::{
        cli_engine::{CliEngine, CliError},
        command_registry::CommandRegistry,
    },
};

#[tokio::main]
async fn main() -> Result<(), CliError> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let registry = CommandRegistry::new()
        .add_command(Box::new(HelpCommand))
        .add_command(Box::new(AddZoneCommand))
        .add_command(Box::new(QuitCommand))
        .build();

    let engine = CliEngine::new(&registry);
    engine.run().await
}
