use crate::cli::config::PowerLimits;
use crate::protocol::waveform::WaveformV3;

use super::device::{ChannelFrame, SEGMENTS};

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
        freq: f.freq.map(|v| clean(v).clamp(10.0, 255.0).round() as u8),
        intensity: intensity.map(|v| v.round() as u8),
        limit_min,
        limit_max,
        ceiling,
    }
}

pub fn build_wave(a: &SafeChannel, b: &SafeChannel) -> WaveformV3 {
    if a.strength == 0 && b.strength == 0 {
        WaveformV3::default()
    } else {
        WaveformV3::new(a.strength, b.strength, a.freq, a.intensity, b.freq, b.intensity)
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

    const NO_ALARM: AlarmOverlay = AlarmOverlay {
        drives: false,
        strength: 0,
        peak: 0,
        active: false,
    };

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
        assert_eq!(s.freq, [10, 10, 10, 255]);
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
        assert_eq!(w.frequency_a, [0; 4], "the idle packet");
    }
}
