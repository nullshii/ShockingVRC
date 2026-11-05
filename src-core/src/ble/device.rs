use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use btleplug::api::{Central, CharPropFlags, Characteristic, Peripheral as _};
use btleplug::platform::{Adapter, Peripheral};
use log::{debug, info, warn};
use tokio::sync::{Mutex, broadcast, watch};
use tokio::task::JoinHandle;
use uuid::Uuid;

use crate::ble::adapter;
use crate::ble::constants::*;
use crate::ble::known_devices::KnownDeviceList;
use crate::ble::transport;
use crate::error::{DGLabError, Result};
use crate::protocol::waveform::WaveformV3;
use crate::protocol::waveform_bf::WaveformBF;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeviceNotification {
    Raw { uuid: Uuid, data: Vec<u8> },
    B1 { number: u8, volt: [u8; 2] },
    BE { params: Vec<u8> },
    E0Reset { number: u8 },
}

pub struct CoyoteDevice {
    peripheral: Peripheral,
    write_char: Characteristic,
    write_lock: Arc<Mutex<()>>,
    name: String,
    id: String,
    mac_address: String,
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

    pub fn mac_address(&self) -> &str {
        &self.mac_address
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
        let _guard = self.write_lock.lock().await;
        transport::write_packet(&self.peripheral, ch, data).await
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
        Ok(parse_battery(&data))
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

        let handle = tokio::spawn(transport::run_output(
            self.peripheral.clone(),
            self.write_char.clone(),
            Arc::clone(&self.wave_now),
            Arc::clone(&self.write_lock),
            stop_rx,
        ));

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
        let result = Self::from_connected_peripheral(peripheral.clone()).await;
        if result.is_err() {
            let _ = peripheral.disconnect().await;
        }
        result
    }

    async fn from_connected_peripheral(peripheral: Peripheral) -> Result<Self> {
        peripheral.discover_services().await?;

        let props = peripheral.properties().await?;
        let name = props.as_ref().and_then(|p| p.local_name.clone()).unwrap_or_default();
        let mac_address = props
            .as_ref()
            .map(|p| p.address.to_string().to_uppercase())
            .unwrap_or_default();
        let id = peripheral.id().to_string();

        let characteristics: Vec<_> = peripheral.characteristics().into_iter().collect();

        debug!("[dglab] Discovered {} characteristics", characteristics.len());
        for ch in &characteristics {
            debug!(
                "[dglab]   service: {}  char: {}  props: {:?}",
                ch.service_uuid, ch.uuid, ch.properties
            );
        }

        let write_char = characteristics
            .iter()
            .find(|c| c.uuid == CHARACTERISTIC_WRITE && c.service_uuid == SERVICE_WRITE)
            .or_else(|| characteristics.iter().find(|c| c.uuid == CHARACTERISTIC_WRITE))
            .cloned()
            .ok_or_else(|| DGLabError::CharacteristicNotFound(CHARACTERISTIC_WRITE))?;

        info!(
            "[dglab] Write characteristic: {} in service {} (props: {:?})",
            write_char.uuid, write_char.service_uuid, write_char.properties
        );

        if let Some(ch) = characteristics
            .iter()
            .find(|c| c.uuid == CHARACTERISTIC_BATTERY && c.properties.contains(CharPropFlags::READ))
        {
            if let Ok(data) = peripheral.read(ch).await {
                if let Some(level) = parse_battery(&data) {
                    info!("[dglab] Battery read: {level}%");
                }
            }
        }

        // shit post becuse vilet update dag-lab firemwere
        if let Some(ch) = characteristics
            .iter()
            .find(|c| c.uuid == CHARACTERISTIC_1501 && c.properties.contains(CharPropFlags::READ))
        {
            if let Ok(data) = peripheral.read(ch).await {
                debug!("[dglab] 1501 read: {:02X?}", data);
            }
        }
        let (notification_tx, _) = broadcast::channel(64);

        let notify_ch = characteristics
            .iter()
            .find(|c| c.uuid == CHARACTERISTIC_NOTIFY && c.service_uuid == SERVICE_NOTIFY)
            .or_else(|| characteristics.iter().find(|c| c.uuid == CHARACTERISTIC_NOTIFY))
            .cloned()
            .ok_or(DGLabError::CharacteristicNotFound(CHARACTERISTIC_NOTIFY))?;
        if !notify_ch
            .properties
            .intersects(CharPropFlags::NOTIFY | CharPropFlags::INDICATE)
        {
            return Err(DGLabError::NotifyError(
                "150B does not support notifications or indications".into(),
            ));
        }

        let mut stream = peripheral
            .notifications()
            .await
            .map_err(|e| DGLabError::NotifyError(format!("notification stream: {e}")))?;
        peripheral
            .subscribe(&notify_ch)
            .await
            .map_err(|e| DGLabError::NotifyError(format!("subscribe to {}: {e}", CHARACTERISTIC_NOTIFY)))?;

        transport::initialize_output(&peripheral, &write_char).await?;

        let tx = notification_tx.clone();
        let notification_task = Some(tokio::spawn(async move {
            use tokio_stream::StreamExt;
            while let Some(notif) = stream.next().await {
                dispatch_notification(&tx, notif.uuid, notif.value);
            }
        }));

        let device = Self {
            peripheral,
            write_char,
            write_lock: Arc::new(Mutex::new(())),
            name,
            id,
            mac_address,
            wave_now: Arc::new(Mutex::new(WaveformV3::default())),
            notification_tx,
            stop_tx: None,
            input_task: None,
            notification_task,
        };

        info!("[dglab] Initialized with zero limits and output; engine will apply configured limits");
        Ok(device)
    }

