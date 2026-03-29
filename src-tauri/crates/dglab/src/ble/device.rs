use std::sync::Arc;
use std::time::Duration;

use btleplug::api::{Central, Characteristic, Peripheral as _, WriteType};
use btleplug::platform::{Adapter, Peripheral};
use log::{debug, error, info, warn};
use tokio::sync::{Mutex, broadcast, watch};
use tokio::task::JoinHandle;
use uuid::Uuid;

use crate::ble::adapter;
use crate::ble::constants::*;
use crate::error::{DGLabError, Result};
use crate::protocol::waveform::WaveformV3;
use crate::protocol::waveform_bf::WaveformBF;

const POST_WRITE_DELAY_MS: u64 = 50;

#[derive(Debug, Clone)]
pub enum DeviceNotification {
    Raw { uuid: Uuid, data: Vec<u8> },
    B1 { number: u8, volt: [u8; 2] },
    BE { params: Vec<u8> },
    E0Reset { number: u8 },
}

pub struct CoyoteDevice {
    peripheral: Peripheral,
    write_char: Characteristic,
    name: String,
    id: String,
    wave_now: Arc<Mutex<WaveformV3>>,
    notification_tx: broadcast::Sender<DeviceNotification>,
    stop_tx: Option<watch::Sender<bool>>,
    input_task: Option<JoinHandle<()>>,
    notification_task: Option<JoinHandle<()>>,
}

impl CoyoteDevice {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn subscribe(&self) -> broadcast::Receiver<DeviceNotification> {
        self.notification_tx.subscribe()
    }

    pub fn wave_now(&self) -> Arc<Mutex<WaveformV3>> {
        Arc::clone(&self.wave_now)
    }

    pub async fn set_wave(&self, wave: WaveformV3) {
        *self.wave_now.lock().await = wave;
    }

    fn find_characteristic(&self, uuid: Uuid) -> Result<Characteristic> {
        self.peripheral
            .characteristics()
            .into_iter()
            .find(|c| c.uuid == uuid)
            .ok_or(DGLabError::CharacteristicNotFound(uuid))
    }

    pub async fn write_characteristic(&self, uuid: Uuid, data: &[u8]) -> Result<bool> {
        let ch = self.find_characteristic(uuid)?;
        self.write_to_char(&ch, data).await
    }

    async fn write_to_char(&self, ch: &Characteristic, data: &[u8]) -> Result<bool> {
        let has_write = ch.properties.contains(btleplug::api::CharPropFlags::WRITE);
        let has_write_no_resp = ch
            .properties
            .contains(btleplug::api::CharPropFlags::WRITE_WITHOUT_RESPONSE);

        let write_type = if has_write {
            WriteType::WithResponse
        } else if has_write_no_resp {
            WriteType::WithoutResponse
        } else {
            return Err(DGLabError::WriteError(format!(
                "Characteristic {} has no write property (props: {:?})",
                ch.uuid, ch.properties
            )));
        };

        debug!("Writing {} bytes to {} ({:?}): {:02X?}", data.len(), ch.uuid, write_type, data);

        match self.peripheral.write(ch, data, write_type).await {
            Ok(()) => {
                if matches!(write_type, WriteType::WithoutResponse) {
                    tokio::time::sleep(Duration::from_millis(POST_WRITE_DELAY_MS)).await;
                }
                Ok(true)
            }
            Err(e) if has_write && has_write_no_resp => {
                warn!("WriteWithResponse failed ({}), retrying with WriteWithoutResponse", e);
                self.peripheral
                    .write(ch, data, WriteType::WithoutResponse)
                    .await
                    .map_err(|e2| DGLabError::WriteError(format!("both write modes failed: {e}, {e2}")))?;
                tokio::time::sleep(Duration::from_millis(POST_WRITE_DELAY_MS)).await;
                Ok(true)
            }
            Err(e) => Err(DGLabError::WriteError(e.to_string())),
        }
    }

