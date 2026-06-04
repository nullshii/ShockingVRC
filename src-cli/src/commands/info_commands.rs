use std::io::Write;

use shocking_vrc_core::cli::{CliConfig, ZoneId};

use crate::display::{print_config_summary, print_full_status};
use crate::engine::command::{Command, CommandData, CommandFuture};

const CONFIG_FILE: &str = "cli_config.json";

pub struct ZonesCommand;

impl Command for ZonesCommand {
    fn names(&self) -> &[&str] {
        &["zones"]
    }

    fn description(&self) -> &str {
        "List all avatar zones and their channel assignments."
    }

    fn execute(&self, _cmd_name: String, _args: Vec<String>, data: CommandData) -> CommandFuture {
        Box::pin(async move {
            let mut w = data.writer;
            let zones = data.state.scanner.zones().await;
            let cfg = data.state.engine.config().await;

            writeln!(w, "\n[zones] {} zone(s) seen on avatar:", zones.len())?;
            writeln!(
                w,
                "  {:<5}  {:<30}  {:<8}  {:<8}  {}",
                "Type", "Name", "Level", "Channel", "Mode"
            )?;
            writeln!(w, "  {}", "─".repeat(72))?;

            for z in &zones {
                let zone_id = ZoneId::new(z.zone_type, &z.id);
                let (ch, mode_str) = if let Some(e) =
                    cfg.channel_a.zones.iter().find(|e| e.id.matches(&zone_id))
                {
                    ("A", e.mode.to_string())
                } else if let Some(e) =
                    cfg.channel_b.zones.iter().find(|e| e.id.matches(&zone_id))
                {
                    ("B", e.mode.to_string())
                } else {
                    ("—", String::from("-"))
                };
                writeln!(
                    w,
                    "  {:<5}  {:<30}  {:<8.3}  {:<8}  {}",
                    z.zone_type.to_string(),
                    z.id,
                    z.level,
                    ch,
                    mode_str
                )?;
            }
            writeln!(w)?;
            Ok(())
        })
    }
}

pub struct StatusCommand;

impl Command for StatusCommand {
    fn names(&self) -> &[&str] {
        &["status"]
    }

    fn description(&self) -> &str {
        "Show current channel levels, strength and active zones."
    }

    fn execute(&self, _cmd_name: String, _args: Vec<String>, data: CommandData) -> CommandFuture {
        Box::pin(async move {
            let mut w = data.writer;
            let status = data.state.engine.current_status().await;
            print_full_status(&status, &mut w)?;
            Ok(())
        })
    }
}

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
