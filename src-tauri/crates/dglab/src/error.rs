use thiserror::Error;

#[derive(Debug, Error)]
pub enum DGLabError {
    #[error("Bluetooth adapter not found")]
    AdapterNotFound,

    #[error("Device '{0}' not found")]
    DeviceNotFound(String),

    #[error("Characteristic {0} not found on device")]
    CharacteristicNotFound(uuid::Uuid),

    #[error("Failed to write characteristic: {0}")]
    WriteError(String),

    #[error("Failed to read characteristic: {0}")]
    ReadError(String),

    #[error("Failed to subscribe to notifications: {0}")]
    NotifyError(String),

    #[error("Device not connected")]
    NotConnected,

    #[error("BLE error: {0}")]
    Ble(#[from] btleplug::Error),

    #[error("Serialization error: {0}")]
    Serialization(String),

    #[error("Base64 decode error: {0}")]
    Base64(#[from] base64::DecodeError),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, DGLabError>;