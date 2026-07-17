use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use log::{debug, info, warn};
use tokio::sync::{Mutex, RwLock, broadcast, watch};

use crate::ble::device::CoyoteDevice;
use crate::modulation::evaluator::{KinematicsInput, advance_and_evaluate_segments};
use crate::osc::scanner::AvatarScanner;
use crate::osc::types::ZoneEvent;
use crate::protocol::waveform::WaveformV3;
use crate::protocol::waveform_bf::WaveformBF;

use super::config::{
    ChannelConfig, CliConfig, ContactMode, MotionNorms, PowerLimits, UkfConfig, ZoneEntry, ZoneId,
    apply_zone_activation_range,
};

const ZONE_IDLE_TIMEOUT: Duration = Duration::from_millis(100);
const WATCHDOG_INTERVAL: Duration = Duration::from_millis(50);

/// Per-channel runtime status snapshot.
#[derive(Debug, Clone)]
pub struct ChannelStatus {
    pub raw_level: f32,
    pub strength: u8,
    pub frequency: [u8; 4],
    pub active_zones: Vec<(ZoneId, f32)>,
    pub kinematics: KinematicsInput,
}

/// Snapshot of both channels' runtime state.
#[derive(Debug, Clone)]
pub struct CliStatus {
    pub channel_a: ChannelStatus,
    pub channel_b: ChannelStatus,
    pub device_connected: bool,
}

#[derive(Debug, Clone, Copy)]
struct ZoneKinematics {
    level: f32,
    velocity: f32,
    acceleration: f32,
    recoil: f32,
    last_update: Instant,
}

impl ZoneKinematics {
    fn is_stale(&self, now: Instant) -> bool {
        now.saturating_duration_since(self.last_update) > ZONE_IDLE_TIMEOUT
    }
}

#[derive(Debug, Default)]
struct ModAccumulators {
    freq_a: [f32; 4],
    freq_b: [f32; 4],
    int_a:  [f32; 4],
    int_b:  [f32; 4],
}

