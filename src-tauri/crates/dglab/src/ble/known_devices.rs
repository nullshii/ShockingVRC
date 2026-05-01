use std::path::PathBuf;

use log::{debug, warn};
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct KnownDeviceList {
    addresses: Vec<String>,
}

impl KnownDeviceList {
    fn file_path() -> PathBuf {
        std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.join("dglab_devices.json")))
            .unwrap_or_else(|| PathBuf::from("dglab_devices.json"))
    }

    pub fn load() -> Self {
        let path = Self::file_path();
        match std::fs::read_to_string(&path) {
            Ok(contents) => match serde_json::from_str(&contents) {
                Ok(list) => {
                    debug!("Loaded {} known device(s) from {}", list, path.display());
                    list
                }
                Err(e) => {
                    warn!("Failed to parse known devices file: {e}");
                    Self::default()
                }
            },
            Err(_) => Self::default(),
        }
    }

    fn fmt_count(&self) -> usize {
        self.addresses.len()
    }

    pub fn save(&self) {
        let path = Self::file_path();
        match serde_json::to_string_pretty(self) {
            Ok(json) => {
                if let Err(e) = std::fs::write(&path, json) {
                    warn!("Failed to save known devices to {}: {e}", path.display());
                } else {
                    debug!("Saved {} known device(s) to {}", self.addresses.len(), path.display());
                }
            }
            Err(e) => warn!("Failed to serialize known devices: {e}"),
        }
    }

    pub fn add(&mut self, address: &str) -> bool {
        let addr = address.to_uppercase();
        if !self.addresses.contains(&addr) {
            self.addresses.push(addr);
            true
        } else {
            false
        }
    }

    pub fn contains(&self, address: &str) -> bool {
        let addr = address.to_uppercase();
        self.addresses.iter().any(|a| *a == addr)
    }

    pub fn is_empty(&self) -> bool {
        self.addresses.is_empty()
    }

    pub fn addresses(&self) -> &[String] {
        &self.addresses
    }
}

impl std::fmt::Display for KnownDeviceList {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.fmt_count())
    }
}
