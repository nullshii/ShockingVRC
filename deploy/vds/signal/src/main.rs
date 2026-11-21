use std::collections::{HashMap, HashSet};
use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use axum::Router;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{ConnectInfo, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use base64::Engine;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use futures_util::{SinkExt, StreamExt};
use hmac::{Hmac, Mac};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio::sync::mpsc;

const MAX_MESSAGE: usize = 64 * 1024;
const MAX_JOINERS: usize = 8;
const IDLE_TIMEOUT: Duration = Duration::from_secs(90);
const MAX_ALIAS_TTL: u64 = 24 * 3600;
const JOIN_BURST: f32 = 20.0;
const JOIN_PER_SEC: f32 = 20.0 / 60.0;
const FREE_FAILURES: u32 = 3;
const MAX_LOCKOUT: Duration = Duration::from_secs(900);

#[derive(Clone, Debug)]
struct Config {
    bind: SocketAddr,
    web_bind: SocketAddr,
    data: PathBuf,
    public_host: String,
    turn_port: u16,
    turns_port: u16,
    turns_443: bool,
    turn_ttl: u64,
    trust_proxy: bool,
    domain: String,
}

fn env(key: &str, default: &str) -> String {
    std::env::var(key)
        .ok()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| default.to_string())
}

impl Config {
    fn from_env() -> Self {
        let domain = env("DOMAIN", "");
        let public_ip = env("PUBLIC_IP", "");
        Self {
            bind: env("SIGNAL_BIND", "0.0.0.0:8080").parse().expect("SIGNAL_BIND"),
            web_bind: env("WEB_BIND", "0.0.0.0:8081").parse().expect("WEB_BIND"),
            data: PathBuf::from(env("DATA_DIR", "/data")),
            public_host: if domain.is_empty() { public_ip } else { domain.clone() },
            turn_port: env("TURN_PORT", "3478").parse().unwrap_or(3478),
            turns_port: env("TURNS_PORT", "5349").parse().unwrap_or(5349),
            turns_443: env("TURNS_443", "true") == "true",
            turn_ttl: env("TURN_TTL", "3600").parse().unwrap_or(3600),
            trust_proxy: env("TRUST_PROXY", "true") == "true",
            domain,
        }
    }
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn b64(bytes: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(bytes)
}

fn unb64(s: &str) -> Option<Vec<u8>> {
    URL_SAFE_NO_PAD.decode(s.trim().trim_end_matches('=')).ok()
}

fn random<const N: usize>() -> [u8; N] {
    let mut b = [0u8; N];
    rand::fill(&mut b[..]);
    b
}

fn room_for(pk: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(b"shockingvrc/2/room");
    h.update(pk);
    let d = h.finalize();
    d[..16].iter().map(|b| format!("{b:02x}")).collect()
}

fn verify(pk: &[u8], msg: &[u8], sig: &[u8]) -> bool {
    let (Ok(pk), Ok(sig)) = (<[u8; 32]>::try_from(pk), <[u8; 64]>::try_from(sig)) else {
        return false;
    };
    let Ok(key) = ed25519_dalek::VerifyingKey::from_bytes(&pk) else {
        return false;
    };
    key.verify_strict(msg, &ed25519_dalek::Signature::from_bytes(&sig))
        .is_ok()
}

fn secret_path(data: &Path) -> PathBuf {
    data.join("secrets").join("turn_secret")
}

fn cert_dir(data: &Path) -> PathBuf {
    data.join("certs")
}

fn write_readable(path: &Path, contents: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, contents)
}

