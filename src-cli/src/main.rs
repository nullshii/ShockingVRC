use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use shocking_vrc_core::cli::{
    AggregationMode, ChannelConfig, CliConfig, ContactMode, PowerLimits, ZoneEntry, ZoneId,
};
use shocking_vrc_core::{AvatarScanner, CliEngine, CoyoteDevice, OldZoneType};

use shocking_vrc_cli::{
    app_state::AppState,
    commands::{
        add_all_zones_command::AddAllZonesCommand,
        add_zone_command::AddZoneCommand,
        aggregation_command::AggregationCommand,
        clear_command::ClearCommand,
        config_command::ConfigCommand,
        freq_command::FreqCommand,
        help_command::HelpCommand,
        intensity_command::IntensityCommand,
        limits_command::LimitsCommand,
        modulation_off_command::ModulationOffCommand,
        modulation_set_command::ModulationSetCommand,
        modulation_show_command::ModulationShowCommand,
        monitor_command::MonitorCommand,
        norms_command::NormsCommand,
        quit_command::QuitCommand,
        remove_zone_command::RemoveZoneCommand,
        status_command::StatusCommand,
        ukf_command::UkfCommand,
        zone_mode_command::ZoneModeCommand,
        zones_command::ZonesCommand,
    },
    display::{print_banner, print_config_summary, print_status_header, print_status_line},
    engine::{
        cli_engine::CliEngine as CliShellEngine,
        command_registry::CommandRegistry,
    },
};

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
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let args = parse_args();

    print_banner();

    let config = if Path::new(CONFIG_FILE).exists() {
        match load_config_from_file(CONFIG_FILE) {
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

    print_config_summary(&config, &mut std::io::stdout()).unwrap();

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
        Ok(false) => {
            println!("[osc] VRChat not found — enable OSC in Settings → OSC. Retrying on avatar change.")
        }
        Err(e) => println!("[osc] Discovery error: {e}"),
    }

    let engine = CliEngine::new(config);
    let status_rx = engine.subscribe_status();
    let _stop_handle = engine.start(&scanner).await;

    let monitor_enabled = Arc::new(AtomicBool::new(true));

    println!("\n[cli] Engine started. Type 'help' for commands, 'quit' to exit.");
    println!("[ble] Searching for DGLab Coyote V3 in background...\n");
    print_status_header();

    let state = Arc::new(AppState {
        engine,
        scanner,
        monitor_enabled: Arc::clone(&monitor_enabled),
    });

    {
        let state_ble = Arc::clone(&state);
        let scan_timeout = args.scan_timeout;
        tokio::spawn(async move {
            loop {
                log::debug!("[ble] Starting BLE scan ({}s)...", scan_timeout);
                match CoyoteDevice::scan_first_with_timeout(Duration::from_secs(scan_timeout)).await {
                    Ok(Some(mut dev)) => {
                        println!("\n[ble] Connected: {} ({})", dev.name(), dev.mac_address());
                        dev.start();
                        let dev = Arc::new(dev);
                        state_ble.engine.connect_device(Arc::clone(&dev)).await;

                        loop {
                            tokio::time::sleep(Duration::from_secs(2)).await;
                            if !dev.is_connected().await {
                                println!("\n[ble] Device disconnected — rescanning...");
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

    {
        let monitor_enabled_bg = Arc::clone(&monitor_enabled);
        tokio::spawn(async move {
            let mut rx = status_rx;
            loop {
                match rx.recv().await {
                    Ok(status) => {
                        if !monitor_enabled_bg.load(Ordering::Relaxed) {
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

    let registry = CommandRegistry::new()
        .add_command(Box::new(HelpCommand))
        .add_command(Box::new(ClearCommand))
        .add_command(Box::new(QuitCommand))
        .add_command(Box::new(AddZoneCommand))
        .add_command(Box::new(RemoveZoneCommand))
        .add_command(Box::new(ZoneModeCommand))
        .add_command(Box::new(AddAllZonesCommand))
        .add_command(Box::new(FreqCommand))
        .add_command(Box::new(IntensityCommand))
        .add_command(Box::new(LimitsCommand))
        .add_command(Box::new(AggregationCommand))
        .add_command(Box::new(UkfCommand))
        .add_command(Box::new(NormsCommand))
        .add_command(Box::new(ModulationSetCommand))
        .add_command(Box::new(ModulationShowCommand))
        .add_command(Box::new(ModulationOffCommand))
        .add_command(Box::new(ZonesCommand))
        .add_command(Box::new(StatusCommand))
        .add_command(Box::new(ConfigCommand))
        .add_command(Box::new(MonitorCommand))
        .build();

    let shell = CliShellEngine::new(registry, Arc::clone(&state));
    if let Err(e) = shell.run().await {
        log::error!("CLI error: {:?}", e);
    }

    state.engine.disconnect_device().await;
    tokio::time::sleep(Duration::from_millis(150)).await;
    println!("[cli] Stopped. Goodbye.");
}
