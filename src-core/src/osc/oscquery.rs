use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use serde::Deserialize;
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::task::JoinHandle;

use super::types::OscValue;
use crate::error::{DGLabError, Result};

const PORT_SEARCH_START: u16 = 33776;
const PORT_SEARCH_RANGE: u16 = 512;

const SERVICE_NAME: &str = "ShockingVRC";

const MDNS_MULTICAST_ADDR: Ipv4Addr = Ipv4Addr::new(224, 0, 0, 251);
const MDNS_PORT: u16 = 5353;
const MDNS_REBROADCAST: Duration = Duration::from_secs(5);
const MDNS_TTL_SECS: u32 = 120;

const VRCHAT_RESCAN_MIN: Duration = Duration::from_secs(5);
const VRCHAT_RESCAN_MAX: Duration = Duration::from_secs(30);
const HTTP_TIMEOUT: Duration = Duration::from_secs(5);
const PROBE_TIMEOUT: Duration = Duration::from_millis(1500);
const LOG_SCAN_COOLDOWN: Duration = Duration::from_secs(30);

const MAX_REQUEST_BYTES: usize = 8 * 1024;

pub fn select_port() -> Result<u16> {
    for port in PORT_SEARCH_START..PORT_SEARCH_START.saturating_add(PORT_SEARCH_RANGE) {
        let tcp_ok = std::net::TcpListener::bind(("127.0.0.1", port)).is_ok();
        let udp_ok = std::net::UdpSocket::bind(("0.0.0.0", port)).is_ok();
        if tcp_ok && udp_ok {
            return Ok(port);
        }
    }
    Err(DGLabError::OscError(format!(
        "no free port found in {PORT_SEARCH_START}..{}",
        PORT_SEARCH_START.saturating_add(PORT_SEARCH_RANGE)
    )))
}

pub struct OscQueryServer {
    port: u16,
    task: JoinHandle<()>,
}

impl OscQueryServer {
    pub async fn start(port: u16) -> Result<Self> {
        let listener = TcpListener::bind(("127.0.0.1", port))
            .await
            .map_err(|e| DGLabError::OscError(format!("OSCQuery HTTP bind failed: {e}")))?;

        let host_info = Arc::new(build_host_info(port).to_string());
        let root_tree = Arc::new(build_root_tree().to_string());

        let task = tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, peer)) => {
                        let host_info = Arc::clone(&host_info);
                        let root_tree = Arc::clone(&root_tree);
                        tokio::spawn(async move {
                            if let Err(e) = handle_connection(stream, host_info, root_tree).await {
                                log::trace!("OSCQuery HTTP connection from {peer} failed: {e}");
                            }
                        });
                    }
                    Err(e) => log::debug!("OSCQuery HTTP accept error: {e}"),
                }
            }
        });

        log::info!("OSCQuery HTTP started on 127.0.0.1:{port}");
        Ok(Self { port, task })
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn shutdown(self) {
        self.task.abort();
    }
}

fn build_host_info(port: u16) -> serde_json::Value {
    json!({
        "EXTENSIONS": {
            "ACCESS": true,
            "VALUE": true,
            "RANGE": true,
            "DESCRIPTION": true,
            "TAGS": true,
            "CRITICAL": true,
            "CLIPMODE": true,
        },
        "OSC_IP": "127.0.0.1",
        "OSC_PORT": port,
        "OSC_TRANSPORT": "UDP",
    })
}

fn build_root_tree() -> serde_json::Value {
    json!({
        "FULL_PATH": "/",
        "DESCRIPTION": "root node",
        "ACCESS": 0,
        "CONTENTS": {
            "avatar": {
                "FULL_PATH": "/avatar",
                "ACCESS": 0,
                "CONTENTS": {
                    "change": {
                        "FULL_PATH": "/avatar/change",
                        "ACCESS": 2,
                    },
                },
            },
        },
    })
}

