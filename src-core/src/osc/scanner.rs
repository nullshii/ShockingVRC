use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use rosc::{OscMessage, OscPacket, OscType};
use tokio::net::UdpSocket;
use tokio::sync::{Mutex, RwLock, broadcast};
use tokio::task::JoinHandle;

use super::game_device::GameDevice;
use super::oscquery::{MdnsBroadcaster, OscQueryServer, VrchatClient, select_port};
use super::types::{OldZoneType, OscValue, ZoneEvent};
use crate::dsp::UkfParams;
use crate::error::{DGLabError, Result};

const STALE_MS: Duration = Duration::from_millis(15_000);

const BULK_DEBOUNCE: Duration = Duration::from_millis(500);
const BULK_RETRY_MIN: Duration = Duration::from_millis(250);
const BULK_RETRY_MAX: Duration = Duration::from_secs(5);
const BULK_MAX_ATTEMPTS: u32 = 20;

const OGB_ENABLED_PARAM: &str = "OGB_ENABLED";
const OGB_ENABLED_INTERVAL: Duration = Duration::from_secs(5);

// Internal shared state
struct ScannerState {
    devices: RwLock<HashMap<(OldZoneType, String), GameDevice>>,
    event_tx: broadcast::Sender<ZoneEvent>,
    refresh_tx: broadcast::Sender<Vec<ZoneEvent>>,
    requested_port: Option<u16>,
    osc_port: RwLock<u16>,
    listener: Mutex<Option<JoinHandle<()>>>,
    http: Mutex<Option<OscQueryServer>>,
    mdns: Mutex<Option<MdnsBroadcaster>>,
    client: Mutex<Option<Arc<VrchatClient>>>,
    keepalive: Mutex<Option<JoinHandle<()>>>,
    send_socket: tokio::sync::OnceCell<UdpSocket>,
    ukf_params: RwLock<UkfParams>,
    epoch: Instant,
    last_packet_ms: AtomicU64,
    has_received: AtomicBool,
    bulk_attempt: AtomicU64,
}

#[derive(Clone)]
pub struct AvatarScanner {
    state: Arc<ScannerState>,
}

impl AvatarScanner {
    pub fn new(port: Option<u16>) -> Self {
        let (event_tx, _) = broadcast::channel(256);
        let (refresh_tx, _) = broadcast::channel(16);
        Self {
            state: Arc::new(ScannerState {
                devices: RwLock::new(HashMap::new()),
                event_tx,
                refresh_tx,
                requested_port: port,
                osc_port: RwLock::new(port.unwrap_or(0)),
                listener: Mutex::new(None),
                http: Mutex::new(None),
                mdns: Mutex::new(None),
                client: Mutex::new(None),
                keepalive: Mutex::new(None),
                send_socket: tokio::sync::OnceCell::new(),
                ukf_params: RwLock::new(UkfParams::default()),
                epoch: Instant::now(),
                last_packet_ms: AtomicU64::new(0),
                has_received: AtomicBool::new(false),
                bulk_attempt: AtomicU64::new(0),
            }),
        }
    }

    pub async fn port(&self) -> u16 {
        *self.state.osc_port.read().await
    }

    pub async fn oscquery_port(&self) -> Option<u16> {
        self.state.http.lock().await.as_ref().map(|s| s.port())
    }

    pub async fn vrchat_connected(&self) -> bool {
        let last = self.state.last_packet_ms.load(Ordering::Relaxed);
        if last == 0 {
            return false;
        }
        let now = self.state.epoch.elapsed().as_millis() as u64;
        now.saturating_sub(last) < STALE_MS.as_millis() as u64
    }

    pub async fn vrchat_address(&self) -> Option<crate::osc::VrchatAddress> {
        self.state.client.lock().await.as_ref().and_then(|c| c.address())
    }

    /// Currently configured UKF parameters (shared by all contacts).
    pub async fn ukf_params(&self) -> UkfParams {
        *self.state.ukf_params.read().await
    }
    pub async fn set_ukf_params(&self, params: UkfParams) {
        *self.state.ukf_params.write().await = params;
        let mut devices = self.state.devices.write().await;
        for d in devices.values_mut() {
            d.set_ukf_params(params);
        }
    }

    /// Subscribe to real-time [`ZoneEvent`] updates (level changes).
    pub fn subscribe(&self) -> broadcast::Receiver<ZoneEvent> {
        self.state.event_tx.subscribe()
    }

    pub fn subscribe_refreshes(&self) -> broadcast::Receiver<Vec<ZoneEvent>> {
        self.state.refresh_tx.subscribe()
    }