struct EngineState {
    config: RwLock<CliConfig>,
    zone_kinematics: RwLock<HashMap<ZoneId, ZoneKinematics>>,
    status_tx: broadcast::Sender<CliStatus>,
    device: Mutex<Option<Arc<CoyoteDevice>>>,
    scanner: RwLock<Option<AvatarScanner>>,
    mod_accums: Mutex<ModAccumulators>,
    last_wave_tick: Mutex<Instant>,
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
                zone_kinematics: RwLock::new(HashMap::new()),
                status_tx,
                device: Mutex::new(None),
                scanner: RwLock::new(None),
                mod_accums: Mutex::new(ModAccumulators::default()),
                last_wave_tick: Mutex::new(Instant::now()),
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
        let ukf = config.ukf;
        *self.state.config.write().await = config;
        self.push_ukf_to_scanner(ukf).await;
    }

    pub async fn set_ukf_params(&self, ukf: UkfConfig) {
        self.state.config.write().await.ukf = ukf;
        self.push_ukf_to_scanner(ukf).await;
    }

    pub async fn ukf_params(&self) -> UkfConfig {
        self.state.config.read().await.ukf
    }

    pub async fn set_norms(&self, norms: MotionNorms) {
        self.state.config.write().await.norms = norms.sanitised();
    }

    pub async fn norms(&self) -> MotionNorms {
        self.state.config.read().await.norms
    }

    async fn push_ukf_to_scanner(&self, ukf: UkfConfig) {
        let scanner_clone = self.state.scanner.read().await.clone();
        if let Some(scanner) = scanner_clone {
            scanner.set_ukf_params(ukf.into()).await;
        }
    }

    pub async fn add_zone_a(&self, zone: ZoneId) {
        self.add_zone_entry_a(ZoneEntry::with_default_mode(zone)).await;
    }

    pub async fn add_zone_entry_a(&self, entry: ZoneEntry) {
        let mut cfg = self.state.config.write().await;
        if let Some(existing) = cfg.channel_a.zones.iter_mut().find(|e| e.id == entry.id) {
            existing.mode = entry.mode;
        } else {
            cfg.channel_a.zones.push(entry);
        }
    }

    pub async fn remove_zone_a(&self, zone: &ZoneId) -> bool {
        let mut cfg = self.state.config.write().await;
        let before = cfg.channel_a.zones.len();
        cfg.channel_a.zones.retain(|e| &e.id != zone);
        cfg.channel_a.zones.len() < before
    }

    pub async fn set_zone_mode_a(&self, zone: &ZoneId, mode: ContactMode) -> bool {
        let mut cfg = self.state.config.write().await;
        if let Some(entry) = cfg.channel_a.zones.iter_mut().find(|e| &e.id == zone) {
            entry.mode = mode;
            true
        } else {
            false
        }
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
        self.add_zone_entry_b(ZoneEntry::with_default_mode(zone)).await;
    }

    pub async fn add_zone_entry_b(&self, entry: ZoneEntry) {
        let mut cfg = self.state.config.write().await;
        if let Some(existing) = cfg.channel_b.zones.iter_mut().find(|e| e.id == entry.id) {
            existing.mode = entry.mode;
        } else {
            cfg.channel_b.zones.push(entry);
        }
    }

    pub async fn remove_zone_b(&self, zone: &ZoneId) -> bool {
        let mut cfg = self.state.config.write().await;
        let before = cfg.channel_b.zones.len();
        cfg.channel_b.zones.retain(|e| &e.id != zone);
        cfg.channel_b.zones.len() < before
    }

    pub async fn set_zone_mode_b(&self, zone: &ZoneId, mode: ContactMode) -> bool {
        let mut cfg = self.state.config.write().await;
        if let Some(entry) = cfg.channel_b.zones.iter_mut().find(|e| &e.id == zone) {
            entry.mode = mode;
            true
        } else {
            false
        }
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
        let kinematics = self.state.zone_kinematics.read().await;
        let connected = self.state.device.lock().await.is_some();
        compute_status(&cfg, &kinematics, Instant::now(), connected)
    }

    pub async fn is_device_connected(&self) -> bool {
        self.state.device.lock().await.is_some()
    }

    pub async fn connect_device(&self, device: Arc<CoyoteDevice>) {
        let cfg = self.state.config.read().await;
        let bf = WaveformBF::new(cfg.channel_a.limits.max, cfg.channel_b.limits.max, 0, 0, 0, 0);
        drop(cfg);

        match device.set_wave_bf(&bf).await {
            Ok(_) => info!("[cli] BF limits sent to device {}", device.mac_address()),
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
        *self.state.scanner.write().await = Some(scanner.clone());
        let initial_ukf = self.state.config.read().await.ukf;
        scanner.set_ukf_params(initial_ukf.into()).await;

        tokio::spawn(async move {
            info!("[cli] Engine started (waiting for device)");
            let mut stop_rx = stop_rx;
            let mut watchdog = tokio::time::interval(WATCHDOG_INTERVAL);
            watchdog.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

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
                    _ = watchdog.tick() => {
                        let status = engine.build_and_push_wave().await;
                        let _ = engine.state.status_tx.send(status);
                    }
                    _ = stop_rx.changed() => {
                        if *stop_rx.borrow() { break; }
                    }
                }
            }

            engine.disconnect_device().await;
            info!("[cli] Engine stopped");
        });

        CliStopHandle { stop_tx }
    }

    async fn process_event(&self, event: &ZoneEvent) {
        let zone_id = ZoneId::from_event(event);
        self.state.zone_kinematics.write().await.insert(
            zone_id,
            ZoneKinematics {
                level: event.level,
                velocity: event.velocity,
                acceleration: event.acceleration,
                recoil: event.recoil,
                last_update: Instant::now(),
            },
        );
    }

    async fn build_and_push_wave(&self) -> CliStatus {
        let cfg = self.state.config.read().await;
        let kinematics = self.state.zone_kinematics.read().await;

        let wave_arc = {
            let device_guard = self.state.device.lock().await;
            device_guard.as_ref().map(|d| d.wave_now())
        };

        let connected = wave_arc.is_some();
        let now = Instant::now();

        let dt = {
            let mut last = self.state.last_wave_tick.lock().await;
            let elapsed = now.saturating_duration_since(*last).as_secs_f32();
            *last = now;
            elapsed.min(0.5)
        };

        let mut status = compute_status(&cfg, &kinematics, now, connected);

        let mut accums = self.state.mod_accums.lock().await;
        let ModAccumulators {
            ref mut freq_a,
            ref mut int_a,
            ref mut freq_b,
            ref mut int_b,
        } = *accums;
        let (freq_a_out, int_a) = modulate_channel(
            cfg.channel_a.frequency,
            cfg.channel_a.intensity,
            &cfg.channel_a,
            &status.channel_a.kinematics,
            freq_a,
            int_a,
            dt,
        );
        let (freq_b_out, int_b) = modulate_channel(
            cfg.channel_b.frequency,
            cfg.channel_b.intensity,
            &cfg.channel_b,
            &status.channel_b.kinematics,
            freq_b,
            int_b,
            dt,
        );
        status.channel_a.frequency = freq_a_out;
        status.channel_b.frequency = freq_b_out;

        if let Some(wave_now) = wave_arc {
            let wave = if status.channel_a.strength == 0 && status.channel_b.strength == 0 {
                WaveformV3::default()
            } else {
                WaveformV3::new(
                    status.channel_a.strength,
                    status.channel_b.strength,
                    freq_a_out,
                    int_a,
                    freq_b_out,
                    int_b,
                )
            };
            *wave_now.lock().await = wave;
        }

        status
    }
}

