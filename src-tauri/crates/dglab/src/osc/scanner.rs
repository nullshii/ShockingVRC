use std::collections::HashMap;
use std::sync::Arc;

use rosc::{OscMessage, OscPacket, OscType};
use tokio::net::UdpSocket;
use tokio::sync::{RwLock, broadcast};

use super::game_device::GameDevice;
use super::oscquery::VrchatOscQuery;
use super::types::{OscValue, ZoneEvent, ZoneType};
use crate::dsp::UkfParams;
use crate::error::{DGLabError, Result};

// Internal shared state
struct ScannerState {
    devices: RwLock<HashMap<(ZoneType, String), GameDevice>>,
    event_tx: broadcast::Sender<ZoneEvent>,
    /// Fired after every completed bulk-fetch — contains full zone snapshot.
    /// Subscribers use this to know "avatar zones are now known".
    refresh_tx: broadcast::Sender<Vec<ZoneEvent>>,
    oscquery: RwLock<VrchatOscQuery>,
    port: u16,
    ukf_params: RwLock<UkfParams>,
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
                oscquery: RwLock::new(VrchatOscQuery::new()),
                port,
                ukf_params: RwLock::new(UkfParams::default()),
            }),
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

    /// Start the background **OSC UDP listener** only.
    ///
    /// VRChat discovery is intentionally *not* started here — call
    /// [`discover_wait`] once after `start()` to perform the first discovery.
    /// Subsequent avatar changes are handled automatically by the listener.
    pub async fn start(&self) -> Result<()> {
        let me = self.clone();
        tokio::spawn(async move {
            if let Err(e) = me.run_listener().await {
                log::error!("OSC listener stopped: {e}");
            }
        });
        Ok(())
    }

    /// Discover VRChat via mDNS + OSCQuery and bulk-fetch avatar parameters
    /// **in the current task** (mDNS scan blocks up to ~5 s).
    /// On success emits a refresh event (see [`subscribe_refreshes`]).
    /// Returns `true` when VRChat was found.
    pub async fn discover_wait(&self) -> Result<bool> {
        let mut osc = self.state.oscquery.write().await;
        let found = osc.discover().await?;
        drop(osc);
        if found {
            self.update_bulk().await;
        }
        Ok(found)
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

    // Internal — OSC listener loop
    async fn run_listener(&self) -> Result<()> {
        let bind_addr = format!("0.0.0.0:{}", self.state.port);
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

    async fn handle_message(&self, msg: OscMessage) {
        if msg.addr == "/avatar/change" {
            log::info!("Avatar changed — clearing zone cache");
            self.state.devices.write().await.clear();

            // Re-discover in background; on success update_bulk() fires refresh_tx
            let me = self.clone();
            tokio::spawn(async move {
                me.try_discover().await;
            });
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
        let device = devices.entry(key).or_insert_with(|| {
            GameDevice::with_ukf_params(zone_type, id, is_tps, ukf_params)
        });
        device.set_value(&contact, value);
        let event = device.to_event();
        drop(devices);

        let _ = self.state.event_tx.send(event);
    }

    // Internal — VRChat OSCQuery discovery & bulk fetch
    async fn try_discover(&self) {
        let mut osc = self.state.oscquery.write().await;
        match osc.discover().await {
            Ok(true) => {
                drop(osc);
                self.update_bulk().await;
            }
            Ok(false) => {
                log::debug!("VRChat not found during OSCQuery scan");
            }
            Err(e) => {
                log::warn!("OSCQuery discovery error: {e}");
            }
        }
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
fn parse_sps_param(parts: &[&str]) -> Option<(ZoneType, String, String, bool)> {
    match parts {
        // DGB: flat zone — value IS the level
        ["DGB", name] => Some((ZoneType::DGB, name.to_string(), "Value".to_string(), false)),

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

fn parse_zone_type(s: &str) -> Option<ZoneType> {
    match s {
        "Pen" => Some(ZoneType::Pen),
        "Orf" => Some(ZoneType::Orf),
        "Touch" => Some(ZoneType::Touch),
        _ => None,
    }
}