async fn handle_connection(
    mut stream: tokio::net::TcpStream,
    host_info: Arc<String>,
    root_tree: Arc<String>,
) -> std::io::Result<()> {
    let mut buf = Vec::with_capacity(512);
    let mut tmp = [0u8; 512];
    loop {
        let n = stream.read(&mut tmp).await?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&tmp[..n]);
        if buf.windows(4).any(|w| w == b"\r\n\r\n") || buf.len() > MAX_REQUEST_BYTES {
            break;
        }
    }

    let text = String::from_utf8_lossy(&buf);
    let mut parts = text.lines().next().unwrap_or("").split_whitespace();
    let method = parts.next().unwrap_or("");
    let target = parts.next().unwrap_or("/");
    let (path, query) = match target.split_once('?') {
        Some((p, q)) => (p, q),
        None => (target, ""),
    };

    let response = if method != "GET" {
        status_only(405, "Method Not Allowed")
    } else if path != "/" {
        status_only(404, "Not Found")
    } else if query == "HOST_INFO" {
        ok_json(&host_info)
    } else if !query.is_empty() {
        status_only(400, "Bad Request")
    } else {
        ok_json(&root_tree)
    };

    stream.write_all(response.as_bytes()).await?;
    stream.flush().await?;
    Ok(())
}

fn ok_json(body: &str) -> String {
    format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    )
}

fn status_only(code: u16, text: &str) -> String {
    format!("HTTP/1.1 {code} {text}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
}

pub struct MdnsBroadcaster {
    task: JoinHandle<()>,
}

impl MdnsBroadcaster {
    pub fn start(http_port: u16) -> Self {
        let packet = build_mdns_packet(SERVICE_NAME, http_port);
        let task = tokio::spawn(async move {
            loop {
                broadcast_once(&packet);
                tokio::time::sleep(MDNS_REBROADCAST).await;
            }
        });
        Self { task }
    }

    pub fn shutdown(self) {
        self.task.abort();
    }
}

fn broadcast_once(packet: &[u8]) {
    let addresses = external_ipv4_addresses();
    if addresses.is_empty() {
        log::debug!("mDNS broadcast skipped: no external IPv4 interfaces");
        return;
    }
    for addr in addresses {
        if let Err(e) = broadcast_on_interface(packet, addr) {
            log::debug!("mDNS broadcast on {addr} failed: {e}");
        }
    }
}

fn broadcast_on_interface(packet: &[u8], addr: Ipv4Addr) -> std::io::Result<()> {
    use socket2::{Domain, Protocol, Socket, Type};

    let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
    socket.set_reuse_address(true)?;
    socket.bind(&SocketAddr::new(IpAddr::V4(addr), MDNS_PORT).into())?;
    socket.set_multicast_ttl_v4(255)?;
    socket.set_multicast_loop_v4(true)?;
    socket.set_multicast_if_v4(&addr)?;
    socket.send_to(
        packet,
        &SocketAddr::new(IpAddr::V4(MDNS_MULTICAST_ADDR), MDNS_PORT).into(),
    )?;
    Ok(())
}

fn external_ipv4_addresses() -> Vec<Ipv4Addr> {
    let Ok(ifaces) = if_addrs::get_if_addrs() else {
        return Vec::new();
    };
    ifaces
        .into_iter()
        .filter(|i| !i.is_loopback())
        .filter_map(|i| match i.ip() {
            IpAddr::V4(v4) => Some(v4),
            _ => None,
        })
        .collect()
}

fn local_addresses() -> Vec<IpAddr> {
    let mut out = vec![IpAddr::V4(Ipv4Addr::LOCALHOST)];
    if let Ok(ifaces) = if_addrs::get_if_addrs() {
        out.extend(ifaces.into_iter().map(|i| i.ip()));
    }
    out
}

fn encode_dns_name(name: &str, out: &mut Vec<u8>) {
    for label in name.split('.').filter(|l| !l.is_empty()) {
        out.push(label.len() as u8);
        out.extend_from_slice(label.as_bytes());
    }
    out.push(0);
}

fn push_u16(out: &mut Vec<u8>, v: u16) {
    out.extend_from_slice(&v.to_be_bytes());
}

fn push_u32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_be_bytes());
}

