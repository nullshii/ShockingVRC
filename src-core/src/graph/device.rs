use serde::{Deserialize, Serialize};

pub const MAX_CHANNELS: usize = 2;
pub const SEGMENTS: usize = 4;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum DeviceBinding {
    Mac(String),
    Slot(u32),
    #[default]
    Unbound,
}

impl DeviceBinding {
    pub fn mac(&self) -> Option<&str> {
        match self {
            DeviceBinding::Mac(m) => Some(m),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PinRole {
    Power,
    Frequency,
    Intensity,
    LimitMin,
    LimitMax,
    Ceiling,
}

#[derive(Debug, Clone, Copy)]
pub struct PinGroupSpec {
    pub key: &'static str,
    pub label: &'static str,
    pub role: PinRole,
    pub unit: &'static str,
    pub default: f32,
    pub range: (f32, f32),
    pub expandable: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct ChannelSpec {
    pub key: &'static str,
    pub label: &'static str,
    pub groups: &'static [PinGroupSpec],
}

#[derive(Debug, Clone, Copy)]
pub struct DeviceProfile {
    pub id: &'static str,
    pub name: &'static str,
    pub channels: &'static [ChannelSpec],
}

pub const CEILING_OFF: f32 = -1.0;

const COYOTE_GROUPS: &[PinGroupSpec] = &[
    PinGroupSpec {
        key: "power",
        label: "Power",
        role: PinRole::Power,
        unit: "0–1",
        default: 0.0,
        range: (0.0, 1.0),
        expandable: false,
    },
    PinGroupSpec {
        key: "freq",
        label: "Frequency",
        role: PinRole::Frequency,
        unit: "",
        default: 0.5,
        range: (0.0, 1.0),
        expandable: true,
    },
    PinGroupSpec {
        key: "int",
        label: "Intensity",
        role: PinRole::Intensity,
        unit: "0–1",
        default: 1.0,
        range: (0.0, 1.0),
        expandable: true,
    },
    PinGroupSpec {
        key: "limit_min",
        label: "Limit Min",
        role: PinRole::LimitMin,
        unit: "0–200",
        default: 0.0,
        range: (0.0, 200.0),
        expandable: false,
    },
    PinGroupSpec {
        key: "limit_max",
        label: "Limit Max",
        role: PinRole::LimitMax,
        unit: "0–200",
        default: 30.0,
        range: (0.0, 200.0),
        expandable: false,
    },
    PinGroupSpec {
        key: "bf",
        label: "HW Ceiling",
        role: PinRole::Ceiling,
        unit: "0–200",
        default: CEILING_OFF,
        range: (CEILING_OFF, 200.0),
        expandable: false,
    },
];

pub const COYOTE_V3: DeviceProfile = DeviceProfile {
    id: "coyote-v3",
    name: "DG-Lab Coyote V3",
    channels: &[
        ChannelSpec {
            key: "a",
            label: "Channel A",
            groups: COYOTE_GROUPS,
        },
        ChannelSpec {
            key: "b",
            label: "Channel B",
            groups: COYOTE_GROUPS,
        },
    ],
};

pub fn profile(_id: &str) -> &'static DeviceProfile {
    &COYOTE_V3
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum FreqMode {
    #[default]
    Normalized,
    Hz,
    Raw,
}

impl FreqMode {
    pub const ALL: [FreqMode; 3] = [FreqMode::Normalized, FreqMode::Hz, FreqMode::Raw];

    pub fn label(self) -> &'static str {
        match self {
            FreqMode::Normalized => "0–1",
            FreqMode::Hz => "Hz",
            FreqMode::Raw => "raw",
        }
    }
}

fn default_freq_hz() -> [f32; 2] {
    [10.0, 100.0]
}

fn default_freq_clamp() -> [u8; 2] {
    [10, 255]
}

fn default_int_clamp() -> [u8; 2] {
    [0, 100]
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ChannelParams {
    pub expanded: Vec<String>,
    pub freq_mode: FreqMode,
    pub freq_hz: [f32; 2],
    pub freq_clamp: [u8; 2],
    pub intensity_clamp: [u8; 2],
}

impl Default for ChannelParams {
    fn default() -> Self {
        Self {
            expanded: Vec::new(),
            freq_mode: FreqMode::Normalized,
            freq_hz: default_freq_hz(),
            freq_clamp: default_freq_clamp(),
            intensity_clamp: default_int_clamp(),
        }
    }
}

pub static DEFAULT_CHANNEL: ChannelParams = ChannelParams {
    expanded: Vec::new(),
    freq_mode: FreqMode::Normalized,
    freq_hz: [10.0, 100.0],
    freq_clamp: [10, 255],
    intensity_clamp: [0, 100],
};

impl ChannelParams {
    pub fn is_expanded(&self, group: &str) -> bool {
        self.expanded.iter().any(|g| g == group)
    }

    pub fn set_expanded(&mut self, group: &str, on: bool) {
        self.expanded.retain(|g| g != group);
        if on {
            self.expanded.push(group.to_string());
        }
    }

    pub fn sanitize(&mut self) {
        for v in &mut self.freq_hz {
            if !v.is_finite() {
                *v = 10.0;
            }
            *v = v.clamp(1.0, 100.0);
        }
        self.freq_clamp[0] = self.freq_clamp[0].clamp(10, 255);
        self.freq_clamp[1] = self.freq_clamp[1].clamp(10, 255);
        if self.freq_clamp[0] > self.freq_clamp[1] {
            self.freq_clamp.swap(0, 1);
        }
        self.intensity_clamp[0] = self.intensity_clamp[0].min(100);
        self.intensity_clamp[1] = self.intensity_clamp[1].min(100);
        if self.intensity_clamp[0] > self.intensity_clamp[1] {
            self.intensity_clamp.swap(0, 1);
        }
    }

    pub fn map_frequency(&self, v: f32) -> f32 {
        let raw = match self.freq_mode {
            FreqMode::Normalized => {
                let t = v.clamp(0.0, 1.0);
                hz_to_raw_f(self.freq_hz[0] + (self.freq_hz[1] - self.freq_hz[0]) * t)
            }
            FreqMode::Hz => hz_to_raw_f(v),
            FreqMode::Raw => v,
        };
        let raw = if raw.is_finite() { raw } else { 100.0 };
        raw.clamp(self.freq_clamp[0] as f32, self.freq_clamp[1] as f32)
    }

    pub fn map_intensity(&self, v: f32) -> f32 {
        let pct = if v.is_finite() { v * 100.0 } else { 0.0 };
        pct.clamp(self.intensity_clamp[0] as f32, self.intensity_clamp[1] as f32)
    }
}

pub fn hz_to_raw_f(hz: f32) -> f32 {
    let hz = if hz.is_finite() { hz.clamp(1.0, 100.0) } else { 100.0 };
    10.0 + (255.0 - 10.0) / (1.0 - 100.0) * (hz - 100.0)
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChannelFrame {
    pub power: f32,
    pub power_seg: [f32; SEGMENTS],
    pub freq: [f32; SEGMENTS],
    pub intensity: [f32; SEGMENTS],
    pub limit_min: f32,
    pub limit_max: f32,
    pub ceiling: f32,
}

impl Default for ChannelFrame {
    fn default() -> Self {
        Self {
            power: 0.0,
            power_seg: [0.0; SEGMENTS],
            freq: [100.0; SEGMENTS],
            intensity: [0.0; SEGMENTS],
            limit_min: 0.0,
            limit_max: 0.0,
            ceiling: CEILING_OFF,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct DeviceFrame {
    pub node: super::model::NodeId,
    pub binding: DeviceBinding,
    pub channels: [ChannelFrame; MAX_CHANNELS],
}

impl DeviceFrame {
    pub fn zeroed(&mut self) {
        self.channels = [ChannelFrame::default(); MAX_CHANNELS];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn continuous_hz_mapping_matches_the_protocol_table() {
        for hz in [1.0f32, 10.0, 50.0, 100.0] {
            let f = hz_to_raw_f(hz).round() as u8;
            assert_eq!(f, crate::hz_to_raw(hz), "{hz} Hz");
        }
    }

    #[test]
    fn normalised_frequency_spans_the_hz_range() {
        let ch = ChannelParams::default();
        assert_eq!(ch.map_frequency(0.0).round() as u8, crate::hz_to_raw(10.0));
        assert_eq!(ch.map_frequency(1.0).round() as u8, crate::hz_to_raw(100.0));
        assert!(ch.map_frequency(f32::NAN).is_finite());
    }

    #[test]
    fn frequency_and_intensity_honour_their_clamps() {
        let ch = ChannelParams {
            freq_mode: FreqMode::Raw,
            freq_clamp: [40, 200],
            intensity_clamp: [10, 60],
            ..ChannelParams::default()
        };
        assert_eq!(ch.map_frequency(5.0), 40.0);
        assert_eq!(ch.map_frequency(250.0), 200.0);
        assert_eq!(ch.map_intensity(1.0), 60.0);
        assert_eq!(ch.map_intensity(0.0), 10.0);
    }
}
