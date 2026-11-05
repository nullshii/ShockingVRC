use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WaveformV3 {
    pub head: u8,
    pub strength_mode: u8,
    pub strength_a: u8,
    pub strength_b: u8,
    pub frequency_a: [u8; 4],
    pub intensity_a: [u8; 4],
    pub frequency_b: [u8; 4],
    pub intensity_b: [u8; 4],

    #[serde(skip)]
    pub number: u8,
}

pub const MODE_NO_CHANGE: u8 = 0b0000;
pub const MODE_A_ABSOLUTE: u8 = 0b1100;
pub const MODE_B_ABSOLUTE: u8 = 0b0011;
pub const MODE_BOTH_ABSOLUTE: u8 = 0b1111;
pub const MIN_PULSE_FREQUENCY: u8 = 10;
pub const MAX_PULSE_FREQUENCY: u8 = 240;

impl Default for WaveformV3 {
    fn default() -> Self {
        Self {
            head: 0xB0,
            strength_mode: MODE_BOTH_ABSOLUTE,
            strength_a: 0,
            strength_b: 0,
            frequency_a: [MIN_PULSE_FREQUENCY; 4],
            intensity_a: [0; 4],
            frequency_b: [MIN_PULSE_FREQUENCY; 4],
            intensity_b: [0; 4],
            number: 0,
        }
    }
}

impl WaveformV3 {
    pub fn with_mode(
        strength_mode: u8,
        strength_a: u8,
        strength_b: u8,
        frequency_a: [u8; 4],
        intensity_a: [u8; 4],
        frequency_b: [u8; 4],
        intensity_b: [u8; 4],
    ) -> Self {
        // Sequence zero is the documented fire-and-forget mode. Callers that
        // explicitly request a B1 acknowledgement can supply the high nibble.
        let number = strength_mode >> 4;
        Self {
            head: 0xB0,
            strength_mode,
            strength_a,
            strength_b,
            // Preserve raw construction, including intentional invalid values
            // used by V3 to suppress a whole channel's pulse data.
            frequency_a,
            intensity_a,
            frequency_b,
            intensity_b,
            number,
        }
    }

    pub fn new(
        strength_a: u8,
        strength_b: u8,
        frequency_a: [u8; 4],
        intensity_a: [u8; 4],
        frequency_b: [u8; 4],
        intensity_b: [u8; 4],
    ) -> Self {
        Self::with_mode(
            MODE_BOTH_ABSOLUTE,
            strength_a,
            strength_b,
            frequency_a,
            intensity_a,
            frequency_b,
            intensity_b,
        )
    }

    pub fn waveform_only_a(frequency_a: [u8; 4], intensity_a: [u8; 4]) -> Self {
        Self::with_mode(MODE_NO_CHANGE, 0, 0, frequency_a, intensity_a, [0; 4], [0, 0, 0, 101])
    }

    pub fn channel_a(strength: u8, frequency: [u8; 4], intensity: [u8; 4]) -> Self {
        Self::new(strength, 0, frequency, intensity, [0; 4], [0, 0, 0, 101])
    }

    pub fn channel_a_quick(strength: u8, frequency: [u8; 4]) -> Self {
        let intensity_a = [strength.min(100); 4];
        Self::new(strength, 0, frequency, intensity_a, [0; 4], [0, 0, 0, 101])
    }

    pub fn duration_a_ms(&self) -> u32 {
        self.frequency_a.iter().map(|&f| map_freq_to_ms(f) as u32).sum()
    }

    pub fn duration_b_ms(&self) -> u32 {
        self.frequency_b.iter().map(|&f| map_freq_to_ms(f) as u32).sum()
    }

    pub fn duration_ms(&self) -> u32 {
        self.duration_a_ms().max(self.duration_b_ms())
    }

    pub fn to_bytes(&self) -> [u8; 20] {
        let mut buf = [0u8; 20];
        buf[0] = self.head;
        buf[1] = self.strength_mode;
        buf[2] = self.strength_a;
        buf[3] = self.strength_b;
        buf[4..8].copy_from_slice(&self.frequency_a);
        buf[8..12].copy_from_slice(&self.intensity_a);
        buf[12..16].copy_from_slice(&self.frequency_b);
        buf[16..20].copy_from_slice(&self.intensity_b);
        buf
    }
}

impl From<WaveformV3> for Vec<u8> {
    fn from(w: WaveformV3) -> Self {
        w.to_bytes().to_vec()
    }
}

impl fmt::Display for WaveformV3 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let hex = self
            .to_bytes()
            .iter()
            .map(|b| format!("{b:02X}"))
            .collect::<Vec<_>>()
            .join("-");

        write!(f, "{hex}")?;

        let format_wave = |name: &str, strength: u8, freq: &[u8; 4], intensity: &[u8; 4]| {
            format!(
                "\n{name}:{strength} [{}/{},{}/{},{}/{},{}/{}]",
                map_freq_to_ms(freq[0]),
                intensity[0],
                map_freq_to_ms(freq[1]),
                intensity[1],
                map_freq_to_ms(freq[2]),
                intensity[2],
                map_freq_to_ms(freq[3]),
                intensity[3],
            )
        };

        match (self.strength_a != 0, self.strength_b != 0) {
            (true, false) => {
                write!(
                    f,
                    "{}",
                    format_wave("WAVE A", self.strength_a, &self.frequency_a, &self.intensity_a)
                )
            }
            (false, true) => {
                write!(
                    f,
                    "{}",
                    format_wave("WAVE B", self.strength_b, &self.frequency_b, &self.intensity_b)
                )
            }
            _ => {
                write!(
                    f,
                    "{}",
                    format_wave("WAVE A", self.strength_a, &self.frequency_a, &self.intensity_a)
                )?;
                write!(
                    f,
                    "{}",
                    format_wave("WAVE B", self.strength_b, &self.frequency_b, &self.intensity_b)
                )
            }
        }
    }
}

