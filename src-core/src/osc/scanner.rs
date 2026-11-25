use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use rosc::{OscMessage, OscPacket, OscType};
use tokio::net::UdpSocket;
use tokio::sync::{Mutex, RwLock, broadcast};
use tokio::task::JoinHandle;

use super::game_device::GameDevice;
use super::oscquery::{
    AvatarSchema, MAX_PARAMS, MdnsBroadcaster, OscQueryServer, SchemaAccept, SchemaStore, VrchatAddress, VrchatClient,
    select_port,
};
use super::output::{ChatboxFrame, OscOutFrame, OscOutput, OutputNodeStatus, OutputSource};
use super::types::{OldZoneType, OscValue, ZoneEvent};
use crate::dsp::UkfParams;
use crate::error::{DGLabError, Result};

const STALE_MS: Duration = Duration::from_millis(15_000);

const BULK_DEBOUNCE: Duration = Duration::from_millis(500);
const BULK_RETRY_MIN: Duration = Duration::from_millis(250);
const BULK_RETRY_MAX: Duration = Duration::from_secs(5);
const BULK_MAX_ATTEMPTS: u32 = 20;
const AVATAR_CHANGE_FOLLOWUP: Duration = Duration::from_secs(2);

const OGB_ENABLED_PARAM: &str = "OGB_ENABLED";
const OGB_ENABLED_INTERVAL: Duration = Duration::from_secs(5);

#[derive(Debug, Clone)]
pub struct RawContact {
    pub zone_type: OldZoneType,
    pub id: String,
    pub level: f32,
    pub updated: Instant,
    pub blocked: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct ParamSample {
    pub value: f32,
    pub updated: Instant,
}

#[derive(Debug, Default)]
pub struct RawInputs {
    pub contacts: Vec<RawContact>,
    pub params: HashMap<String, ParamSample>,
}

impl RawInputs {
    fn record_param(&mut self, addr: &str, value: f32, only_if_new: bool, now: Instant) {
        if let Some(p) = self.params.get_mut(addr) {
            if !only_if_new {
                *p = ParamSample { value, updated: now };
            }
        } else if self.params.len() < MAX_PARAMS {
            self.params
                .insert(addr.to_string(), ParamSample { value, updated: now });
        }
    }

    fn record_contact(&mut self, zone_type: OldZoneType, id: &str, level: f32, now: Instant) {
        use crate::cli::engine::{ENTRY_JUMP_LIMIT, RELEASE_LEVEL, ZONE_IDLE_TIMEOUT};
        let level = if level.is_finite() { level.clamp(0.0, 1.0) } else { 0.0 };
        match self
            .contacts
            .iter_mut()
            .find(|c| c.zone_type == zone_type && c.id == id)
        {
            Some(c) => {
                let was_idle = now.saturating_duration_since(c.updated) > ZONE_IDLE_TIMEOUT || c.level <= RELEASE_LEVEL;
                c.blocked = if level <= RELEASE_LEVEL {
                    false
                } else if was_idle {
                    level > ENTRY_JUMP_LIMIT
                } else {
                    c.blocked
                };
                c.level = level;
                c.updated = now;
            }
            None => self.contacts.push(RawContact {
                zone_type,
                id: id.to_string(),
                level,
                updated: now,
                blocked: level > ENTRY_JUMP_LIMIT,
            }),
        }
    }
}

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
    addr_watcher: Mutex<Option<JoinHandle<()>>>,
    keepalive: Mutex<Option<JoinHandle<()>>>,
    send_socket: Arc<tokio::sync::OnceCell<UdpSocket>>,
    ukf_params: RwLock<UkfParams>,
    epoch: Instant,
    last_packet_ms: AtomicU64,
    has_received: AtomicBool,
    bulk_attempt: AtomicU64,
    raw: std::sync::RwLock<RawInputs>,
    schema: std::sync::RwLock<SchemaStore>,
    output: OscOutput,
}

