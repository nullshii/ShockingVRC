use std::collections::HashMap;
use std::net::SocketAddr;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::types::OscValue;
use crate::error::{DGLabError, Result};

const MDNS_BROWSE_SECS: u64 = 15;

const OSCQUERY_HTTP_TIMEOUT: Duration = Duration::from_secs(3);
const OSCQUERY_CONNECT_TIMEOUT: Duration = Duration::from_secs(1);

pub const DEFAULT_OSC_PORT: u16 = 9001;

const VRCHAT_DEFAULT_OSC_PORT: u16 = 9000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum DiscoveryMode {
    #[default]
    #[serde(rename = "oscquery")]
    OscQuery,
    #[serde(rename = "osc")]
    Osc,
}

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

fn build_http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(OSCQUERY_HTTP_TIMEOUT)
        .connect_timeout(OSCQUERY_CONNECT_TIMEOUT)
        .build()
        .unwrap_or_else(|_| reqwest::Client::new())
}

// VrchatOscQuery
/// Discovers VRChat over mDNS + OSCQuery and fetches avatar parameter trees.
pub struct VrchatOscQuery {
    client: reqwest::Client,
    address: Option<VrchatAddress>,
    mode: DiscoveryMode,
    fallback_port: u16,
}

impl VrchatOscQuery {
    pub fn new() -> Self {
        Self {
            client: build_http_client(),
            address: None,
            mode: DiscoveryMode::default(),
            fallback_port: DEFAULT_OSC_PORT,
        }
    }

    pub fn get_address(&self) -> Option<&VrchatAddress> {
        self.address.as_ref()
    }

    pub fn set_address(&mut self, address: Option<VrchatAddress>) {
        self.address = address;
    }

    pub fn discovery_mode(&self) -> DiscoveryMode {
        self.mode
    }

    pub fn set_discovery_mode(&mut self, mode: DiscoveryMode) {
        self.mode = mode;
    }

    pub fn fallback_port(&self) -> u16 {
        self.fallback_port
    }

    pub fn set_fallback_port(&mut self, port: u16) {
        self.fallback_port = port;
    }

    pub fn client(&self) -> reqwest::Client {
        self.client.clone()
    }

    pub async fn locate(
        client: &reqwest::Client,
        mode: DiscoveryMode,
        fallback_port: u16,
    ) -> Result<Option<VrchatAddress>> {
        match mode {
            DiscoveryMode::Osc => {
                log::info!(
                    "OSC mode: listening for UDP avatar params (send → 127.0.0.1:{VRCHAT_DEFAULT_OSC_PORT})"
                );
                return Ok(Some(VrchatAddress {
                    osc_ip: "127.0.0.1".to_string(),
                    osc_port: VRCHAT_DEFAULT_OSC_PORT,
                    http_addr: SocketAddr::from(([127, 0, 0, 1], fallback_port)),
                }));
            }
            DiscoveryMode::OscQuery => {
                // Prefer localhost before a long mDNS browse — common when VRChat is local.
                let local = localhost_candidate(fallback_port);
                match check_candidate(client, &local).await {
                    Ok(Some(addr)) => {
                        log::info!(
                            "VRChat OSCQuery found at {} (OSC {}:{})",
                            local.http_addr,
                            addr.osc_ip,
                            addr.osc_port
                        );
                        return Ok(Some(addr));
                    }
                    Ok(None) => {
                        log::debug!(
                            "localhost:{} is not VRChat OSCQuery; trying mDNS...",
                            fallback_port
                        );
                    }
                    Err(e) => {
                        log::debug!(
                            "localhost:{} OSCQuery probe failed ({e}); trying mDNS...",
                            fallback_port
                        );
                    }
                }

                let candidates = tokio::task::spawn_blocking(move || scan_mdns(fallback_port))
                    .await
                    .map_err(|e| DGLabError::OscError(format!("spawn_blocking failed: {e}")))?;

                for candidate in candidates {
                    match check_candidate(client, &candidate).await {
                        Ok(Some(addr)) => {
                            log::info!(
                                "VRChat OSCQuery found at {} (OSC {}:{})",
                                candidate.http_addr,
                                addr.osc_ip,
                                addr.osc_port
                            );
                            return Ok(Some(addr));
                        }
                        Ok(None) => {}
                        Err(e) => {
                            log::debug!(
                                "OSCQuery candidate {} rejected: {}",
                                candidate.http_addr,
                                e
                            );
                        }
                    }
                }

                Ok(None)
            }
        }
    }

    pub async fn discover(&mut self) -> Result<bool> {
        match Self::locate(&self.client, self.mode, self.fallback_port).await? {
            Some(addr) => {
                self.address = Some(addr);
                Ok(true)
            }
            None => Ok(false),
        }
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

        let node: OscQueryNode = resp.json().await.map_err(|e| DGLabError::OscError(e.to_string()))?;

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

fn localhost_candidate(port: u16) -> MdnsCandidate {
    MdnsCandidate {
        http_addr: SocketAddr::from(([127, 0, 0, 1], port)),
    }
}

async fn check_candidate(
    client: &reqwest::Client,
    candidate: &MdnsCandidate,
) -> Result<Option<VrchatAddress>> {
    let url = format!("http://{}/?HOST_INFO", candidate.http_addr);
    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|e| DGLabError::OscError(e.to_string()))?;

    let info: HostInfo = resp.json().await.map_err(|e| DGLabError::OscError(e.to_string()))?;

    if !info.name.starts_with("VRChat-Client-") {
        return Ok(None);
    }

    Ok(Some(VrchatAddress {
        osc_ip: info.osc_ip,
        osc_port: info.osc_port,
        http_addr: candidate.http_addr,
    }))
}


fn scan_mdns(fallback_port: u16) -> Vec<MdnsCandidate> {
    let mut candidates = Vec::new();

    match try_scan_mdns(&mut candidates) {
        Ok(()) => {}
        Err(e) => log::warn!("mDNS scan failed: {}", e),
    }

    if candidates.is_empty() {
        log::debug!(
            "mDNS found nothing after {}s (localhost:{} already probed)",
            MDNS_BROWSE_SECS,
            fallback_port
        );
    }

    candidates
        .into_iter()
        .filter(|c| c.http_addr != SocketAddr::from(([127, 0, 0, 1], fallback_port)))
        .collect()
}

fn try_scan_mdns(out: &mut Vec<MdnsCandidate>) -> std::result::Result<(), String> {
    use mdns_sd::{ServiceDaemon, ServiceEvent};

    let mdns = ServiceDaemon::new().map_err(|e| e.to_string())?;
    let receiver = mdns.browse("_oscjson._tcp.local.").map_err(|e| e.to_string())?;

    let deadline = std::time::Instant::now() + Duration::from_secs(MDNS_BROWSE_SECS);

    loop {
        let now = std::time::Instant::now();
        if now >= deadline {
            break;
        }
        if !out.is_empty() {
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
            Err(_) => {}
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
