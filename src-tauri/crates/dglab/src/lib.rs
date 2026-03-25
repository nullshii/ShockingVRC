pub mod ble;
pub mod codec;
pub mod dsp;
pub mod error;
pub mod protocol;

pub use ble::device::{CoyoteDevice, DeviceNotification};
pub use error::{DGLabError, Result};
pub use protocol::waveform::{map_freq_to_ms, map_ms_to_freq, WaveformV3};
pub use protocol::waveform_bf::WaveformBF;