fn init(cfg: &Config) -> Result<(), String> {
    let secret = secret_path(&cfg.data);
    if !secret.exists() {
        let s = b64(&random::<32>());
        write_readable(&secret, s.as_bytes()).map_err(|e| format!("{}: {e}", secret.display()))?;
        log::info!("Generated a new TURN shared secret");
    }
    if cfg.domain.is_empty() {
        let dir = cert_dir(&cfg.data);
        let (cert, key) = (dir.join("cert.pem"), dir.join("key.pem"));
        if !cert.exists() || !key.exists() {
            let mut names = vec!["shockingvrc-vds".to_string()];
            if !cfg.public_host.is_empty() {
                names.insert(0, cfg.public_host.clone());
            }
            let mut params = rcgen::CertificateParams::new(names).map_err(|e| e.to_string())?;
            params.not_before = rcgen::date_time_ymd(2024, 1, 1);
            params.not_after = rcgen::date_time_ymd(2099, 1, 1);
            let key_pair = rcgen::KeyPair::generate().map_err(|e| e.to_string())?;
            let c = params.self_signed(&key_pair).map_err(|e| e.to_string())?;
            write_readable(&cert, c.pem().as_bytes()).map_err(|e| e.to_string())?;
            write_readable(&key, key_pair.serialize_pem().as_bytes()).map_err(|e| e.to_string())?;
            log::info!("Generated a self-signed certificate for {}", cfg.public_host);
        }
    }
    if let Some(pin) = cert_pin(cfg) {
        log::info!("IP-only mode — certificate pin for the app: {pin}");
    }
    Ok(())
}

fn cert_pin(cfg: &Config) -> Option<String> {
    if !cfg.domain.is_empty() {
        return None;
    }
    let pem = std::fs::read_to_string(cert_dir(&cfg.data).join("cert.pem")).ok()?;
    let body: String = pem
        .lines()
        .filter(|l| !l.starts_with("-----"))
        .collect::<Vec<_>>()
        .join("");
    let der = STANDARD.decode(body.trim()).ok()?;
    Some(b64(&Sha256::digest(&der)))
}