    pub async fn scan_all() -> Result<Vec<CoyoteDevice>> {
        Self::scan_all_with_timeout(Duration::from_secs(5)).await
    }

    pub async fn scan_all_with_timeout(timeout: Duration) -> Result<Vec<CoyoteDevice>> {
        adapter::with_scan_lock(|| Self::scan_all_with_timeout_inner(timeout)).await
    }

    async fn scan_all_with_timeout_inner(timeout: Duration) -> Result<Vec<CoyoteDevice>> {
        let known = KnownDeviceList::load();
        let adapter = adapter::get_adapter().await?;
        adapter::start_scan(&adapter).await?;

        let mut devices: Vec<CoyoteDevice> = Vec::new();
        let mut connected_ids: HashSet<String> = HashSet::new();

        //poll briefly for known MAC addresses so we can connect without waiting for
        let poll_budget = if known.is_empty() {
            Duration::ZERO
        } else {
            timeout.min(Duration::from_secs(3))
        };

        if !poll_budget.is_zero() {
            let step = Duration::from_millis(500);
            let steps = (poll_budget.as_millis() / step.as_millis()).max(1) as u32;
            'outer: for _ in 0..steps {
                tokio::time::sleep(step).await;
                for p in adapter.peripherals().await.unwrap_or_default() {
                    if let Ok(Some(props)) = p.properties().await {
                        let mac = props.address.to_string().to_uppercase();
                        if known.contains(&mac) {
                            let pid = p.id().to_string();
                            if connected_ids.contains(&pid) {
                                continue;
                            }
                            connected_ids.insert(pid);
                            info!(
                                "Found known device {} ({mac}), connecting early",
                                props.local_name.as_deref().unwrap_or("?")
                            );
                            match CoyoteDevice::from_peripheral(p).await {
                                Ok(dev) => {
                                    info!("Connected to known device {}", dev.name());
                                    devices.push(dev);
                                    // All known devices found — skip remaining poll budget
                                    if devices.len() >= known.addresses().len() {
                                        break 'outer;
                                    }
                                }
                                Err(e) => warn!("Failed to connect to known device {mac}: {e}"),
                            }
                        }
                    }
                }
            }
        }

        //wait out the remaining scan window then stop.
        let remaining = timeout.saturating_sub(poll_budget);
        if !remaining.is_zero() {
            tokio::time::sleep(remaining).await;
        }
        adapter::stop_scan(&adapter).await?;