    pub async fn read_characteristic(&self, uuid: Uuid) -> Result<Vec<u8>> {
        let ch = self.find_characteristic(uuid)?;
        self.peripheral
            .read(&ch)
            .await
            .map_err(|e| DGLabError::ReadError(e.to_string()))
    }

    pub async fn battery_level(&self) -> Result<Option<u8>> {
        let data = self.read_characteristic(CHARACTERISTIC_BATTERY).await?;
        Ok(data.first().copied())
    }

    pub async fn set_waveform(&self, wave: &WaveformV3) -> Result<bool> {
        let bytes = wave.to_bytes();
        self.write_to_char(&self.write_char, &bytes).await
    }

    pub async fn set_wave_bf(&self, bf: &WaveformBF) -> Result<bool> {
        let bytes = bf.to_bytes();
        self.write_to_char(&self.write_char, &bytes).await
    }

    pub async fn write_command(&self, data: &[u8]) -> Result<bool> {
        self.write_to_char(&self.write_char, data).await
    }

    pub async fn send_one_shot(&self, wave: &WaveformV3) -> Result<bool> {
        self.set_waveform(wave).await
    }

    pub async fn is_connected(&self) -> bool {
        self.peripheral.is_connected().await.unwrap_or(false)
    }

    pub fn list_characteristics(&self) -> Vec<(Uuid, Uuid, btleplug::api::CharPropFlags)> {
        self.peripheral
            .characteristics()
            .into_iter()
            .map(|c| (c.service_uuid, c.uuid, c.properties))
            .collect()
    }

    pub fn start(&mut self) -> bool {
        if self.input_task.is_some() {
            return false;
        }

        let (stop_tx, stop_rx) = watch::channel(false);
        self.stop_tx = Some(stop_tx);

        let peripheral = self.peripheral.clone();
        let write_char = self.write_char.clone();
        let wave_now = Arc::clone(&self.wave_now);

        let handle = tokio::spawn(async move {
            println!("[dglab] Waveform output loop started");
            let mut stop_rx = stop_rx;
            let mut last_log = String::new();

            loop {
                if *stop_rx.borrow() {
                    break;
                }

                let connected = peripheral.is_connected().await.unwrap_or(false);
                if !connected {
                    eprintln!("[dglab] Device disconnected, stopping output loop");
                    break;
                }

                let wave = wave_now.lock().await.clone();
                let bytes = wave.to_bytes();
                let delay_ms = 100u64;

                let has_write = write_char.properties.contains(btleplug::api::CharPropFlags::WRITE);
                let write_type = if has_write {
                    WriteType::WithResponse
                } else {
                    WriteType::WithoutResponse
                };

                match peripheral.write(&write_char, &bytes, write_type).await {
                    Ok(()) => {
                        tokio::time::sleep(Duration::from_millis(POST_WRITE_DELAY_MS)).await;
                        let desc = format!("{wave}");
                        if desc != last_log {
                            debug!("Sent waveform: {desc}");
                            last_log = desc;
                        }
                    }
                    Err(e) => {
                        if has_write {
                            if let Err(e2) = peripheral.write(&write_char, &bytes, WriteType::WithoutResponse).await {
                                error!("Write failed (both modes): {e}, {e2}");
                            }
                        } else {
                            error!("Write failed: {e}");
                        }
                    }
                }

                tokio::select! {
                    _ = tokio::time::sleep(Duration::from_millis(delay_ms)) => {}
                    _ = stop_rx.changed() => { break; }
                }
            }

            println!("[dglab] Waveform output loop ended");
        });

        self.input_task = Some(handle);
        true
    }

    pub async fn stop(&mut self) -> bool {
        if let Some(tx) = self.stop_tx.take() {
            let _ = tx.send(true);
        }
        if let Some(handle) = self.input_task.take() {
            let _ = handle.await;
            return true;
        }
        false
    }

