use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use log::{info, warn};
use serde::{Deserialize, Serialize};

pub const ALARM_TICK: Duration = Duration::from_millis(50);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum AlarmChannels {
    A,
    B,
    #[default]
    Both,
}

impl AlarmChannels {
    pub const ALL: [AlarmChannels; 3] = [AlarmChannels::A, AlarmChannels::B, AlarmChannels::Both];

    pub fn drives_a(self) -> bool {
        matches!(self, AlarmChannels::A | AlarmChannels::Both)
    }

    pub fn drives_b(self) -> bool {
        matches!(self, AlarmChannels::B | AlarmChannels::Both)
    }

    pub fn label(self) -> &'static str {
        match self {
            AlarmChannels::A => "A",
            AlarmChannels::B => "B",
            AlarmChannels::Both => "A+B",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AlarmConfig {
    pub enabled: bool,
    pub hour: u8,
    pub minute: u8,
    pub channels: AlarmChannels,
    pub start_strength: u8,
    pub peak_strength: u8,
    pub ramp_secs: u16,
    pub max_duration_secs: u16,
    pub repeats: u8,
    pub pulse_on_ms: u16,
    pub pulse_off_ms: u16,
    pub snooze_mins: u8,
}

impl Default for AlarmConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            hour: 7,
            minute: 0,
            channels: AlarmChannels::Both,
            start_strength: 5,
            peak_strength: 30,
            ramp_secs: 120,
            max_duration_secs: 300,
            repeats: 3,
            pulse_on_ms: 800,
            pulse_off_ms: 400,
            snooze_mins: 5,
        }
    }
}

impl AlarmConfig {
    pub const MAX_STRENGTH: u8 = 200;
    pub const RAMP_MAX_SECS: u16 = 3600;
    pub const DURATION_MIN_SECS: u16 = 30;
    pub const DURATION_MAX_SECS: u16 = 3600;
    pub const PULSE_MIN_MS: u16 = 100;
    pub const PULSE_MAX_MS: u16 = 10_000;
    pub const SNOOZE_MAX_MINS: u8 = 60;
    pub const MAX_REPEATS: u8 = 20;

    pub fn sanitise(&mut self) {
        self.hour = self.hour.min(23);
        self.minute = self.minute.min(59);
        self.start_strength = self.start_strength.min(Self::MAX_STRENGTH);
        self.peak_strength = self
            .peak_strength
            .clamp(self.start_strength, Self::MAX_STRENGTH);
        self.ramp_secs = self.ramp_secs.min(Self::RAMP_MAX_SECS);
        self.max_duration_secs = self
            .max_duration_secs
            .clamp(Self::DURATION_MIN_SECS, Self::DURATION_MAX_SECS);
        self.repeats = self.repeats.clamp(1, Self::MAX_REPEATS);
        self.pulse_on_ms = self.pulse_on_ms.clamp(Self::PULSE_MIN_MS, Self::PULSE_MAX_MS);
        self.pulse_off_ms = self.pulse_off_ms.min(Self::PULSE_MAX_MS);
        self.snooze_mins = self.snooze_mins.clamp(1, Self::SNOOZE_MAX_MINS);
    }

    pub fn minute_of_day(&self) -> u16 {
        self.hour as u16 * 60 + self.minute as u16
    }

    pub fn fires_at(&self, clock: LocalClock) -> bool {
        clock.minute_of_day == self.minute_of_day()
    }

    pub fn next_fire_in_mins(&self, clock: LocalClock, fired_this_minute: bool) -> u32 {
        let delta = self.minute_of_day() as i32 - clock.minute_of_day as i32;
        if delta > 0 || (delta == 0 && !fired_this_minute) {
            delta as u32
        } else {
            (delta + 1440) as u32
        }
    }

    pub fn ramp_strength(&self, elapsed: Duration) -> u8 {
        let peak = self.peak_strength.max(self.start_strength);
        let t = if self.ramp_secs == 0 {
            1.0
        } else {
            (elapsed.as_secs_f32() / self.ramp_secs as f32).clamp(0.0, 1.0)
        };
        let start = self.start_strength as f32;
        (start + (peak as f32 - start) * t).round() as u8
    }

    pub fn pulse_open(&self, elapsed: Duration) -> bool {
        if self.pulse_off_ms == 0 {
            return true;
        }
        let period = self.pulse_on_ms as u128 + self.pulse_off_ms as u128;
        if period == 0 {
            return true;
        }
        (elapsed.as_millis() % period) < self.pulse_on_ms as u128
    }