fn project_kinematics(k: &ZoneKinematics, mode: ContactMode, norms: &MotionNorms) -> f32 {
    match mode {
        ContactMode::Depth => k.level.clamp(0.0, 1.0),
        ContactMode::Speed => (k.velocity.abs() / norms.speed).clamp(0.0, 1.0),
        ContactMode::Acc => (k.acceleration.abs() / norms.acc).clamp(0.0, 1.0),
        ContactMode::Recoil => (k.recoil / norms.recoil).clamp(0.0, 1.0),
    }
}

fn modulate_channel(
    base_freq: [u8; 4],
    base_intensity: [u8; 4],
    channel: &ChannelConfig,
    kin: &KinematicsInput,
    freq_accums: &mut [f32; 4],
    int_accums: &mut [f32; 4],
    dt: f32,
) -> ([u8; 4], [u8; 4]) {
    let freq_f: [f32; 4] = base_freq.map(|v| v as f32);
    let int_f: [f32; 4] = base_intensity.map(|v| v as f32);

    let mod_freq = advance_and_evaluate_segments(&channel.freq_modulation, freq_accums, freq_f, kin, dt);
    let mod_int = advance_and_evaluate_segments(&channel.intensity_modulation, int_accums, int_f, kin, dt);

    let freq_out = [
        (mod_freq[0].round() as u8).clamp(10, 255),
        (mod_freq[1].round() as u8).clamp(10, 255),
        (mod_freq[2].round() as u8).clamp(10, 255),
        (mod_freq[3].round() as u8).clamp(10, 255),
    ];
    let int_out = [
        (mod_int[0].round() as u8).clamp(0, 100),
        (mod_int[1].round() as u8).clamp(0, 100),
        (mod_int[2].round() as u8).clamp(0, 100),
        (mod_int[3].round() as u8).clamp(0, 100),
    ];

    (freq_out, int_out)
}

fn project_with_freshness(k: &ZoneKinematics, mode: ContactMode, norms: &MotionNorms, now: Instant) -> f32 {
    if k.is_stale(now) {
        0.0
    } else {
        project_kinematics(k, mode, norms)
    }
}

