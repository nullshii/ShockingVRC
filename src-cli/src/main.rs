/// App Usage:
///   cargo run --release --example Cli
///   cargo run --release --example Cli -- --port 9001 --scan-timeout 10
///   cargo run --release --example Cli -- --help
///   $env:RUST_LOG="debug"; cargo run --release --example cli
///
/// Controls (interactive, type + Enter):
///   add-a  <type> <name> [mode] — add zone to channel A  (e.g. add-a Orf Pussy depth)
///   add-a  <type> * [mode]      — add ALL zones of a type to channel A (wildcard)
///   add-a  * * [mode]           — add every avatar zone to channel A
///   add-b  <type> <name> [mode] — same for channel B
///   mode-a <type> <name> <mode> — change mode of an existing entry on A
///   mode-b <type> <name> <mode> — change mode of an existing entry on B
///   add-all-a [type]            — add all currently detected avatar zones to A
///   add-all-b [type]            — add all currently detected avatar zones to B
///   rm-a   <type> <name>        — remove zone from channel A (* supported)
///   rm-b   <type> <name>        — remove zone from channel B
///   <mode> = depth  |speed | acc | recoil  (default: depth, all UKF-filtered)
///   ukf [q r [alpha beta kappa] | reset] — per-contact UKF tuning (process/measurement noise)
///   freq-a <v0> <v1> <v2> <v3> — channel A frequency segments (raw 10–255)
///   freq-b <v0> <v1> <v2> <v3> — channel B frequency segments (raw 10–255)
///   freq-a-hz <h0> <h1> <h2> <h3> — channel A frequency in Hz (1–100)
///   freq-b-hz <h0> <h1> <h2> <h3> — channel B frequency in Hz (1–100)
///   int-a  <v0> <v1> <v2> <v3> — channel A intensity segments (0–100)
///   int-b  <v0> <v1> <v2> <v3> — channel B intensity segments
///   lim-a  <min> <max>        — channel A power limits (0–200)
///   lim-b  <min> <max>        — channel B power limits
///   agg-a  max|sum|avg        — aggregation mode channel A
///   agg-b  max|sum|avg        — aggregation mode channel B
///   zones                     — list all avatar zones (shows channel assignment)
///   status                    — current channel levels and active zones
///   mon on|off                — enable/disable the live power stream (default: on)
///   config                    — print current config
///   save                      — save config to cli_config.json
///   load                      — load config from cli_config.json
///   quit / exit               — stop and exit
use std::path::Path;
use std::str::FromStr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use shocking_vrc_core::cli::{
    AggregationMode, ChannelConfig, CliConfig, CliEngine, ContactMode, MotionNorms, PowerLimits, UkfConfig, ZoneEntry,
    ZoneId,
};
use shocking_vrc_core::{AvatarScanner, CoyoteDevice, ZoneEvent, ZoneType, hz_to_raw, raw_to_hz};

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

    let monitor_enabled = Arc::new(AtomicBool::new(true));

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
                        let dev = Arc::new(dev);
                        engine_ble.connect_device(Arc::clone(&dev)).await;

                        // Monitor until device disconnects
                        loop {
                            tokio::time::sleep(Duration::from_secs(2)).await;
                            if !dev.is_connected().await {
                                println!("\n[ble] Device disconnected — rescanning...");
                                engine_ble.disconnect_device().await;
                                break;
                            }
                        }
                        // dev Arc drops here; engine already released its ref in disconnect_device
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
    {
        let monitor_enabled = Arc::clone(&monitor_enabled);
        tokio::spawn(async move {
            let mut rx = status_rx;
            loop {
                match rx.recv().await {
                    Ok(status) => {
                        if !monitor_enabled.load(Ordering::Relaxed) {
                            continue;
                        }
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
    }

    // Interactive command loop
    command_loop(&engine, &scanner, &monitor_enabled).await;

    // Graceful shutdown
    engine.disconnect_device().await;
    tokio::time::sleep(Duration::from_millis(150)).await;
    println!("[cli] Stopped. Goodbye.");
}

//Interactive command loop
async fn command_loop(engine: &CliEngine, scanner: &AvatarScanner, monitor_enabled: &AtomicBool) {
    use tokio::io::AsyncBufReadExt;
    let stdin = tokio::io::stdin();
    let mut reader = tokio::io::BufReader::new(stdin).lines();

    while let Ok(Some(line)) = reader.next_line().await {
        let trimmed: String = line.trim().to_string().to_lowercase();
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
                println!("  {:<5}  {:<30}  {:<8}  {:<8}  {}", "Type", "Name", "Level", "Channel", "Mode");
                println!("  {}", "─".repeat(72));
                for z in &zones {
                    let zone_id = ZoneId::new(z.zone_type, &z.id);
                    let (ch, mode_str) = if let Some(e) = cfg.channel_a.zones.iter().find(|e| e.id.matches(&zone_id)) {
                        ("A", e.mode.to_string())
                    } else if let Some(e) = cfg.channel_b.zones.iter().find(|e| e.id.matches(&zone_id)) {
                        ("B", e.mode.to_string())
                    } else {
                        ("—", String::from("-"))
                    };
                    println!(
                        "  {:<5}  {:<30}  {:<8.3}  {:<8}  {}",
                        z.zone_type.to_string(),
                        z.id,
                        z.level,
                        ch,
                        mode_str
                    );
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
                    engine.sync_hardware_limits().await;
                    println!("[config] Loaded from {CONFIG_FILE}");
                    print_config_summary(&engine.config().await);
                }
                Err(e) => println!("[config] Load failed: {e}"),
            },

            ["add-a", ztype, name] => {
                let id = parse_zone_id(ztype, name);
                engine.add_zone_entry_a(ZoneEntry::with_default_mode(id.clone())).await;
                report_zone_added("A", &id, ContactMode::default(), scanner).await;
            }
            ["add-a", ztype, name, mode] => {
                let id = parse_zone_id(ztype, name);
                match ContactMode::from_str(mode) {
                    Ok(m) => {
                        engine.add_zone_entry_a(ZoneEntry::new(id.clone(), m)).await;
                        report_zone_added("A", &id, m, scanner).await;
                    }
                    Err(e) => println!("[ch-A] {e}"),
                }
            }

            ["add-b", ztype, name] => {
                let id = parse_zone_id(ztype, name);
                engine.add_zone_entry_b(ZoneEntry::with_default_mode(id.clone())).await;
                report_zone_added("B", &id, ContactMode::default(), scanner).await;
            }
            ["add-b", ztype, name, mode] => {
                let id = parse_zone_id(ztype, name);
                match ContactMode::from_str(mode) {
                    Ok(m) => {
                        engine.add_zone_entry_b(ZoneEntry::new(id.clone(), m)).await;
                        report_zone_added("B", &id, m, scanner).await;
                    }
                    Err(e) => println!("[ch-B] {e}"),
                }
            }

            ["mode-a", ztype, name, mode] => {
                let id = parse_zone_id(ztype, name);
                match ContactMode::from_str(mode) {
                    Ok(m) => {
                        if engine.set_zone_mode_a(&id, m).await {
                            println!("[ch-A] Mode for {id} set to {m}");
                        } else {
                            println!("[ch-A] Zone {id} not found in channel A");
                        }
                    }
                    Err(e) => println!("[ch-A] {e}"),
                }
            }

            ["mode-b", ztype, name, mode] => {
                let id = parse_zone_id(ztype, name);
                match ContactMode::from_str(mode) {
                    Ok(m) => {
                        if engine.set_zone_mode_b(&id, m).await {
                            println!("[ch-B] Mode for {id} set to {m}");
                        } else {
                            println!("[ch-B] Zone {id} not found in channel B");
                        }
                    }
                    Err(e) => println!("[ch-B] {e}"),
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

            ["freq-a-hz", h0, h1, h2, h3] => {
                if let Some(f) = parse_freq_4_hz(h0, h1, h2, h3) {
                    let hz = f.map(raw_to_hz);
                    engine.set_frequency_a(f).await;
                    println!(
                        "[ch-A] Frequency set: {:.1}Hz {:.1}Hz {:.1}Hz {:.1}Hz (raw {:?})",
                        hz[0], hz[1], hz[2], hz[3], f
                    );
                } else {
                    println!("Usage: freq-a-hz <hz0> <hz1> <hz2> <hz3>  (1–100 Hz each)");
                }
            }

            ["freq-b-hz", h0, h1, h2, h3] => {
                if let Some(f) = parse_freq_4_hz(h0, h1, h2, h3) {
                    let hz = f.map(raw_to_hz);
                    engine.set_frequency_b(f).await;
                    println!(
                        "[ch-B] Frequency set: {:.1}Hz {:.1}Hz {:.1}Hz {:.1}Hz (raw {:?})",
                        hz[0], hz[1], hz[2], hz[3], f
                    );
                } else {
                    println!("Usage: freq-b-hz <hz0> <hz1> <hz2> <hz3>  (1–100 Hz each)");
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
                    println!("[ch-A] Limits set: {mn}–{mx}");
                } else {
                    println!("Usage: lim-a <min> <max>  (0–200)");
                }
            }

            ["lim-b", min_s, max_s] => {
                if let (Ok(mn), Ok(mx)) = (min_s.parse::<u8>(), max_s.parse::<u8>()) {
                    engine.set_limits_b(PowerLimits::new(mn, mx)).await;
                    println!("[ch-B] Limits set: {mn}–{mx}");
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

            ["ukf"] => {
                let p = engine.ukf_params().await;
                println!(
                    "[ukf] q={:.4}  r={:.4}  alpha={:.3}  beta={:.3}  kappa={:.3}",
                    p.q, p.r, p.alpha, p.beta, p.kappa
                );
            }
            ["ukf", "reset"] | ["ukf", "default"] | ["ukf", "defaults"] => {
                engine.set_ukf_params(UkfConfig::default()).await;
                println!("[ukf] Reset to defaults");
            }
            ["ukf", q, r] => match (q.parse::<f32>(), r.parse::<f32>()) {
                (Ok(qv), Ok(rv)) if qv > 0.0 && rv > 0.0 => {
                    let mut p = engine.ukf_params().await;
                    p.q = qv;
                    p.r = rv;
                    engine.set_ukf_params(p).await;
                    println!("[ukf] q={qv:.4}  r={rv:.4}");
                }
                _ => println!("Usage: ukf <q> <r>  (positive floats; q=process noise, r=measurement noise)"),
            },
            ["ukf", q, r, alpha, beta, kappa] => {
                match (
                    q.parse::<f32>(),
                    r.parse::<f32>(),
                    alpha.parse::<f32>(),
                    beta.parse::<f32>(),
                    kappa.parse::<f32>(),
                ) {
                    (Ok(qv), Ok(rv), Ok(av), Ok(bv), Ok(kv)) if qv > 0.0 && rv > 0.0 && av > 0.0 => {
                        engine
                            .set_ukf_params(UkfConfig {
                                q: qv,
                                r: rv,
                                alpha: av,
                                beta: bv,
                                kappa: kv,
                            })
                            .await;
                        println!("[ukf] q={qv:.4}  r={rv:.4}  alpha={av:.3}  beta={bv:.3}  kappa={kv:.3}");
                    }
                    _ => println!(
                        "Usage: ukf <q> <r> <alpha> <beta> <kappa>  (q,r,alpha > 0; typical alpha 0.001..1, beta 2, kappa 0)"
                    ),
                }
            }

            ["norms"] => {
                let n = engine.norms().await;
                println!("[norms] speed={:.3}  acc={:.3}  recoil={:.3} ", n.speed, n.acc, n.recoil);
            }
            ["norms", "reset"] | ["norms", "default"] | ["norms", "defaults"] => {
                engine.set_norms(MotionNorms::default()).await;
                let n = engine.norms().await;
                println!(
                    "[norms] Reset to defaults: speed={:.3}  acc={:.3}  recoil={:.3}",
                    n.speed, n.acc, n.recoil
                );
            }
            ["norms", speed, acc, recoil] => match (speed.parse::<f32>(), acc.parse::<f32>(), recoil.parse::<f32>()) {
                (Ok(sv), Ok(av), Ok(rv)) if sv > 0.0 && av > 0.0 && rv > 0.0 => {
                    engine
                        .set_norms(MotionNorms {
                            speed: sv,
                            acc: av,
                            recoil: rv,
                        })
                        .await;
                    println!("[norms] speed={sv:.3}  acc={av:.3}  recoil={rv:.3}");
                }
                _ => println!("Usage: norms <speed> <acc> <recoil>  (all positive floats)"),
            },
            ["norm-speed", v] => match v.parse::<f32>() {
                Ok(val) if val > 0.0 => {
                    let mut n = engine.norms().await;
                    n.speed = val;
                    engine.set_norms(n).await;
                    println!("[norms] speed={val:.3}");
                }
                _ => println!("Usage: norm-speed <positive float>"),
            },
            ["norm-acc", v] => match v.parse::<f32>() {
                Ok(val) if val > 0.0 => {
                    let mut n = engine.norms().await;
                    n.acc = val;
                    engine.set_norms(n).await;
                    println!("[norms] acc={val:.3}");
                }
                _ => println!("Usage: norm-acc <positive float>"),
            },
            ["norm-recoil", v] => match v.parse::<f32>() {
                Ok(val) if val > 0.0 => {
                    let mut n = engine.norms().await;
                    n.recoil = val;
                    engine.set_norms(n).await;
                    println!("[norms] recoil={val:.3}");
                }
                _ => println!("Usage: norm-recoil <positive float>"),
            },

            ["mon"] | ["monitor"] => {
                let state = if monitor_enabled.load(Ordering::Relaxed) {
                    "on"
                } else {
                    "off"
                };
                println!("[mon] Live power stream is {state}. Use 'mon on' / 'mon off' to toggle.");
            }
            ["mon", arg] | ["monitor", arg] => match parse_on_off(arg) {
                Some(true) => {
                    let was_on = monitor_enabled.swap(true, Ordering::Relaxed);
                    if !was_on {
                        print_status_header();
                    }
                    println!("[mon] Live power stream: ON");
                }
                Some(false) => {
                    monitor_enabled.store(false, Ordering::Relaxed);
                    println!("[mon] Live power stream: OFF (use 'status' for a snapshot)");
                }
                None => println!("Usage: mon <on|off>"),
            },

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
            Channel::A => cfg.channel_a.zones.iter().any(|e| e.id.matches(&id)),
            Channel::B => cfg.channel_b.zones.iter().any(|e| e.id.matches(&id)),
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

fn parse_zone_id(ztype: &str, name: &str) -> ZoneId {
    let zt = ZoneType::from_str(ztype).unwrap_or_else(|_| {
        eprintln!("Warning: '{}' invalid, using DGB", ztype);
        ZoneType::DGB
    });
    ZoneId::new(zt, name)
}

async fn report_zone_added(channel: &str, id: &ZoneId, mode: ContactMode, scanner: &AvatarScanner) {
    if id.is_wildcard() {
        let matched = count_wildcard_matches(id, &scanner.zones().await);
        println!("[ch-{channel}] Wildcard added: {id} [{mode}]  (matches {matched} zone(s) currently on avatar)");
    } else {
        println!("[ch-{channel}] Zone added: {id} [{mode}]");
    }
}

//Default config
fn default_config() -> CliConfig {
    CliConfig {
        channel_a: ChannelConfig {
            zones: vec![
                ZoneEntry::new(ZoneId::new(shocking_vrc_core::ZoneType::Orf, "Pussy"), ContactMode::Depth),
                ZoneEntry::new(ZoneId::new(shocking_vrc_core::ZoneType::Orf, "Anal"), ContactMode::Depth),
            ],
            frequency: [30, 200, 30, 200],
            intensity: [80, 80, 80, 80],
            limits: PowerLimits::new(0, 30),
            aggregation: AggregationMode::Max,
        },
        channel_b: ChannelConfig {
            zones: vec![ZoneEntry::new(
                ZoneId::new(shocking_vrc_core::ZoneType::Pen, "Cock"),
                ContactMode::Depth,
            )],
            frequency: [60, 120, 60, 120],
            intensity: [80, 80, 80, 80],
            limits: PowerLimits::new(0, 30),
            aggregation: AggregationMode::Max,
        },
        ..CliConfig::default()
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
  add-a  <type> <name> [mode] Add exact zone to channel A (mode: depth|speed|acc|recoil, default depth)
  add-a  Orf * [mode]         Add ALL Orf zones to channel A (wildcard)
  add-a  * * [mode]           Add every avatar zone to channel A
  add-b  <type> <name | *> [mode]  Same for channel B
  mode-a <type> <name | *> <mode>  Change mode of an existing entry on A
  mode-b <type> <name | *> <mode>  Change mode of an existing entry on B
  add-all-a [type]            Add all avatar zones currently seen to A
  add-all-b [type]            Add all avatar zones currently seen to B
  rm-a   <type> <name | *>      Remove zone/pattern from channel A
  rm-b   <type> <name | *>      Remove zone/pattern from channel B

Modes  (all derivative modes):
  depth   — current contact level (raw)
  speed   — |dlevel/dt|, normalised
  acc     — |d²level/dt²|, normalised
  recoil  — |jerk| = |d³level/dt³|, normalised (sudden motion changes)

UKF tuning  (per-contact Unscented Kalman Filter — shared by every contact)
  ukf                                 Show current Q/R/alpha/beta/kappa
  ukf <q> <r>                         Set process / measurement noise (q,r > 0)
  ukf <q> <r> <alpha> <beta> <kappa>  Full tuning (alpha~0.5, beta=2, kappa=0)
  ukf reset                           Restore default tuning

Motion normalisation (divisors that map raw derivatives → 0..1; smaller = more sensitive)
  norms                               Show current speed / acc / recoil divisors
  norms <speed> <acc> <recoil>        Set all three at once (positive floats)
  norm-speed  <v>                     Set the speed divisor only
  norm-acc    <v>                     Set the acc divisor only
  norm-recoil <v>                     Set the recoil divisor only
  norms reset                         Restore defaults (speed=5, acc=30, recoil=100)

Pulse shape
  freq-a-hz <h0> <h1> <h2> <h3>  Channel A frequency in Hz (1–100) per segment
  freq-b-hz <h0> <h1> <h2> <h3>  Channel B frequency in Hz (1–100) per segment
  freq-a <v0> <v1> <v2> <v3> Channel A frequency segments (raw 10–255)
  freq-b <v0> <v1> <v2> <v3> Channel B frequency segments (raw 10–255)
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
  mon on|off                  Toggle live power-stream printout (default: on)
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
    let a_zones: Vec<_> = a.zones.iter().map(|e| e.to_string()).collect();
    let b_zones: Vec<_> = b.zones.iter().map(|e| e.to_string()).collect();
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
    let fmt_freq_hz = |f: &[u8; 4]| -> String {
        f.iter()
            .map(|&r| format!("{:>3.0}Hz", raw_to_hz(r)))
            .collect::<Vec<_>>()
            .join(" ")
    };
    println!("│  freq   : {:?}  │  freq   : {:?} │", a.frequency, b.frequency);
    println!(
        "│    (Hz) : {:<21}│    (Hz) : {:<13}│",
        fmt_freq_hz(&a.frequency),
        fmt_freq_hz(&b.frequency)
    );
    println!("│  intens : {:?}  │  intens : {:?} │", a.intensity, b.intensity);
    println!("├─────────────────────────────────┴──────────────────────────┤");
    let u = &cfg.ukf;
    println!(
        "│  UKF: q={:.4}  r={:.4}  alpha={:.2}  beta={:.2}  kappa={:.2}    │",
        u.q, u.r, u.alpha, u.beta, u.kappa
    );
    let n = &cfg.norms;
    println!(
        "│  Norms: speed={:.2}  acc={:.2}  recoil={:.2}                    │",
        n.speed, n.acc, n.recoil
    );
    println!("└────────────────────────────────────────────────────────────┘");
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

async fn print_full_status(status: &shocking_vrc_core::cli::engine::CliStatus) {
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

fn parse_hz_single(s: &str) -> Option<u8> {
    let hz: f32 = s.parse().ok()?;
    if !(1.0..=100.0).contains(&hz) {
        return None;
    }
    Some(hz_to_raw(hz))
}

fn parse_freq_4_hz(v0: &str, v1: &str, v2: &str, v3: &str) -> Option<[u8; 4]> {
    Some([
        parse_hz_single(v0)?,
        parse_hz_single(v1)?,
        parse_hz_single(v2)?,
        parse_hz_single(v3)?,
    ])
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

fn parse_on_off(s: &str) -> Option<bool> {
    match s.to_ascii_lowercase().as_str() {
        "on" | "1" | "true" | "yes" | "y" | "enable" | "enabled" => Some(true),
        "off" | "0" | "false" | "no" | "n" | "disable" | "disabled" => Some(false),
        _ => None,
    }
}
