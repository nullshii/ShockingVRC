/// Usage:
///   cargo run --release --example Cli
///   cargo run --release --example Cli -- --port 9001 --scan-timeout 10
///   cargo run --release --example Cli -- --help
///   $env:RUST_LOG="debug"; cargo run --release --example cli
///
/// Controls (interactive, type + Enter):
///   add-a  <type> <name>      — add zone to channel A  (e.g. add-a Orf Cock)
///   add-a  <type> *           — add ALL zones of a type to channel A (wildcard)
///   add-a  * *                — add every avatar zone to channel A
///   add-b  <type> <name>      — same for channel B
///   add-all-a [type]          — add all currently detected avatar zones to A
///   add-all-b [type]          — add all currently detected avatar zones to B
///   rm-a   <type> <name>      — remove zone from channel A (* supported)
///   rm-b   <type> <name>      — remove zone from channel B
///   freq-a <v0> <v1> <v2> <v3> — channel A frequency segments (100–240)
///   freq-b <v0> <v1> <v2> <v3> — channel B frequency segments
///   int-a  <v0> <v1> <v2> <v3> — channel A intensity segments (0–100)
///   int-b  <v0> <v1> <v2> <v3> — channel B intensity segments
///   lim-a  <min> <max>        — channel A power limits (0–200)
///   lim-b  <min> <max>        — channel B power limits
///   agg-a  max|sum|avg        — aggregation mode channel A
///   agg-b  max|sum|avg        — aggregation mode channel B
///   zones                     — list all avatar zones (shows channel assignment)
///   status                    — current channel levels and active zones
///   config                    — print current config
///   save                      — save config to cli_config.json
///   load                      — load config from cli_config.json
///   quit / exit               — stop and exit
use std::path::Path;
use std::str::FromStr;
use std::time::Duration;

use dglab::cli::{AggregationMode, ChannelConfig, CliConfig, CliEngine, PowerLimits, ZoneId};
use dglab::{AvatarScanner, CoyoteDevice, ZoneEvent, ZoneType};

const CONFIG_FILE: &str = "cli_config.json";