fn turn_secret(cfg: &Config) -> Option<String> {
    std::fs::read_to_string(secret_path(&cfg.data))
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn ice_servers(cfg: &Config, secret: &str, peer: u64) -> Value {
    let host = &cfg.public_host;
    if host.is_empty() {
        return json!({ "servers": [] });
    }
    let username = format!("{}:{peer:x}", now_unix() + cfg.turn_ttl);
    let mut mac = <Hmac<sha1::Sha1> as Mac>::new_from_slice(secret.as_bytes()).expect("any key length");
    mac.update(username.as_bytes());
    let credential = STANDARD.encode(mac.finalize().into_bytes());
    let mut turn = vec![
        format!("turn:{host}:{}?transport=udp", cfg.turn_port),
        format!("turn:{host}:{}?transport=tcp", cfg.turn_port),
        format!("turns:{host}:{}?transport=tcp", cfg.turns_port),
    ];
    if cfg.turns_443 {
        turn.push(format!("turns:{host}:443?transport=tcp"));
    }
    json!({
        "servers": [
            { "urls": [format!("stun:{host}:{}", cfg.turn_port)] },
            { "urls": turn, "username": username, "credential": credential }
        ],
        "ttl": cfg.turn_ttl,
    })
}

#[derive(Default)]
struct Strikes {
    failures: u32,
    locked_until: Option<Instant>,
    last: Option<Instant>,
}

impl Strikes {
    fn locked(&self, now: Instant) -> Option<Duration> {
        self.locked_until.filter(|t| *t > now).map(|t| t - now)
    }

    fn fail(&mut self, now: Instant) {
        if self
            .last
            .is_some_and(|l| now.duration_since(l) > Duration::from_secs(3600))
        {
            self.failures = 0;
        }
        self.failures += 1;
        self.last = Some(now);
        if self.failures >= FREE_FAILURES {
            let exp = (self.failures - FREE_FAILURES).min(20);
            self.locked_until = Some(now + Duration::from_secs(1u64 << exp).min(MAX_LOCKOUT));
        }
    }
}

struct Bucket {
    tokens: f32,
    at: Instant,
}

enum Role {
    None,
    Challenged { room: String, nonce: [u8; 32], pk: Vec<u8> },
    Host { room: String },
    Joiner { room: String, host: u64 },
}

struct Conn {
    tx: mpsc::Sender<String>,
    ip: IpAddr,
    role: Role,
}

struct Room {
    host: u64,
    joiners: HashSet<u64>,
    strikes: Strikes,
}

#[derive(Default)]
struct Hub {
    next: u64,
    conns: HashMap<u64, Conn>,
    rooms: HashMap<String, Room>,
    aliases: HashMap<String, (String, Instant)>,
    ip_strikes: HashMap<IpAddr, Strikes>,
    buckets: HashMap<IpAddr, Bucket>,
}

impl Hub {
    fn send(&self, to: u64, v: Value) {
        if let Some(c) = self.conns.get(&to) {
            let _ = c.tx.try_send(v.to_string());
        }
    }

    fn take_token(&mut self, ip: IpAddr, now: Instant) -> bool {
        let b = self.buckets.entry(ip).or_insert(Bucket {
            tokens: JOIN_BURST,
            at: now,
        });
        b.tokens = (b.tokens + now.duration_since(b.at).as_secs_f32() * JOIN_PER_SEC).min(JOIN_BURST);
        b.at = now;
        if b.tokens >= 1.0 {
            b.tokens -= 1.0;
            true
        } else {
            false
        }
    }

    fn sweep(&mut self, now: Instant) {
        self.aliases.retain(|_, (_, until)| *until > now);
        self.ip_strikes.retain(|_, s| {
            s.last
                .is_some_and(|l| now.duration_since(l) < Duration::from_secs(3600))
        });
        self.buckets
            .retain(|_, b| now.duration_since(b.at) < Duration::from_secs(600));
    }

    fn remove(&mut self, id: u64) {
        let Some(conn) = self.conns.remove(&id) else { return };
        match conn.role {
            Role::Host { room } => {
                let owned = self.rooms.get(&room).is_some_and(|r| r.host == id);
                if owned && let Some(r) = self.rooms.remove(&room) {
                    for j in r.joiners {
                        self.send(j, json!({"op": "left", "peer": id.to_string()}));
                    }
                    self.aliases.retain(|_, (target, _)| *target != room);
                }
            }
            Role::Joiner { room, host } => {
                if let Some(r) = self.rooms.get_mut(&room) {
                    r.joiners.remove(&id);
                }
                self.send(host, json!({"op": "left", "peer": id.to_string()}));
            }
            _ => {}
        }
    }
}

struct App {
    cfg: Config,
    hub: Mutex<Hub>,
}

fn err(code: &str, message: &str) -> Value {
    json!({"op": "error", "code": code, "message": message})
}

fn handle(app: &App, id: u64, msg: &Value) -> Option<Value> {
    let op = msg.get("op").and_then(|o| o.as_str()).unwrap_or_default();
    let now = Instant::now();
    let mut hub = app.hub.lock().ok()?;
    let ip = hub.conns.get(&id)?.ip;
    match op {
        "ping" => Some(json!({"op": "pong"})),
        "host" => {
            let room = msg.get("room").and_then(|r| r.as_str()).unwrap_or_default().to_string();
            let pk = msg
                .get("pk")
                .and_then(|p| p.as_str())
                .and_then(unb64)
                .unwrap_or_default();
            if pk.len() != 32 || room != room_for(&pk) {
                return Some(err("bad_room", "the room does not belong to that key"));
            }
            let nonce = random::<32>();
            if let Some(c) = hub.conns.get_mut(&id) {
                c.role = Role::Challenged { room, nonce, pk };
            }
            Some(json!({"op": "challenge", "nonce": b64(&nonce)}))
        }
        "prove" => {
            let sig = msg
                .get("sig")
                .and_then(|s| s.as_str())
                .and_then(unb64)
                .unwrap_or_default();
            let (room, ok) = match hub.conns.get(&id).map(|c| &c.role) {
                Some(Role::Challenged { room, nonce, pk }) => {
                    let mut m = b"shockingvrc/2/room".to_vec();
                    m.extend_from_slice(room.as_bytes());
                    m.extend_from_slice(nonce);
                    (room.clone(), verify(pk, &m, &sig))
                }
                _ => return Some(err("bad_state", "send host first")),
            };
            if !ok {
                hub.ip_strikes.entry(ip).or_default().fail(now);
                return Some(err("bad_proof", "the room proof did not verify"));
            }
            if let Some(old) = hub.rooms.get(&room).map(|r| r.host).filter(|h| *h != id) {
                hub.send(old, err("replaced", "this room is now hosted elsewhere"));
                hub.remove(old);
            }
            hub.rooms.entry(room.clone()).or_insert(Room {
                host: id,
                joiners: HashSet::new(),
                strikes: Strikes::default(),
            });
            if let Some(r) = hub.rooms.get_mut(&room) {
                r.host = id;
            }
            if let Some(c) = hub.conns.get_mut(&id) {
                c.role = Role::Host { room: room.clone() };
            }
            let ice = turn_secret(&app.cfg)
                .map(|s| ice_servers(&app.cfg, &s, id))
                .unwrap_or(json!({"servers": []}));
            log::info!("room {}… hosted", &room[..8]);
            Some(json!({"op": "hosting", "room": room, "peer": id.to_string(), "ice": ice}))
        }
        "alias" | "unalias" => {
            let Some(Role::Host { room }) = hub.conns.get(&id).map(|c| &c.role) else {
                return Some(err("bad_state", "only a host can register a code"));
            };
            let room = room.clone();
            let code = msg
                .get("code")
                .and_then(|c| c.as_str())
                .unwrap_or_default()
                .to_uppercase();
            if !code.starts_with("C-") || code.len() != 6 {
                return Some(err("bad_code", "codes look like c-XXXX"));
            }
            if op == "unalias" {
                if hub.aliases.get(&code).is_some_and(|(r, _)| *r == room) {
                    hub.aliases.remove(&code);
                }
                return None;
            }
            if hub.aliases.get(&code).is_some_and(|(r, t)| *r != room && *t > now) {
                return Some(err("code_taken", "that code is in use"));
            }
            let ttl = msg
                .get("ttl")
                .and_then(|t| t.as_u64())
                .unwrap_or(600)
                .clamp(60, MAX_ALIAS_TTL);
            hub.aliases.insert(code, (room, now + Duration::from_secs(ttl)));
            None
        }
        "join" => {
            if let Some(wait) = hub.ip_strikes.get(&ip).and_then(|s| s.locked(now)) {
                return Some(json!({"op": "error", "code": "rate_limited", "retry_ms": wait.as_millis() as u64}));
            }
            if !hub.take_token(ip, now) {
                return Some(json!({"op": "error", "code": "rate_limited", "retry_ms": 3000}));
            }
            let asked = msg.get("room").and_then(|r| r.as_str()).unwrap_or_default().to_string();
            let room = match hub.aliases.get(&asked.to_uppercase()) {
                Some((r, until)) if *until > now => r.clone(),
                _ => asked,
            };
            let Some(r) = hub.rooms.get(&room) else {
                hub.ip_strikes.entry(ip).or_default().fail(now);
                return Some(err("no_room", "nobody is waiting under that code"));
            };
            if let Some(wait) = r.strikes.locked(now) {
                return Some(json!({"op": "error", "code": "rate_limited", "retry_ms": wait.as_millis() as u64}));
            }
            if r.joiners.len() >= MAX_JOINERS {
                return Some(err("busy", "too many people are connecting"));
            }
            let host = r.host;
            if let Some(r) = hub.rooms.get_mut(&room) {
                r.joiners.insert(id);
            }
            if let Some(c) = hub.conns.get_mut(&id) {
                c.role = Role::Joiner {
                    room: room.clone(),
                    host,
                };
            }
            hub.send(host, json!({"op": "peer", "peer": id.to_string(), "ip": ip.to_string()}));
            let ice = turn_secret(&app.cfg)
                .map(|s| ice_servers(&app.cfg, &s, id))
                .unwrap_or(json!({"servers": []}));
            Some(json!({"op": "joined", "peer": host.to_string(), "ice": ice}))
        }
        "signal" => {
            let to: u64 = msg.get("to").and_then(|t| t.as_str()).and_then(|t| t.parse().ok())?;
            let allowed = match hub.conns.get(&id).map(|c| &c.role) {
                Some(Role::Joiner { host, .. }) => *host == to,
                Some(Role::Host { room }) => hub.rooms.get(room).is_some_and(|r| r.joiners.contains(&to)),
                _ => false,
            };
            if allowed {
                let data = msg.get("data").cloned().unwrap_or(Value::Null);
                hub.send(to, json!({"op": "signal", "from": id.to_string(), "data": data}));
            }
            None
        }
        "fail" => {
            let Some(Role::Host { room }) = hub.conns.get(&id).map(|c| &c.role) else {
                return None;
            };
            let room = room.clone();
            let peer: Option<u64> = msg.get("peer").and_then(|p| p.as_str()).and_then(|p| p.parse().ok());
            if let Some(peer_ip) = peer.and_then(|p| hub.conns.get(&p)).map(|c| c.ip) {
                hub.ip_strikes.entry(peer_ip).or_default().fail(now);
            }
            if let Some(r) = hub.rooms.get_mut(&room) {
                r.strikes.fail(now);
            }
            None
        }
        "ice" => {
            let ice = turn_secret(&app.cfg)
                .map(|s| ice_servers(&app.cfg, &s, id))
                .unwrap_or(json!({"servers": []}));
            Some(json!({"op": "ice", "ice": ice}))
        }
        _ => Some(err("bad_op", "unknown operation")),
    }
}

fn client_ip(app: &App, peer: SocketAddr, headers: &HeaderMap) -> IpAddr {
    if app.cfg.trust_proxy
        && let Some(ip) = headers
            .get("x-forwarded-for")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.split(',').next())
            .and_then(|v| v.trim().parse().ok())
    {
        return ip;
    }
    peer.ip()
}