    async fn from_peripheral(peripheral: Peripheral) -> Result<Self> {
        peripheral.connect().await?;
        peripheral.discover_services().await?;

        let name = peripheral
            .properties()
            .await?
            .and_then(|p| p.local_name)
            .unwrap_or_default();
        let id = peripheral.id().to_string();

        let characteristics: Vec<_> = peripheral.characteristics().into_iter().collect();

        println!("[dglab] Discovered characteristics:");
        for ch in &characteristics {
            println!("  service: {}  char: {}  props: {:?}", ch.service_uuid, ch.uuid, ch.properties);
        }

        let write_char = characteristics
            .iter()
            .find(|c| c.uuid == CHARACTERISTIC_WRITE && c.service_uuid == SERVICE_WRITE)
            .or_else(|| characteristics.iter().find(|c| c.uuid == CHARACTERISTIC_WRITE))
            .cloned()
            .ok_or_else(|| DGLabError::CharacteristicNotFound(CHARACTERISTIC_WRITE))?;

        println!(
            "[dglab] Write characteristic: {} in service {} (props: {:?})",
            write_char.uuid, write_char.service_uuid, write_char.properties
        );

        if let Some(ch) = characteristics.iter().find(|c| c.uuid == CHARACTERISTIC_BATTERY) {
            if let Ok(data) = peripheral.read(ch).await {
                if !data.is_empty() {
                    println!("[dglab] Battery read: {}%", data[0]);
                }
            }
        }
        tokio::time::sleep(Duration::from_millis(25)).await;

        if let Some(ch) = characteristics.iter().find(|c| c.uuid == CHARACTERISTIC_1501) {
            if let Ok(data) = peripheral.read(ch).await {
                debug!("[dglab] 1501 read: {:02X?}", data);
            }
        }
        tokio::time::sleep(Duration::from_millis(25)).await;

        let (notification_tx, _) = broadcast::channel(64);

        let notify_ch = characteristics
            .iter()
            .find(|c| c.uuid == CHARACTERISTIC_NOTIFY && c.service_uuid == SERVICE_NOTIFY)
            .or_else(|| characteristics.iter().find(|c| c.uuid == CHARACTERISTIC_NOTIFY))
            .cloned();

        let notification_task = if let Some(ch) = notify_ch {
            peripheral
                .subscribe(&ch)
                .await
                .map_err(|e| DGLabError::NotifyError(format!("subscribe to {}: {e}", CHARACTERISTIC_NOTIFY)))?;

            let tx = notification_tx.clone();
            let p = peripheral.clone();
            Some(tokio::spawn(async move {
                use tokio_stream::StreamExt;
                let mut stream = match p.notifications().await {
                    Ok(s) => s,
                    Err(e) => {
                        error!("Failed to get notification stream: {e}");
                        return;
                    }
                };
                while let Some(notif) = stream.next().await {
                    let data = notif.value;
                    let uuid = notif.uuid;

                    let _ = tx.send(DeviceNotification::Raw {
                        uuid,
                        data: data.clone(),
                    });

                    if data.is_empty() {
                        continue;
                    }

                    match data[0] {
                        0xB1 if data.len() >= 4 => {
                            let _ = tx.send(DeviceNotification::B1 {
                                number: data[1],
                                volt: [data[2], data[3]],
                            });
                        }
                        0xBE if data.len() >= 2 => {
                            let _ = tx.send(DeviceNotification::BE {
                                params: data[1..].to_vec(),
                            });
                        }
                        0xE0 if data.len() >= 2 => {
                            let _ = tx.send(DeviceNotification::E0Reset { number: data[1] });
                        }
                        _ => {}
                    }
                }
            }))
        } else {
            warn!("Notify characteristic not found on device");
            None
        };

        let default_wave = WaveformV3::channel_a_quick(40, [100, 100, 100, 100]);
        let device = Self {
            peripheral: peripheral.clone(),
            write_char,
            name,
            id,
            wave_now: Arc::new(Mutex::new(default_wave)),
            notification_tx,
            stop_tx: None,
            input_task: None,
            notification_task,
        };

        let bf = WaveformBF::new(200, 0, 0, 0, 0, 0);
        match device.set_wave_bf(&bf).await {
            Ok(_) => println!("[dglab] Initial BF command sent OK"),
            Err(e) => eprintln!("[dglab] WARNING: Failed to send initial BF command: {e}"),
        }

        tokio::time::sleep(Duration::from_millis(150)).await;

        let init_b0 = WaveformV3::waveform_only_a([10, 10, 10, 10], [0, 10, 20, 30]);
        match device.set_waveform(&init_b0).await {
            Ok(_) => println!("[dglab] Initial B0 sent OK (device should show connected)"),
            Err(e) => println!("[dglab] Initial B0 failed: {e}"),
        }

        tokio::time::sleep(Duration::from_millis(100)).await;

        Ok(device)
    }