    pub async fn start(&self) -> Result<()> {
        self.stop().await;
        let (osc_port, http_port, advertise) = match self.state.requested_port {
            None => {
                let p = select_port()?;
                (p, p, true)
            }
            Some(p) => {
                let hp = select_port()?;
                (p, hp, false)
            }
        };
        *self.state.osc_port.write().await = osc_port;
        self.state.has_received.store(false, Ordering::Relaxed);
        self.state.last_packet_ms.store(0, Ordering::Relaxed);

        let socket = UdpSocket::bind(("0.0.0.0", osc_port))
            .await
            .map_err(|e| DGLabError::OscError(format!("OSC bind 0.0.0.0:{osc_port} failed: {e}")))?;
        log::info!("OSC listener bound to 0.0.0.0:{osc_port}");
        let me = self.clone();
        let handle = tokio::spawn(async move {
            me.run_listener(socket).await;
        });
        *self.state.listener.lock().await = Some(handle);

        *self.state.http.lock().await = Some(OscQueryServer::start(http_port).await?);

        if advertise {
            *self.state.mdns.lock().await = Some(MdnsBroadcaster::start(http_port));
        } else {
            log::info!("Legacy fixed-port mode: mDNS self-advertising disabled");
        }

        *self.state.client.lock().await = Some(Arc::new(VrchatClient::start()));

        let me = self.clone();
        let keepalive = tokio::spawn(async move {
            loop {
                me.send_ogb_enabled().await;
                tokio::time::sleep(OGB_ENABLED_INTERVAL).await;
            }
        });
        *self.state.keepalive.lock().await = Some(keepalive);

        Ok(())
    }

    pub async fn stop(&self) {
        if let Some(handle) = self.state.listener.lock().await.take() {
            handle.abort();
            let _ = handle.await;
        }
        if let Some(server) = self.state.http.lock().await.take() {
            server.shutdown();
        }
        if let Some(mdns) = self.state.mdns.lock().await.take() {
            mdns.shutdown();
        }
        if let Some(client) = self.state.client.lock().await.take() {
            client.shutdown();
        }
        if let Some(handle) = self.state.keepalive.lock().await.take() {
            handle.abort();
        }
        self.state.bulk_attempt.fetch_add(1, Ordering::Relaxed);
    }


    pub async fn send_param(&self, param: &str, value: f32) -> Result<()> {
        self.send_osc(param, OscType::Float(value)).await
    }
    pub async fn send_param_bool(&self, param: &str, value: bool) -> Result<()> {
        self.send_osc(param, OscType::Bool(value)).await
    }
    async fn send_osc(&self, param: &str, arg: OscType) -> Result<()> {
        let addr = self
            .vrchat_address()
            .await
            .ok_or_else(|| DGLabError::OscError("VRChat address unknown".to_string()))?;
        let target = format!("{}:{}", addr.osc_ip, addr.osc_port);

        let msg = rosc::encoder::encode(&OscPacket::Message(OscMessage {
            addr: format!("/avatar/parameters/{param}"),
            args: vec![arg],
        }))
        .map_err(|e| DGLabError::OscError(e.to_string()))?;

        let socket = self
            .state
            .send_socket
            .get_or_try_init(|| UdpSocket::bind("0.0.0.0:0"))
            .await?;
        socket.send_to(&msg, &target).await?;
        Ok(())
    }

    async fn send_ogb_enabled(&self) {
        if let Err(e) = self.send_param_bool(OGB_ENABLED_PARAM, true).await {
            log::trace!("OGB_ENABLED send skipped: {e}");
        }
    }

    /// Snapshot of all zones seen so far (level may be 0.0 if no active contact).
    pub async fn zones(&self) -> Vec<ZoneEvent> {
        self.state.devices.read().await.values().map(|d| d.to_event()).collect()
    }

    // Internal — OSC listener loop
    async fn run_listener(&self, socket: UdpSocket) {
        let mut buf = vec![0u8; 65_535];
        loop {
            let (len, _src) = match socket.recv_from(&mut buf).await {
                Ok(r) => r,
                Err(e) => {
                    log::error!("OSC listener stopped: {e}");
                    return;
                }
            };
            match rosc::decoder::decode_udp(&buf[..len]) {
                Ok((_, packet)) => {
                    for msg in flatten_packet(packet) {
                        self.handle_message(msg).await;
                    }
                }
                Err(e) => {
                    log::trace!("OSC decode error: {e:?}");
                }
            }
        }
    }

    async fn handle_message(&self, msg: OscMessage) {
        let stamp = (self.state.epoch.elapsed().as_millis() as u64).max(1);
        self.state.last_packet_ms.store(stamp, Ordering::Relaxed);

        if !self.state.has_received.swap(true, Ordering::Relaxed) {
            log::info!("First OSC packet received — starting bulk parameter sync");
            self.trigger_bulk();
        }

        if msg.addr == "/avatar/change" {
            log::info!("Avatar changed — clearing zone cache");
            self.state.devices.write().await.clear();
            let _ = self.state.refresh_tx.send(Vec::new());
            self.trigger_bulk();
            let me = self.clone();
            tokio::spawn(async move {
                me.send_ogb_enabled().await;
            });
            return;
        }

        let Some(param) = msg.addr.strip_prefix("/avatar/parameters/") else {
            return;
        };
        let Some(value) = extract_osc_value(&msg.args) else {
            return;
        };
        self.received_param(param, value, false).await;
    }