fn build_mdns_packet(service_name: &str, port: u16) -> Vec<u8> {
    let service_type = "_oscjson._tcp.local";
    let instance_name = format!("{service_name}.{service_type}");
    let target_name = format!("{service_name}.oscjson.tcp");

    const TYPE_A: u16 = 1;
    const TYPE_PTR: u16 = 12;
    const TYPE_TXT: u16 = 16;
    const TYPE_SRV: u16 = 33;
    const CLASS_IN: u16 = 1;
    const CLASS_IN_FLUSH: u16 = 0x8001;

    let mut p = Vec::with_capacity(256);

    push_u16(&mut p, 0);
    push_u16(&mut p, 0x8400);
    push_u16(&mut p, 0);
    push_u16(&mut p, 1);
    push_u16(&mut p, 0);
    push_u16(&mut p, 3);

    encode_dns_name(service_type, &mut p);
    push_u16(&mut p, TYPE_PTR);
    push_u16(&mut p, CLASS_IN);
    push_u32(&mut p, MDNS_TTL_SECS);
    let mut rdata = Vec::new();
    encode_dns_name(&instance_name, &mut rdata);
    push_u16(&mut p, rdata.len() as u16);
    p.extend_from_slice(&rdata);

    encode_dns_name(&instance_name, &mut p);
    push_u16(&mut p, TYPE_SRV);
    push_u16(&mut p, CLASS_IN_FLUSH);
    push_u32(&mut p, MDNS_TTL_SECS);
    let mut rdata = Vec::new();
    push_u16(&mut rdata, 0);
    push_u16(&mut rdata, 0);
    push_u16(&mut rdata, port);
    encode_dns_name(&target_name, &mut rdata);
    push_u16(&mut p, rdata.len() as u16);
    p.extend_from_slice(&rdata);

    encode_dns_name(&instance_name, &mut p);
    push_u16(&mut p, TYPE_TXT);
    push_u16(&mut p, CLASS_IN_FLUSH);
    push_u32(&mut p, MDNS_TTL_SECS);
    let txt = b"txtvers=1";
    push_u16(&mut p, (txt.len() + 1) as u16);
    p.push(txt.len() as u8);
    p.extend_from_slice(txt);

    encode_dns_name(&target_name, &mut p);
    push_u16(&mut p, TYPE_A);
    push_u16(&mut p, CLASS_IN_FLUSH);
    push_u32(&mut p, MDNS_TTL_SECS);
    push_u16(&mut p, 4);
    p.extend_from_slice(&Ipv4Addr::LOCALHOST.octets());

    p
}

#[derive(Debug, Clone)]
pub struct VrchatAddress {
    pub osc_ip: String,
    pub osc_port: u16,
    pub http_addr: SocketAddr,
}

#[derive(Debug, Deserialize)]
struct HostInfo {
    #[serde(rename = "NAME")]
    name: String,
    #[serde(rename = "OSC_IP")]
    osc_ip: String,
    #[serde(rename = "OSC_PORT")]
    osc_port: u16,
}

#[derive(Debug, Deserialize)]
struct OscQueryNode {
    #[serde(rename = "FULL_PATH")]
    full_path: Option<String>,
    #[serde(rename = "VALUE")]
    value: Option<Vec<serde_json::Value>>,
    #[serde(rename = "CONTENTS")]
    contents: Option<HashMap<String, OscQueryNode>>,
}

struct ClientInner {
    http: reqwest::Client,
    probe: reqwest::Client,
    address: std::sync::RwLock<Option<VrchatAddress>>,
    services: std::sync::Mutex<HashMap<String, (Vec<IpAddr>, u16)>>,
    log_scan: std::sync::Mutex<LogScan>,
    stop: AtomicBool,
}

