use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::cli::config::PowerLimits;
use crate::protocol::waveform::{MAX_PULSE_FREQUENCY, WaveformV3};

use super::device::{CEILING_OFF, ChannelFrame, MAX_CHANNELS, SEGMENTS};

pub const DEFAULT_POWER_CAP: u8 = 50;
pub const RAMP_SECS: f32 = 2.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SafeChannel {
    pub strength: u8,
    pub freq: [u8; SEGMENTS],
    pub intensity: [u8; SEGMENTS],
    pub limit_min: u8,
    pub limit_max: u8,
    pub ceiling: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AlarmOverlay {
    pub drives: bool,
    pub strength: u8,
    pub peak: u8,
    pub active: bool,
}

fn clean(v: f32) -> f32 {
    if v.is_finite() { v } else { 0.0 }
}

pub fn ramp_factor(elapsed: f32) -> f32 {
    if !elapsed.is_finite() {
        return 0.0;
    }
    (elapsed / RAMP_SECS).clamp(0.0, 1.0)
}

pub fn safe_channel(f: &ChannelFrame, cap: u8, alarm: AlarmOverlay, ramp: f32) -> SafeChannel {
    let cap = cap.min(200);
    let limit_max = (clean(f.limit_max).clamp(0.0, 200.0).round() as u8).min(cap);
    let limit_min = (clean(f.limit_min).clamp(0.0, 200.0).round() as u8).min(limit_max);
    let power = clean(f.power).clamp(0.0, 1.0);
    let limits = PowerLimits {
        min: limit_min,
        max: limit_max,
    };

    let full = limits.scale(power).min(cap);
    let mut strength = (full as f32 * clean(ramp).clamp(0.0, 1.0)).round() as u8;

    let raw_intensity = f.intensity.map(|v| clean(v).clamp(0.0, 100.0));
    let mut intensity = raw_intensity;
    if full > 0 {
        for (v, p) in intensity.iter_mut().zip(f.power_seg) {
            let seg = limits.scale(clean(p).clamp(0.0, power)).min(cap);
            *v *= seg as f32 / full as f32;
        }
    }

    let requested = if f.ceiling.is_finite() && f.ceiling >= 0.0 {
        f.ceiling.clamp(0.0, 200.0).round() as u8
    } else {
        limit_max
    };
    let mut ceiling = requested.min(cap);

    if alarm.drives {
        if alarm.active {
            ceiling = ceiling.max(alarm.peak).min(cap);
        }
        if alarm.strength > strength {
            strength = alarm.strength.min(cap);
            intensity = raw_intensity;
        }
    }
    strength = strength.min(ceiling);

    SafeChannel {
        strength,
        freq: f
            .freq
            .map(|v| clean(v).clamp(10.0, MAX_PULSE_FREQUENCY as f32).round() as u8),
        intensity: intensity.map(|v| v.round() as u8),
        limit_min,
        limit_max,
        ceiling,
    }
}

pub fn build_wave(a: &SafeChannel, b: &SafeChannel) -> WaveformV3 {
    WaveformV3::output(a.strength, b.strength, a.freq, a.intensity, b.freq, b.intensity)
}

pub const REMOTE_FRAME_TIMEOUT: Duration = Duration::from_millis(1000);
pub const DEFAULT_REMOTE_CAP: u8 = 30;

pub const NO_ALARM: AlarmOverlay = AlarmOverlay {
    drives: false,
    strength: 0,
    peak: 0,
    active: false,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum MergeMode {
    RemoteOnly,
    LocalOnly,
    #[default]
    Max,
    SumCapped,
}

impl MergeMode {
    pub const ALL: [MergeMode; 4] = [
        MergeMode::RemoteOnly,
        MergeMode::LocalOnly,
        MergeMode::Max,
        MergeMode::SumCapped,
    ];

    pub fn label(self) -> &'static str {
        match self {
            MergeMode::RemoteOnly => "Remote only",
            MergeMode::LocalOnly => "Local only",
            MergeMode::Max => "Max",
            MergeMode::SumCapped => "Sum, capped",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RemoteChannelPolicy {
    pub allowed: bool,
    pub cap: u8,
    pub freq_bounds: [u8; 2],
    pub int_bounds: [u8; 2],
    pub ramp_per_s: f32,
    pub merge: MergeMode,
}

impl Default for RemoteChannelPolicy {
    fn default() -> Self {
        Self {
            allowed: true,
            cap: DEFAULT_REMOTE_CAP,
            freq_bounds: [10, MAX_PULSE_FREQUENCY],
            int_bounds: [0, 100],
            ramp_per_s: 40.0,
            merge: MergeMode::Max,
        }
    }
}

impl RemoteChannelPolicy {
    pub fn sanitized(mut self) -> Self {
        self.cap = self.cap.min(200);
        self.freq_bounds = [10, MAX_PULSE_FREQUENCY];
        self.int_bounds = [0, 100];
        self.ramp_per_s = if self.ramp_per_s.is_finite() {
            self.ramp_per_s.clamp(0.0, 1000.0)
        } else {
            0.0
        };
        self
    }
}

pub fn remote_channel(f: &ChannelFrame, policy: &RemoteChannelPolicy, ramp: f32) -> SafeChannel {
    let p = policy.sanitized();
    if !p.allowed || p.cap == 0 {
        return SafeChannel::default();
    }
    let requested = if f.limit_max.is_finite() { f.limit_max } else { 0.0 };
    let frame = ChannelFrame {
        limit_min: 0.0,
        limit_max: requested.min(p.cap as f32),
        ceiling: CEILING_OFF,
        ..*f
    };
    let mut s = safe_channel(&frame, p.cap, NO_ALARM, ramp);
    s.freq = s.freq.map(|v| v.clamp(p.freq_bounds[0], p.freq_bounds[1]));
    s.intensity = s.intensity.map(|v| v.clamp(p.int_bounds[0], p.int_bounds[1]));
    s.limit_max = s.limit_max.min(p.cap);
    s.limit_min = s.limit_min.min(s.limit_max);
    s.ceiling = s.ceiling.min(p.cap);
    s.strength = s.strength.min(s.ceiling);
    s
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RemoteInput {
    Inactive,
    Stale,
    Live(SafeChannel),
}

fn with_ceiling(mut s: SafeChannel, ceiling: u8) -> SafeChannel {
    s.ceiling = s.ceiling.max(ceiling);
    s
}

pub fn merge_remote(
    local: SafeChannel,
    remote: RemoteInput,
    policy: &RemoteChannelPolicy,
    alarm_takeover: bool,
    panic: bool,
) -> SafeChannel {
    if panic {
        return SafeChannel { strength: 0, ..local };
    }
    let remote = match remote {
        RemoteInput::Inactive => return local,
        RemoteInput::Stale => SafeChannel::default(),
        RemoteInput::Live(r) => r,
    };
    if alarm_takeover || !policy.allowed {
        return local;
    }
    match policy.merge {
        MergeMode::LocalOnly => local,
        MergeMode::RemoteOnly => {
            if remote.strength == 0 {
                SafeChannel { strength: 0, ..local }
            } else {
                remote
            }
        }
        MergeMode::Max => {
            if remote.strength > local.strength {
                with_ceiling(remote, local.ceiling)
            } else {
                with_ceiling(local, remote.ceiling)
            }
        }
        MergeMode::SumCapped => {
            let ceiling = local.ceiling.max(remote.ceiling);
            let mut base = if remote.strength > local.strength {
                remote
            } else {
                local
            };
            base.strength = (local.strength as u16 + remote.strength as u16).min(ceiling as u16) as u8;
            base.ceiling = ceiling;
            base
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct RemoteOverlay {
    active: bool,
    policy: [RemoteChannelPolicy; MAX_CHANNELS],
    frame: [Option<ChannelFrame>; MAX_CHANNELS],
    frame_at: Option<Instant>,
    live_since: Option<Instant>,
    level: [f32; MAX_CHANNELS],
    tick_at: Option<Instant>,
    output: [SafeChannel; MAX_CHANNELS],
}

impl RemoteOverlay {
    pub fn is_active(&self) -> bool {
        self.active
    }

    pub fn set_active(&mut self, on: bool) {
        if self.active != on {
            *self = Self {
                active: on,
                policy: self.policy,
                ..Self::default()
            };
        }
    }

    pub fn set_policy(&mut self, policy: [RemoteChannelPolicy; MAX_CHANNELS]) {
        self.policy = policy.map(RemoteChannelPolicy::sanitized);
    }

    pub fn policy(&self) -> [RemoteChannelPolicy; MAX_CHANNELS] {
        self.policy
    }

    pub fn submit(&mut self, frame: [Option<ChannelFrame>; MAX_CHANNELS], now: Instant) {
        if !self.active {
            return;
        }
        self.frame = frame;
        self.frame_at = Some(now);
    }

    pub fn output(&self) -> [SafeChannel; MAX_CHANNELS] {
        self.output
    }

    pub fn is_stale(&self, now: Instant) -> bool {
        self.frame_at
            .is_none_or(|t| now.saturating_duration_since(t) > REMOTE_FRAME_TIMEOUT)
    }

    pub fn mix(
        &mut self,
        local: [SafeChannel; MAX_CHANNELS],
        alarm_takeover: [bool; MAX_CHANNELS],
        panic: bool,
        now: Instant,
    ) -> [SafeChannel; MAX_CHANNELS] {
        if !self.active {
            self.output = Default::default();
            return local;
        }
        let dt = self
            .tick_at
            .map(|t| now.saturating_duration_since(t).as_secs_f32().min(0.5))
            .unwrap_or(0.0);
        self.tick_at = Some(now);
        let stale = self.is_stale(now);
        if stale {
            self.live_since = None;
        } else if self.live_since.is_none() {
            self.live_since = Some(now);
        }
        let ramp = self
            .live_since
            .map(|t| ramp_factor(now.saturating_duration_since(t).as_secs_f32()))
            .unwrap_or(0.0);
        let mut out = local;
        for ch in 0..MAX_CHANNELS {
            let policy = self.policy[ch];
            let input = match (stale || panic, &self.frame[ch]) {
                (false, Some(f)) => {
                    let mut r = remote_channel(f, &policy, ramp);
                    let target = r.strength as f32;
                    let level = &mut self.level[ch];
                    *level = if policy.ramp_per_s > 0.0 && target > *level {
                        (*level + policy.ramp_per_s * dt).min(target)
                    } else {
                        target
                    };
                    r.strength = (level.round() as u8).min(r.strength);
                    RemoteInput::Live(r)
                }
                _ => {
                    self.level[ch] = 0.0;
                    RemoteInput::Stale
                }
            };
            self.output[ch] = match input {
                RemoteInput::Live(r) => r,
                _ => SafeChannel::default(),
            };
            out[ch] = merge_remote(local[ch], input, &policy, alarm_takeover[ch], panic);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(power: f32, min: f32, max: f32) -> ChannelFrame {
        ChannelFrame {
            power,
            power_seg: [power; SEGMENTS],
            freq: [100.0; SEGMENTS],
            intensity: [100.0; SEGMENTS],
            limit_min: min,
            limit_max: max,
            ceiling: -1.0,
        }
    }

    #[test]
    fn generated_graph_waveforms_limit_legacy_frequencies_to_valid_wire_values() {
        let mut f = frame(1.0, 0.0, 30.0);
        f.freq = [10.0, 240.0, 241.0, 255.0];
        let channel = safe_channel(&f, 50, NO_ALARM, 1.0);
        let wave = build_wave(&channel, &channel);
        assert_eq!(wave.frequency_a, [10, 240, 240, 240]);
        assert_eq!(wave.frequency_b, [10, 240, 240, 240]);
        assert_eq!(wave.strength_a, 30);
        assert_eq!(f.freq, [10.0, 240.0, 241.0, 255.0]);
    }

    #[test]
    fn non_finite_values_are_sanitised() {
        let f = ChannelFrame {
            power: f32::NAN,
            power_seg: [f32::NAN; SEGMENTS],
            freq: [f32::INFINITY, f32::NAN, -5.0, 999.0],
            intensity: [f32::NAN, f32::NEG_INFINITY, 50.0, 300.0],
            limit_min: f32::NAN,
            limit_max: f32::INFINITY,
            ceiling: f32::NAN,
        };
        let s = safe_channel(&f, 50, NO_ALARM, 1.0);
        assert_eq!(s.strength, 0, "NaN power is no power");
        assert_eq!(s.freq, [10, 10, 10, 240]);
        assert_eq!(s.intensity, [0, 0, 50, 100]);
        assert_eq!(s.limit_max, 0, "infinite limit reads 0");
    }

    #[test]
    fn the_cap_can_never_be_exceeded() {
        for (max, ceiling) in [(200.0, -1.0), (200.0, 200.0), (10.0, 200.0)] {
            let mut f = frame(1.0, 0.0, max);
            f.ceiling = ceiling;
            let s = safe_channel(&f, 40, NO_ALARM, 1.0);
            assert!(s.strength <= 40, "strength {} over cap", s.strength);
            assert!(s.ceiling <= 40, "ceiling {} over cap", s.ceiling);
            assert!(s.limit_max <= 40);
        }
    }

    #[test]
    fn limit_pins_are_requests_clamped_by_the_cap() {
        let s = safe_channel(&frame(1.0, 0.0, 120.0), 60, NO_ALARM, 1.0);
        assert_eq!(s.limit_max, 60);
        assert_eq!(s.strength, 60);
        let s = safe_channel(&frame(1.0, 0.0, 25.0), 60, NO_ALARM, 1.0);
        assert_eq!((s.limit_max, s.strength), (25, 25));
        let s = safe_channel(&frame(0.5, 90.0, 30.0), 60, NO_ALARM, 1.0);
        assert!(s.limit_min <= s.limit_max, "min is pulled under max");
    }

    #[test]
    fn zero_power_is_zero_output_even_with_a_floor() {
        let s = safe_channel(&frame(0.0, 20.0, 40.0), 60, NO_ALARM, 1.0);
        assert_eq!(s.strength, 0);
        let s = safe_channel(&frame(0.01, 20.0, 40.0), 60, NO_ALARM, 1.0);
        assert_eq!(s.strength, 20);
    }

    #[test]
    fn the_ramp_scales_from_zero() {
        let f = frame(1.0, 0.0, 40.0);
        assert_eq!(safe_channel(&f, 60, NO_ALARM, 0.0).strength, 0);
        assert_eq!(safe_channel(&f, 60, NO_ALARM, 0.5).strength, 20);
        assert_eq!(safe_channel(&f, 60, NO_ALARM, ramp_factor(10.0)).strength, 40);
        assert_eq!(ramp_factor(f32::NAN), 0.0);
    }

    #[test]
    fn the_alarm_raises_strength_only_within_the_cap() {
        let alarm = AlarmOverlay {
            drives: true,
            strength: 120,
            peak: 150,
            active: true,
        };
        let s = safe_channel(&frame(0.0, 0.0, 20.0), 50, alarm, 1.0);
        assert_eq!(s.strength, 50);
        assert_eq!(s.ceiling, 50);
        let quiet = AlarmOverlay { drives: false, ..alarm };
        assert_eq!(safe_channel(&frame(0.0, 0.0, 20.0), 50, quiet, 1.0).strength, 0);
    }

    #[test]
    fn strength_stays_under_a_lower_requested_ceiling() {
        let mut f = frame(1.0, 0.0, 40.0);
        f.ceiling = 15.0;
        let s = safe_channel(&f, 60, NO_ALARM, 1.0);
        assert_eq!((s.ceiling, s.strength), (15, 15));
    }

    #[test]
    fn quieter_segments_play_softer_but_never_louder() {
        let mut f = frame(1.0, 0.0, 40.0);
        f.power_seg = [1.0, 0.0, 0.5, 1.0];
        let s = safe_channel(&f, 60, NO_ALARM, 1.0);
        assert_eq!(s.strength, 40);
        assert_eq!(s.intensity, [100, 0, 50, 100]);
        f.power_seg = [9.0, f32::NAN, 1.0, 1.0];
        assert_eq!(safe_channel(&f, 60, NO_ALARM, 1.0).intensity, [100, 0, 100, 100]);
    }

    #[test]
    fn segment_power_follows_the_limits_not_a_plain_ratio() {
        let mut f = frame(1.0, 20.0, 40.0);
        f.power_seg = [1.0, 0.5, 0.0, 1.0];
        let s = safe_channel(&f, 60, NO_ALARM, 1.0);
        assert_eq!(s.intensity, [100, 75, 0, 100]);
    }

    #[test]
    fn an_alarm_that_takes_over_plays_unscaled() {
        let alarm = AlarmOverlay {
            drives: true,
            strength: 45,
            peak: 50,
            active: true,
        };
        let mut f = frame(1.0, 0.0, 20.0);
        f.power_seg = [1.0, 0.0, 0.0, 1.0];
        let s = safe_channel(&f, 60, alarm, 1.0);
        assert_eq!(s.strength, 45);
        assert_eq!(s.intensity, [100; 4], "the alarm's own pattern is not gated by the graph");
    }

    #[test]
    fn silence_when_both_channels_are_zero() {
        let z = SafeChannel::default();
        let w = build_wave(&z, &z);
        assert_eq!(w.strength_a, 0);
        assert_eq!(w.strength_b, 0);
        assert_eq!(w.strength_mode, 0x0F, "idle clears both stored device strengths");
        assert_eq!(w.frequency_a, [10; 4], "the idle packet uses valid pulse data");
        assert_eq!(w.intensity_a, [0; 4]);
        assert_eq!(w.intensity_b, [0; 4]);
    }
}