    async fn received_param(&self, param: &str, value: OscValue, only_if_new: bool) {
        let parts: Vec<&str> = param.split('/').collect();
        let Some((zone_type, id, contact, is_tps)) = parse_sps_param(&parts) else {
            return;
        };

        let key = (zone_type.clone(), id.clone());
        let ukf_params = *self.state.ukf_params.read().await;
        let mut devices = self.state.devices.write().await;
        let is_new_zone = !devices.contains_key(&key);
        let device = devices
            .entry(key)
            .or_insert_with(|| GameDevice::with_ukf_params(zone_type, id, is_tps, ukf_params));
        if only_if_new && device.has_value(&contact) {
            return;
        }
        device.set_value(&contact, value);
        let event = device.to_event();
        drop(devices);

        let _ = self.state.event_tx.send(event);
        if is_new_zone {
            let zones = self.zones().await;
            let _ = self.state.refresh_tx.send(zones);
        }
    }
    fn trigger_bulk(&self) {
        let attempt = self.state.bulk_attempt.fetch_add(1, Ordering::Relaxed) + 1;
        let me = self.clone();
        tokio::spawn(async move {
            tokio::time::sleep(BULK_DEBOUNCE).await;

            let mut delay = BULK_RETRY_MIN;
            for try_no in 1..=BULK_MAX_ATTEMPTS {
                if me.state.bulk_attempt.load(Ordering::Relaxed) != attempt {
                    return;
                }
                match me.fetch_bulk().await {
                    Ok(()) => return,
                    Err(e) => {
                        log::debug!("Bulk fetch not ready (try {try_no}/{BULK_MAX_ATTEMPTS}): {e}");
                        tokio::time::sleep(delay).await;
                        delay = (delay * 2).min(BULK_RETRY_MAX);
                    }
                }
            }
            log::debug!("Bulk parameter sync gave up after {BULK_MAX_ATTEMPTS} attempts");
        });
    }

    async fn fetch_bulk(&self) -> Result<()> {
        let Some(client) = self.state.client.lock().await.clone() else {
            return Err(DGLabError::OscError("scanner stopped".into()));
        };
        let params = client.get_bulk().await?;

        log::debug!("Bulk OSCQuery: {} values received", params.len());
        for (path, value) in params {
            if let Some(param) = path.strip_prefix("/avatar/parameters/") {
                self.received_param(param, value, true).await;
            }
        }

        let zones = self.zones().await;
        let _ = self.state.refresh_tx.send(zones);
        Ok(())
    }
}

// Helpers
/// Flatten a (possibly nested) OSC packet into a list of messages.
fn flatten_packet(packet: OscPacket) -> Vec<OscMessage> {
    let mut out = Vec::new();
    let mut stack = vec![packet];
    while let Some(pkt) = stack.pop() {
        match pkt {
            OscPacket::Message(msg) => out.push(msg),
            OscPacket::Bundle(bundle) => stack.extend(bundle.content),
        }
    }
    out
}

/// Extract the first meaningful argument from an OSC message.
fn extract_osc_value(args: &[OscType]) -> Option<OscValue> {
    args.first().map(|arg| match arg {
        OscType::Float(f) => OscValue::Float(*f),
        OscType::Double(d) => OscValue::Float(*d as f32),
        OscType::Int(i) => OscValue::Int(*i),
        OscType::Long(l) => OscValue::Int(*l as i32),
        OscType::Bool(b) => OscValue::Bool(*b),
        OscType::Nil => OscValue::Bool(false),
        OscType::Inf => OscValue::Float(1.0),
        _ => OscValue::Bool(false),
    })
}

/// Parse a split parameter path into `(ZoneType, id, contact, is_tps)`.
///
/// | Format | Example |
/// |--------|---------|
/// | `OGB/<Type>/<id>/<contact…>` | `OGB/Pen/Cock/PenOthers` |
/// | `TPS_Internal/<Type>/<id>/<contact…>` | `TPS_Internal/Orf/Anal/Depth_In` |
/// | `VFH/Zone/<Type>/<id>/<contact>` | `VFH/Zone/Pen/Cock/PenOthers` |
/// | `DGB/<name>` | `DGB/TouchAreaA` |
fn parse_sps_param(parts: &[&str]) -> Option<(OldZoneType, String, String, bool)> {
    match parts {
        // DGB: flat zone — value IS the level
        ["DGB", name] => Some((OldZoneType::DGB, name.to_string(), "Value".to_string(), false)),

        // OGB / TPS_Internal
        [prefix, type_str, id, contact @ ..]
            if (*prefix == "OGB" || *prefix == "TPS_Internal") && !contact.is_empty() =>
        {
            let zone_type = parse_zone_type(type_str)?;
            Some((zone_type, id.to_string(), contact.join("/"), *prefix == "TPS_Internal"))
        }

        // VFH
        ["VFH", "Zone", type_str, id, contact] => {
            let zone_type = parse_zone_type(type_str)?;
            Some((zone_type, id.to_string(), contact.to_string(), false))
        }

        _ => None,
    }
}

fn parse_zone_type(s: &str) -> Option<OldZoneType> {
    match s {
        "Pen" => Some(OldZoneType::Pen),
        "Orf" => Some(OldZoneType::Orf),
        "Touch" => Some(OldZoneType::Touch),
        _ => None,
    }
}