pub fn map_freq_to_ms(value: u8) -> f64 {
    let v = value as f64;
    match value {
        10..=100 => v,
        101..=200 => (v - 100.0) * 5.0 + 100.0,
        201..=240 => (v - 200.0) * 10.0 + 600.0,
        _ => 10.0,
    }
}

pub fn map_ms_to_freq(ms: f64) -> u8 {
    map_ms_to_freq_value(ms) as u8
}

fn map_ms_to_freq_value(ms: f64) -> f64 {
    if (10.0..=100.0).contains(&ms) {
        ms
    } else if (100.0..=600.0).contains(&ms) {
        (ms - 100.0) / 5.0 + 100.0
    } else if (600.0..=1000.0).contains(&ms) {
        (ms - 600.0) / 10.0 + 200.0
    } else {
        MIN_PULSE_FREQUENCY as f64
    }
}

pub fn raw_to_hz(raw: u8) -> f32 {
    (1000.0 / map_freq_to_ms(raw.min(MAX_PULSE_FREQUENCY))) as f32
}

pub fn hz_to_raw(hz: f32) -> u8 {
    hz_to_raw_f32(hz) as u8
}

pub(crate) fn hz_to_raw_f32(hz: f32) -> f32 {
    let hz = if hz.is_finite() { hz.clamp(1.0, 100.0) } else { 100.0 };
    map_ms_to_freq_value(1000.0 / hz as f64) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn waveform_matches_official_b0_example() {
        let wave = WaveformV3::waveform_only_a([10; 4], [0, 10, 20, 30]);
        assert_eq!(
            wave.to_bytes(),
            [0xB0, 0, 0, 0, 10, 10, 10, 10, 0, 10, 20, 30, 0, 0, 0, 0, 0, 0, 0, 101]
        );
    }

    #[test]
    fn normal_commands_do_not_request_unhandled_acknowledgements() {
        for _ in 0..32 {
            let wave = WaveformV3::new(15, 20, [10; 4], [100; 4], [100; 4], [50; 4]);
            assert_eq!(wave.to_bytes()[1], MODE_BOTH_ABSOLUTE);
            assert_eq!(wave.number, 0);
        }
        let wave = WaveformV3::waveform_only_a([10; 4], [0; 4]);
        assert_eq!(wave.to_bytes()[1], MODE_NO_CHANGE);
    }

    #[test]
    fn explicit_sequence_and_disabled_channel_data_are_preserved() {
        let wave = WaveformV3::with_mode(0x14, 5, 0, [10; 4], [0, 10, 20, 30], [0; 4], [0, 0, 0, 101]);
        assert_eq!(wave.to_bytes()[1], 0x14);
        assert_eq!(wave.number, 1);
        assert_eq!(&wave.to_bytes()[12..], &[0, 0, 0, 0, 0, 0, 0, 101]);
        let raw = WaveformV3 {
            frequency_a: [255; 4],
            intensity_a: [101; 4],
            ..WaveformV3::default()
        };
        assert_eq!(&raw.to_bytes()[4..12], &[255, 255, 255, 255, 101, 101, 101, 101]);
    }

    #[test]
    fn raw_high_frequency_suppression_is_preserved() {
        let wave = WaveformV3::new(10, 10, [10, 100, 240, 255], [100; 4], [255; 4], [100; 4]);
        assert_eq!(wave.frequency_a, [10, 100, 240, 255]);
        assert_eq!(
            wave.frequency_b, [255; 4],
            "a deliberately invalid channel must remain disabled"
        );
        assert_eq!(wave.intensity_b, [100; 4]);
    }

    #[test]
    fn idle_explicitly_clears_both_strengths_with_zero_pulses() {
        assert_eq!(
            WaveformV3::default().to_bytes(),
            [0xB0, 0x0F, 0, 0, 10, 10, 10, 10, 0, 0, 0, 0, 10, 10, 10, 10, 0, 0, 0, 0]
        );
    }

    #[test]
    fn pulse_period_mapping_follows_official_piecewise_boundaries() {
        for (ms, raw) in [
            (10.0, 10),
            (100.0, 100),
            (105.0, 101),
            (600.0, 200),
            (610.0, 201),
            (1000.0, 240),
        ] {
            assert_eq!(map_ms_to_freq(ms), raw, "{ms} ms");
            assert_eq!(map_freq_to_ms(raw), ms, "raw {raw}");
        }
        assert_eq!(map_ms_to_freq(104.0), 100);
        assert_eq!(map_ms_to_freq(609.0), 200);
        for invalid in [0.0, 9.0, 1001.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert_eq!(map_ms_to_freq(invalid), 10);
        }
    }

    #[test]
    fn hz_conversion_uses_pulse_periods_and_valid_wire_values() {
        for (hz, raw) in [(1.0, 240), (10.0, 100), (50.0, 20), (100.0, 10)] {
            assert_eq!(hz_to_raw(hz), raw);
            assert_eq!(raw_to_hz(raw), hz);
        }
        assert_eq!(hz_to_raw(0.0), 240);
        assert_eq!(hz_to_raw(1000.0), 10);
        assert_eq!(raw_to_hz(255), 1.0);
        for invalid in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            assert_eq!(hz_to_raw(invalid), 10);
        }
        for raw in MIN_PULSE_FREQUENCY..=MAX_PULSE_FREQUENCY {
            assert!((1.0..=100.0).contains(&raw_to_hz(raw)));
        }
    }
}
