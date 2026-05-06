pub mod ble;
pub mod cli;
pub mod codec;
pub mod dsp;
pub mod error;
pub mod input_zone;
pub mod osc;
pub mod protocol;
pub mod zone_type;

pub use ble::device::{CoyoteDevice, DeviceNotification};
pub use cli::{CliConfig, CliEngine, CliStopHandle};
pub use error::{DGLabError, Result};
pub use osc::{AvatarScanner, OldZoneType, OscValue, VrchatAddress, ZoneEvent};
pub use protocol::waveform::{WaveformV3, hz_to_raw, map_freq_to_ms, map_ms_to_freq, raw_to_hz};
pub use protocol::waveform_bf::WaveformBF;