#[derive(Default)]
struct LogScan {
    path: Option<PathBuf>,
    offset: u64,
    port: Option<u16>,
    last_scan: Option<std::time::Instant>,
}

pub struct VrchatClient {
    inner: Arc<ClientInner>,
    rescan_task: JoinHandle<()>,
}

impl VrchatClient {
    pub fn start() -> Self {
        let inner = Arc::new(ClientInner {
            http: reqwest::Client::builder()
                .timeout(HTTP_TIMEOUT)
                .build()
                .unwrap_or_default(),
            probe: reqwest::Client::builder()
                .timeout(PROBE_TIMEOUT)
                .build()
                .unwrap_or_default(),
            address: std::sync::RwLock::new(None),
            services: std::sync::Mutex::new(HashMap::new()),
            log_scan: std::sync::Mutex::new(LogScan::default()),
            stop: AtomicBool::new(false),
        });

        let browse_inner = Arc::clone(&inner);
        std::thread::spawn(move || browse_worker(browse_inner));

        let rescan_inner = Arc::clone(&inner);
        let rescan_task = tokio::spawn(async move {
            let mut backoff = VRCHAT_RESCAN_MIN;
            loop {
                if rescan_inner.stop.load(Ordering::Relaxed) {
                    return;
                }
                backoff = if rescan(&rescan_inner).await {
                    VRCHAT_RESCAN_MIN
                } else {
                    (backoff * 2).min(VRCHAT_RESCAN_MAX)
                };
                tokio::time::sleep(backoff).await;
            }
        });

        Self { inner, rescan_task }
    }

    pub fn address(&self) -> Option<VrchatAddress> {
        self.inner.address.read().ok().and_then(|a| a.clone())
    }

    pub async fn get_bulk(&self) -> Result<Vec<(String, OscValue)>> {
        let addr = self
            .address()
            .ok_or_else(|| DGLabError::OscError("VRChat OSCQuery not discovered yet".into()))?;

        let url = format!("http://{}/", addr.http_addr);
        let node: OscQueryNode = self
            .inner
            .http
            .get(&url)
            .send()
            .await
            .map_err(|e| DGLabError::OscError(e.to_string()))?
            .json()
            .await
            .map_err(|e| DGLabError::OscError(e.to_string()))?;

        let mut params = Vec::new();
        collect_values(&node, &mut params);
        Ok(params)
    }

    pub fn shutdown(&self) {
        self.inner.stop.store(true, Ordering::Relaxed);
        self.rescan_task.abort();
    }
}

impl Drop for VrchatClient {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn browse_worker(inner: Arc<ClientInner>) {
    use mdns_sd::{ScopedIp, ServiceDaemon, ServiceEvent};

    let daemon = match ServiceDaemon::new() {
        Ok(d) => d,
        Err(e) => {
            log::warn!("mDNS browse unavailable: {e}");
            return;
        }
    };
    let receiver = match daemon.browse("_oscjson._tcp.local.") {
        Ok(r) => r,
        Err(e) => {
            log::warn!("mDNS browse failed: {e}");
            return;
        }
    };