        //connect to any Coyote devices found by name that we haven't already connected.
        let mut known_list = KnownDeviceList::load();
        let mut changed = false;
        for p in find_coyote_peripherals(&adapter).await? {
            let pid = p.id().to_string();
            if connected_ids.contains(&pid) {
                continue;
            }
            connected_ids.insert(pid);
            match CoyoteDevice::from_peripheral(p).await {
                Ok(dev) => {
                    info!("Connected to {}", dev.name());
                    if known_list.add(dev.mac_address()) {
                        changed = true;
                    }
                    devices.push(dev);
                }
                Err(e) => warn!("Failed to connect to peripheral: {e}"),
            }
        }
        if changed {
            known_list.save();
        }
        Ok(devices)
    }

    pub async fn scan_first() -> Result<Option<CoyoteDevice>> {
        Self::scan_first_with_timeout(Duration::from_secs(5)).await
    }

    pub async fn scan_first_with_timeout(timeout: Duration) -> Result<Option<CoyoteDevice>> {
        let known = KnownDeviceList::load();
        let adapter = adapter::get_adapter().await?;
        adapter::start_scan(&adapter).await?;

        //poll briefly for a known MAC address for a faster connection.
        let poll_budget = if known.is_empty() {
            Duration::ZERO
        } else {
            timeout.min(Duration::from_secs(3))
        };

        if !poll_budget.is_zero() {
            let step = Duration::from_millis(500);
            let steps = (poll_budget.as_millis() / step.as_millis()).max(1) as u32;
            for _ in 0..steps {
                tokio::time::sleep(step).await;
                for p in adapter.peripherals().await.unwrap_or_default() {
                    if let Ok(Some(props)) = p.properties().await {
                        let mac = props.address.to_string().to_uppercase();
                        if known.contains(&mac) {
                            info!(
                                "Found known device {} ({mac}), connecting early",
                                props.local_name.as_deref().unwrap_or("?")
                            );
                            match CoyoteDevice::from_peripheral(p).await {
                                Ok(dev) => {
                                    info!("Connected to known device {}", dev.name());
                                    let _ = adapter::stop_scan(&adapter).await;
                                    return Ok(Some(dev));
                                }
                                Err(e) => warn!("Failed to connect to known device {mac}: {e}"),
                            }
                        }
                    }
                }
            }
        }

        //wait out the remaining scan window then stop.
        let remaining = timeout.saturating_sub(poll_budget);
        if !remaining.is_zero() {
            tokio::time::sleep(remaining).await;
        }
        adapter::stop_scan(&adapter).await?;

        //fall back to name-based scan result and save the MAC for next time.
        let peripherals = find_coyote_peripherals(&adapter).await?;
        if let Some(p) = peripherals.into_iter().next() {
            let dev = CoyoteDevice::from_peripheral(p).await?;
            info!("Connected to {}", dev.name());
            let mut known_list = KnownDeviceList::load();
            if known_list.add(dev.mac_address()) {
                known_list.save();
                info!("Saved new device MAC {} to known devices list", dev.mac_address());
            }
            return Ok(Some(dev));
        }
        Ok(None)
    }

    pub async fn connect_by_mac(mac: &str, timeout: Duration) -> Result<Option<Self>> {
        adapter::with_scan_lock(|| Self::connect_by_mac_inner(mac, timeout)).await
    }

    async fn connect_by_mac_inner(mac: &str, timeout: Duration) -> Result<Option<Self>> {
        let target = mac.to_uppercase();
        let adapter = adapter::get_adapter().await?;
        adapter::start_scan(&adapter).await?;

        let result = async {
            let step = Duration::from_millis(500);
            let steps = (timeout.as_millis() / step.as_millis()).max(1) as u32;
            for _ in 0..steps {
                tokio::time::sleep(step).await;
                for p in adapter.peripherals().await.unwrap_or_default() {
                    if let Ok(Some(props)) = p.properties().await {
                        let addr = props.address.to_string().to_uppercase();
                        if addr == target {
                            let connect_timeout = Duration::from_secs(12);
                            match tokio::time::timeout(connect_timeout, Self::from_peripheral(p)).await {
                                Ok(Ok(dev)) => {
                                    info!("Reconnected to {} ({target})", dev.name());
                                    return Ok(Some(dev));
                                }
                                Ok(Err(e)) => return Err(e),
                                Err(_) => {
                                    warn!("Reconnect to {target} timed out during GATT connect");
                                }
                            }
                        }
                    }
                }
            }
            Ok(None)
        }
        .await;

        let _ = adapter::stop_scan(&adapter).await;
        result
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
        info!("[dglab] Device disconnected");
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

fn parse_battery(data: &[u8]) -> Option<u8> {
    match data {
        [level] if *level <= 100 => Some(*level),
        _ => None,
    }
}

fn parse_notification(uuid: Uuid, data: &[u8]) -> Option<DeviceNotification> {
    if uuid != CHARACTERISTIC_NOTIFY {
        return None;
    }
    match data {
        [0xB1, number, a, b] if *number <= 15 && *a <= 200 && *b <= 200 => Some(DeviceNotification::B1 {
            number: *number,
            volt: [*a, *b],
        }),
        // Keep the application's existing opaque legacy BE event.
        [0xBE, _, ..] => Some(DeviceNotification::BE {
            params: data[1..].to_vec(),
        }),
        [0xE0, number, ..] => Some(DeviceNotification::E0Reset { number: *number }),
        _ => None,
    }
}

fn dispatch_notification(tx: &broadcast::Sender<DeviceNotification>, uuid: Uuid, data: Vec<u8>) {
    let parsed = parse_notification(uuid, &data);
    let _ = tx.send(DeviceNotification::Raw { uuid, data });
    if let Some(notification) = parsed {
        let _ = tx.send(notification);
    }
}

async fn find_coyote_peripherals(adapter: &Adapter) -> Result<Vec<Peripheral>> {
    let mut found = Vec::new();
    for p in adapter.peripherals().await? {
        if let Some(props) = p.properties().await? {
            if let Some(ref name) = props.local_name {
                if name.trim() == DEVICE_NAME {
                    info!("[dglab] Found Coyote V3: {name} ({:?})", props.address);
                    found.push(p);
                }
            }
        }
    }
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn b1_parses_solicited_and_wheel_responses() {
        for number in [0, 1, 15] {
            assert_eq!(
                parse_notification(CHARACTERISTIC_NOTIFY, &[0xB1, number, 0, 200]),
                Some(DeviceNotification::B1 { number, volt: [0, 200] })
            );
        }
    }

    #[test]
    fn legacy_be_is_accepted_without_becoming_required() {
        assert_eq!(
            parse_notification(CHARACTERISTIC_NOTIFY, &[0xBE, 100, 200, 0, 255, 128, 10]),
            Some(DeviceNotification::BE {
                params: vec![100, 200, 0, 255, 128, 10]
            })
        );
        assert_eq!(
            parse_notification(CHARACTERISTIC_NOTIFY, &[0xBE, 255]),
            Some(DeviceNotification::BE { params: vec![255] })
        );
        assert!(matches!(
            parse_notification(CHARACTERISTIC_NOTIFY, &[0xB1, 0, 10, 20]),
            Some(DeviceNotification::B1 { .. })
        ));
    }

    #[test]
    fn legacy_and_current_response_streams_deliver_the_same_b1_events() {
        for legacy_be in [None, Some(vec![0xBE, 100, 200, 0, 255, 128, 10])] {
            let (tx, mut rx) = broadcast::channel(16);
            if let Some(data) = legacy_be {
                dispatch_notification(&tx, CHARACTERISTIC_NOTIFY, data);
            }
            dispatch_notification(&tx, CHARACTERISTIC_NOTIFY, vec![0xB1, 0, 10, 20]);
            dispatch_notification(&tx, CHARACTERISTIC_NOTIFY, vec![0xB1, 15, 30, 40]);
            let mut b1 = Vec::new();
            while let Ok(event) = rx.try_recv() {
                if let DeviceNotification::B1 { number, volt } = event {
                    b1.push((number, volt));
                }
            }
            assert_eq!(b1, vec![(0, [10, 20]), (15, [30, 40])]);
        }
    }

    #[test]
    fn malformed_and_unknown_messages_remain_raw_only() {
        let (tx, mut rx) = broadcast::channel(64);
        for data in [
            vec![],
            vec![0xB1],
            vec![0xB1, 1, 2],
            vec![0xB1, 1, 2, 3, 4],
            vec![0xB1, 16, 2, 3],
            vec![0xB1, 1, 201, 3],
            vec![0xBE],
            vec![0xE0],
            vec![0xFF, 1, 2, 3],
        ] {
            dispatch_notification(&tx, CHARACTERISTIC_NOTIFY, data.clone());
            assert_eq!(
                rx.try_recv().unwrap(),
                DeviceNotification::Raw {
                    uuid: CHARACTERISTIC_NOTIFY,
                    data
                }
            );
            assert!(rx.try_recv().is_err());
        }
        dispatch_notification(&tx, CHARACTERISTIC_BATTERY, vec![0xB1, 1, 2, 3]);
        assert!(matches!(rx.try_recv().unwrap(), DeviceNotification::Raw { .. }));
        assert!(rx.try_recv().is_err(), "never decode another characteristic as B1");
    }

    #[test]
    fn valid_messages_keep_raw_and_typed_subscription_events() {
        let (tx, mut rx) = broadcast::channel(4);
        dispatch_notification(&tx, CHARACTERISTIC_NOTIFY, vec![0xB1, 1, 2, 3]);
        assert!(matches!(rx.try_recv().unwrap(), DeviceNotification::Raw { .. }));
        assert_eq!(
            rx.try_recv().unwrap(),
            DeviceNotification::B1 {
                number: 1,
                volt: [2, 3]
            }
        );
        assert_eq!(
            parse_notification(CHARACTERISTIC_NOTIFY, &[0xE0, 9]),
            Some(DeviceNotification::E0Reset { number: 9 })
        );
        assert_eq!(
            parse_notification(CHARACTERISTIC_NOTIFY, &[0xE0, 9, 42]),
            Some(DeviceNotification::E0Reset { number: 9 })
        );
    }

    #[test]
    fn battery_requires_one_valid_percentage_byte() {
        assert_eq!(parse_battery(&[0]), Some(0));
        assert_eq!(parse_battery(&[100]), Some(100));
        for data in [vec![], vec![101], vec![255], vec![50, 0]] {
            assert_eq!(parse_battery(&data), None);
        }
    }
}
