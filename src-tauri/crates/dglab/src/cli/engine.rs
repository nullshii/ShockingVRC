use std::collections::HashMap;
use std::sync::Arc;

use log::{debug, info, warn};
use tokio::sync::{Mutex, RwLock, broadcast, watch};

use crate::ble::device::CoyoteDevice;
use crate::osc::scanner::AvatarScanner;
use crate::osc::types::ZoneEvent;
use crate::protocol::waveform::WaveformV3;
use crate::protocol::waveform_bf::WaveformBF;

use super::config::{ChannelConfig, CliConfig, PowerLimits, ZoneId};

/// Per-channel runtime status snapshot.
#[derive(Debug, Clone)]
pub struct ChannelStatus {
    pub raw_level: f32,
    pub strength: u8,
    pub active_zones: Vec<(ZoneId, f32)>,
}

/// Snapshot of both channels' runtime state.
#[derive(Debug, Clone)]
pub struct CliStatus {
    pub channel_a: ChannelStatus,
    pub channel_b: ChannelStatus,
    pub device_connected: bool,
}

struct EngineState {
    config: RwLock<CliConfig>,
    zone_levels: RwLock<HashMap<ZoneId, f32>>,
    status_tx: broadcast::Sender<CliStatus>,
    device: Mutex<Option<Arc<CoyoteDevice>>>,
}

#[derive(Clone)]
pub struct CliEngine {
    state: Arc<EngineState>,
}

impl CliEngine {
    pub fn new(config: CliConfig) -> Self {
        let (status_tx, _) = broadcast::channel(64);
        Self {
            state: Arc::new(EngineState {
                config: RwLock::new(config),
                zone_levels: RwLock::new(HashMap::new()),
                status_tx,
                device: Mutex::new(None),
            }),
        }
    }

    /// Subscribe to real-time status updates (emitted on every zone event).
    pub fn subscribe_status(&self) -> broadcast::Receiver<CliStatus> {
        self.state.status_tx.subscribe()
    }

    pub async fn config(&self) -> CliConfig {
        self.state.config.read().await.clone()
    }

    pub async fn set_config(&self, config: CliConfig) {
        *self.state.config.write().await = config;
    }

    pub async fn add_zone_a(&self, zone: ZoneId) {
        let mut cfg = self.state.config.write().await;
        if !cfg.channel_a.zones.contains(&zone) {
            cfg.channel_a.zones.push(zone);
        }
    }

    pub async fn remove_zone_a(&self, zone: &ZoneId) {
        self.state.config.write().await.channel_a.zones.retain(|z| z != zone);
    }

    pub async fn set_frequency_a(&self, freq: [u8; 4]) {
        self.state.config.write().await.channel_a.frequency = freq;
    }

    pub async fn set_intensity_a(&self, intensity: [u8; 4]) {
        self.state.config.write().await.channel_a.intensity = intensity;
    }

    pub async fn set_limits_a(&self, limits: PowerLimits) {
        self.state.config.write().await.channel_a.limits = limits;
        self.try_sync_hardware_limits().await;
    }

    pub async fn add_zone_b(&self, zone: ZoneId) {
        let mut cfg = self.state.config.write().await;
        if !cfg.channel_b.zones.contains(&zone) {
            cfg.channel_b.zones.push(zone);
        }
    }

    pub async fn remove_zone_b(&self, zone: &ZoneId) {
        self.state.config.write().await.channel_b.zones.retain(|z| z != zone);
    }

    pub async fn set_frequency_b(&self, freq: [u8; 4]) {
        self.state.config.write().await.channel_b.frequency = freq;
    }

    pub async fn set_intensity_b(&self, intensity: [u8; 4]) {
        self.state.config.write().await.channel_b.intensity = intensity;
    }

    pub async fn set_limits_b(&self, limits: PowerLimits) {
        self.state.config.write().await.channel_b.limits = limits;
        self.try_sync_hardware_limits().await;
    }

    pub async fn current_status(&self) -> CliStatus {
        let cfg = self.state.config.read().await;
        let levels = self.state.zone_levels.read().await;
        let connected = self.state.device.lock().await.is_some();
        compute_status(&cfg, &levels, connected)
    }

    pub async fn is_device_connected(&self) -> bool {
        self.state.device.lock().await.is_some()
    }

    pub async fn connect_device(&self, device: Arc<CoyoteDevice>) {
        let cfg = self.state.config.read().await;
        let bf = WaveformBF::new(cfg.channel_a.limits.max, cfg.channel_b.limits.max, 0, 0, 0, 0);
        drop(cfg);

        match device.set_wave_bf(&bf).await {
            Ok(_) => info!("[cli] BF limits sent to device"),
            Err(e) => warn!("[cli] Failed to send BF limits: {e}"),
        }

        *self.state.device.lock().await = Some(device);
        info!("[cli] Device attached");
    }

    pub async fn disconnect_device(&self) {
        let mut guard = self.state.device.lock().await;
        if let Some(dev) = guard.take() {
            *dev.wave_now().lock().await = WaveformV3::default();
        }
        info!("[cli] Device detached");
    }

