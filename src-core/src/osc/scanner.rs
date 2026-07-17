use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use rosc::{OscMessage, OscPacket, OscType};
use tokio::net::UdpSocket;
use tokio::sync::{Mutex, RwLock, broadcast};
use tokio::task::JoinHandle;

use super::avatar_config::{load_avatar_param_paths, load_latest_avatar_param_paths};
use super::game_device::GameDevice;
use super::oscquery::{DiscoveryMode, VrchatOscQuery};
use super::types::{OldZoneType, OscValue, ZoneEvent};
use crate::dsp::UkfParams;
use crate::error::{DGLabError, Result};

const OSCQUERY_POLL_INTERVAL: Duration = Duration::from_millis(50);

// Internal shared state
struct ScannerState {
    devices: RwLock<HashMap<(OldZoneType, String), GameDevice>>,
    event_tx: broadcast::Sender<ZoneEvent>,
    /// Fired after every completed bulk-fetch — contains full zone snapshot.
    /// Subscribers use this to know "avatar zones are now known".
    refresh_tx: broadcast::Sender<Vec<ZoneEvent>>,
    oscquery: RwLock<VrchatOscQuery>,
    port: RwLock<u16>,
    osc_avatars_dir: RwLock<Option<PathBuf>>,
    listener: Mutex<Option<JoinHandle<()>>>,
    poller: Mutex<Option<JoinHandle<()>>>,
    discovery: Mutex<Option<JoinHandle<()>>>,
    ukf_params: RwLock<UkfParams>,
    last_bulk_zones: Mutex<HashSet<(OldZoneType, String)>>,
}

// AvatarScanner — public handle
/// Listens for VRChat OSC avatar parameters on a UDP port, discovers VRChat
/// via OSCQuery, parses SPS contact zones and emits [`ZoneEvent`]s.
/// The scanner is cheaply cloneable; all clones share the same internal state.
#[derive(Clone)]
pub struct AvatarScanner {
    state: Arc<ScannerState>,
}

impl AvatarScanner {
    /// Create a scanner that will listen on `port` for OSC UDP packets.
    pub fn new(port: u16) -> Self {
        let (event_tx, _) = broadcast::channel(256);
        let (refresh_tx, _) = broadcast::channel(16);
        Self {
            state: Arc::new(ScannerState {
                devices: RwLock::new(HashMap::new()),
                event_tx,
                refresh_tx,
                oscquery: RwLock::new({
                    let mut q = VrchatOscQuery::new();
                    q.set_fallback_port(port);
                    q
                }),
                port: RwLock::new(port),
                osc_avatars_dir: RwLock::new(None),
                listener: Mutex::new(None),
                poller: Mutex::new(None),
                discovery: Mutex::new(None),
                ukf_params: RwLock::new(UkfParams::default()),
                last_bulk_zones: Mutex::new(HashSet::new()),
            }),
        }
    }

    pub async fn port(&self) -> u16 {
        *self.state.port.read().await
    }

    pub async fn osc_avatars_dir(&self) -> Option<PathBuf> {
        self.state.osc_avatars_dir.read().await.clone()
    }

    pub async fn set_osc_avatars_dir(&self, dir: Option<PathBuf>) {
        *self.state.osc_avatars_dir.write().await = dir.filter(|p| !p.as_os_str().is_empty());
    }

    pub async fn set_port(&self, port: u16) -> Result<()> {
        if !(1024..=65535).contains(&port) {
            return Err(DGLabError::OscError(format!(
                "Invalid OSC port {port} (use 1024–65535)"
            )));
        }
        *self.state.port.write().await = port;
        self.state.oscquery.write().await.set_fallback_port(port);
        if self.discovery_mode().await == DiscoveryMode::Osc {
            self.restart_listener().await
        } else {
            Ok(())
        }
    }

    pub async fn discovery_mode(&self) -> DiscoveryMode {
        self.state.oscquery.read().await.discovery_mode()
    }

