use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::RwLock;

use shocking_vrc_core::cli::{CliConfig, CliEngine, CliStopHandle};
use shocking_vrc_core::{AvatarScanner};

#[derive(Debug, Clone)]
pub struct DeviceSlotInfo {
    pub mac: String,
    pub name: String,
    pub battery: Option<u8>,
    pub connected: bool,
    pub reconnecting: bool,
}

pub struct DeviceSlot {
    pub mac: String,
    pub name: String,
    pub engine: CliEngine,
    pub stop_handle: CliStopHandle,
    pub battery: Option<u8>,
    pub connected: bool,
    pub reconnecting: bool,
}

impl DeviceSlot {
    pub fn info(&self) -> DeviceSlotInfo {
        DeviceSlotInfo {
            mac: self.mac.clone(),
            name: self.name.clone(),
            battery: self.battery,
            connected: self.connected,
            reconnecting: self.reconnecting,
        }
    }
}

pub struct AppState {
    pub scanner: AvatarScanner,
    pub monitor_enabled: Arc<AtomicBool>,
    pub default_config: CliConfig,
    pub device_slots: Arc<RwLock<Vec<DeviceSlot>>>,
}

impl AppState {
    pub fn slot_infos(&self) -> Vec<DeviceSlotInfo> {
        self.device_slots
            .read()
            .ok()
            .map(|slots| slots.iter().map(DeviceSlot::info).collect())
            .unwrap_or_default()
    }

    pub fn find_slot_index(&self, mac: &str) -> Option<usize> {
        let target = mac.to_uppercase();
        self.device_slots
            .read()
            .ok()
            .and_then(|slots| slots.iter().position(|s| s.mac.eq_ignore_ascii_case(&target)))
    }

    pub fn engine_at(&self, index: usize) -> Option<CliEngine> {
        self.device_slots
            .read()
            .ok()
            .and_then(|slots| slots.get(index).map(|s| s.engine.clone()))
    }

    pub fn engine_for_mac(&self, mac: &str) -> Option<CliEngine> {
        self.find_slot_index(mac)
            .and_then(|i| self.engine_at(i))
    }

    pub fn update_slot<F>(&self, mac: &str, f: F)
    where
        F: FnOnce(&mut DeviceSlot),
    {
        let target = mac.to_uppercase();
        if let Ok(mut slots) = self.device_slots.write() {
            if let Some(slot) = slots.iter_mut().find(|s| s.mac.eq_ignore_ascii_case(&target)) {
                f(slot);
            }
        }
    }

    pub fn disconnect_all(&self) {
        if let Ok(slots) = self.device_slots.read() {
            for slot in slots.iter() {
                slot.stop_handle.stop();
                let engine = slot.engine.clone();
                tokio::spawn(async move {
                    engine.disconnect_device().await;
                });
            }
        }
    }
}

pub type ConnectedDeviceInfo = DeviceSlotInfo;