async fn ws(
    State(app): State<Arc<App>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    up: WebSocketUpgrade,
) -> Response {
    let ip = client_ip(&app, peer, &headers);
    up.max_message_size(MAX_MESSAGE)
        .on_upgrade(move |socket| session(app, socket, ip))
}

async fn session(app: Arc<App>, socket: WebSocket, ip: IpAddr) {
    let (tx, mut rx) = mpsc::channel::<String>(64);
    let id = {
        let Ok(mut hub) = app.hub.lock() else { return };
        hub.next += 1;
        let id = hub.next;
        hub.conns.insert(
            id,
            Conn {
                tx: tx.clone(),
                ip,
                role: Role::None,
            },
        );
        id
    };
    let (mut sink, mut stream) = socket.split();
    let writer = tokio::spawn(async move {
        while let Some(text) = rx.recv().await {
            if sink.send(Message::Text(text.into())).await.is_err() {
                break;
            }
        }
        let _ = sink.close().await;
    });
    loop {
        let next = tokio::time::timeout(IDLE_TIMEOUT, stream.next()).await;
        let Ok(Some(Ok(msg))) = next else { break };
        let text = match msg {
            Message::Text(t) => t.to_string(),
            Message::Binary(b) => String::from_utf8_lossy(&b).into_owned(),
            Message::Close(_) => break,
            _ => continue,
        };
        let Ok(v) = serde_json::from_str::<Value>(&text) else {
            continue;
        };
        if let Some(reply) = handle(&app, id, &v)
            && tx.try_send(reply.to_string()).is_err()
        {
            break;
        }
    }
    if let Ok(mut hub) = app.hub.lock() {
        hub.remove(id);
    }
    drop(tx);
    let _ = writer.await;
}

