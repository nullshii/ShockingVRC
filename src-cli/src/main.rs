use std::path::Path;
use std::sync::Arc;
use std::sync::RwLock;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use shocking_vrc_core::cli::{
    AggregationMode, ChannelConfig, CliConfig, ContactMode, PowerLimits, ZoneEntry, ZoneId,
};
use shocking_vrc_core::{AvatarScanner, CliEngine, CoyoteDevice, OldZoneType};

use shocking_vrc_cli::{app_state::AppState, tui, tui_logger};

const CONFIG_FILE: &str = "cli_config.json";

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
                println!("Usage: shockingvrc-cli [--port <n>] [--scan-timeout <secs>]");
                println!("  --port          UDP OSC port (default: 9001)");
                println!("  --scan-timeout  BLE scan timeout in seconds (default: 8)");
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

fn default_config() -> CliConfig {
    CliConfig {
        channel_a: ChannelConfig {
            zones: vec![
                ZoneEntry::new(ZoneId::new(OldZoneType::Orf, "Pussy"), ContactMode::Depth),
                ZoneEntry::new(ZoneId::new(OldZoneType::DGB, "TouchAreaA"), ContactMode::Depth),
            ],
            frequency: [30, 200, 30, 200],
            intensity: [100, 100, 100, 100],
            limits: PowerLimits::new(0, 30),
            aggregation: AggregationMode::Max,
            ..ChannelConfig::default()
        },
        channel_b: ChannelConfig {
            zones: vec![ZoneEntry::new(
                ZoneId::new(OldZoneType::Pen, "Cock"),
                ContactMode::Depth,
            )],
            frequency: [30, 220, 60, 140],
            intensity: [100, 100, 100, 100],
            limits: PowerLimits::new(0, 30),
            aggregation: AggregationMode::Max,
            ..ChannelConfig::default()
        },
        ..CliConfig::default()
    }
}

fn load_config_from_file(path: &str) -> Result<CliConfig, Box<dyn std::error::Error>> {
    let json = std::fs::read_to_string(path)?;
    Ok(serde_json::from_str(&json)?)
}

#[tokio::main]
async fn main() {
    let log_buffer = tui_logger::init("info");

    let args = parse_args();

    log::info!("ShockingVRC CLI — Two-Channel OSC Controller (TUI)");

    let config = if Path::new(CONFIG_FILE).exists() {
        match load_config_from_file(CONFIG_FILE) {
            Ok(c) => {
                log::info!("[config] Loaded from {CONFIG_FILE}");
                c
            }
            Err(e) => {
                log::warn!("[config] Failed to load {CONFIG_FILE}: {e} — using defaults");
                default_config()
            }
        }
    } else {
        log::info!("[config] No {CONFIG_FILE} found — using defaults (Channels ▸ Save to create)");
        default_config()
    };

    log::info!("[osc] Starting OSC listener on UDP port {}...", args.port);
    let scanner = AvatarScanner::new(args.port);
    scanner.start().await.expect("Failed to start OSC listener");

    log::info!("[osc] Scanning for VRChat...");
    match scanner.discover_wait().await {
        Ok(true) => {
            if let Some(addr) = scanner.vrchat_address().await {
                log::info!(
                    "[osc] VRChat found → {} (OSC {}:{})",
                    addr.http_addr,
                    addr.osc_ip,
                    addr.osc_port
                );
            }
            let zones = scanner.zones().await;
            log::info!("[osc] Avatar zones found: {}", zones.len());
            for z in &zones {
                log::info!("      [{:<5}] {}", z.zone_type, z.id);
            }
        }
        Ok(false) => {
            log::warn!(
                "[osc] VRChat not found — enable OSC in Settings → OSC. Retrying on avatar change."
            )
        }
        Err(e) => log::error!("[osc] Discovery error: {e}"),
    }

    let engine = CliEngine::new(config);
    let status_rx = engine.subscribe_status();
    let _stop_handle = engine.start(&scanner).await;

    let monitor_enabled = Arc::new(AtomicBool::new(true));
    let battery_level = Arc::new(RwLock::new(None));

    log::info!("[cli] Engine started.");
    log::info!("[ble] Searching for DGLab Coyote V3 in background...");

    let state = Arc::new(AppState {
        engine,
        scanner,
        monitor_enabled: Arc::clone(&monitor_enabled),
        battery_level: Arc::clone(&battery_level),
    });

    {
        let state_ble = Arc::clone(&state);
        let scan_timeout = args.scan_timeout;
        tokio::spawn(async move {
            loop {
                log::debug!("[ble] Starting BLE scan ({}s)...", scan_timeout);
                match CoyoteDevice::scan_first_with_timeout(Duration::from_secs(scan_timeout)).await
                {
                    Ok(Some(mut dev)) => {
                        log::info!("[ble] Connected: {} ({})", dev.name(), dev.mac_address());
                        dev.start();
                        let dev = Arc::new(dev);
                        state_ble.engine.connect_device(Arc::clone(&dev)).await;

                        tokio::time::sleep(Duration::from_millis(500)).await;
                        match dev.battery_level().await {
                            Ok(Some(lvl)) => {
                                if let Ok(mut g) = state_ble.battery_level.write() {
                                    *g = Some(lvl);
                                }
                            }
                            Ok(None) => {}
                            Err(e) => log::debug!("[ble] Battery read failed: {e}"),
                        }

                        let mut last_battery = Instant::now();
                        loop {
                            if last_battery.elapsed() >= Duration::from_secs(30) {
                                match dev.battery_level().await {
                                    Ok(Some(lvl)) => {
                                        if let Ok(mut g) = state_ble.battery_level.write() {
                                            *g = Some(lvl);
                                        }
                                    }
                                    Ok(None) => {}
                                    Err(e) => log::debug!("[ble] Battery read failed: {e}"),
                                }
                                last_battery = Instant::now();
                            }

                            tokio::time::sleep(Duration::from_secs(2)).await;
                            if !dev.is_connected().await {
                                log::warn!("[ble] Device disconnected — rescanning...");
                                if let Ok(mut g) = state_ble.battery_level.write() {
                                    *g = None;
                                }
                                state_ble.engine.disconnect_device().await;
                                break;
                            }
                        }
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


    if let Err(e) = tui::run(Arc::clone(&state), status_rx, log_buffer).await {
        eprintln!("[cli] TUI error: {e}");
    }

    state.engine.disconnect_device().await;
    tokio::time::sleep(Duration::from_millis(150)).await;
    println!("[cli] Stopped. Goodbye.");
}