#[tokio::main]
async fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let args = parse_args();

    print_banner();

    //Load or build config
    let config = if Path::new(CONFIG_FILE).exists() {
        match load_config(CONFIG_FILE) {
            Ok(c) => {
                println!("[config] Loaded from {CONFIG_FILE}");
                c
            }
            Err(e) => {
                println!("[config] Failed to load {CONFIG_FILE}: {e} — using defaults");
                default_config()
            }
        }
    } else {
        println!("[config] No {CONFIG_FILE} found — using defaults (see 'save' command)");
        default_config()
    };

    print_config_summary(&config);

    //OSC scanner
    println!("\n[osc] Starting OSC listener on UDP port {}...", args.port);
    let scanner = AvatarScanner::new(args.port);
    scanner.start().await.expect("Failed to start OSC listener");

    println!("[osc] Scanning for VRChat (up to 5 s)...");
    match scanner.discover_wait().await {
        Ok(true) => {
            if let Some(addr) = scanner.vrchat_address().await {
                println!(
                    "[osc] VRChat found → {} (OSC {}:{})",
                    addr.http_addr, addr.osc_ip, addr.osc_port
                );
            }
            let zones = scanner.zones().await;
            println!("[osc] Avatar zones found: {}", zones.len());
            for z in &zones {
                println!("      [{:<5}] {}", z.zone_type, z.id);
            }
        }
        Ok(false) => println!("[osc] VRChat not found — enable OSC in Settings → OSC. Retrying on avatar change."),
        Err(e) => println!("[osc] Discovery error: {e}"),
    }

    // CLI engine (starts immediately, no device needed)
    let engine = CliEngine::new(config);
    let status_rx = engine.subscribe_status();
    let _handle = engine.start(&scanner).await;

    println!("\n[cli] Engine started. Type 'help' for commands, 'quit' to exit.");
    println!("[ble] Searching for DGLab Coyote V3 in background...\n");
    print_status_header();

    // Background BLE reconnect loop
    {
        let engine_ble = engine.clone();
        let scan_timeout = args.scan_timeout;
        tokio::spawn(async move {
            loop {
                log::debug!("[ble] Starting BLE scan ({}s)...", scan_timeout);
                match CoyoteDevice::scan_first_with_timeout(Duration::from_secs(scan_timeout)).await {
                    Ok(Some(mut dev)) => {
                        println!("\n[ble] Connected: {} ({})", dev.name(), dev.mac_address());
                        dev.start();
                        engine_ble.connect_device(&dev).await;

                        // Monitor until device disconnects
                        loop {
                            tokio::time::sleep(Duration::from_secs(2)).await;
                            if !dev.is_connected().await {
                                println!("\n[ble] Device disconnected — rescanning...");
                                engine_ble.disconnect_device().await;
                                break;
                            }
                        }
                        // dev drops here; its tasks are aborted, output loop already stopped
                    }
                    Ok(None) => {
                        log::debug!("[ble] No device found, retrying in 10 s");
                        tokio::time::sleep(Duration::from_secs(10)).await;
                    }
                    Err(e) => {
                        log::warn!("[ble] Scan error: {e}, retrying in 10 s");
                        tokio::time::sleep(Duration::from_secs(10)).await;
                    }
                }
            }
        });
    }

    // Background status display
    tokio::spawn(async move {
        let mut rx = status_rx;
        loop {
            match rx.recv().await {
                Ok(status) => {
                    let a = &status.channel_a;
                    let b = &status.channel_b;
                    if a.raw_level > 0.001 || b.raw_level > 0.001 {
                        print_status_line(a.raw_level, a.strength, b.raw_level, b.strength);
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    log::debug!("Status receiver lagged {n}");
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    });

    // Interactive command loop
    command_loop(&engine, &scanner).await;

    // Graceful shutdown
    engine.disconnect_device().await;
    tokio::time::sleep(Duration::from_millis(150)).await;
    println!("[cli] Stopped. Goodbye.");
}

//Interactive command loop
async fn command_loop(engine: &CliEngine, scanner: &AvatarScanner) {
    use tokio::io::AsyncBufReadExt;
    let stdin = tokio::io::stdin();
    let mut reader = tokio::io::BufReader::new(stdin).lines();

    while let Ok(Some(line)) = reader.next_line().await {
        let trimmed: String = line.trim().to_string();
        let parts: Vec<&str> = trimmed.split_whitespace().collect();
        if parts.is_empty() {
            continue;
        }

        match parts.as_slice() {
            ["help"] | ["?"] => print_help(),

            ["quit"] | ["exit"] => break,

            ["status"] => {
                let s = engine.current_status().await;
                print_full_status(&s).await;
            }

            ["zones"] => {
                let zones = scanner.zones().await;
                let cfg = engine.config().await;
                println!("\n[zones] {} zone(s) seen on avatar:", zones.len());
                println!("  {:<5}  {:<30}  {:<8}  {}", "Type", "Name", "Level", "Channel");
                println!("  {}", "─".repeat(60));
                for z in &zones {
                    let zone_id = ZoneId::new(z.zone_type, &z.id);
                    let ch = if cfg.channel_a.zones.iter().any(|p| p.matches(&zone_id)) {
                        "A"
                    } else if cfg.channel_b.zones.iter().any(|p| p.matches(&zone_id)) {
                        "B"
                    } else {
                        "—"
                    };
                    println!("  {:<5}  {:<30}  {:<8.3}  {}", z.zone_type.to_string(), z.id, z.level, ch);
                }
                println!();
            }

            ["config"] => {
                print_config_summary(&engine.config().await);
            }

            ["save"] => {
                let cfg = engine.config().await;
                match save_config(CONFIG_FILE, &cfg) {
                    Ok(_) => println!("[config] Saved to {CONFIG_FILE}"),
                    Err(e) => println!("[config] Save failed: {e}"),
                }
            }

            ["load"] => match load_config(CONFIG_FILE) {
                Ok(cfg) => {
                    engine.set_config(cfg).await;
                    println!("[config] Loaded from {CONFIG_FILE}");
                    println!("[config] Power limits will apply on next device connection.");
                    print_config_summary(&engine.config().await);
                }
                Err(e) => println!("[config] Load failed: {e}"),
            },

            ["add-a", ztype, name] => {
                let id = ZoneId::new(
                    ZoneType::from_str(ztype).unwrap_or_else(|_| {
                        eprintln!("Warning: '{}' invalid, using DGB", ztype);
                        ZoneType::DGB
                    }),
                    *name,
                );
                engine.add_zone_a(id.clone()).await;
                if id.is_wildcard() {
                    let matched = count_wildcard_matches(&id, &scanner.zones().await);
                    println!("[ch-A] Wildcard added: {id}  (matches {matched} zone(s) currently on avatar)");
                } else {
                    println!("[ch-A] Zone added: {id}");
                }
            }

            ["add-b", ztype, name] => {
                let id = ZoneId::new(
                    ZoneType::from_str(ztype).unwrap_or_else(|_| {
                        eprintln!("Warning: '{}' invalid, using DGB", ztype);
                        ZoneType::DGB
                    }),
                    *name,
                );
                engine.add_zone_b(id.clone()).await;
                if id.is_wildcard() {
                    let matched = count_wildcard_matches(&id, &scanner.zones().await);
                    println!("[ch-B] Wildcard added: {id}  (matches {matched} zone(s) currently on avatar)");
                } else {
                    println!("[ch-B] Zone added: {id}");
                }
            }

            // add-all-a [type] — add every currently visible zone (optionally filtered by type) to channel A
            ["add-all-a"] => {
                let added = add_all_zones(engine, scanner, Channel::A, None).await;
                println!("[ch-A] Added {added} zone(s) from avatar");
            }
            ["add-all-a", filter_type] => {
                let ft = ZoneType::from_str(*filter_type).unwrap_or_else(|_| {
                    eprintln!("Warning: '{}' invalid, using DGB", *filter_type);
                    ZoneType::DGB
                });
                let added = add_all_zones(engine, scanner, Channel::A, Some(ft)).await;
                println!("[ch-A] Added {added} zone(s) of type '{filter_type}' from avatar");
            }

            // add-all-b [type]
            ["add-all-b"] => {
                let added = add_all_zones(engine, scanner, Channel::B, None).await;
                println!("[ch-B] Added {added} zone(s) from avatar");
            }
            ["add-all-b", filter_type] => {
                let ft = ZoneType::from_str(*filter_type).unwrap_or_else(|_| {
                    eprintln!("Warning: '{}' invalid, using DGB", *filter_type);
                    ZoneType::DGB
                });
                let added = add_all_zones(engine, scanner, Channel::B, Some(ft)).await;
                println!("[ch-B] Added {added} zone(s) of type '{ft}' from avatar");
            }

            ["rm-a", ztype, name] => {
                let id = ZoneId::new(
                    ZoneType::from_str(ztype).unwrap_or_else(|_| {
                        eprintln!("Warning: '{}' invalid, using DGB", ztype);
                        ZoneType::DGB
                    }),
                    *name,
                );
                engine.remove_zone_a(&id).await;
                println!("[ch-A] Zone removed: {id}");
            }

            ["rm-b", ztype, name] => {
                let id = ZoneId::new(
                    ZoneType::from_str(ztype).unwrap_or_else(|_| {
                        eprintln!("Warning: '{}' invalid, using DGB", ztype);
                        ZoneType::DGB
                    }),
                    *name,
                );
                engine.remove_zone_b(&id).await;
                println!("[ch-B] Zone removed: {id}");
            }

            ["freq-a", v0, v1, v2, v3] => {
                if let Some(f) = parse_freq_4(v0, v1, v2, v3) {
                    engine.set_frequency_a(f).await;
                    println!("[ch-A] Frequency set: {f:?}");
                } else {
                    println!("Usage: freq-a <v0> <v1> <v2> <v3>  (values 100–240)");
                }
            }

            ["freq-b", v0, v1, v2, v3] => {
                if let Some(f) = parse_freq_4(v0, v1, v2, v3) {
                    engine.set_frequency_b(f).await;
                    println!("[ch-B] Frequency set: {f:?}");
                } else {
                    println!("Usage: freq-b <v0> <v1> <v2> <v3>  (values 100–240)");
                }
            }

            ["int-a", v0, v1, v2, v3] => {
                if let Some(i) = parse_4u8(v0, v1, v2, v3) {
                    engine.set_intensity_a(i).await;
                    println!("[ch-A] Intensity set: {i:?}");
                } else {
                    println!("Usage: int-a <v0> <v1> <v2> <v3>  (values 0–100)");
                }
            }

            ["int-b", v0, v1, v2, v3] => {
                if let Some(i) = parse_4u8(v0, v1, v2, v3) {
                    engine.set_intensity_b(i).await;
                    println!("[ch-B] Intensity set: {i:?}");
                } else {
                    println!("Usage: int-b <v0> <v1> <v2> <v3>  (values 0–100)");
                }
            }

            ["lim-a", min_s, max_s] => {
                if let (Ok(mn), Ok(mx)) = (min_s.parse::<u8>(), max_s.parse::<u8>()) {
                    engine.set_limits_a(PowerLimits::new(mn, mx)).await;
                    println!("[ch-A] Limits set: {mn}–{mx} (applies on next device connection)");
                } else {
                    println!("Usage: lim-a <min> <max>  (0–200)");
                }
            }

            ["lim-b", min_s, max_s] => {
                if let (Ok(mn), Ok(mx)) = (min_s.parse::<u8>(), max_s.parse::<u8>()) {
                    engine.set_limits_b(PowerLimits::new(mn, mx)).await;
                    println!("[ch-B] Limits set: {mn}–{mx} (applies on next device connection)");
                } else {
                    println!("Usage: lim-b <min> <max>  (0–200)");
                }
            }

            ["agg-a", mode] => {
                if let Some(m) = parse_agg(mode) {
                    let mut cfg = engine.config().await;
                    cfg.channel_a.aggregation = m;
                    engine.set_config(cfg).await;
                    println!("[ch-A] Aggregation set to {mode}");
                } else {
                    println!("Usage: agg-a <max|sum|avg>");
                }
            }

            ["agg-b", mode] => {
                if let Some(m) = parse_agg(mode) {
                    let mut cfg = engine.config().await;
                    cfg.channel_b.aggregation = m;
                    engine.set_config(cfg).await;
                    println!("[ch-B] Aggregation set to {mode}");
                } else {
                    println!("Usage: agg-b <max|sum|avg>");
                }
            }

            _ => {
                println!("Unknown command '{trimmed}'. Type 'help' for a list.");
            }
        }
    }
}

//Zone helpers
enum Channel {
    A,
    B,
}
async fn add_all_zones(
    engine: &CliEngine,
    scanner: &AvatarScanner,
    channel: Channel,
    type_filter: Option<ZoneType>,
) -> usize {
    let zones = scanner.zones().await;
    let cfg = engine.config().await;
    let mut added = 0usize;

    for z in &zones {
        if let Some(f) = type_filter {
            if z.zone_type != f {
                continue;
            }
        }
        let id = ZoneId::new(z.zone_type, &z.id);
        // Skip if already covered by an existing pattern
        let already = match channel {
            Channel::A => cfg.channel_a.zones.iter().any(|p| p.matches(&id)),
            Channel::B => cfg.channel_b.zones.iter().any(|p| p.matches(&id)),
        };
        if !already {
            match channel {
                Channel::A => engine.add_zone_a(id.clone()).await,
                Channel::B => engine.add_zone_b(id.clone()).await,
            }
            println!("  + {id}");
            added += 1;
        }
    }
    added
}

fn count_wildcard_matches(pattern: &ZoneId, zones: &[ZoneEvent]) -> usize {
    zones.iter().filter(|z| pattern.matches_event(z)).count()
}

//Default config
fn default_config() -> CliConfig {
    CliConfig {
        channel_a: ChannelConfig {
            zones: vec![
                ZoneId::new(dglab::ZoneType::Orf, "Pussy"),
                ZoneId::new(dglab::ZoneType::Orf, "Anal"),
            ],
            frequency: [30, 200, 30, 200],
            intensity: [80, 80, 80, 80],
            limits: PowerLimits::new(0, 30),
            aggregation: AggregationMode::Max,
        },
        channel_b: ChannelConfig {
            zones: vec![ZoneId::new(dglab::ZoneType::Pen, "Cock")],
            frequency: [60, 120, 60, 120],
            intensity: [80, 80, 80, 80],
            limits: PowerLimits::new(0, 30),
            aggregation: AggregationMode::Max,
        },
    }
}

//Config file I/O
fn save_config(path: &str, config: &CliConfig) -> Result<(), Box<dyn std::error::Error>> {
    let json = serde_json::to_string_pretty(config)?;
    std::fs::write(path, json)?;
    Ok(())
}

fn load_config(path: &str) -> Result<CliConfig, Box<dyn std::error::Error>> {
    let json = std::fs::read_to_string(path)?;
    Ok(serde_json::from_str(&json)?)
}

//Display helpers
fn print_banner() {
    println!("╔══════════════════════════════════════════════════════╗");
    println!("║         DGLab CLI — Two-Channel OSC Controller       ║");
    println!("║  Channels A & B  ·  Zone mapping  ·  Power limits    ║");
    println!("╚══════════════════════════════════════════════════════╝");
    println!();
}

fn print_help() {
    println!(
        "
Zone commands  (type: Orf | Pen | Touch | DGB  |  * = wildcard)
  add-a  <type> <name>        Add exact zone to channel A
  add-a  Orf *                Add ALL Orf zones to channel A (wildcard)
  add-a  * *                  Add every avatar zone to channel A
  add-b  <type> <name|*>      Same for channel B
  add-all-a [type]            Add all avatar zones currently seen to A
  add-all-b [type]            Add all avatar zones currently seen to B
  rm-a   <type> <name|*>      Remove zone/pattern from channel A
  rm-b   <type> <name|*>      Remove zone/pattern from channel B

Pulse shape
  freq-a <v0> <v1> <v2> <v3> Channel A frequency segments (100–240)
  freq-b <v0> <v1> <v2> <v3> Channel B frequency segments
  int-a  <v0> <v1> <v2> <v3> Channel A intensity segments  (0–100)
  int-b  <v0> <v1> <v2> <v3> Channel B intensity segments

Power limits
  lim-a  <min> <max>          Channel A strength range (0–200)
  lim-b  <min> <max>          Channel B strength range (0–200)
  agg-a  max|sum|avg          Channel A aggregation mode
  agg-b  max|sum|avg          Channel B aggregation mode

Info / config
  zones                       List all avatar zones + which channel uses them
  status                      Current levels, strength and active zones
  config                      Print full config
  save                        Save config to cli_config.json
  load                        Load config from cli_config.json
  quit / exit                 Stop and exit
"
    );
}

fn print_config_summary(cfg: &CliConfig) {
    println!();
    println!("┌────────────────────────────────────────────────────────────┐");
    println!("│                    Current CLI Config                      │");
    println!("├─────────────────────────────────┬──────────────────────────┤");
    println!("│  Channel A                      │  Channel B               │");
    println!("├─────────────────────────────────┼──────────────────────────┤");

    let a = &cfg.channel_a;
    let b = &cfg.channel_b;
    let a_zones: Vec<_> = a.zones.iter().map(|z| z.to_string()).collect();
    let b_zones: Vec<_> = b.zones.iter().map(|z| z.to_string()).collect();
    let max_rows = a_zones.len().max(b_zones.len()).max(1);

    for i in 0..max_rows {
        let az = a_zones.get(i).map(|s| s.as_str()).unwrap_or("");
        let bz = b_zones.get(i).map(|s| s.as_str()).unwrap_or("");
        println!("│  zone: {az:<25}│  zone: {bz:<17}│");
    }

    println!("├─────────────────────────────────┼──────────────────────────┤");
    println!(
        "│  limits : {:>3}–{:<3}                 │  limits : {:>3}–{:<3}           │",
        a.limits.min, a.limits.max, b.limits.min, b.limits.max
    );
    println!("│  freq   : {:?}  │  freq   : {:?} │", a.frequency, b.frequency);
    println!("│  intens : {:?}  │  intens : {:?} │", a.intensity, b.intensity);
    println!("└─────────────────────────────────┴──────────────────────────┘");
    println!();
}

fn print_status_header() {
    println!("{:<42} {:<42}", "  Channel A", "  Channel B");
    println!(
        "{:<8} {:<22} {:<8}   {:<8} {:<22} {:<8}",
        "Level", "Bar", "Str", "Level", "Bar", "Str"
    );
    println!("{}", "─".repeat(88));
}

fn print_status_line(la: f32, sa: u8, lb: f32, sb: u8) {
    let bar_a = power_bar(la, 20);
    let bar_b = power_bar(lb, 20);
    println!("{:.3}   {}  {:>3}     {:.3}   {}  {:>3}", la, bar_a, sa, lb, bar_b, sb);
}

async fn print_full_status(status: &dglab::cli::engine::CliStatus) {
    let a = &status.channel_a;
    let b = &status.channel_b;
    let dev_str = if status.device_connected {
        "connected"
    } else {
        "searching..."
    };
    println!();
    println!("┌──────────────────────────────────────────────────────┐");
    println!("│  Device: {dev_str:<44}│");
    println!("├──────────────────────────┬──────────────────────────┤");
    println!("│  Channel A               │  Channel B               │");
    println!("├──────────────────────────┼──────────────────────────┤");
    println!("│  level    : {:<13.3}│  level    : {:<13.3}│", a.raw_level, b.raw_level);
    println!("│  strength : {:<13}│  strength : {:<13}│", a.strength, b.strength);
    println!("│  active zones:           │  active zones:           │");

    let max_zones = a.active_zones.len().max(b.active_zones.len()).max(1);
    for i in 0..max_zones {
        let az = a
            .active_zones
            .get(i)
            .map(|(id, lvl)| format!("{id} ({lvl:.2})"))
            .unwrap_or_default();
        let bz = b
            .active_zones
            .get(i)
            .map(|(id, lvl)| format!("{id} ({lvl:.2})"))
            .unwrap_or_default();
        println!("│    {az:<22}│    {bz:<22}│");
    }

    println!("└──────────────────────────┴──────────────────────────┘");
}

fn power_bar(level: f32, width: usize) -> String {
    let filled = ((level * width as f32).round() as usize).min(width);
    let bar: String = "█".repeat(filled) + &"░".repeat(width - filled);
    format!("[{bar}]")
}

// Argument parsing

struct Args {
    port: u16,
    scan_timeout: u64,
}

fn parse_args() -> Args {
    let args: Vec<String> = std::env::args().collect();
    let mut port = 9001u16;
    let mut scan_timeout = 8u64;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--help" | "-h" => {
                println!("Usage: cli [--port <n>] [--scan-timeout <secs>]");
                println!("  --port          UDP OSC port (default: 9001)");
                println!("  --scan-timeout  BLE scan timeout seconds (default: 8)");
                std::process::exit(0);
            }
            "--port" => {
                if let Some(v) = args.get(i + 1) {
                    port = v.parse().unwrap_or(port);
                    i += 1;
                }
            }
            "--scan-timeout" => {
                if let Some(v) = args.get(i + 1) {
                    scan_timeout = v.parse().unwrap_or(scan_timeout);
                    i += 1;
                }
            }
            _ => {}
        }
        i += 1;
    }
    Args { port, scan_timeout }
}

fn parse_freq_4(v0: &str, v1: &str, v2: &str, v3: &str) -> Option<[u8; 4]> {
    let a: u8 = v0.parse().ok()?;
    let b: u8 = v1.parse().ok()?;
    let c: u8 = v2.parse().ok()?;
    let d: u8 = v3.parse().ok()?;
    Some([a, b, c, d])
}

fn parse_4u8(v0: &str, v1: &str, v2: &str, v3: &str) -> Option<[u8; 4]> {
    let a: u8 = v0.parse().ok()?;
    let b: u8 = v1.parse().ok()?;
    let c: u8 = v2.parse().ok()?;
    let d: u8 = v3.parse().ok()?;
    if a > 100 || b > 100 || c > 100 || d > 100 {
        return None;
    }
    Some([a, b, c, d])
}

fn parse_agg(s: &str) -> Option<AggregationMode> {
    match s {
        "max" => Some(AggregationMode::Max),
        "sum" => Some(AggregationMode::Sum),
        "avg" | "average" => Some(AggregationMode::Average),
        _ => None,
    }
}
