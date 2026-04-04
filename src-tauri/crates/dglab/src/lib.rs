pub mod ble;
pub mod codec;
pub mod dsp;
pub mod error;
pub mod osc;
pub mod protocol;
pub mod cli;

pub use ble::device::{CoyoteDevice, DeviceNotification};
pub use error::{DGLabError, Result};
pub use osc::{AvatarScanner, OscValue, VrchatAddress, ZoneEvent, ZoneType};
pub use protocol::waveform::{WaveformV3, map_freq_to_ms, map_ms_to_freq};
pub use protocol::waveform_bf::WaveformBF;
pub use cli::{CliConfig, CliEngine, CliStopHandle};