    loop {
        if inner.stop.load(Ordering::Relaxed) {
            break;
        }
        match receiver.recv_timeout(Duration::from_secs(1)) {
            Ok(ServiceEvent::ServiceResolved(info)) => {
                let addrs: Vec<IpAddr> = info
                    .get_addresses()
                    .iter()
                    .map(|a| match a {
                        ScopedIp::V4(v4) => IpAddr::V4(*v4.addr()),
                        ScopedIp::V6(v6) => IpAddr::V6(*v6.addr()),
                        _ => IpAddr::V4(Ipv4Addr::UNSPECIFIED),
                    })
                    .filter(|ip| !ip.is_unspecified())
                    .collect();
                if let Ok(mut services) = inner.services.lock() {
                    services.insert(info.get_fullname().to_string(), (addrs, info.get_port()));
                }
            }
            Ok(ServiceEvent::ServiceRemoved(_, fullname)) => {
                if let Ok(mut services) = inner.services.lock() {
                    services.remove(&fullname);
                }
            }
            Ok(_) => {}
            Err(_) => {}
        }
    }
    let _ = daemon.shutdown();
}

async fn rescan(inner: &Arc<ClientInner>) -> bool {
    let cached = inner.address.read().ok().and_then(|a| a.clone());
    if let Some(addr) = cached
        && check_endpoint(inner, addr.http_addr).await
    {
        return true;
    }

    let candidates: Vec<(Vec<IpAddr>, u16)> = inner
        .services
        .lock()
        .map(|s| s.values().cloned().collect())
        .unwrap_or_default();
    let local = local_addresses();

    for (ips, port) in candidates {
        for ip in ips {
            if !local.contains(&ip) {
                log::debug!("Skipping mDNS candidate {ip}:{port} (not a local address)");
                continue;
            }
            if !ip.is_loopback()
                && check_endpoint(inner, SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port))
                    .await
            {
                return true;
            }
            if check_endpoint(inner, SocketAddr::new(ip, port)).await {
                return true;
            }
        }
    }

    if let Some(port) = oscquery_port_from_logs(inner).await {
        log::debug!("Found VRChat OSCQuery port {port} in log file");
        return check_endpoint(inner, SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port)).await;
    }
    false
}

async fn check_endpoint(inner: &Arc<ClientInner>, http_addr: SocketAddr) -> bool {
    let url = format!("http://{http_addr}/?HOST_INFO");
    let info: HostInfo = match inner.probe.get(&url).send().await {
        Ok(resp) => match resp.json().await {
            Ok(json) => json,
            Err(_) => return false,
        },
        Err(_) => return false,
    };

    if !info.name.starts_with("VRChat-Client-") {
        log::debug!("Skipping {http_addr} ({} is not VRChat)", info.name);
        return false;
    }

    let had_address = inner.address.read().is_ok_and(|a| a.is_some());
    if !had_address {
        log::info!(
            "VRChat OSCQuery found at {http_addr} (OSC {}:{})",
            info.osc_ip,
            info.osc_port
        );
    }
    if let Ok(mut slot) = inner.address.write() {
        *slot = Some(VrchatAddress {
            osc_ip: info.osc_ip,
            osc_port: info.osc_port,
            http_addr,
        });
    }
    true
}

fn collect_values(node: &OscQueryNode, out: &mut Vec<(String, OscValue)>) {
    if let Some(path) = &node.full_path
        && let Some(values) = &node.value
        && let Some(first) = values.first()
        && let Some(v) = json_to_osc_value(first)
    {
        out.push((path.clone(), v));
    }
    if let Some(contents) = &node.contents {
        for child in contents.values() {
            collect_values(child, out);
        }
    }
}

fn json_to_osc_value(v: &serde_json::Value) -> Option<OscValue> {
    match v {
        serde_json::Value::Bool(b) => Some(OscValue::Bool(*b)),
        serde_json::Value::Number(n) => n.as_f64().map(|f| OscValue::Float(f as f32)),
        _ => None,
    }
}

fn vrchat_log_dir() -> Option<PathBuf> {
    let home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME"))?;
    Some(
        PathBuf::from(home)
            .join("AppData")
            .join("LocalLow")
            .join("VRChat")
            .join("VRChat"),
    )
}

async fn oscquery_port_from_logs(inner: &Arc<ClientInner>) -> Option<u16> {
    let inner = Arc::clone(inner);
    tokio::task::spawn_blocking(move || {
        let mut scan = inner.log_scan.lock().unwrap_or_else(|e| e.into_inner());
        if scan
            .last_scan
            .is_some_and(|t| t.elapsed() < LOG_SCAN_COOLDOWN)
        {
            return scan.port;
        }
        scan.last_scan = Some(std::time::Instant::now());
        let dir = vrchat_log_dir()?;
        scan_log_for_port(&mut scan, &dir)
    })
    .await
    .ok()
    .flatten()
}