#[derive(Clone)]
pub struct AvatarScanner {
    state: Arc<ScannerState>,
}

impl AvatarScanner {
    pub fn new(port: Option<u16>) -> Self {
        let (event_tx, _) = broadcast::channel(256);
        let (refresh_tx, _) = broadcast::channel(16);
        let send_socket = Arc::new(tokio::sync::OnceCell::new());
        let output = OscOutput::new(Arc::clone(&send_socket));
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
                addr_watcher: Mutex::new(None),
                keepalive: Mutex::new(None),
                send_socket,
                ukf_params: RwLock::new(UkfParams::default()),
                epoch: Instant::now(),
                last_packet_ms: AtomicU64::new(0),
                has_received: AtomicBool::new(false),
                bulk_attempt: AtomicU64::new(0),
                raw: std::sync::RwLock::new(RawInputs::default()),
                schema: std::sync::RwLock::new(SchemaStore::default()),
                output,
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
        self.state.output.reset_shutdown();
        self.state.output.ensure_worker();
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

        let client = Arc::new(VrchatClient::start());
        let mut addr_rx = client.subscribe_address();
        *self.state.client.lock().await = Some(client);

        let me = self.clone();
        let watcher = tokio::spawn(async move {
            while addr_rx.changed().await.is_ok() {
                let addr = addr_rx.borrow_and_update().clone();
                me.on_vrchat_address_changed(addr);
            }
        });
        *self.state.addr_watcher.lock().await = Some(watcher);

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
        if let Some(handle) = self.state.addr_watcher.lock().await.take() {
            handle.abort();
        }
        if let Some(client) = self.state.client.lock().await.take() {
            client.shutdown();
        }
        if let Some(handle) = self.state.keepalive.lock().await.take() {
            handle.abort();
        }
        self.state.bulk_attempt.fetch_add(1, Ordering::Relaxed);
        self.state.output.set_vrchat_address(None);
    }

    pub async fn probe_vrchat_endpoint(&self, http_addr: std::net::SocketAddr) -> bool {
        let Some(client) = self.state.client.lock().await.clone() else {
            return false;
        };
        client.probe_endpoint(http_addr).await
    }

    pub fn avatar_schema(&self) -> Arc<AvatarSchema> {
        self.with_schema(|s| s.snapshot())
    }

    pub fn refresh_schema(&self) {
        self.trigger_bulk(None, false);
    }

    pub fn submit_outputs(
        &self,
        source: OutputSource,
        osc: Vec<OscOutFrame>,
        chat: Vec<ChatboxFrame>,
        stop_param: Option<&str>,
    ) {
        self.state.output.submit(source, osc, chat, stop_param);
    }

    pub fn release_outputs(&self, source: OutputSource) {
        self.state.output.release(source);
    }

    pub fn output_statuses(&self, source: OutputSource) -> BTreeMap<u64, OutputNodeStatus> {
        self.state.output.statuses(source)
    }

    pub async fn shutdown_outputs(&self, budget: Duration) {
        self.state.output.shutdown_and_flush(budget).await;
    }

    fn on_vrchat_address_changed(&self, addr: Option<VrchatAddress>) {
        self.state.output.set_vrchat_address(addr.clone());
        self.with_schema_mut(|s| {
            s.clear_and_bump();
        });
        self.state.output.set_schema(self.avatar_schema());
        if addr.is_some() {
            self.trigger_bulk(None, false);
        }
    }

    fn with_schema<R>(&self, f: impl FnOnce(&SchemaStore) -> R) -> R {
        match self.state.schema.read() {
            Ok(s) => f(&s),
            Err(poisoned) => f(&poisoned.into_inner()),
        }
    }