fn kinematics_for_zone(k: &ZoneKinematics, norms: &MotionNorms, now: Instant) -> KinematicsInput {
    if k.is_stale(now) {
        KinematicsInput::default()
    } else {
        KinematicsInput {
            depth: k.level.clamp(0.0, 1.0),
            speed: (k.velocity.abs() / norms.speed).clamp(0.0, 1.0),
            acc: (k.acceleration.abs() / norms.acc).clamp(0.0, 1.0),
            recoil: (k.recoil / norms.recoil).clamp(0.0, 1.0),
        }
    }
}

fn compute_channel_status(
    channel: &ChannelConfig,
    kinematics: &HashMap<ZoneId, ZoneKinematics>,
    norms: &MotionNorms,
    now: Instant,
) -> ChannelStatus {
    let mut active_zones: Vec<(ZoneId, f32)> = Vec::new();
    let mut zone_levels: Vec<f32> = Vec::new();
    let mut seen: std::collections::HashSet<ZoneId> = std::collections::HashSet::new();

    let mut kin_accum = KinematicsInput::default();
    let mut kin_count = 0u32;

    for entry in &channel.zones {
        let pattern = &entry.id;
        let zone_scale = entry.scale as f32 / 100.0;

        if pattern.is_wildcard() {
            for (known_id, k) in kinematics {
                if pattern.matches(known_id) && seen.insert(known_id.clone()) {
                    let raw =
                        (project_with_freshness(k, entry.mode, norms, now) * zone_scale).clamp(0.0, 1.0);
                    let (thr_min, thr_max) = entry.activation_range();
                    let value = apply_zone_activation_range(raw, thr_min, thr_max);
                    zone_levels.push(value);
                    if value > 0.0 {
                        active_zones.push((known_id.clone(), value));
                        let ki = kinematics_for_zone(k, norms, now);
                        kin_accum.depth = kin_accum.depth.max(ki.depth);
                        kin_accum.speed = kin_accum.speed.max(ki.speed);
                        kin_accum.acc = kin_accum.acc.max(ki.acc);
                        kin_accum.recoil = kin_accum.recoil.max(ki.recoil);
                        kin_count += 1;
                    }
                }
            }
        } else if seen.insert(pattern.clone()) {
            let raw = kinematics
                .get(pattern)
                .map(|k| (project_with_freshness(k, entry.mode, norms, now) * zone_scale).clamp(0.0, 1.0))
                .unwrap_or(0.0);
            let (thr_min, thr_max) = entry.activation_range();
            let value = apply_zone_activation_range(raw, thr_min, thr_max);
            zone_levels.push(value);
            if value > 0.0 {
                active_zones.push((pattern.clone(), value));
                if let Some(k) = kinematics.get(pattern) {
                    let ki = kinematics_for_zone(k, norms, now);
                    kin_accum.depth = kin_accum.depth.max(ki.depth);
                    kin_accum.speed = kin_accum.speed.max(ki.speed);
                    kin_accum.acc = kin_accum.acc.max(ki.acc);
                    kin_accum.recoil = kin_accum.recoil.max(ki.recoil);
                    kin_count += 1;
                }
            }
        }
    }

    let _ = kin_count;

    let raw_level = channel.aggregate(&zone_levels);
    let strength = channel.limits.scale(raw_level);
    ChannelStatus {
        raw_level,
        strength,
        frequency: channel.frequency,
        active_zones,
        kinematics: kin_accum,
    }
}

fn compute_status(
    cfg: &CliConfig,
    kinematics: &HashMap<ZoneId, ZoneKinematics>,
    now: Instant,
    device_connected: bool,
) -> CliStatus {
    let norms = cfg.norms.sanitised();
    CliStatus {
        channel_a: compute_channel_status(&cfg.channel_a, kinematics, &norms, now),
        channel_b: compute_channel_status(&cfg.channel_b, kinematics, &norms, now),
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