fn scan_log_for_port(scan: &mut LogScan, dir: &std::path::Path) -> Option<u16> {
    use std::io::{BufRead, Seek, SeekFrom};

    let newest = std::fs::read_dir(dir)
        .ok()?
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.file_name()
                .to_string_lossy()
                .starts_with("output_log_")
        })
        .max_by_key(|e| e.metadata().and_then(|m| m.modified()).ok())?;
    let path = newest.path();

    if scan.path.as_deref() != Some(path.as_path()) {
        scan.path = Some(path.clone());
        scan.offset = 0;
        scan.port = None;
    }

    let len = newest.metadata().ok()?.len();
    if len < scan.offset {
        scan.offset = 0;
        scan.port = None;
    }
    if len == scan.offset {
        return scan.port;
    }

    let mut file = std::fs::File::open(&path).ok()?;
    file.seek(SeekFrom::Start(scan.offset)).ok()?;
    let mut reader = std::io::BufReader::new(file);
    let mut line = String::new();
    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => break,
            Ok(n) => {
                if !line.ends_with('\n') {
                    break;
                }
                scan.offset += n as u64;
                if let Some(port) = parse_oscquery_port(&line) {
                    scan.port = Some(port);
                }
            }
            Err(_) => break,
        }
    }
    scan.port
}

