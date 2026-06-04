use std::io::Write;
use std::sync::atomic::Ordering;

use crate::display::print_status_header;
use crate::engine::command::{Command, CommandData, CommandFuture};

pub struct MonitorCommand;

impl Command for MonitorCommand {
    fn names(&self) -> &[&str] {
        &["mon", "monitor"]
    }

    fn description(&self) -> &str {
        "Toggle live power-stream display. Usage: mon on|off"
    }

    fn execute(&self, _cmd_name: String, args: Vec<String>, data: CommandData) -> CommandFuture {
        Box::pin(async move {
            let mut w = data.writer;
            let monitor = &data.state.monitor_enabled;

            match args.first().map(|s| s.as_str()) {
                None => {
                    let state = if monitor.load(Ordering::Relaxed) { "on" } else { "off" };
                    writeln!(w, "[mon] Live power stream is {state}. Use 'mon on' / 'mon off' to toggle.")?;
                }
                Some(arg) => match parse_on_off(arg) {
                    Some(true) => {
                        let was_on = monitor.swap(true, Ordering::Relaxed);
                        if !was_on {
                            print_status_header();
                        }
                        writeln!(w, "[mon] Live power stream: ON")?;
                    }
                    Some(false) => {
                        monitor.store(false, Ordering::Relaxed);
                        writeln!(w, "[mon] Live power stream: OFF (use 'status' for a snapshot)")?;
                    }
                    None => writeln!(w, "Usage: mon <on|off>")?,
                },
            }
            Ok(())
        })
    }
}

fn parse_on_off(s: &str) -> Option<bool> {
    match s {
        "on" | "1" | "true" | "yes" | "y" | "enable" | "enabled" => Some(true),
        "off" | "0" | "false" | "no" | "n" | "disable" | "disabled" => Some(false),
        _ => None,
    }
}
