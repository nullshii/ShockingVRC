use std::path::Path;
use std::sync::Arc;
use std::sync::RwLock;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use shocking_vrc_core::cli::{
    AggregationMode, ChannelConfig, CliConfig, ContactMode, PowerLimits, ZoneEntry, ZoneId,
};
use shocking_vrc_core::{AvatarScanner, CliEngine, CoyoteDevice, OldZoneType};

use shocking_vrc_cli::app_state::{AppState, DeviceSlot};
use shocking_vrc_cli::device_config;
use shocking_vrc_cli::{tui, tui_logger};

const CONFIG_FILE: &str = "cli_config.json";

struct Args {
    port: u16,
    scan_timeout: u64,
    skip_update_check: bool,
}

fn parse_args() -> Args {
    let args: Vec<String> = std::env::args().collect();
    let mut port = 9001u16;
    let mut scan_timeout = 8u64;
    let mut skip_update_check = false;
    let mut i = 1;

    while i < args.len() {
        match args[i].as_str() {
            "--help" | "-h" => {
                println!("Usage: shockingvrc-cli [--port <n>] [--scan-timeout <secs>] [--skip-update-check]");
                println!("  --port               UDP OSC port (default: 9001)");
                println!("  --scan-timeout       BLE scan timeout in seconds (default: 8)");
                println!("  --skip-update-check  Do not check GitHub for updates on startup");
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
            "--skip-update-check" => {
                skip_update_check = true;
            }
            _ => {}
        }
        i += 1;
    }
    Args {
        port,
        scan_timeout,
        skip_update_check,
    }
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

async fn attach_device_to_slot(state: &Arc<AppState>, mac: &str, mut dev: CoyoteDevice) -> Arc<CoyoteDevice> {
    dev.start();
    let arc = Arc::new(dev);
    if let Some(engine) = state.engine_for_mac(mac) {
        engine.connect_device(Arc::clone(&arc)).await;
    }

    tokio::time::sleep(Duration::from_millis(500)).await;
    if let Ok(Some(lvl)) = arc.battery_level().await {
        state.update_slot(mac, |s| s.battery = Some(lvl));
    }

    state.update_slot(mac, |s| {
        s.connected = true;
        s.reconnecting = false;
    });
    arc
}

async fn add_device_slot(state: &Arc<AppState>, mut dev: CoyoteDevice, scanner: &AvatarScanner) {
    let mac = dev.mac_address().to_uppercase();
    if state.find_slot_index(&mac).is_some() {
        let _ = dev.disconnect().await;
        return;
    }

    let name = dev.name().to_string();
    let cfg = device_config::load_device_config(&mac, &state.default_config);
    let engine = CliEngine::new(cfg);
    let stop_handle = engine.start(scanner).await;

    dev.start();
    let arc_dev = Arc::new(dev);
    engine.connect_device(Arc::clone(&arc_dev)).await;

    let mut battery = None;
    tokio::time::sleep(Duration::from_millis(500)).await;
    if let Ok(Some(lvl)) = arc_dev.battery_level().await {
        battery = Some(lvl);
    }

    let slot = DeviceSlot {
        mac: mac.clone(),
        name,
        engine,
        stop_handle,
        battery,
        connected: true,
        reconnecting: false,
    };

    if let Ok(mut slots) = state.device_slots.write() {
        slots.push(slot);
    }

    log::info!("[ble] Device slot created for {mac}");
    spawn_device_monitor(Arc::clone(state), mac, arc_dev);
}

async fn reconnect_to_slot(state: &Arc<AppState>, mac: String, dev: CoyoteDevice) {
    log::info!("[ble] Re-attaching device {mac}");
    let arc = attach_device_to_slot(state, &mac, dev).await;
    spawn_device_monitor(Arc::clone(state), mac.clone(), arc);
}

async fn handle_scanned_device(state: &Arc<AppState>, dev: CoyoteDevice, scanner: &AvatarScanner) {
    let mac = dev.mac_address().to_uppercase();
    if state.find_slot_index(&mac).is_some() {
        let needs_reconnect = state
            .device_slots
            .read()
            .ok()
            .and_then(|slots| {
                slots
                    .iter()
                    .find(|s| s.mac.eq_ignore_ascii_case(&mac))
                    .map(|s| !s.connected || s.reconnecting)
            })
            .unwrap_or(false);
        if needs_reconnect {
            reconnect_to_slot(state, mac, dev).await;
        } else {
            log::debug!("[ble] Ignoring duplicate scan result for {mac}");
            let _ = dev.disconnect().await;
        }
    } else {
        add_device_slot(state, dev, scanner).await;
    }
}

async fn try_reconnect_pending(state: &Arc<AppState>) {
    let pending: Vec<String> = state
        .device_slots
        .read()
        .ok()
        .map(|slots| {
            slots
                .iter()
                .filter(|s| s.reconnecting || !s.connected)
                .map(|s| s.mac.clone())
                .collect()
        })
        .unwrap_or_default();

    for mac in pending {
        log::debug!("[ble] Targeted reconnect attempt for {mac}");
        match tokio::time::timeout(
            Duration::from_secs(25),
            CoyoteDevice::connect_by_mac(&mac, Duration::from_secs(12)),
        )
        .await
        {
            Ok(Ok(Some(dev))) => reconnect_to_slot(state, mac, dev).await,
            Ok(Ok(None)) => log::debug!("[ble] {mac} not found during targeted reconnect"),
            Ok(Err(e)) => log::warn!("[ble] Targeted reconnect error for {mac}: {e}"),
            Err(_) => log::warn!("[ble] Targeted reconnect timed out for {mac}"),
        }
    }
}

fn spawn_device_monitor(state: Arc<AppState>, mac: String, dev: Arc<CoyoteDevice>) {
    tokio::spawn(async move {
        let mut last_battery = Instant::now();
        loop {
            tokio::time::sleep(Duration::from_secs(2)).await;

            if dev.is_connected().await {
                if last_battery.elapsed() >= Duration::from_secs(30) {
                    if let Ok(Some(lvl)) = dev.battery_level().await {
                        state.update_slot(&mac, |s| s.battery = Some(lvl));
                    }
                    last_battery = Instant::now();
                }
                continue;
            }

            log::warn!("[ble] Device {mac} disconnected — coordinator will reconnect");
            state.update_slot(&mac, |s| {
                s.connected = false;
                s.reconnecting = true;
            });

            if let Some(engine) = state.engine_for_mac(&mac) {
                engine.disconnect_device().await;
            }
            return;
        }
    });
}

fn spawn_ble_coordinator(state: Arc<AppState>, scanner: AvatarScanner, scan_timeout: u64) {
    tokio::spawn(async move {
        loop {
            try_reconnect_pending(&state).await;

            log::debug!("[ble] Scanning for devices ({}s)...", scan_timeout);
            match CoyoteDevice::scan_all_with_timeout(Duration::from_secs(scan_timeout)).await {
                Ok(devices) => {
                    for dev in devices {
                        handle_scanned_device(&state, dev, &scanner).await;
                    }
                }
                Err(e) => log::warn!("[ble] Scan error: {e}"),
            }

            let interval = if state
                .slot_infos()
                .iter()
                .any(|s| s.reconnecting || !s.connected)
            {
                Duration::from_secs(5)
            } else {
                Duration::from_secs(15)
            };
            tokio::time::sleep(interval).await;
        }
    });
}

#[tokio::main]
async fn main() {
    shocking_vrc_core::update::cleanup_staging_files();

    let log_buffer = tui_logger::init("info");

    let args = parse_args();

    log::info!("ShockingVRC CLI v{} — Hello DG-Lab Users", shocking_vrc_core::VERSION);

    let default_config = if Path::new(CONFIG_FILE).exists() {
        match load_config_from_file(CONFIG_FILE) {
            Ok(c) => {
                log::info!("[config] Loaded default from {CONFIG_FILE}");
                c
            }
            Err(e) => {
                log::warn!("[config] Failed to load {CONFIG_FILE}: {e} — using defaults");
                default_config()
            }
        }
    } else {
        log::info!("[config] No {CONFIG_FILE} found — using defaults");
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

    let monitor_enabled = Arc::new(AtomicBool::new(true));

    let state = Arc::new(AppState {
        scanner: scanner.clone(),
        monitor_enabled: Arc::clone(&monitor_enabled),
        default_config,
        device_slots: Arc::new(RwLock::new(Vec::new())),
    });

    log::info!("[cli] Ready.");
    log::info!("[ble] Searching for DGLab Coyote V3 in background...");

    spawn_ble_coordinator(Arc::clone(&state), scanner.clone(), args.scan_timeout);

    let pending_update = match tui::run(
        Arc::clone(&state),
        log_buffer,
        args.skip_update_check,
    )
    .await
    {
        Ok(pending) => pending,
        Err(e) => {
            eprintln!("[cli] TUI error: {e}");
            None
        }
    };

    state.disconnect_all();
    state.scanner.stop().await;
    tokio::time::sleep(Duration::from_millis(600)).await;

    if let Some((downloaded, restart_args)) = pending_update {
        match shocking_vrc_core::update::apply_self_update(&downloaded, &restart_args) {
            Ok(()) => {
                let _ = std::fs::remove_file(&downloaded);
                return;
            }
            Err(e) => eprintln!("[update] install failed: {e}"),
        }
    }

    tokio::time::sleep(Duration::from_millis(150)).await;
    println!("[cli] Stopped. Goodbye.");
}