fn parse_oscquery_port(content: &str) -> Option<u16> {
    const MARKER: &str = "of type OSCQuery on ";
    let mut result = None;
    let mut rest = content;
    while let Some(pos) = rest.find(MARKER) {
        let after = &rest[pos + MARKER.len()..];
        let digits: String = after.chars().take_while(|c| c.is_ascii_digit()).collect();
        if let Ok(port) = digits.parse::<u16>() {
            result = Some(port);
        }
        rest = &after[digits.len()..];
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_info_matches_ogb_shape() {
        let info = build_host_info(33776);
        assert_eq!(info["OSC_PORT"], 33776);
        assert_eq!(info["OSC_IP"], "127.0.0.1");
        assert_eq!(info["OSC_TRANSPORT"], "UDP");
        assert!(info.get("NAME").is_none(), "OGB's HOST_INFO has no NAME");
        assert_eq!(info["EXTENSIONS"]["ACCESS"], true);
    }

    #[test]
    fn root_tree_advertises_avatar_change() {
        let tree = build_root_tree();
        assert_eq!(tree["FULL_PATH"], "/");
        assert_eq!(tree["CONTENTS"]["avatar"]["CONTENTS"]["change"]["ACCESS"], 2);
    }

    #[test]
    fn dns_name_encoding() {
        let mut out = Vec::new();
        encode_dns_name("_oscjson._tcp.local", &mut out);
        assert_eq!(out[0], 8);
        assert_eq!(&out[1..9], b"_oscjson");
        assert_eq!(*out.last().unwrap(), 0);
    }

    #[test]
    fn mdns_packet_structure() {
        let p = build_mdns_packet("OGB", 12345);
        assert_eq!(&p[2..4], &0x8400u16.to_be_bytes());
        assert_eq!(&p[6..8], &1u16.to_be_bytes());
        assert_eq!(&p[10..12], &3u16.to_be_bytes());
        assert!(p.windows(2).any(|w| w == 12345u16.to_be_bytes()));
        assert!(p.windows(4).any(|w| w == [127, 0, 0, 1]));
    }

    #[test]
    fn log_port_parsing_takes_last_match() {
        let log = "junk\nBlah of type OSCQuery on 9012 something\nof type OSCQuery on 34567\n";
        assert_eq!(parse_oscquery_port(log), Some(34567));
        assert_eq!(parse_oscquery_port("no match"), None);
    }

    fn scratch_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("svrc_logscan_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn append(path: &std::path::Path, text: &str) {
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .unwrap();
        f.write_all(text.as_bytes()).unwrap();
    }

    #[test]
    fn log_scan_only_reads_bytes_appended_since_the_last_pass() {
        let dir = scratch_dir("incremental");
        let log = dir.join("output_log_2026-01-01_00-00-00.txt");
        append(&log, "boot\nserver of type OSCQuery on 34567 ready\n");

        let mut scan = LogScan::default();
        assert_eq!(scan_log_for_port(&mut scan, &dir), Some(34567));
        let after_first = scan.offset;
        assert!(after_first > 0);

        assert_eq!(scan_log_for_port(&mut scan, &dir), Some(34567));
        assert_eq!(scan.offset, after_first);

        append(&log, "chatter\nserver of type OSCQuery on 41000 ready\n");
        assert_eq!(scan_log_for_port(&mut scan, &dir), Some(41000));
        assert!(scan.offset > after_first);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn log_scan_leaves_a_half_written_line_for_the_next_pass() {
        let dir = scratch_dir("partial");
        let log = dir.join("output_log_2026-01-01_00-00-00.txt");
        append(&log, "boot\nserver of type OSCQuery on 3");

        let mut scan = LogScan::default();
        assert_eq!(scan_log_for_port(&mut scan, &dir), None, "line is incomplete");
        let stopped_at = scan.offset;
        assert_eq!(stopped_at, "boot\n".len() as u64);

        append(&log, "4567 ready\n");
        assert_eq!(
            scan_log_for_port(&mut scan, &dir),
            Some(34567),
            "the completed line is re-read in full"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn log_scan_restarts_on_a_new_session_file() {
        let dir = scratch_dir("rotate");
        let old = dir.join("output_log_2026-01-01_00-00-00.txt");
        append(&old, "server of type OSCQuery on 34567 ready\n");

        let mut scan = LogScan::default();
        assert_eq!(scan_log_for_port(&mut scan, &dir), Some(34567));

        std::thread::sleep(Duration::from_millis(20));
        let new = dir.join("output_log_2026-01-02_00-00-00.txt");
        append(&new, "server of type OSCQuery on 9001 ready\n");
        assert_eq!(scan_log_for_port(&mut scan, &dir), Some(9001));
        assert_eq!(scan.path.as_deref(), Some(new.as_path()));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn select_port_returns_usable_port() {
        let port = select_port().expect("a free port should exist");
        assert!(port >= PORT_SEARCH_START);
        std::net::TcpListener::bind(("127.0.0.1", port)).unwrap();
        std::net::UdpSocket::bind(("0.0.0.0", port)).unwrap();
    }

    #[tokio::test]
    async fn server_routes_like_ogb() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let port = select_port().unwrap();
        let server = OscQueryServer::start(port).await.unwrap();
        assert_eq!(server.port(), port);

        let send = |req: String| async move {
            let mut s = tokio::net::TcpStream::connect(("127.0.0.1", port)).await.unwrap();
            s.write_all(req.as_bytes()).await.unwrap();
            let mut resp = String::new();
            s.read_to_string(&mut resp).await.unwrap();
            resp
        };
        let get = |path: &str| send(format!("GET {path} HTTP/1.1\r\nHost: x\r\n\r\n"));

        let host_info = get("/?HOST_INFO").await;
        assert!(host_info.contains("200 OK"));
        assert!(host_info.contains(&format!("\"OSC_PORT\":{port}")));

        let root = get("/").await;
        assert!(root.contains("200 OK"));
        assert!(root.contains("\"FULL_PATH\":\"/\""));
        assert!(root.contains("change"));

        assert!(get("/avatar").await.contains("404"));
        assert!(get("/?EXPLORER").await.contains("400"));
        assert!(
            send("POST / HTTP/1.1\r\nHost: x\r\n\r\n".into())
                .await
                .contains("405")
        );

        server.shutdown();
    }
}
