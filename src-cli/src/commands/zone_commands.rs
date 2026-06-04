use std::io::Write;
use std::str::FromStr;

use shocking_vrc_core::cli::{ContactMode, ZoneEntry, ZoneId};
use shocking_vrc_core::OldZoneType;

use crate::commands::add_zone_command::{parse_zone_id, report_zone_added};
use crate::engine::command::{Command, CommandData, CommandFuture};

pub struct RemoveZoneCommand;

impl Command for RemoveZoneCommand {
    fn names(&self) -> &[&str] {
        &["rm-a", "rm-b"]
    }

    fn description(&self) -> &str {
        "Remove zone from channel. Usage: rm-a <type> <name>"
    }

    fn execute(&self, cmd_name: String, args: Vec<String>, data: CommandData) -> CommandFuture {
        Box::pin(async move {
            let mut w = data.writer;
            let channel = if cmd_name == "rm-a" { "A" } else { "B" };

            if args.len() < 2 {
                writeln!(w, "Usage: {cmd_name} <type> <name>  (* for wildcard name)")?;
                return Ok(());
            }

            let id = parse_zone_id(&args[0], &args[1]);
            if channel == "A" {
                data.state.engine.remove_zone_a(&id).await;
            } else {
                data.state.engine.remove_zone_b(&id).await;
            }
            writeln!(w, "[ch-{channel}] Zone removed: {id}")?;
            Ok(())
        })
    }
}

pub struct ZoneModeCommand;

impl Command for ZoneModeCommand {
    fn names(&self) -> &[&str] {
        &["mode-a", "mode-b"]
    }

    fn description(&self) -> &str {
        "Change zone mode on channel. Usage: mode-a <type> <name> <mode>"
    }

    fn execute(&self, cmd_name: String, args: Vec<String>, data: CommandData) -> CommandFuture {
        Box::pin(async move {
            let mut w = data.writer;
            let channel = if cmd_name == "mode-a" { "A" } else { "B" };

            if args.len() < 3 {
                writeln!(w, "Usage: {cmd_name} <type> <name> <mode>  (mode: depth|speed|acc|recoil)")?;
                return Ok(());
            }

            let id = parse_zone_id(&args[0], &args[1]);
            match ContactMode::from_str(&args[2]) {
                Ok(m) => {
                    let found = if channel == "A" {
                        data.state.engine.set_zone_mode_a(&id, m).await
                    } else {
                        data.state.engine.set_zone_mode_b(&id, m).await
                    };
                    if found {
                        writeln!(w, "[ch-{channel}] Mode for {id} set to {m}")?;
                    } else {
                        writeln!(w, "[ch-{channel}] Zone {id} not found in channel {channel}")?;
                    }
                }
                Err(e) => writeln!(w, "[ch-{channel}] {e}")?,
            }
            Ok(())
        })
    }
}

pub struct AddAllZonesCommand;

impl Command for AddAllZonesCommand {
    fn names(&self) -> &[&str] {
        &["add-all-a", "add-all-b"]
    }

    fn description(&self) -> &str {
        "Add all currently visible avatar zones to channel. Usage: add-all-a [type]"
    }

    fn execute(&self, cmd_name: String, args: Vec<String>, data: CommandData) -> CommandFuture {
        Box::pin(async move {
            let mut w = data.writer;
            let channel = if cmd_name == "add-all-a" { "A" } else { "B" };

            let type_filter = if let Some(t) = args.first() {
                match OldZoneType::from_str(t) {
                    Ok(zt) => Some(zt),
                    Err(_) => {
                        writeln!(w, "Warning: '{}' is not a valid zone type, ignoring filter", t)?;
                        None
                    }
                }
            } else {
                None
            };

            let zones = data.state.scanner.zones().await;
            let cfg = data.state.engine.config().await;
            let mut added = 0usize;

            for z in &zones {
                if let Some(f) = type_filter {
                    if z.zone_type != f {
                        continue;
                    }
                }
                let id = ZoneId::new(z.zone_type, &z.id);
                let already = match channel {
                    "A" => cfg.channel_a.zones.iter().any(|e| e.id.matches(&id)),
                    _ => cfg.channel_b.zones.iter().any(|e| e.id.matches(&id)),
                };
                if !already {
                    let entry = ZoneEntry::with_default_mode(id.clone());
                    if channel == "A" {
                        data.state.engine.add_zone_entry_a(entry).await;
                    } else {
                        data.state.engine.add_zone_entry_b(entry).await;
                    }
                    let mode = ContactMode::default();
                    report_zone_added(&mut w, channel, &id, mode, &data.state.scanner).await?;
                    added += 1;
                }
            }
            writeln!(w, "[ch-{channel}] Added {added} zone(s) from avatar")?;
            Ok(())
        })
    }
}
