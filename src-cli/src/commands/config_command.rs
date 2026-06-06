use std::io::Write;

use shocking_vrc_core::cli::CliConfig;

use crate::display::print_config_summary;
use crate::engine::command::{Command, CommandData, CommandFuture};

const CONFIG_FILE: &str = "cli_config.json";

pub struct ConfigCommand;

impl Command for ConfigCommand {
    fn names(&self) -> &[&str] {
        &["config", "save", "load"]
    }

    fn description(&self) -> &str {
        "Print config, save to or load from cli_config.json."
    }

    fn execute(&self, cmd_name: String, _args: Vec<String>, data: CommandData) -> CommandFuture {
        Box::pin(async move {
            let mut w = data.writer;
            match cmd_name.as_str() {
                "config" => {
                    let cfg = data.state.engine.config().await;
                    print_config_summary(&cfg, &mut w)?;
                }
                "save" => {
                    let cfg = data.state.engine.config().await;
                    match save_config(CONFIG_FILE, &cfg) {
                        Ok(_) => writeln!(w, "[config] Saved to {CONFIG_FILE}")?,
                        Err(e) => writeln!(w, "[config] Save failed: {e}")?,
                    }
                }
                "load" => {
                    let load_result = load_config(CONFIG_FILE)
                        .map_err(|e| e.to_string());
                    match load_result {
                        Ok(cfg) => {
                            data.state.engine.set_config(cfg).await;
                            data.state.engine.sync_hardware_limits().await;
                            writeln!(w, "[config] Loaded from {CONFIG_FILE}")?;
                            let cfg = data.state.engine.config().await;
                            print_config_summary(&cfg, &mut w)?;
                        }
                        Err(e) => writeln!(w, "[config] Load failed: {e}")?,
                    }
                }
                _ => {}
            }
            Ok(())
        })
    }
}

fn save_config(path: &str, config: &CliConfig) -> Result<(), Box<dyn std::error::Error>> {
    let json = serde_json::to_string_pretty(config)?;
    std::fs::write(path, json)?;
    Ok(())
}

fn load_config(path: &str) -> Result<CliConfig, Box<dyn std::error::Error>> {
    let json = std::fs::read_to_string(path)?;
    Ok(serde_json::from_str(&json)?)
}
