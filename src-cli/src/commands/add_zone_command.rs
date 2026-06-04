use std::io::Write;
use std::str::FromStr;

use shocking_vrc_core::cli::{ContactMode, ZoneEntry, ZoneId};
use shocking_vrc_core::{AvatarScanner, OldZoneType, ZoneEvent};

use crate::engine::command::{Command, CommandData, CommandFuture};

pub struct AddZoneCommand;

impl Command for AddZoneCommand {
    fn names(&self) -> &[&str] {
        &["add-a", "add-b"]
    }

    fn description(&self) -> &str {
        "Add zone to channel. Usage: add-a <type> <name> [mode]"
    }

    fn execute(&self, cmd_name: String, args: Vec<String>, data: CommandData) -> CommandFuture {
        Box::pin(async move {
            let mut w = data.writer;
            let channel = if cmd_name == "add-a" { "A" } else { "B" };

            if args.len() < 2 {
                writeln!(w, "Usage: {cmd_name} <type> <name> [mode]  (mode: depth|speed|acc|recoil)")?;
                return Ok(());
            }

            let id = parse_zone_id(&args[0], &args[1]);
            let mode = if args.len() >= 3 {
                match ContactMode::from_str(&args[2]) {
                    Ok(m) => m,
                    Err(e) => {
                        writeln!(w, "[ch-{channel}] {e}")?;
                        return Ok(());
                    }
                }
            } else {
                ContactMode::default()
            };

            let entry = ZoneEntry::new(id.clone(), mode);
            if channel == "A" {
                data.state.engine.add_zone_entry_a(entry).await;
            } else {
                data.state.engine.add_zone_entry_b(entry).await;
            }

            report_zone_added(&mut w, channel, &id, mode, &data.state.scanner).await?;
            Ok(())
        })
    }
}

pub fn parse_zone_id(ztype: &str, name: &str) -> ZoneId {
    let zt = OldZoneType::from_str(ztype).unwrap_or_else(|_| {
        eprintln!("Warning: '{}' is not a valid zone type, using DGB", ztype);
        OldZoneType::DGB
    });
    ZoneId::new(zt, name)
}

fn count_wildcard_matches(pattern: &ZoneId, zones: &[ZoneEvent]) -> usize {
    zones.iter().filter(|z| pattern.matches_event(z)).count()
}

pub async fn report_zone_added(
    w: &mut impl Write,
    channel: &str,
    id: &ZoneId,
    mode: ContactMode,
    scanner: &AvatarScanner,
) -> std::io::Result<()> {
    if id.is_wildcard() {
        let matched = count_wildcard_matches(id, &scanner.zones().await);
        writeln!(
            w,
            "[ch-{channel}] Wildcard added: {id} [{mode}]  (matches {matched} zone(s) currently on avatar)"
        )?;
    } else {
        writeln!(w, "[ch-{channel}] Zone added: {id} [{mode}]")?;
    }
    Ok(())
}