    pub async fn scan_all() -> Result<Vec<CoyoteDevice>> {
        Self::scan_all_with_timeout(Duration::from_secs(5)).await
    }

    pub async fn scan_all_with_timeout(timeout: Duration) -> Result<Vec<CoyoteDevice>> {
        let adapter = adapter::get_adapter().await?;
        adapter::start_scan(&adapter).await?;
        tokio::time::sleep(timeout).await;
        adapter::stop_scan(&adapter).await?;

        let mut devices = Vec::new();
        for p in find_coyote_peripherals(&adapter).await? {
            match CoyoteDevice::from_peripheral(p).await {
                Ok(dev) => {
                    info!("Connected to {}", dev.name());
                    devices.push(dev);
                }
                Err(e) => warn!("Failed to connect to peripheral: {e}"),
            }
        }
        Ok(devices)
    }

    pub async fn scan_first() -> Result<Option<CoyoteDevice>> {
        Self::scan_first_with_timeout(Duration::from_secs(5)).await
    }

    pub async fn scan_first_with_timeout(timeout: Duration) -> Result<Option<CoyoteDevice>> {
        let adapter = adapter::get_adapter().await?;
        adapter::start_scan(&adapter).await?;
        tokio::time::sleep(timeout).await;
        adapter::stop_scan(&adapter).await?;

        let peripherals = find_coyote_peripherals(&adapter).await?;
        if let Some(p) = peripherals.into_iter().next() {
            let dev = CoyoteDevice::from_peripheral(p).await?;
            info!("Connected to {}", dev.name());
            return Ok(Some(dev));
        }
        Ok(None)
    }

    pub async fn disconnect(mut self) -> Result<()> {
        self.stop().await;

        if let Some(handle) = self.notification_task.take() {
            handle.abort();
        }

        let notify_ch = self
            .peripheral
            .characteristics()
            .into_iter()
            .find(|c| c.uuid == CHARACTERISTIC_NOTIFY);
        if let Some(ch) = notify_ch {
            let _ = self.peripheral.unsubscribe(&ch).await;
        }

        self.peripheral.disconnect().await?;
        println!("[dglab] Device disconnected");
        Ok(())
    }
}

impl Drop for CoyoteDevice {
    fn drop(&mut self) {
        if let Some(tx) = self.stop_tx.take() {
            let _ = tx.send(true);
        }
        if let Some(h) = self.notification_task.take() {
            h.abort();
        }
        if let Some(h) = self.input_task.take() {
            h.abort();
        }
    }
}

async fn find_coyote_peripherals(adapter: &Adapter) -> Result<Vec<Peripheral>> {
    let mut found = Vec::new();
    for p in adapter.peripherals().await? {
        if let Some(props) = p.properties().await? {
            if let Some(ref name) = props.local_name {
                if name.trim() == DEVICE_NAME {
                    println!("[dglab] Found Coyote V3: {name} (paired: {:?})", props.address);
                    found.push(p);
                }
            }
        }
    }
    Ok(found)
}