    pub async fn set_discovery_mode(&self, mode: DiscoveryMode) {
        self.state.oscquery.write().await.set_discovery_mode(mode);
        if let Err(e) = self.sync_transport().await {
            log::error!("Failed to switch OSC transport: {e}");
        }
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

    /// Subscribe to bulk-refresh notifications.
    /// A message is sent every time a successful OSCQuery bulk-fetch completes
    /// (on first connection and after every `/avatar/change`).  The payload is
    /// a snapshot of **all zones** found on the new avatar.
    /// Use this to re-run a zone-discovery report without polling.
    pub fn subscribe_refreshes(&self) -> broadcast::Receiver<Vec<ZoneEvent>> {
        self.state.refresh_tx.subscribe()
    }


    pub async fn start(&self) -> Result<()> {
        self.sync_transport().await
    }

    pub async fn stop(&self) {
        if let Some(handle) = self.state.discovery.lock().await.take() {
            handle.abort();
            let _ = handle.await;
        }
        self.stop_listener().await;
        self.stop_poller().await;
    }

    async fn sync_transport(&self) -> Result<()> {
        match self.discovery_mode().await {
            DiscoveryMode::Osc => {
                self.stop_poller().await;
                self.restart_listener().await
            }
            DiscoveryMode::OscQuery => {
                self.stop_listener().await;
                self.restart_poller().await;
                Ok(())
            }
        }
    }

    async fn stop_listener(&self) {
        if let Some(handle) = self.state.listener.lock().await.take() {
            handle.abort();
            let _ = handle.await;
            log::info!("OSC UDP listener stopped (port free for other apps)");
        }
    }

    async fn stop_poller(&self) {
        if let Some(handle) = self.state.poller.lock().await.take() {
            handle.abort();
            let _ = handle.await;
        }
    }

    async fn restart_listener(&self) -> Result<()> {
        self.stop_listener().await;

        let me = self.clone();
        let handle = tokio::spawn(async move {
            if let Err(e) = me.run_listener().await {
                log::error!("OSC listener stopped: {e}");
            }
        });
        *self.state.listener.lock().await = Some(handle);
        Ok(())
    }

    async fn restart_poller(&self) {
        self.stop_poller().await;

        let me = self.clone();
        let handle = tokio::spawn(async move {
            me.run_oscquery_poller().await;
        });
        *self.state.poller.lock().await = Some(handle);
        log::info!(
            "OSCQuery poller started (no UDP bind — port {} left free)",
            *self.state.port.read().await
        );    }

    pub async fn discover_wait(&self) -> Result<bool> {
        let (found, mode) = self.run_discover().await?;
        if found {
            match mode {
                DiscoveryMode::OscQuery => self.update_bulk().await,
                DiscoveryMode::Osc => self.apply_local_avatar_config(None).await,
            }
        }
        Ok(found)
    }

    pub fn rediscover_background(&self) {
        let me = self.clone();
        tokio::spawn(async move {
            me.replace_discovery_task().await;
        });
    }

    async fn replace_discovery_task(&self) {
        let me = self.clone();
        let handle = tokio::spawn(async move {
            me.try_discover().await;
        });
        let mut slot = self.state.discovery.lock().await;
        if let Some(prev) = slot.take() {
            prev.abort();
        }
        *slot = Some(handle);
    }

    async fn run_discover(&self) -> Result<(bool, DiscoveryMode)> {
        let (client, mode, port) = {
            let osc = self.state.oscquery.read().await;
            (osc.client(), osc.discovery_mode(), osc.fallback_port())
        };

        let found = VrchatOscQuery::locate(&client, mode, port).await?;
        let mut osc = self.state.oscquery.write().await;
        osc.set_address(found.clone());
        Ok((found.is_some(), mode))
    }

    /// Send an OSC float parameter back to VRChat (e.g. haptic feedback level).
    pub async fn send_param(&self, param: &str, value: f32) -> Result<()> {
        let osc = self.state.oscquery.read().await;
        let addr = osc
            .get_address()
            .ok_or_else(|| DGLabError::OscError("VRChat address unknown".to_string()))?;

        let target = format!("{}:{}", addr.osc_ip, addr.osc_port);
        drop(osc);

        let msg = rosc::encoder::encode(&OscPacket::Message(OscMessage {
            addr: format!("/avatar/parameters/{param}"),
            args: vec![OscType::Float(value)],
        }))
        .map_err(|e| DGLabError::OscError(e.to_string()))?;

        let socket = UdpSocket::bind("0.0.0.0:0").await?;
        socket.send_to(&msg, &target).await?;
        Ok(())
    }

    /// Snapshot of all zones seen so far (level may be 0.0 if no active contact).
    pub async fn zones(&self) -> Vec<ZoneEvent> {
        self.state.devices.read().await.values().map(|d| d.to_event()).collect()
    }

    /// Return the VRChat OSC address if already discovered.
    pub async fn vrchat_address(&self) -> Option<crate::osc::VrchatAddress> {
        self.state.oscquery.read().await.get_address().cloned()
    }

    async fn run_listener(&self) -> Result<()> {
        let port = *self.state.port.read().await;
        let bind_addr = format!("0.0.0.0:{port}");
        let socket = UdpSocket::bind(&bind_addr).await?;
        log::info!("OSC listener bound to {bind_addr}");

        let mut buf = vec![0u8; 65_535];
        loop {
            let (len, _src) = socket.recv_from(&mut buf).await?;
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

    async fn run_oscquery_poller(&self) {
        let mut consecutive_failures = 0u32;
        loop {
            let ok = self.poll_oscquery_once().await;
            if ok {
                consecutive_failures = 0;
            } else {
                consecutive_failures = consecutive_failures.saturating_add(1);
                if consecutive_failures == 1 || consecutive_failures % 40 == 0 {
                    let busy = self
                        .state
                        .discovery
                        .lock()
                        .await
                        .as_ref()
                        .is_some_and(|h| !h.is_finished());
                    if !busy {
                        self.replace_discovery_task().await;
                    }
                }
            }
            tokio::time::sleep(OSCQUERY_POLL_INTERVAL).await;
        }
    }

    async fn poll_oscquery_once(&self) -> bool {
        let osc = self.state.oscquery.read().await;
        if osc.get_address().is_none() {
            return false;
        }
        let bulk_result = osc.get_bulk().await;
        drop(osc);

        let params = match bulk_result {
            Ok(p) => p,
            Err(_) => return false,
        };

        let mut new_zones: HashSet<(OldZoneType, String)> = HashSet::new();
        for (path, _) in &params {
            if let Some(param) = path.strip_prefix("/avatar/parameters/") {
                let parts: Vec<&str> = param.split('/').collect();
                if let Some((zone_type, id, _, _)) = parse_sps_param(&parts) {
                    new_zones.insert((zone_type, id));
                }
            }
        }

        {
            let mut prev = self.state.last_bulk_zones.lock().await;
            if !prev.is_empty() && !new_zones.is_empty() && prev.is_disjoint(&new_zones) {
                log::info!("OSCQuery: avatar zone set changed — clearing cache");
                self.state.devices.write().await.clear();
            }
            *prev = new_zones;
        }

        for (path, value) in params {
            if let Some(param) = path.strip_prefix("/avatar/parameters/") {
                self.received_param(param, value).await;
            }
        }
        true
    }

    async fn handle_message(&self, msg: OscMessage) {
        if msg.addr == "/avatar/change" {
            let avatar_id = msg.args.iter().find_map(|a| match a {
                OscType::String(s) => Some(s.clone()),
                _ => None,
            });
            log::info!(
                "Avatar changed{} — clearing zone cache",
                avatar_id
                    .as_ref()
                    .map(|id| format!(" ({id})"))
                    .unwrap_or_default()
            );
            self.state.devices.write().await.clear();
            self.state.last_bulk_zones.lock().await.clear();

            let mode = self.discovery_mode().await;
            if mode == DiscoveryMode::Osc {
                let me = self.clone();
                tokio::spawn(async move {
                    me.apply_local_avatar_config(avatar_id.as_deref()).await;
                });
            } else {
                self.rediscover_background();
            }
            return;
        }

        let Some(param) = msg.addr.strip_prefix("/avatar/parameters/") else {
            return;
        };
        let Some(value) = extract_osc_value(&msg.args) else {
            return;
        };
        self.received_param(param, value).await;
    }

    // Internal — parameter routing=
    async fn received_param(&self, param: &str, value: OscValue) {
        let parts: Vec<&str> = param.split('/').collect();
        let Some((zone_type, id, contact, is_tps)) = parse_sps_param(&parts) else {
            return;
        };

        let key = (zone_type.clone(), id.clone());
        let ukf_params = *self.state.ukf_params.read().await;
        let mut devices = self.state.devices.write().await;
        let device = devices
            .entry(key)
            .or_insert_with(|| GameDevice::with_ukf_params(zone_type, id, is_tps, ukf_params));
        device.set_value(&contact, value);
        let event = device.to_event();
        drop(devices);

        let _ = self.state.event_tx.send(event);
    }

    // Internal — VRChat OSCQuery discovery & bulk fetch
    async fn try_discover(&self) {
        match self.run_discover().await {
            Ok((true, DiscoveryMode::OscQuery)) => {
                self.update_bulk().await;
            }
            Ok((true, DiscoveryMode::Osc)) => {
                self.apply_local_avatar_config(None).await;
            }
            Ok((false, _)) => {
                log::debug!("VRChat not found during discovery");
            }
            Err(e) => {
                log::warn!("Discovery error: {e}");
            }
        }
    }

    async fn apply_local_avatar_config(&self, avatar_id: Option<&str>) {
        let override_dir = self.state.osc_avatars_dir.read().await.clone();
        let override_ref = override_dir.as_deref();

        let loaded = match avatar_id {
            Some(id) if !id.is_empty() => load_avatar_param_paths(id, override_ref),
            _ => load_latest_avatar_param_paths(override_ref),
        };

        let (label, paths) = match loaded {
            Ok(v) => v,
            Err(e) => {
                log::warn!(
                    "[osc] Local avatar config: {e} — zones will appear when contacts fire"
                );
                return;
            }
        };

        let mut seeded = 0usize;
        for path in &paths {
            let parts: Vec<&str> = path.split('/').collect();
            if parse_sps_param(&parts).is_none() {
                continue;
            }
            self.received_param(path, OscValue::Float(0.0)).await;
            seeded += 1;
        }

        log::info!(
            "[osc] Seeded {seeded} contact params from local config ({label})"
        );

        let zones = self.zones().await;
        let _ = self.state.refresh_tx.send(zones);
    }

    async fn update_bulk(&self) {
        let osc = self.state.oscquery.read().await;
        let bulk_result = osc.get_bulk().await;
        drop(osc);

        let params = match bulk_result {
            Ok(p) => {
                log::debug!("Bulk OSCQuery: {} parameters received", p.len());
                p
            }
            Err(e) => {
                log::warn!("OSCQuery bulk fetch failed: {e}");
                return;
            }
        };

        for (path, value) in params {
            if let Some(param) = path.strip_prefix("/avatar/parameters/") {
                self.received_param(param, value).await;
            }
        }

        // Notify subscribers that a fresh zone list is available
        let zones = self.zones().await;
        let _ = self.state.refresh_tx.send(zones);
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