async fn info(State(app): State<Arc<App>>) -> impl IntoResponse {
    let body = json!({
        "service": "shockingvrc-signal",
        "protocol": 2,
        "domain": app.cfg.domain,
        "host": app.cfg.public_host,
        "turn": turn_secret(&app.cfg).is_some(),
        "pin": cert_pin(&app.cfg),
    });
    ([(header::CONTENT_TYPE, "application/json")], body.to_string())
}

async fn healthz(State(app): State<Arc<App>>) -> impl IntoResponse {
    if turn_secret(&app.cfg).is_some() {
        (StatusCode::OK, "ok")
    } else {
        (StatusCode::SERVICE_UNAVAILABLE, "no secret yet")
    }
}

const INDEX: &str = include_str!("../../web/index.html");
const STYLE: &str = include_str!("../../web/style.css");
const APP_JS: &str = include_str!("../../web/app.js");
const PROTO_JS: &str = include_str!("../../web/proto.js");
const PANEL_JS: &str = include_str!("../../web/panel.js");

fn page(kind: &'static str, body: &'static str) -> Response {
    (
        [
            (header::CONTENT_TYPE, kind),
            (header::CACHE_CONTROL, "no-cache"),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
        ],
        body,
    )
        .into_response()
}

fn web_router() -> Router {
    Router::new()
        .route("/", get(|| async { page("text/html; charset=utf-8", INDEX) }))
        .route("/style.css", get(|| async { page("text/css; charset=utf-8", STYLE) }))
        .route("/app.js", get(|| async { page("text/javascript; charset=utf-8", APP_JS) }))
        .route("/proto.js", get(|| async { page("text/javascript; charset=utf-8", PROTO_JS) }))
        .route("/panel.js", get(|| async { page("text/javascript; charset=utf-8", PANEL_JS) }))
}

