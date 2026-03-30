use std::collections::HashMap;
use std::net::SocketAddr;
use std::time::Duration;

use serde::Deserialize;

use super::types::OscValue;
use crate::error::{DGLabError, Result};

// Public address info
/// Addresses needed to talk to VRChat's OSC / OSCQuery endpoints.
#[derive(Debug, Clone)]
pub struct VrchatAddress {
    /// IP to use when sending OSC packets to VRChat.
    pub osc_ip: String,
    /// UDP port VRChat is listening on for incoming OSC.
    pub osc_port: u16,
    /// HTTP socket of VRChat's OSCQuery server.
    pub http_addr: SocketAddr,
}

// JSON shapes for OSCQuery HTTP responses
#[derive(Debug, Deserialize)]
struct HostInfo {
    #[serde(rename = "NAME")]
    name: String,
    #[serde(rename = "OSC_IP")]
    osc_ip: String,
    #[serde(rename = "OSC_PORT")]
    osc_port: u16,
}

/// Recursive OSCQuery node (only the fields we care about).
#[derive(Debug, Deserialize)]
struct OscQueryNode {
    #[serde(rename = "FULL_PATH")]
    full_path: Option<String>,
    #[serde(rename = "VALUE")]
    value: Option<Vec<serde_json::Value>>,
    #[serde(rename = "CONTENTS")]
    contents: Option<HashMap<String, OscQueryNode>>,
}

// Internal mDNS result
struct MdnsCandidate {
    http_addr: SocketAddr,
}


// VrchatOscQuery
/// Discovers VRChat over mDNS + OSCQuery and fetches avatar parameter trees.
pub struct VrchatOscQuery {
    client: reqwest::Client,
    address: Option<VrchatAddress>,
}

impl VrchatOscQuery {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
            address: None,
        }
    }

    pub fn get_address(&self) -> Option<&VrchatAddress> {
        self.address.as_ref()
    }

    /// Scan mDNS for `_oscjson._tcp.local.` services, probe each via
    /// `HOST_INFO`, and keep the first that identifies itself as VRChat.
    /// Falls back to `localhost:9001` if mDNS yields nothing.
    /// Returns `true` when VRChat was successfully located.
    pub async fn discover(&mut self) -> Result<bool> {
        let candidates = tokio::task::spawn_blocking(scan_mdns)
            .await
            .map_err(|e| DGLabError::OscError(format!("spawn_blocking failed: {e}")))?;

        for candidate in candidates {
            match self.check_candidate(&candidate).await {
                Ok(Some(addr)) => {
                    log::info!(
                        "VRChat OSCQuery found at {} (OSC {}:{})",
                        candidate.http_addr,
                        addr.osc_ip,
                        addr.osc_port
                    );
                    self.address = Some(addr);
                    return Ok(true);
                }
                Ok(None) => {}
                Err(e) => {
                    log::debug!("OSCQuery candidate {} rejected: {}", candidate.http_addr, e);
                }
            }
        }

        Ok(false)
    }

    async fn check_candidate(&self, candidate: &MdnsCandidate) -> Result<Option<VrchatAddress>> {
        let url = format!("http://{}/?HOST_INFO", candidate.http_addr);
        let resp = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| DGLabError::OscError(e.to_string()))?;

        let info: HostInfo = resp
            .json()
            .await
            .map_err(|e| DGLabError::OscError(e.to_string()))?;

        if !info.name.starts_with("VRChat-Client-") {
            return Ok(None);
        }

        Ok(Some(VrchatAddress {
            osc_ip: info.osc_ip,
            osc_port: info.osc_port,
            http_addr: candidate.http_addr,
        }))
    }

    /// Fetch the full `/avatar/parameters` tree from VRChat's OSCQuery server
    /// and flatten it into `(full_osc_path, value)` pairs.
    pub async fn get_bulk(&self) -> Result<Vec<(String, OscValue)>> {
        let addr = self
            .address
            .as_ref()
            .ok_or_else(|| DGLabError::OscError("VRChat not yet discovered".to_string()))?;

        let url = format!("http://{}/avatar/parameters", addr.http_addr);
        let resp = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| DGLabError::OscError(e.to_string()))?;

        let node: OscQueryNode = resp
            .json()
            .await
            .map_err(|e| DGLabError::OscError(e.to_string()))?;

        let mut params = Vec::new();
        collect_params(&node, &mut params);
        Ok(params)
    }
}

impl Default for VrchatOscQuery {
    fn default() -> Self {
        Self::new()
    }
}

// mDNS scanning (blocking, intended for spawn_blocking)
/// Scan mDNS for `_oscjson._tcp.local.` for up to 5 seconds.
/// If nothing is found, appends the VRChat default `localhost:9001` as a
/// fallback so basic local testing works without a working mDNS stack.
fn scan_mdns() -> Vec<MdnsCandidate> {
    let mut candidates = Vec::new();

    match try_scan_mdns(&mut candidates) {
        Ok(()) => {}
        Err(e) => log::warn!("mDNS scan failed: {}", e),
    }

    if candidates.is_empty() {
        log::debug!("mDNS found nothing; falling back to localhost:9001");
        if let Ok(addr) = "127.0.0.1:9001".parse() {
            candidates.push(MdnsCandidate { http_addr: addr });
        }
    }

    candidates
}

fn try_scan_mdns(out: &mut Vec<MdnsCandidate>) -> std::result::Result<(), String> {
    use mdns_sd::{ServiceDaemon, ServiceEvent};

    let mdns = ServiceDaemon::new().map_err(|e| e.to_string())?;
    let receiver = mdns
        .browse("_oscjson._tcp.local.")
        .map_err(|e| e.to_string())?;

    let deadline = std::time::Instant::now() + Duration::from_secs(5);

    loop {
        let now = std::time::Instant::now();
        if now >= deadline {
            break;
        }
        let remaining = deadline - now;
        let poll = remaining.min(Duration::from_millis(500));

        match receiver.recv_timeout(poll) {
            Ok(ServiceEvent::ServiceResolved(info)) => {
                let port = info.get_port();
                for addr in info.get_addresses() {
                    use mdns_sd::ScopedIp;
                    let ip: std::net::IpAddr = match addr {
                        ScopedIp::V4(v4) => std::net::IpAddr::V4(*v4.addr()),
                        ScopedIp::V6(v6) => std::net::IpAddr::V6(*v6.addr()),
                        _ => continue,
                    };
                    let socket_addr = SocketAddr::new(ip, port);
                    log::debug!("mDNS found oscjson service at {}", socket_addr);
                    out.push(MdnsCandidate { http_addr: socket_addr });
                }
            }
            Ok(_) => {}
            Err(_) => break,
        }
    }

    mdns.stop_browse("_oscjson._tcp.local.").ok();
    Ok(())
}

// OSCQuery tree walker
fn collect_params(node: &OscQueryNode, out: &mut Vec<(String, OscValue)>) {
    if let Some(path) = &node.full_path {
        if path.starts_with("/avatar/parameters/") {
            if let Some(values) = &node.value {
                if let Some(first) = values.first() {
                    if let Some(v) = json_to_osc_value(first) {
                        out.push((path.clone(), v));
                    }
                }
            }
        }
    }
    if let Some(contents) = &node.contents {
        for child in contents.values() {
            collect_params(child, out);
        }
    }
}

pub(super) fn json_to_osc_value(v: &serde_json::Value) -> Option<OscValue> {
    match v {
        serde_json::Value::Bool(b) => Some(OscValue::Bool(*b)),
        serde_json::Value::Number(n) => n.as_f64().map(|f| OscValue::Float(f as f32)),
        _ => None,
    }
}
