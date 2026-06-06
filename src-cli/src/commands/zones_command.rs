use std::io::Write;

use shocking_vrc_core::cli::ZoneId;

use crate::engine::command::{Command, CommandData, CommandFuture};

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