async fn healthcheck(addr: SocketAddr) -> bool {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let target = SocketAddr::new(
        if addr.ip().is_unspecified() {
            IpAddr::from([127, 0, 0, 1])
        } else {
            addr.ip()
        },
        addr.port(),
    );
    let Ok(Ok(mut s)) = tokio::time::timeout(Duration::from_secs(3), tokio::net::TcpStream::connect(target)).await
    else {
        return false;
    };
    if s.write_all(b"GET /healthz HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .await
        .is_err()
    {
        return false;
    }
    let mut buf = vec![0u8; 64];
    let n = tokio::time::timeout(Duration::from_secs(3), s.read(&mut buf))
        .await
        .ok()
        .and_then(|r| r.ok())
        .unwrap_or(0);
    String::from_utf8_lossy(&buf[..n]).starts_with("HTTP/1.1 200")
}

async fn shutdown() {
    let _ = tokio::signal::ctrl_c().await;
}

#[tokio::main]
async fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let cfg = Config::from_env();
    let cmd = std::env::args().nth(1).unwrap_or_else(|| "serve".into());
    match cmd.as_str() {
        "healthcheck" => {
            let addr = if std::env::args().nth(2).as_deref() == Some("web") {
                cfg.web_bind
            } else {
                cfg.bind
            };
            std::process::exit(if healthcheck(addr).await { 0 } else { 1 });
        }
        "init" => {
            if let Err(e) = init(&cfg) {
                log::error!("init failed: {e}");
                std::process::exit(1);
            }
        }
        "web" => {
            let app = web_router().route("/healthz", get(|| async { "ok" }));
            let listener = tokio::net::TcpListener::bind(cfg.web_bind).await.expect("bind web");
            log::info!("Web control panel on {}", cfg.web_bind);
            axum::serve(listener, app).with_graceful_shutdown(shutdown()).await.ok();
        }
        _ => {
            if let Err(e) = init(&cfg) {
                log::error!("init failed: {e}");
                std::process::exit(1);
            }
            if cfg.public_host.is_empty() {
                log::warn!("Neither DOMAIN nor PUBLIC_IP is set — TURN relays will not be offered");
            }
            let app = Arc::new(App {
                cfg: cfg.clone(),
                hub: Mutex::new(Hub::default()),
            });
            let sweeper = Arc::clone(&app);
            tokio::spawn(async move {
                let mut t = tokio::time::interval(Duration::from_secs(60));
                loop {
                    t.tick().await;
                    if let Ok(mut hub) = sweeper.hub.lock() {
                        hub.sweep(Instant::now());
                    }
                }
            });
            let router = Router::new()
                .route("/ws", get(ws))
                .route("/api/info", get(info))
                .route("/healthz", get(healthz))
                .with_state(app)
                .merge(web_router());
            let listener = tokio::net::TcpListener::bind(cfg.bind).await.expect("bind signal");
            log::info!("Signal service on {}", cfg.bind);
            axum::serve(listener, router.into_make_service_with_connect_info::<SocketAddr>())
                .with_graceful_shutdown(shutdown())
                .await
                .ok();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> Config {
        Config {
            bind: "127.0.0.1:0".parse().unwrap(),
            web_bind: "127.0.0.1:0".parse().unwrap(),
            data: std::env::temp_dir().join(format!("svrc-signal-{}", b64(&random::<6>()))),
            public_host: "vds.example".into(),
            turn_port: 3478,
            turns_port: 5349,
            turns_443: true,
            turn_ttl: 3600,
            trust_proxy: true,
            domain: String::new(),
        }
    }

    fn app() -> App {
        let c = cfg();
        init(&c).unwrap();
        App {
            cfg: c,
            hub: Mutex::new(Hub::default()),
        }
    }

    fn connect(app: &App, ip: &str) -> (u64, mpsc::Receiver<String>) {
        let (tx, rx) = mpsc::channel(64);
        let mut hub = app.hub.lock().unwrap();
        hub.next += 1;
        let id = hub.next;
        hub.conns.insert(
            id,
            Conn {
                tx,
                ip: ip.parse().unwrap(),
                role: Role::None,
            },
        );
        (id, rx)
    }

    fn host(app: &App, id: u64, key: &ed25519_dalek::SigningKey) -> String {
        use ed25519_dalek::Signer;
        let pk = key.verifying_key().to_bytes();
        let room = room_for(&pk);
        let ch = handle(app, id, &json!({"op": "host", "room": room, "pk": b64(&pk)})).unwrap();
        let nonce = unb64(ch["nonce"].as_str().unwrap()).unwrap();
        let mut m = b"shockingvrc/2/room".to_vec();
        m.extend_from_slice(room.as_bytes());
        m.extend_from_slice(&nonce);
        let sig = key.sign(&m).to_bytes();
        let ok = handle(app, id, &json!({"op": "prove", "sig": b64(&sig)})).unwrap();
        assert_eq!(ok["op"], "hosting", "{ok}");
        room
    }

    #[test]
    fn a_host_proves_its_room_and_a_joiner_reaches_it() {
        let app = app();
        let key = ed25519_dalek::SigningKey::from_bytes(&random::<32>());
        let (h, mut h_rx) = connect(&app, "1.1.1.1");
        let room = host(&app, h, &key);
        let (j, mut j_rx) = connect(&app, "2.2.2.2");
        let joined = handle(&app, j, &json!({"op": "join", "room": room})).unwrap();
        assert_eq!(joined["op"], "joined");
        let creds = &joined["ice"]["servers"][1];
        assert!(
            creds["urls"]
                .as_array()
                .unwrap()
                .iter()
                .any(|u| u == "turns:vds.example:443?transport=tcp")
        );
        let peer: Value = serde_json::from_str(&h_rx.try_recv().unwrap()).unwrap();
        assert_eq!(peer["op"], "peer");
        assert_eq!(peer["ip"], "2.2.2.2");
        handle(&app, j, &json!({"op": "signal", "to": h.to_string(), "data": {"kind": "bye"}}));
        let got: Value = serde_json::from_str(&h_rx.try_recv().unwrap()).unwrap();
        assert_eq!(got["from"], j.to_string());
        handle(&app, h, &json!({"op": "signal", "to": "999", "data": {}}));
        assert!(j_rx.try_recv().is_err(), "a host cannot signal strangers");
    }

    #[test]
    fn a_wrong_key_cannot_host_someone_elses_room() {
        let app = app();
        let key = ed25519_dalek::SigningKey::from_bytes(&random::<32>());
        let other = ed25519_dalek::SigningKey::from_bytes(&random::<32>());
        let (h, _rx) = connect(&app, "1.1.1.1");
        let room = room_for(&key.verifying_key().to_bytes());
        let r = handle(
            &app,
            h,
            &json!({"op": "host", "room": room, "pk": b64(&other.verifying_key().to_bytes())}),
        )
        .unwrap();
        assert_eq!(r["code"], "bad_room");
    }

    #[test]
    fn missing_rooms_lock_an_address_out() {
        let app = app();
        let (j, _rx) = connect(&app, "3.3.3.3");
        let mut codes = Vec::new();
        for _ in 0..5 {
            let r = handle(&app, j, &json!({"op": "join", "room": "c-ZZZZ"})).unwrap();
            codes.push(r["code"].as_str().unwrap().to_string());
        }
        assert_eq!(&codes[..3], ["no_room", "no_room", "no_room"]);
        assert_eq!(codes[3], "rate_limited", "locked after three misses");
    }

    #[test]
    fn web_codes_alias_a_hosted_room() {
        let app = app();
        let key = ed25519_dalek::SigningKey::from_bytes(&random::<32>());
        let (h, _h_rx) = connect(&app, "1.1.1.1");
        host(&app, h, &key);
        assert!(handle(&app, h, &json!({"op": "alias", "code": "c-ABCD", "ttl": 600})).is_none());
        let (j, _rx) = connect(&app, "4.4.4.4");
        let r = handle(&app, j, &json!({"op": "join", "room": "c-abcd"})).unwrap();
        assert_eq!(r["op"], "joined");
        app.hub.lock().unwrap().remove(h);
        let (k, _rx2) = connect(&app, "5.5.5.5");
        let r = handle(&app, k, &json!({"op": "join", "room": "c-ABCD"})).unwrap();
        assert_eq!(r["code"], "no_room", "codes die with their host");
    }

    #[test]
    fn turn_credentials_follow_the_coturn_rest_scheme() {
        let c = cfg();
        let v = ice_servers(&c, "secret", 0x2a);
        let s = &v["servers"][1];
        let user = s["username"].as_str().unwrap();
        assert!(user.ends_with(":2a"));
        let mut mac = <Hmac<sha1::Sha1> as Mac>::new_from_slice(b"secret").unwrap();
        mac.update(user.as_bytes());
        assert_eq!(s["credential"], STANDARD.encode(mac.finalize().into_bytes()));
    }

    #[test]
    fn ip_only_mode_creates_a_pinnable_certificate() {
        let c = cfg();
        init(&c).unwrap();
        let pin = cert_pin(&c).expect("pin");
        assert_eq!(unb64(&pin).unwrap().len(), 32);
        init(&c).unwrap();
        assert_eq!(cert_pin(&c), Some(pin), "the certificate is kept across restarts");
        let _ = std::fs::remove_dir_all(&c.data);
    }
}