    pub fn time_label(&self) -> String {
        format!("{:02}:{:02}", self.hour, self.minute)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocalClock {
    pub day_index: i32,
    pub minute_of_day: u16,
}

impl LocalClock {
    pub fn now() -> Self {
        use chrono::{Datelike, Local, Timelike};
        let now = Local::now();
        Self {
            day_index: now.num_days_from_ce(),
            minute_of_day: now.hour() as u16 * 60 + now.minute() as u16,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AlarmPhase {
    #[default]
    Idle,
    Ringing,
    Snoozed,
}

impl AlarmPhase {
    pub fn is_ringing(self) -> bool {
        self == AlarmPhase::Ringing
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AlarmStatus {
    pub phase: AlarmPhase,
    pub strength: u8,
    pub elapsed_secs: u32,
    pub auto_stop_in_secs: u32,
    pub snooze_left_secs: u32,
    pub next_fire_in_mins: u32,
    pub attempt: u8,
    pub retrying: bool,
    pub test: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlarmEvent {
    Fired,
    TestStarted,
    Snoozed,
    SnoozeEnded,
    Retrying,
    AutoStopped,
    Stopped,
}

#[derive(Debug, Clone, Copy)]
enum RingState {
    Idle,
    Ringing {
        started: Instant,
        test: bool,
    },
    Snoozed {
        until: Instant,
        test: bool,
        retrying: bool,
    },
}

#[derive(Debug)]
pub struct AlarmRuntime {
    state: RingState,
    last_fire: Option<(i32, u16)>,
    attempt: u8,
    primed: bool,
}

impl Default for AlarmRuntime {
    fn default() -> Self {
        Self::new()
    }
}

impl AlarmRuntime {
    pub fn new() -> Self {
        Self {
            state: RingState::Idle,
            last_fire: None,
            attempt: 0,
            primed: false,
        }
    }

    pub fn prime(&mut self, cfg: &AlarmConfig, clock: LocalClock) {
        self.primed = true;
        if cfg.enabled && cfg.fires_at(clock) && matches!(self.state, RingState::Idle) {
            self.last_fire = Some((clock.day_index, clock.minute_of_day));
        }
    }

    pub fn phase(&self) -> AlarmPhase {
        match self.state {
            RingState::Idle => AlarmPhase::Idle,
            RingState::Ringing { .. } => AlarmPhase::Ringing,
            RingState::Snoozed { .. } => AlarmPhase::Snoozed,
        }
    }

    pub fn stop(&mut self) -> Option<AlarmEvent> {
        if matches!(self.state, RingState::Idle) {
            return None;
        }
        self.state = RingState::Idle;
        self.attempt = 0;
        Some(AlarmEvent::Stopped)
    }

    pub fn snooze(&mut self, cfg: &AlarmConfig, now: Instant) -> Option<AlarmEvent> {
        let test = match self.state {
            RingState::Ringing { test, .. } => test,
            _ => return None,
        };
        self.state = RingState::Snoozed {
            until: now + snooze_gap(cfg),
            test,
            retrying: false,
        };
        Some(AlarmEvent::Snoozed)
    }

    pub fn start_test(&mut self, now: Instant) -> Option<AlarmEvent> {
        self.state = RingState::Ringing {
            started: now,
            test: true,
        };
        self.attempt = 1;
        Some(AlarmEvent::TestStarted)
    }

    pub fn tick(
        &mut self,
        cfg: &AlarmConfig,
        clock: LocalClock,
        now: Instant,
    ) -> (AlarmStatus, Option<AlarmEvent>) {
        if !self.primed {
            self.prime(cfg, clock);
        }

        let mut event = None;

        if !cfg.enabled {
            let scheduled = match self.state {
                RingState::Ringing { test, .. } | RingState::Snoozed { test, .. } => !test,
                RingState::Idle => false,
            };
            if scheduled {
                self.state = RingState::Idle;
                self.attempt = 0;
                event = Some(AlarmEvent::Stopped);
            }
        }

        if let RingState::Snoozed { until, test, .. } = self.state {
            if now >= until {
                self.state = RingState::Ringing {
                    started: now,
                    test,
                };
                event = Some(AlarmEvent::SnoozeEnded);
            }
        }

        if let RingState::Ringing { started, test } = self.state {
            let limit = Duration::from_secs(cfg.max_duration_secs.max(1) as u64);
            if now.saturating_duration_since(started) >= limit {
                let repeats = cfg.repeats.max(1);
                if !test && self.attempt < repeats {
                    self.attempt += 1;
                    self.state = RingState::Snoozed {
                        until: now + snooze_gap(cfg),
                        test: false,
                        retrying: true,
                    };
                    event = Some(AlarmEvent::Retrying);
                } else {
                    self.state = RingState::Idle;
                    self.attempt = 0;
                    event = Some(AlarmEvent::AutoStopped);
                }
            }
        }

        if matches!(self.state, RingState::Idle) && cfg.enabled && cfg.fires_at(clock) {
            let key = (clock.day_index, clock.minute_of_day);
            if self.last_fire != Some(key) {
                self.last_fire = Some(key);
                self.state = RingState::Ringing {
                    started: now,
                    test: false,
                };
                self.attempt = 1;
                event = Some(AlarmEvent::Fired);
            }
        }

        let fired_this_minute = self.last_fire == Some((clock.day_index, clock.minute_of_day));
        let mut status = AlarmStatus {
            phase: self.phase(),
            attempt: self.attempt,
            next_fire_in_mins: cfg.next_fire_in_mins(clock, fired_this_minute),
            ..AlarmStatus::default()
        };

        match self.state {
            RingState::Idle => {}
            RingState::Ringing { started, test } => {
                let elapsed = now.saturating_duration_since(started);
                let limit = cfg.max_duration_secs.max(1) as u64;
                status.test = test;
                status.elapsed_secs = elapsed.as_secs() as u32;
                status.auto_stop_in_secs = limit.saturating_sub(elapsed.as_secs()) as u32;
                status.strength = if cfg.pulse_open(elapsed) {
                    cfg.ramp_strength(elapsed)
                } else {
                    0
                };
            }
            RingState::Snoozed {
                until,
                test,
                retrying,
            } => {
                status.test = test;
                status.retrying = retrying;
                status.snooze_left_secs = until.saturating_duration_since(now).as_secs() as u32;
            }
        }

        (status, event)
    }
}

fn snooze_gap(cfg: &AlarmConfig) -> Duration {
    Duration::from_secs(cfg.snooze_mins.clamp(1, AlarmConfig::SNOOZE_MAX_MINS) as u64 * 60)
}

#[derive(Clone)]
pub struct AlarmController {
    inner: Arc<AlarmInner>,
}

struct AlarmInner {
    config: RwLock<AlarmConfig>,
    runtime: Mutex<AlarmRuntime>,
    status: RwLock<AlarmStatus>,
    strength: AtomicU8,
}

impl AlarmController {
    pub fn new(mut config: AlarmConfig) -> Self {
        config.sanitise();
        let mut runtime = AlarmRuntime::new();
        runtime.prime(&config, LocalClock::now());
        let controller = Self {
            inner: Arc::new(AlarmInner {
                config: RwLock::new(config),
                runtime: Mutex::new(runtime),
                status: RwLock::new(AlarmStatus::default()),
                strength: AtomicU8::new(0),
            }),
        };
        controller.tick();
        controller
    }

    pub fn config(&self) -> AlarmConfig {
        self.inner
            .config
            .read()
            .map(|c| c.clone())
            .unwrap_or_default()
    }

    pub fn set_config(&self, mut config: AlarmConfig) {
        config.sanitise();
        if let Ok(mut runtime) = self.inner.runtime.lock() {
            runtime.prime(&config, LocalClock::now());
        }
        if let Ok(mut slot) = self.inner.config.write() {
            *slot = config;
        }
    }

    pub fn status(&self) -> AlarmStatus {
        self.inner.status.read().map(|s| *s).unwrap_or_default()
    }

    pub fn strength(&self) -> u8 {
        self.inner.strength.load(Ordering::Relaxed)
    }

    pub fn is_active(&self) -> bool {
        self.status().phase != AlarmPhase::Idle
    }

    pub fn stop(&self) {
        let event = self
            .inner
            .runtime
            .lock()
            .ok()
            .and_then(|mut runtime| runtime.stop());
        self.tick();
        if event.is_some() {
            info!("[alarm] Dismissed");
        }
    }

    pub fn snooze(&self) {
        let cfg = self.config();
        let event = self
            .inner
            .runtime
            .lock()
            .ok()
            .and_then(|mut runtime| runtime.snooze(&cfg, Instant::now()));
        self.tick();
        if event.is_some() {
            info!("[alarm] Snoozed for {} min", cfg.snooze_mins);
        }
    }

    pub fn test(&self) {
        if let Ok(mut runtime) = self.inner.runtime.lock() {
            runtime.start_test(Instant::now());
        }
        self.tick();
        info!("[alarm] Test ring started");
    }

    pub fn tick(&self) -> Option<AlarmEvent> {
        let cfg = self.config();
        let (status, event) = {
            let Ok(mut runtime) = self.inner.runtime.lock() else {
                return None;
            };
            runtime.tick(&cfg, LocalClock::now(), Instant::now())
        };
        self.publish(status);
        if let Some(event) = event {
            log_alarm_event(event, &cfg, status.attempt);
        }
        event
    }

    pub fn spawn_ticker(&self) -> tokio::task::JoinHandle<()> {
        let controller = self.clone();
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(ALARM_TICK);
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                ticker.tick().await;
                controller.tick();
            }
        })
    }

    fn publish(&self, status: AlarmStatus) {
        self.inner.strength.store(status.strength, Ordering::Relaxed);
        if let Ok(mut slot) = self.inner.status.write() {
            *slot = status;
        }
    }
}

impl std::fmt::Debug for AlarmController {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AlarmController")
            .field("status", &self.status())
            .finish()
    }
}

fn log_alarm_event(event: AlarmEvent, cfg: &AlarmConfig, attempt: u8) {
    match event {
        AlarmEvent::Fired => warn!(
            "[alarm] {} — ringing on channel {} (ramp {}s → {}, attempt 1/{})",
            cfg.time_label(),
            cfg.channels.label(),
            cfg.ramp_secs,
            cfg.peak_strength,
            cfg.repeats
        ),
        AlarmEvent::TestStarted => info!("[alarm] Test ring started"),
        AlarmEvent::Snoozed => info!("[alarm] Snoozed for {} min", cfg.snooze_mins),
        AlarmEvent::SnoozeEnded => info!(
            "[alarm] Ringing again (attempt {attempt}/{})",
            cfg.repeats
        ),
        AlarmEvent::Retrying => info!(
            "[alarm] Nobody woke up — retrying in {} min (attempt {attempt}/{})",
            cfg.snooze_mins, cfg.repeats
        ),
        AlarmEvent::AutoStopped => info!(
            "[alarm] Gave up after {} attempt(s) of {}s",
            cfg.repeats, cfg.max_duration_secs
        ),
        AlarmEvent::Stopped => info!("[alarm] Dismissed"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clock(day: i32, hour: u16, minute: u16) -> LocalClock {
        LocalClock {
            day_index: 739_000 + day,
            minute_of_day: hour * 60 + minute,
        }
    }

    fn armed() -> AlarmConfig {
        AlarmConfig {
            enabled: true,
            hour: 7,
            minute: 0,
            repeats: 1,
            ..AlarmConfig::default()
        }
    }

    fn ringing(cfg: &AlarmConfig, t0: Instant) -> AlarmRuntime {
        let mut rt = AlarmRuntime::new();
        rt.tick(cfg, clock(0, 6, 59), t0);
        let (_, ev) = rt.tick(cfg, clock(0, 7, 0), t0 + Duration::from_secs(1));
        assert_eq!(ev, Some(AlarmEvent::Fired));
        rt
    }

    #[test]
    fn sanitise_keeps_peak_above_start_and_caps_the_rest() {
        let mut cfg = AlarmConfig {
            start_strength: 90,
            peak_strength: 10,
            max_duration_secs: 5,
            pulse_on_ms: 1,
            snooze_mins: 0,
            repeats: 0,
            ..AlarmConfig::default()
        };
        cfg.sanitise();
        assert_eq!(cfg.peak_strength, 90);
        assert_eq!(cfg.max_duration_secs, AlarmConfig::DURATION_MIN_SECS);
        assert_eq!(cfg.pulse_on_ms, AlarmConfig::PULSE_MIN_MS);
        assert_eq!(cfg.snooze_mins, 1);
        assert_eq!(cfg.repeats, 1);
    }

    #[test]
    fn peak_is_not_bound_by_anything_but_the_hardware_ceiling() {
        let mut cfg = AlarmConfig {
            peak_strength: 255,
            ..AlarmConfig::default()
        };
        cfg.sanitise();
        assert_eq!(cfg.peak_strength, AlarmConfig::MAX_STRENGTH);
    }

    #[test]
    fn ramp_climbs_from_start_to_peak() {
        let cfg = AlarmConfig {
            start_strength: 10,
            peak_strength: 50,
            ramp_secs: 100,
            ..AlarmConfig::default()
        };
        assert_eq!(cfg.ramp_strength(Duration::ZERO), 10);
        assert_eq!(cfg.ramp_strength(Duration::from_secs(50)), 30);
        assert_eq!(cfg.ramp_strength(Duration::from_secs(100)), 50);
        assert_eq!(cfg.ramp_strength(Duration::from_secs(1000)), 50);
    }

    #[test]
    fn pulse_alternates_and_zero_off_is_continuous() {
        let cfg = AlarmConfig {
            pulse_on_ms: 800,
            pulse_off_ms: 400,
            ..AlarmConfig::default()
        };
        assert!(cfg.pulse_open(Duration::from_millis(0)));
        assert!(cfg.pulse_open(Duration::from_millis(799)));
        assert!(!cfg.pulse_open(Duration::from_millis(800)));
        assert!(!cfg.pulse_open(Duration::from_millis(1199)));
        assert!(cfg.pulse_open(Duration::from_millis(1200)));

        let steady = AlarmConfig {
            pulse_off_ms: 0,
            ..cfg
        };
        assert!(steady.pulse_open(Duration::from_millis(5000)));
    }

    #[test]
    fn next_fire_is_today_then_rolls_over_to_tomorrow() {
        let cfg = armed();
        assert_eq!(cfg.next_fire_in_mins(clock(0, 6, 0), false), 60);
        assert_eq!(cfg.next_fire_in_mins(clock(0, 7, 0), false), 0);
        assert_eq!(cfg.next_fire_in_mins(clock(0, 7, 0), true), 1440);
        assert_eq!(cfg.next_fire_in_mins(clock(0, 8, 0), false), 23 * 60);
    }

    #[test]
    fn fires_once_per_scheduled_minute() {
        let cfg = armed();
        let t0 = Instant::now();
        let mut rt = ringing(&cfg, t0);

        let (_, ev) = rt.tick(&cfg, clock(0, 7, 0), t0 + Duration::from_secs(2));
        assert_eq!(ev, None, "still the same ring, not a second fire");

        rt.stop();
        let (st, ev) = rt.tick(&cfg, clock(0, 7, 0), t0 + Duration::from_secs(3));
        assert_eq!(ev, None, "dismissed alarms must not re-fire in the same minute");
        assert_eq!(st.phase, AlarmPhase::Idle);
        assert_eq!(st.attempt, 0);
    }

    #[test]
    fn starting_inside_the_alarm_minute_does_not_fire_retroactively() {
        let cfg = armed();
        let mut rt = AlarmRuntime::new();
        let (st, ev) = rt.tick(&cfg, clock(0, 7, 0), Instant::now());
        assert_eq!(ev, None);
        assert_eq!(st.phase, AlarmPhase::Idle);
    }

    #[test]
    fn a_single_attempt_gives_up_at_the_duration_limit() {
        let cfg = AlarmConfig {
            max_duration_secs: 60,
            repeats: 1,
            ..armed()
        };
        let t0 = Instant::now();
        let mut rt = ringing(&cfg, t0);

        let (st, ev) = rt.tick(&cfg, clock(0, 7, 0), t0 + Duration::from_secs(30));
        assert_eq!(ev, None);
        assert_eq!(st.phase, AlarmPhase::Ringing);
        assert!(st.auto_stop_in_secs > 0);

        let (st, ev) = rt.tick(&cfg, clock(0, 7, 1), t0 + Duration::from_secs(62));
        assert_eq!(ev, Some(AlarmEvent::AutoStopped));
        assert_eq!(st.phase, AlarmPhase::Idle);
        assert_eq!(st.strength, 0);
    }

    #[test]
    fn it_retries_until_the_repeat_count_runs_out() {
        let cfg = AlarmConfig {
            max_duration_secs: 60,
            snooze_mins: 5,
            repeats: 3,
            ..armed()
        };
        let t0 = Instant::now();
        let mut rt = ringing(&cfg, t0);
        let mut at = 62u64;

        for attempt in 2..=3u8 {
            let (st, ev) = rt.tick(&cfg, clock(0, 7, 1), t0 + Duration::from_secs(at));
            assert_eq!(ev, Some(AlarmEvent::Retrying), "attempt {attempt} should be queued");
            assert_eq!(st.phase, AlarmPhase::Snoozed);
            assert!(st.retrying, "an automatic retry is not a manual snooze");
            assert_eq!(st.attempt, attempt);
            assert_eq!(st.strength, 0);

            at += 5 * 60 + 1;
            let (st, ev) = rt.tick(&cfg, clock(0, 7, 6), t0 + Duration::from_secs(at));
            assert_eq!(ev, Some(AlarmEvent::SnoozeEnded));
            assert_eq!(st.phase, AlarmPhase::Ringing);
            assert_eq!(st.attempt, attempt);
            at += 62;
        }

        let (st, ev) = rt.tick(&cfg, clock(0, 7, 20), t0 + Duration::from_secs(at));
        assert_eq!(ev, Some(AlarmEvent::AutoStopped), "no attempts left");
        assert_eq!(st.phase, AlarmPhase::Idle);
        assert_eq!(st.attempt, 0);
    }

    #[test]
    fn a_manual_snooze_does_not_consume_a_repeat() {
        let cfg = AlarmConfig {
            max_duration_secs: 600,
            snooze_mins: 5,
            repeats: 2,
            ..armed()
        };
        let t0 = Instant::now();
        let mut rt = ringing(&cfg, t0);

        assert_eq!(rt.snooze(&cfg, t0 + Duration::from_secs(2)), Some(AlarmEvent::Snoozed));
        let (st, ev) = rt.tick(&cfg, clock(0, 7, 1), t0 + Duration::from_secs(60));
        assert_eq!(ev, None);
        assert_eq!(st.phase, AlarmPhase::Snoozed);
        assert!(!st.retrying);
        assert_eq!(st.attempt, 1, "still the first attempt");
        assert_eq!(st.strength, 0);

        let (st, ev) = rt.tick(&cfg, clock(0, 7, 5), t0 + Duration::from_secs(303));
        assert_eq!(ev, Some(AlarmEvent::SnoozeEnded));
        assert_eq!(st.phase, AlarmPhase::Ringing);
    }

    #[test]
    fn disabling_dismisses_a_scheduled_ring() {
        let cfg = armed();
        let t0 = Instant::now();
        let mut rt = ringing(&cfg, t0);

        let off = AlarmConfig { enabled: false, ..cfg };
        let (st, ev) = rt.tick(&off, clock(0, 7, 0), t0 + Duration::from_secs(2));
        assert_eq!(ev, Some(AlarmEvent::Stopped));
        assert_eq!(st.phase, AlarmPhase::Idle);
    }

    #[test]
    fn a_test_ring_survives_a_disabled_schedule_and_never_repeats() {
        let cfg = AlarmConfig {
            enabled: false,
            max_duration_secs: 30,
            repeats: 5,
            ..armed()
        };
        let mut rt = AlarmRuntime::new();
        let t0 = Instant::now();
        rt.start_test(t0);

        let (st, ev) = rt.tick(&cfg, clock(0, 12, 0), t0 + Duration::from_secs(1));
        assert_eq!(ev, None);
        assert_eq!(st.phase, AlarmPhase::Ringing);
        assert!(st.test);

        let (st, ev) = rt.tick(&cfg, clock(0, 12, 0), t0 + Duration::from_secs(31));
        assert_eq!(ev, Some(AlarmEvent::AutoStopped));
        assert_eq!(st.phase, AlarmPhase::Idle);
    }

    #[test]
    fn controller_publishes_strength_for_engines_to_read() {
        let cfg = AlarmConfig {
            enabled: false,
            start_strength: 40,
            peak_strength: 40,
            pulse_off_ms: 0,
            ..AlarmConfig::default()
        };
        let controller = AlarmController::new(cfg);
        assert_eq!(controller.strength(), 0);
        assert!(!controller.is_active());

        controller.test();
        controller.tick();
        assert_eq!(controller.strength(), 40);
        assert!(controller.is_active());
        assert!(controller.status().phase.is_ringing());

        controller.stop();
        assert_eq!(controller.strength(), 0, "stop must silence output immediately");
        assert!(!controller.is_active());
    }
}
