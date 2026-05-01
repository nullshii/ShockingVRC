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

impl Default for WaveformV3 {
    fn default() -> Self {
        Self {
            head: 0xB0,
            strength_mode: MODE_NO_CHANGE,
            strength_a: 0,
            strength_b: 0,
            frequency_a: [0; 4],
            intensity_a: [0; 4],
            frequency_b: [0; 4],
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
        let number: u8 = rand::random::<u8>() & 0x0F;
        Self {
            head: 0xB0,
            strength_mode: (number << 4) | (strength_mode & 0x0F),
            strength_a,
            strength_b,
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
    if (100.0..=180.0).contains(&v) {
        5.0 * v - 400.0
    } else if v > 180.0 && v <= 200.0 {
        5.0 * (v - 180.0) + 500.0
    } else if v > 200.0 && v <= 240.0 {
        10.0 * (v - 200.0) + 600.0
    } else {
        100.0
    }
}

pub fn map_ms_to_freq(ms: f64) -> u8 {
    (ms * (7.0 / 45.0) + 84.4444) as u8
}

pub fn raw_to_hz(raw: u8) -> f32 {
    const INPUT_START: f32 = 10.0;
    const INPUT_END: f32 = 255.0;
    const OUTPUT_START: f32 = 100.0;
    const OUTPUT_END: f32 = 1.0;
    OUTPUT_START + (OUTPUT_END - OUTPUT_START) / (INPUT_END - INPUT_START) * (raw as f32 - INPUT_START)
}

pub fn hz_to_raw(hz: f32) -> u8 {
    const INPUT_START: f32 = 100.0;
    const INPUT_END: f32 = 1.0;
    const OUTPUT_START: f32 = 10.0;
    const OUTPUT_END: f32 = 255.0;
    let hz = hz.clamp(INPUT_END, INPUT_START);
    let raw = OUTPUT_START + (OUTPUT_END - OUTPUT_START) / (INPUT_END - INPUT_START) * (hz - INPUT_START);
    raw.round() as u8
}