    /// Immediately push current power limits to the connected device (if any).
    pub async fn sync_hardware_limits(&self) {
        self.try_sync_hardware_limits().await;
    }

    async fn try_sync_hardware_limits(&self) {
        let device_guard = self.state.device.lock().await;
        if let Some(dev) = device_guard.as_ref() {
            let cfg = self.state.config.read().await;
            let bf = WaveformBF::new(cfg.channel_a.limits.max, cfg.channel_b.limits.max, 0, 0, 0, 0);
            drop(cfg);
            match dev.set_wave_bf(&bf).await {
                Ok(_) => info!("[cli] BF limits synced to device"),
                Err(e) => warn!("[cli] Failed to sync BF limits: {e}"),
            }
        }
    }

    pub async fn start(&self, scanner: &AvatarScanner) -> CliStopHandle {
        let (stop_tx, stop_rx) = watch::channel(false);
        let engine = self.clone();
        let mut zone_rx = scanner.subscribe();

        tokio::spawn(async move {
            info!("[cli] Engine started (waiting for device)");
            let mut stop_rx = stop_rx;

            loop {
                tokio::select! {
                    result = zone_rx.recv() => {
                        match result {
                            Ok(event) => {
                                engine.process_event(&event).await;
                                let status = engine.build_and_push_wave().await;
                                let _ = engine.state.status_tx.send(status);
                            }
                            Err(broadcast::error::RecvError::Lagged(n)) => {
                                debug!("[cli] Skipped {n} zone events (lagged)");
                            }
                            Err(broadcast::error::RecvError::Closed) => {
                                info!("[cli] Zone event channel closed, shutting down");
                                break;
                            }
                        }
                    }
                    _ = stop_rx.changed() => {
                        if *stop_rx.borrow() { break; }
                    }
                }
            }

            // On stop: send idle wave to device if still connected
            engine.disconnect_device().await;
            info!("[cli] Engine stopped");
        });

        CliStopHandle { stop_tx }
    }

    async fn process_event(&self, event: &ZoneEvent) {
        let zone_id = ZoneId::from_event(event);
        self.state.zone_levels.write().await.insert(zone_id, event.level);
    }

    async fn build_and_push_wave(&self) -> CliStatus {
        let cfg = self.state.config.read().await;
        let levels = self.state.zone_levels.read().await;

        let wave_arc = {
            let device_guard = self.state.device.lock().await;
            device_guard.as_ref().map(|d| d.wave_now())
        };

        let connected = wave_arc.is_some();
        let status = compute_status(&cfg, &levels, connected);

        if let Some(wave_now) = wave_arc {
            let wave = build_wave(&status, &cfg);
            *wave_now.lock().await = wave;
        }

        status
    }
}

fn build_wave(status: &CliStatus, cfg: &CliConfig) -> WaveformV3 {
    let sa = status.channel_a.strength;
    let sb = status.channel_b.strength;

    if sa == 0 && sb == 0 {
        // No active contacts — idle, don't disturb device's internal state.
        WaveformV3::default()
    } else {
        WaveformV3::new(
            sa,
            sb,
            cfg.channel_a.frequency,
            cfg.channel_a.intensity,
            cfg.channel_b.frequency,
            cfg.channel_b.intensity,
        )
    }
}

fn compute_channel_status(channel: &ChannelConfig, all_levels: &HashMap<ZoneId, f32>) -> ChannelStatus {
    let mut active_zones: Vec<(ZoneId, f32)> = Vec::new();
    let mut zone_levels: Vec<f32> = Vec::new();
    let mut seen: std::collections::HashSet<ZoneId> = std::collections::HashSet::new();

    for pattern in &channel.zones {
        if pattern.is_wildcard() {
            for (known_id, &level) in all_levels {
                if pattern.matches(known_id) && seen.insert(known_id.clone()) {
                    zone_levels.push(level);
                    if level > 0.0 {
                        active_zones.push((known_id.clone(), level));
                    }
                }
            }
        } else if seen.insert(pattern.clone()) {
            let level = all_levels.get(pattern).copied().unwrap_or(0.0);
            zone_levels.push(level);
            if level > 0.0 {
                active_zones.push((pattern.clone(), level));
            }
        }
    }

    let raw_level = channel.aggregate(&zone_levels);
    let strength = channel.limits.scale(raw_level);
    ChannelStatus {
        raw_level,
        strength,
        active_zones,
    }
}

fn compute_status(cfg: &CliConfig, levels: &HashMap<ZoneId, f32>, device_connected: bool) -> CliStatus {
    CliStatus {
        channel_a: compute_channel_status(&cfg.channel_a, levels),
        channel_b: compute_channel_status(&cfg.channel_b, levels),
        device_connected,
    }
}

pub struct CliStopHandle {
    stop_tx: watch::Sender<bool>,
}

impl CliStopHandle {
    pub fn stop(&self) {
        let _ = self.stop_tx.send(true);
    }
}

impl Drop for CliStopHandle {
    fn drop(&mut self) {
        let _ = self.stop_tx.send(true);
    }
}
