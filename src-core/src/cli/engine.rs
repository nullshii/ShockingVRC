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

use super::alarm::{AlarmConfig, AlarmController};
use super::config::{ChannelConfig, CliConfig, ContactMode, MotionNorms, PowerLimits, UkfConfig, ZoneEntry, ZoneId};

const ZONE_IDLE_TIMEOUT: Duration = Duration::from_millis(500);
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
    alarm: AlarmController,
    last_bf: Mutex<Option<(u8, u8)>>,
}

#[derive(Clone)]
pub struct CliEngine {
    state: Arc<EngineState>,
}

impl CliEngine {
    pub fn new(mut config: CliConfig, alarm: AlarmController) -> Self {
        config.sanitise();
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
                alarm,
                last_bf: Mutex::new(None),
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

    pub async fn set_config(&self, mut config: CliConfig) {
        config.sanitise();
        let ukf = config.ukf;
        *self.state.config.write().await = config;
        self.push_ukf_to_scanner(ukf).await;
        self.try_sync_hardware_limits().await;
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

    pub fn alarm(&self) -> &AlarmController {
        &self.state.alarm
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
        let mut status = compute_status(&cfg, &kinematics, Instant::now(), connected);
        apply_alarm_to_channels(
            &self.state.alarm.config(),
            self.state.alarm.strength(),
            &mut status,
        );
        status
    }

    pub async fn is_device_connected(&self) -> bool {
        self.state.device.lock().await.is_some()
    }

    pub async fn connect_device(&self, device: Arc<CoyoteDevice>) {
        *self.state.last_bf.lock().await = None;
        *self.state.device.lock().await = Some(device);
        info!("[cli] Device attached");
        self.try_sync_hardware_limits().await;
    }

    pub async fn disconnect_device(&self) {
        let mut guard = self.state.device.lock().await;
        if let Some(dev) = guard.take() {
            *dev.wave_now().lock().await = WaveformV3::default();
        }
        drop(guard);
        *self.state.last_bf.lock().await = None;
        info!("[cli] Device detached");
    }

    /// Immediately push current power limits to the connected device (if any).
    pub async fn sync_hardware_limits(&self) {
        self.try_sync_hardware_limits().await;
    }

    async fn try_sync_hardware_limits(&self) {
        let cfg = self.state.config.read().await;
        let (a, b) = bf_targets(&cfg, &self.state.alarm.config(), self.state.alarm.is_active());
        drop(cfg);
        self.push_bf(a, b).await;
    }

    async fn push_bf(&self, a: u8, b: u8) {
        let device = self.state.device.lock().await.clone();
        let Some(device) = device else {
            *self.state.last_bf.lock().await = None;
            return;
        };
        {
            let mut last = self.state.last_bf.lock().await;
            if *last == Some((a, b)) {
                return;
            }
            *last = Some((a, b));
        }
        let bf = WaveformBF::new(a, b, 0, 0, 0, 0);
        match device.set_wave_bf(&bf).await {
            Ok(_) => info!("[cli] BF ceiling set to A={a} B={b}"),
            Err(e) => {
                warn!("[cli] Failed to send BF ceiling: {e}");
                *self.state.last_bf.lock().await = None;
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
            let mut batch: Vec<ZoneEvent> = Vec::new();

            loop {
                tokio::select! {
                    result = zone_rx.recv() => {
                        match result {
                            Ok(event) => {
                                batch.push(event);
                                loop {
                                    match zone_rx.try_recv() {
                                        Ok(ev) => batch.push(ev),
                                        Err(broadcast::error::TryRecvError::Lagged(n)) => {
                                            debug!("[cli] Skipped {n} zone events (lagged)");
                                        }
                                        Err(_) => break,
                                    }
                                }
                                engine.process_events(&batch).await;
                                batch.clear();
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

    async fn process_events(&self, events: &[ZoneEvent]) {
        if events.is_empty() {
            return;
        }
        let now = Instant::now();
        let mut kinematics = self.state.zone_kinematics.write().await;
        for event in events {
            kinematics.insert(
                ZoneId::from_event(event),
                ZoneKinematics {
                    level: event.level,
                    velocity: event.velocity,
                    acceleration: event.acceleration,
                    recoil: event.recoil,
                    last_update: now,
                },
            );
        }
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

        let alarm_cfg = self.state.alarm.config();
        apply_alarm_to_channels(&alarm_cfg, self.state.alarm.strength(), &mut status);
        let (bf_a, bf_b) = bf_targets(&cfg, &alarm_cfg, self.state.alarm.is_active());
        self.push_bf(bf_a, bf_b).await;

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

fn threshold_window_is_open(min_threshold: u8, max_threshold: u8) -> bool {
    min_threshold <= ZoneEntry::THRESHOLD_MIN && max_threshold >= ZoneEntry::THRESHOLD_MAX
}

fn project_zone(k: &ZoneKinematics, entry: &ZoneEntry, norms: &MotionNorms, now: Instant) -> f32 {
    if k.is_stale(now) {
        return 0.0;
    }
    let depth = apply_threshold(k.level.clamp(0.0, 1.0), entry.min_threshold, entry.max_threshold);
    match entry.mode {
        ContactMode::Depth => depth,
        motion => {
            if depth <= 0.0 && !threshold_window_is_open(entry.min_threshold, entry.max_threshold) {
                0.0
            } else {
                project_kinematics(k, motion, norms)
            }
        }
    }
}

fn apply_threshold(level: f32, min_threshold: u8, max_threshold: u8) -> f32 {
    if min_threshold <= ZoneEntry::THRESHOLD_MIN && max_threshold >= ZoneEntry::THRESHOLD_MAX {
        return level;
    }
    let lo = min_threshold as f32 / 100.0;
    let hi = max_threshold as f32 / 100.0;
    if level <= lo {
        return 0.0;
    }
    if hi <= lo {
        return 1.0;
    }
    ((level - lo) / (hi - lo)).clamp(0.0, 1.0)
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
                    let value = (project_zone(k, entry, norms, now) * zone_scale).clamp(0.0, 1.0);
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
            let value = kinematics
                .get(pattern)
                .map(|k| (project_zone(k, entry, norms, now) * zone_scale).clamp(0.0, 1.0))
                .unwrap_or(0.0);
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

fn apply_alarm_to_channels(alarm: &AlarmConfig, commanded: u8, status: &mut CliStatus) {
    if commanded == 0 {
        return;
    }
    if alarm.channels.drives_a() {
        status.channel_a.strength = status.channel_a.strength.max(commanded);
    }
    if alarm.channels.drives_b() {
        status.channel_b.strength = status.channel_b.strength.max(commanded);
    }
}

fn bf_targets(cfg: &CliConfig, alarm: &AlarmConfig, alarm_active: bool) -> (u8, u8) {
    let mut a = cfg.channel_a.limits.max;
    let mut b = cfg.channel_b.limits.max;
    if alarm_active {
        if alarm.channels.drives_a() {
            a = a.max(alarm.peak_strength);
        }
        if alarm.channels.drives_b() {
            b = b.max(alarm.peak_strength);
        }
    }
    (a, b)
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

#[cfg(test)]
mod alarm_output_tests {
    use super::*;
    use crate::cli::alarm::AlarmChannels;

    fn status_with(a: u8, b: u8) -> CliStatus {
        let channel = |strength| ChannelStatus {
            raw_level: 0.0,
            strength,
            frequency: [100; 4],
            active_zones: Vec::new(),
            kinematics: KinematicsInput::default(),
        };
        CliStatus {
            channel_a: channel(a),
            channel_b: channel(b),
            device_connected: true,
        }
    }

    fn alarm_on(channels: AlarmChannels, peak: u8) -> AlarmConfig {
        AlarmConfig {
            channels,
            peak_strength: peak,
            ..AlarmConfig::default()
        }
    }

    fn limits(max_a: u8, max_b: u8) -> CliConfig {
        let mut cfg = CliConfig::default();
        cfg.channel_a.limits = PowerLimits::new(0, max_a);
        cfg.channel_b.limits = PowerLimits::new(0, max_b);
        cfg
    }

    #[test]
    fn alarm_drives_the_selected_channels_only() {
        let mut status = status_with(0, 0);
        apply_alarm_to_channels(&alarm_on(AlarmChannels::A, 40), 40, &mut status);
        assert_eq!(status.channel_a.strength, 40);
        assert_eq!(status.channel_b.strength, 0, "channel B was not selected");
    }

    #[test]
    fn alarm_output_ignores_the_channel_power_limits() {
        let mut status = status_with(0, 0);
        apply_alarm_to_channels(&alarm_on(AlarmChannels::Both, 120), 120, &mut status);
        assert_eq!(status.channel_a.strength, 120);
        assert_eq!(status.channel_b.strength, 120);
    }

    #[test]
    fn a_stronger_zone_touch_is_not_cut_by_the_alarm() {
        let mut status = status_with(80, 5);
        apply_alarm_to_channels(&alarm_on(AlarmChannels::Both, 30), 30, &mut status);
        assert_eq!(status.channel_a.strength, 80);
        assert_eq!(status.channel_b.strength, 30);
    }

    #[test]
    fn a_silent_pulse_gap_leaves_the_channels_untouched() {
        let mut status = status_with(12, 0);
        apply_alarm_to_channels(&alarm_on(AlarmChannels::Both, 30), 0, &mut status);
        assert_eq!(status.channel_a.strength, 12);
        assert_eq!(status.channel_b.strength, 0);
    }

    #[test]
    fn hardware_ceiling_follows_the_channel_limits_while_idle() {
        let (a, b) = bf_targets(&limits(25, 60), &alarm_on(AlarmChannels::Both, 120), false);
        assert_eq!((a, b), (25, 60), "an idle alarm must not raise the ceiling");
    }

    #[test]
    fn hardware_ceiling_is_raised_for_the_driven_channels_while_active() {
        let (a, b) = bf_targets(&limits(25, 60), &alarm_on(AlarmChannels::A, 120), true);
        assert_eq!(a, 120, "channel A needs headroom for the alarm peak");
        assert_eq!(b, 60, "channel B is untouched by this alarm");
    }

    #[test]
    fn hardware_ceiling_never_drops_below_the_channel_limit() {
        let (a, b) = bf_targets(&limits(90, 90), &alarm_on(AlarmChannels::Both, 20), true);
        assert_eq!((a, b), (90, 90));
    }
}

#[cfg(test)]
mod threshold_tests {
    use super::apply_threshold;

    #[test]
    fn full_range_is_pass_through() {
        for level in [0.0, 0.005, 0.3, 0.99, 1.0] {
            assert_eq!(apply_threshold(level, 1, 100), level);
        }
    }

    #[test]
    fn min_threshold_gates_low_input() {
        assert_eq!(apply_threshold(0.2, 30, 100), 0.0);
        assert_eq!(apply_threshold(0.30, 30, 100), 0.0);
        assert!(apply_threshold(0.31, 30, 100) > 0.0);
    }

    #[test]
    fn window_is_stretched_to_full_output() {
        assert!((apply_threshold(0.50, 20, 80) - 0.5).abs() < 1e-6);
        assert_eq!(apply_threshold(0.80, 20, 80), 1.0);
        assert_eq!(apply_threshold(0.95, 20, 80), 1.0);
    }
}

#[cfg(test)]
mod threshold_is_a_depth_calibration_tests {
    use super::*;
    use crate::cli::config::ZoneId;
    use crate::osc::types::OldZoneType;

    fn zone(depth: f32, velocity: f32) -> ZoneKinematics {
        ZoneKinematics {
            level: depth,
            velocity,
            acceleration: velocity,
            recoil: velocity,
            last_update: Instant::now(),
        }
    }

    fn entry(mode: ContactMode, min: u8, max: u8) -> ZoneEntry {
        let mut e = ZoneEntry::new(ZoneId::new(OldZoneType::Orf, "Pussy"), mode);
        e.min_threshold = min;
        e.max_threshold = max;
        e
    }

    fn unit_norms() -> MotionNorms {
        MotionNorms {
            speed: 1.0,
            acc: 1.0,
            recoil: 1.0,
        }
    }

    #[test]
    fn a_shallow_contact_is_silent_in_every_mode() {
        let now = Instant::now();
        let k = zone(0.20, 0.9);
        for mode in [ContactMode::Depth, ContactMode::Speed, ContactMode::Acc, ContactMode::Recoil] {
            assert_eq!(
                project_zone(&k, &entry(mode, 30, 80), &unit_norms(), now),
                0.0,
                "{mode:?} fires below the configured depth window"
            );
        }
    }

    #[test]
    fn inside_the_window_motion_modes_pass_through_unstretched() {
        let now = Instant::now();
        let k = zone(0.50, 0.9);
        for mode in [ContactMode::Speed, ContactMode::Acc, ContactMode::Recoil] {
            assert_eq!(
                project_zone(&k, &entry(mode, 30, 80), &unit_norms(), now),
                0.9,
                "{mode:?} must report the motion itself, not a remapped depth"
            );
        }
        let depth = project_zone(&k, &entry(ContactMode::Depth, 30, 80), &unit_norms(), now);
        assert!((depth - 0.4).abs() < 1e-6, "got {depth}");
    }

    #[test]
    fn past_the_max_edge_the_contact_counts_as_full() {
        let now = Instant::now();
        let k = zone(0.95, 0.9);
        assert_eq!(
            project_zone(&k, &entry(ContactMode::Depth, 30, 80), &unit_norms(), now),
            1.0,
            "the max edge saturates — that is the point of trimming an oversized contact"
        );
        assert_eq!(
            project_zone(&k, &entry(ContactMode::Speed, 30, 80), &unit_norms(), now),
            0.9,
            "and it does not gate the motion modes either"
        );
    }

    #[test]
    fn a_default_window_never_gates_a_motion_mode() {
        let now = Instant::now();
        let k = zone(0.0, 0.9);
        let default = entry(ContactMode::Speed, ZoneEntry::THRESHOLD_MIN, ZoneEntry::THRESHOLD_MAX);
        assert_eq!(
            project_zone(&k, &default, &unit_norms(), now),
            0.9,
            "an untouched threshold must stay a no-op"
        );
    }

    #[test]
    fn a_stale_zone_stays_silent() {
        let now = Instant::now() + ZONE_IDLE_TIMEOUT * 2;
        let k = zone(0.50, 0.9);
        assert_eq!(project_zone(&k, &entry(ContactMode::Speed, 1, 100), &unit_norms(), now), 0.0);
    }
}

#[cfg(test)]
mod staleness_tests {
    use super::{ZoneKinematics, ZONE_IDLE_TIMEOUT};
    use std::time::{Duration, Instant};

    fn zone_at(base: Instant) -> ZoneKinematics {
        ZoneKinematics {
            level: 1.0,
            velocity: 0.0,
            acceleration: 0.0,
            recoil: 0.0,
            last_update: base,
        }
    }

    #[test]
    fn brief_osc_gaps_do_not_drop_a_held_zone() {
        let base = Instant::now();
        let k = zone_at(base);
        assert!(!k.is_stale(base + ZONE_IDLE_TIMEOUT / 5));
        assert!(!k.is_stale(base + ZONE_IDLE_TIMEOUT / 2));
    }

    #[test]
    fn a_stale_nonzero_value_stops_within_the_window() {
        let base = Instant::now();
        let k = zone_at(base);
        assert!(k.is_stale(base + ZONE_IDLE_TIMEOUT * 2));
        assert!(ZONE_IDLE_TIMEOUT <= Duration::from_millis(600), "phantom-stim window too long");
    }
}