    fn with_schema_mut<R>(&self, f: impl FnOnce(&mut SchemaStore) -> R) -> R {
        match self.state.schema.write() {
            Ok(mut s) => f(&mut s),
            Err(poisoned) => f(&mut poisoned.into_inner()),
        }
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
        encode_and_send(&self.state.send_socket, &addr, format!("/avatar/parameters/{param}"), vec![arg]).await
    }

    async fn send_ogb_enabled(&self) {
        if let Err(e) = self.send_param_bool(OGB_ENABLED_PARAM, true).await {
            log::trace!("OGB_ENABLED send skipped: {e}");
        }
    }

    pub fn with_raw_inputs<R>(&self, f: impl FnOnce(&RawInputs) -> R) -> R {
        match self.state.raw.read() {
            Ok(raw) => f(&raw),
            Err(poisoned) => f(&poisoned.into_inner()),
        }
    }

    pub fn seen_params(&self) -> Vec<(String, f32)> {
        let mut v: Vec<(String, f32)> =
            self.with_raw_inputs(|raw| raw.params.iter().map(|(k, p)| (k.clone(), p.value)).collect());
        v.sort_by(|a, b| a.0.cmp(&b.0));
        v
    }

    fn with_raw_mut(&self, f: impl FnOnce(&mut RawInputs)) {
        match self.state.raw.write() {
            Ok(mut raw) => f(&mut raw),
            Err(poisoned) => f(&mut poisoned.into_inner()),
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
            self.trigger_bulk(None, true);
        }

        if msg.addr == "/avatar/change" {
            log::info!("Avatar changed — clearing zone cache and schema");
            let avatar_id = match msg.args.first() {
                Some(OscType::String(s)) if !s.is_empty() => Some(s.clone()),
                _ => None,
            };
            self.state.devices.write().await.clear();
            self.with_raw_mut(|raw| {
                raw.contacts.clear();
                raw.params.clear();
            });
            self.with_schema_mut(|s| {
                s.clear_and_bump();
            });
            self.state.output.on_avatar_change();
            self.state.output.set_schema(self.avatar_schema());
            let _ = self.state.refresh_tx.send(Vec::new());
            self.trigger_bulk(avatar_id, true);
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
        let now = Instant::now();
        self.with_raw_mut(|raw| raw.record_param(&msg.addr, value.as_float(), false, now));
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
        let raw_level = device.compute_level();
        drop(devices);
        let now = Instant::now();
        self.with_raw_mut(|raw| raw.record_contact(event.zone_type, &event.id, raw_level, now));

        let _ = self.state.event_tx.send(event);
        if is_new_zone {
            let zones = self.zones().await;
            let _ = self.state.refresh_tx.send(zones);
        }
    }

    fn trigger_bulk(&self, expected_avatar: Option<String>, debounce: bool) {
        let attempt = self.state.bulk_attempt.fetch_add(1, Ordering::Relaxed) + 1;
        let request_epoch = self.with_schema(|s| s.epoch());
        let me = self.clone();
        tokio::spawn(async move {
            if debounce {
                tokio::time::sleep(BULK_DEBOUNCE).await;
            }

            let mut delay = BULK_RETRY_MIN;
            for try_no in 1..=BULK_MAX_ATTEMPTS {
                if me.state.bulk_attempt.load(Ordering::Relaxed) != attempt
                    || me.with_schema(|s| s.epoch()) != request_epoch
                {
                    return;
                }
                match me.fetch_bulk(request_epoch, expected_avatar.as_deref()).await {
                    Ok(SchemaAccept::Accepted | SchemaAccept::Stale) => return,
                    Ok(SchemaAccept::AcceptedNeedsFollowUp) => {
                        tokio::time::sleep(AVATAR_CHANGE_FOLLOWUP).await;
                        if me.state.bulk_attempt.load(Ordering::Relaxed) == attempt
                            && me.with_schema(|s| s.epoch()) == request_epoch
                        {
                            let _ = me.fetch_bulk(request_epoch, None).await;
                        }
                        return;
                    }
                    Ok(SchemaAccept::AvatarMismatch) => {
                        log::debug!("Bulk OSCQuery schema still on previous avatar (try {try_no}/{BULK_MAX_ATTEMPTS})");
                        tokio::time::sleep(delay).await;
                        delay = (delay * 2).min(BULK_RETRY_MAX);
                    }
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

    async fn fetch_bulk(&self, request_epoch: u64, expected_avatar: Option<&str>) -> Result<SchemaAccept> {
        let Some(client) = self.state.client.lock().await.clone() else {
            return Err(DGLabError::OscError("scanner stopped".into()));
        };
        let (params, schema) = client.get_bulk_with_schema(request_epoch).await?;

        let outcome = self.with_schema_mut(|s| s.accept(request_epoch, expected_avatar, schema));
        match outcome {
            SchemaAccept::Stale | SchemaAccept::AvatarMismatch => return Ok(outcome),
            SchemaAccept::Accepted | SchemaAccept::AcceptedNeedsFollowUp => {
                self.state.output.set_schema(self.avatar_schema());
            }
        }

        log::debug!("Bulk OSCQuery: {} values received", params.len());
        for (path, value) in params {
            let now = Instant::now();
            self.with_raw_mut(|raw| raw.record_param(&path, value.as_float(), true, now));
            if let Some(param) = path.strip_prefix("/avatar/parameters/") {
                self.received_param(param, value, true).await;
            }
        }

        let zones = self.zones().await;
        let _ = self.state.refresh_tx.send(zones);
        Ok(outcome)
    }
}

pub(crate) fn osc_target(addr: &VrchatAddress) -> String {
    let ip = if addr.osc_ip == "0.0.0.0" {
        "127.0.0.1"
    } else {
        addr.osc_ip.as_str()
    };
    format!("{ip}:{}", addr.osc_port)
}

pub(crate) async fn encode_and_send(
    socket_cell: &tokio::sync::OnceCell<UdpSocket>,
    addr: &VrchatAddress,
    osc_addr: String,
    args: Vec<OscType>,
) -> Result<()> {
    let target = osc_target(addr);
    let msg = rosc::encoder::encode(&OscPacket::Message(OscMessage { addr: osc_addr, args }))
        .map_err(|e| DGLabError::OscError(e.to_string()))?;

    let socket = socket_cell.get_or_try_init(|| UdpSocket::bind("0.0.0.0:0")).await?;
    socket.send_to(&msg, &target).await?;
    Ok(())
}

pub fn is_sps_param(param: &str) -> bool {
    let parts: Vec<&str> = param.split('/').collect();
    parse_sps_param(&parts).is_some()
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
/// | `DGB/<name>` | `DGB/TouchAreaA`, `Control/Contact/DGB/FrontR` |
fn parse_sps_param(parts: &[&str]) -> Option<(OldZoneType, String, String, bool)> {
    parts
        .iter()
        .enumerate()
        .filter(|(_, seg)| matches!(**seg, "DGB" | "OGB" | "TPS_Internal" | "VFH"))
        .find_map(|(i, _)| parse_sps_tail(&parts[i..]))
}

fn parse_sps_tail(parts: &[&str]) -> Option<(OldZoneType, String, String, bool)> {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(param: &str) -> Option<(OldZoneType, String, String, bool)> {
        let parts: Vec<&str> = param.split('/').collect();
        parse_sps_param(&parts)
    }

    #[test]
    fn parses_flat_dgb() {
        assert_eq!(
            parse("DGB/TouchAreaA"),
            Some((OldZoneType::DGB, "TouchAreaA".into(), "Value".into(), false))
        );
    }

    #[test]
    fn parses_prefixed_dgb() {
        let expected = Some((OldZoneType::DGB, "FrontR".into(), "Value".into(), false));
        assert_eq!(parse("Contact/DGB/FrontR"), expected);
        assert_eq!(parse("Control/Contact/DGB/FrontR"), expected);
        assert_eq!(parse("a/b/c/d/e/DGB/FrontR"), expected);
    }

    #[test]
    fn parses_prefixed_ogb_and_tps() {
        assert_eq!(
            parse("Control/OGB/Pen/Cock/PenOthers"),
            Some((OldZoneType::Pen, "Cock".into(), "PenOthers".into(), false))
        );
        assert_eq!(
            parse("Control/TPS_Internal/Orf/Anal/Depth_In"),
            Some((OldZoneType::Orf, "Anal".into(), "Depth_In".into(), true))
        );
        assert_eq!(
            parse("Control/VFH/Zone/Touch/Chest/TouchOthers"),
            Some((OldZoneType::Touch, "Chest".into(), "TouchOthers".into(), false))
        );
    }

    #[test]
    fn skips_markers_that_do_not_parse() {
        assert_eq!(
            parse("DGB/OGB/Pen/Cock/PenOthers"),
            Some((OldZoneType::Pen, "Cock".into(), "PenOthers".into(), false))
        );
    }

    #[test]
    fn raw_contacts_keep_the_unfiltered_level_and_guard_jumps() {
        let mut raw = RawInputs::default();
        let t0 = Instant::now();
        raw.record_contact(OldZoneType::Pen, "Cock", 0.9, t0);
        assert!(raw.contacts[0].blocked, "idle → 90% in one update is suspicious");
        raw.record_contact(OldZoneType::Pen, "Cock", 0.0, t0);
        assert!(!raw.contacts[0].blocked, "release re-arms");
        raw.record_contact(OldZoneType::Pen, "Cock", 0.2, t0);
        raw.record_contact(OldZoneType::Pen, "Cock", 0.95, t0);
        assert!(!raw.contacts[0].blocked, "a ramp passes");
        assert_eq!(raw.contacts[0].level, 0.95, "no filtering");
        raw.record_contact(OldZoneType::Pen, "Cock", f32::NAN, t0);
        assert_eq!(raw.contacts[0].level, 0.0);
    }

    #[test]
    fn params_are_remembered_by_full_address() {
        let mut raw = RawInputs::default();
        let t0 = Instant::now();
        raw.record_param("/avatar/parameters/Foo", 0.5, false, t0);
        raw.record_param("/avatar/parameters/Foo", 0.9, true, t0);
        assert_eq!(
            raw.params["/avatar/parameters/Foo"].value, 0.5,
            "bulk sync never overwrites live data"
        );
        raw.record_param("/avatar/parameters/Foo", 0.7, false, t0);
        assert_eq!(raw.params["/avatar/parameters/Foo"].value, 0.7);
    }

    #[test]
    fn rejects_unrelated_params() {
        assert_eq!(parse("VelocityX"), None);
        assert_eq!(parse("Control/EQ/EQShow"), None);
        assert_eq!(parse("Control/Contact/Front_ON"), None);
        assert_eq!(parse("DGB"), None);
        assert_eq!(parse("DGB/Front/R"), None);
        assert_eq!(parse("Control/OGB/Bogus/Cock/PenOthers"), None);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn end_to_end_oscquery_discovery_sends_typed_osc_and_rest_on_stop() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::TcpListener;

        let udp = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let udp_port = udp.local_addr().unwrap().port();

        let tcp = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let http_addr = tcp.local_addr().unwrap();

        let host_info = serde_json::json!({
            "NAME": "VRChat-Client-E2ETest",
            "OSC_IP": "0.0.0.0",
            "OSC_PORT": udp_port,
            "OSC_TRANSPORT": "UDP"
        })
        .to_string();
        let tree = serde_json::json!({
            "FULL_PATH": "/",
            "ACCESS": 0,
            "CONTENTS": {
                "avatar": {
                    "FULL_PATH": "/avatar",
                    "ACCESS": 0,
                    "CONTENTS": {
                        "parameters": {
                            "FULL_PATH": "/avatar/parameters",
                            "ACCESS": 0,
                            "CONTENTS": {
                                "Glow": {
                                    "FULL_PATH": "/avatar/parameters/Glow",
                                    "TYPE": "f",
                                    "ACCESS": 3,
                                    "VALUE": [0.0]
                                }
                            }
                        }
                    }
                }
            }
        })
        .to_string();

        let server = tokio::spawn(async move {
            loop {
                let Ok((mut stream, _)) = tcp.accept().await else { break };
                let hi = host_info.clone();
                let tr = tree.clone();
                tokio::spawn(async move {
                    let mut buf = Vec::with_capacity(512);
                    let mut tmp = [0u8; 512];
                    loop {
                        match stream.read(&mut tmp).await {
                            Ok(0) | Err(_) => break,
                            Ok(n) => {
                                buf.extend_from_slice(&tmp[..n]);
                                if buf.windows(4).any(|w| w == b"\r\n\r\n") || buf.len() > 8192 {
                                    break;
                                }
                            }
                        }
                    }
                    let req = String::from_utf8_lossy(&buf);
                    let body = if req.contains("HOST_INFO") { hi } else { tr };
                    let resp = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    let _ = stream.write_all(resp.as_bytes()).await;
                    let _ = stream.flush().await;
                    let _ = stream.shutdown().await;
                });
            }
        });

        let scanner = AvatarScanner::new(Some(0));
        scanner.start().await.unwrap();
        assert!(scanner.probe_vrchat_endpoint(http_addr).await);

        for _ in 0..200 {
            if scanner.avatar_schema().params.contains_key("/avatar/parameters/Glow") {
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        assert!(
            scanner.avatar_schema().params.contains_key("/avatar/parameters/Glow"),
            "schema populated from fake OSCQuery HTTP server"
        );

        let frame = OscOutFrame {
            flat: 1,
            root: 1,
            address: "/avatar/parameters/Glow".into(),
            ty: crate::osc::output::OscOutType::Auto,
            map: false,
            min: -1.0,
            max: 1.0,
            rest: 0.125,
            rate_hz: 10.0,
            value: 0.75,
            enable: true,
            active: true,
            refused: None,
        };

        scanner.submit_outputs(OutputSource::Local, vec![frame.clone()], vec![], None);
        tokio::time::sleep(Duration::from_millis(430)).await;
        scanner.submit_outputs(OutputSource::Local, vec![frame], vec![], None);

        async fn recv_glow(udp: &UdpSocket, stage: &str) -> OscMessage {
            let mut buf = [0u8; 1024];
            let deadline = Instant::now() + Duration::from_secs(2);
            loop {
                let rem = deadline.saturating_duration_since(Instant::now());
                assert!(!rem.is_zero(), "timed out waiting for /avatar/parameters/Glow ({stage})");
                let (len, _) = tokio::time::timeout(rem, udp.recv_from(&mut buf))
                    .await
                    .unwrap_or_else(|_| panic!("udp timeout ({stage})"))
                    .unwrap();
                let (_, pkt) = rosc::decoder::decode_udp(&buf[..len]).unwrap();
                for msg in flatten_packet(pkt) {
                    if msg.addr == "/avatar/parameters/Glow" {
                        return msg;
                    }
                }
            }
        }

        let live_msg = recv_glow(&udp, "live").await;
        assert_eq!(live_msg.addr, "/avatar/parameters/Glow");
        assert_eq!(live_msg.args, vec![OscType::Float(0.75)]);

        scanner.shutdown_outputs(Duration::from_millis(300)).await;
        let rest_msg = recv_glow(&udp, "rest").await;
        assert_eq!(rest_msg.addr, "/avatar/parameters/Glow");
        assert_eq!(rest_msg.args, vec![OscType::Float(0.125)]);

        scanner.stop().await;
        server.abort();
    }
}
