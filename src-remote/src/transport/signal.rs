use std::collections::HashMap;
use std::io;
use std::net::IpAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message as WsMessage;

use super::direct::TaskGuard;
use super::rtc::{IceConfig, IceServer, SignalMsg, Signaler};
use crate::crypto::{Identity, b64, unb64};

pub const WS_PATH: &str = "/ws";
const CONNECT_TIMEOUT: Duration = Duration::from_secs(8);

fn io_err(e: impl std::fmt::Display) -> io::Error {
    io::Error::other(e.to_string())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VdsAddr {
    pub secure: bool,
    pub host: String,
    pub port: u16,
}

impl VdsAddr {
    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim().trim_end_matches('/');
        let (secure, rest) = if let Some(r) = s.strip_prefix("https://").or_else(|| s.strip_prefix("wss://")) {
            (true, r)
        } else if let Some(r) = s.strip_prefix("http://").or_else(|| s.strip_prefix("ws://")) {
            (false, r)
        } else {
            (true, s)
        };
        let hostport = rest.split('/').next()?;
        if hostport.is_empty() {
            return None;
        }
        let default = if secure { 443 } else { 80 };
        let (host, port) = if let Some(stripped) = hostport.strip_prefix('[') {
            let (h, tail) = stripped.split_once(']')?;
            let port = tail.strip_prefix(':').and_then(|p| p.parse().ok()).unwrap_or(default);
            (h.to_string(), port)
        } else {
            match hostport.rsplit_once(':') {
                Some((h, p)) => (h.to_string(), p.parse().ok()?),
                None => (hostport.to_string(), default),
            }
        };
        Some(Self { secure, host, port })
    }

    pub fn ws_url(&self) -> String {
        let scheme = if self.secure { "wss" } else { "ws" };
        let host = if self.host.contains(':') {
            format!("[{}]", self.host)
        } else {
            self.host.clone()
        };
        format!("{scheme}://{host}:{}{WS_PATH}", self.port)
    }
}

trait Stream: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> Stream for T {}

type Ws = tokio_tungstenite::WebSocketStream<Box<dyn Stream>>;

fn http_client(pin: Option<&str>) -> io::Result<std::sync::Arc<rustls::ClientConfig>> {
    let mut cfg = (*crate::tls::client_for(pin).map_err(io_err)?).clone();
    cfg.alpn_protocols = vec![b"http/1.1".to_vec()];
    Ok(std::sync::Arc::new(cfg))
}

async fn open(addr: &VdsAddr, pin: Option<&str>) -> io::Result<Ws> {
    let tcp = tokio::time::timeout(CONNECT_TIMEOUT, TcpStream::connect((addr.host.as_str(), addr.port)))
        .await
        .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "rendezvous server timed out"))??;
    let _ = tcp.set_nodelay(true);
    let stream: Box<dyn Stream> = if addr.secure {
        let cfg = http_client(pin)?;
        let name = match addr.host.parse::<IpAddr>() {
            Ok(ip) => rustls::pki_types::ServerName::IpAddress(ip.into()),
            Err(_) => rustls::pki_types::ServerName::try_from(addr.host.clone()).map_err(io_err)?,
        };
        let tls = tokio::time::timeout(CONNECT_TIMEOUT, tokio_rustls::TlsConnector::from(cfg).connect(name, tcp))
            .await
            .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "TLS timed out"))??;
        Box::new(tls)
    } else {
        Box::new(tcp)
    };
    let (ws, _) = tokio::time::timeout(CONNECT_TIMEOUT, tokio_tungstenite::client_async(addr.ws_url(), stream))
        .await
        .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "WebSocket timed out"))?
        .map_err(io_err)?;
    Ok(ws)
}

fn parse_ice(v: &Value, pin: Option<&str>) -> IceConfig {
    let servers = v
        .get("servers")
        .and_then(|s| serde_json::from_value::<Vec<IceServer>>(s.clone()).ok())
        .unwrap_or_default();
    IceConfig {
        servers,
        relay_only: false,
        tls_pin: pin.map(str::to_string),
    }
}

struct Conn {
    tx: mpsc::Sender<Value>,
    rx: mpsc::Receiver<Value>,
    _guard: TaskGuard,
}

fn spawn_conn(ws: Ws) -> Conn {
    let (mut sink, mut stream) = ws.split();
    let (out_tx, mut out_rx) = mpsc::channel::<Value>(64);
    let (in_tx, in_rx) = mpsc::channel::<Value>(64);
    let keepalive = out_tx.clone();
    let writer = tokio::spawn(async move {
        while let Some(v) = out_rx.recv().await {
            if sink.send(WsMessage::Text(v.to_string().into())).await.is_err() {
                break;
            }
        }
        let _ = sink.close().await;
    });
    let reader = tokio::spawn(async move {
        while let Some(Ok(msg)) = stream.next().await {
            let text = match msg {
                WsMessage::Text(t) => t.to_string(),
                WsMessage::Binary(b) => String::from_utf8_lossy(&b).into_owned(),
                WsMessage::Close(_) => break,
                _ => continue,
            };
            if let Ok(v) = serde_json::from_str::<Value>(&text)
                && in_tx.send(v).await.is_err()
            {
                break;
            }
        }
    });
    let pinger = tokio::spawn(async move {
        let mut t = tokio::time::interval(Duration::from_secs(20));
        loop {
            t.tick().await;
            if keepalive.send(json!({"op": "ping"})).await.is_err() {
                break;
            }
        }
    });
    Conn {
        tx: out_tx,
        rx: in_rx,
        _guard: TaskGuard(vec![writer, reader, pinger]),
    }
}

async fn expect(conn: &mut Conn, op: &str) -> io::Result<Value> {
    loop {
        let v = tokio::time::timeout(CONNECT_TIMEOUT, conn.rx.recv())
            .await
            .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "rendezvous server did not answer"))?
            .ok_or_else(|| io::Error::new(io::ErrorKind::ConnectionAborted, "rendezvous server closed"))?;
        match v.get("op").and_then(|o| o.as_str()) {
            Some(o) if o == op => return Ok(v),
            Some("error") => {
                let code = v.get("code").and_then(|c| c.as_str()).unwrap_or("error");
                let msg = v.get("message").and_then(|c| c.as_str()).unwrap_or(code);
                return Err(io::Error::new(
                    if code == "no_room" {
                        io::ErrorKind::NotFound
                    } else {
                        io::ErrorKind::PermissionDenied
                    },
                    msg.to_string(),
                ));
            }
            _ => continue,
        }
    }
}

struct PeerSignaler {
    peer: String,
    ip: String,
    out: mpsc::Sender<Value>,
    rx: mpsc::Receiver<SignalMsg>,
    _conn: Option<Conn>,
}

#[async_trait::async_trait]
impl Signaler for PeerSignaler {
    async fn send(&mut self, msg: SignalMsg) -> io::Result<()> {
        let data = serde_json::to_value(&msg).map_err(io_err)?;
        self.out
            .send(json!({"op": "signal", "to": self.peer, "data": data}))
            .await
            .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "rendezvous connection closed"))
    }

    async fn recv(&mut self) -> Option<SignalMsg> {
        self.rx.recv().await
    }

    fn peer(&self) -> String {
        if self.ip.is_empty() {
            format!("vds:{}", self.peer)
        } else {
            format!("{}|vds:{}", self.ip, self.peer)
        }
    }
}

pub async fn join(addr: &VdsAddr, pin: Option<&str>, room: &str) -> io::Result<(Box<dyn Signaler>, IceConfig)> {
    let ws = open(addr, pin).await?;
    let mut conn = spawn_conn(ws);
    conn.tx
        .send(json!({"op": "join", "room": room, "v": 2}))
        .await
        .map_err(io_err)?;
    let joined = expect(&mut conn, "joined").await?;
    let host = joined
        .get("peer")
        .and_then(|p| p.as_str())
        .unwrap_or_default()
        .to_string();
    let ice = joined.get("ice").map(|i| parse_ice(i, pin)).unwrap_or_default();
    let (sig_tx, sig_rx) = mpsc::channel(64);
    let out = conn.tx.clone();
    let mut rx = std::mem::replace(&mut conn.rx, mpsc::channel(1).1);
    let host_id = host.clone();
    let forward = tokio::spawn(async move {
        while let Some(v) = rx.recv().await {
            match v.get("op").and_then(|o| o.as_str()) {
                Some("signal") if v.get("from").and_then(|f| f.as_str()) == Some(host_id.as_str()) => {
                    if let Some(m) = v
                        .get("data")
                        .and_then(|d| serde_json::from_value::<SignalMsg>(d.clone()).ok())
                        && sig_tx.send(m).await.is_err()
                    {
                        break;
                    }
                }
                Some("left") => {
                    let _ = sig_tx.send(SignalMsg::Bye).await;
                }
                _ => {}
            }
        }
    });
    let mut conn = conn;
    conn._guard.0.push(forward);
    Ok((
        Box::new(PeerSignaler {
            peer: host,
            ip: String::new(),
            out,
            rx: sig_rx,
            _conn: Some(conn),
        }),
        ice,
    ))
}

#[derive(Debug)]
pub enum HostEvent {
    Peer { signaler: Box<dyn Signaler> },
    Online { ice: IceConfig },
}

impl std::fmt::Debug for dyn Signaler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Signaler({})", self.peer())
    }
}

pub struct VdsHost {
    out: mpsc::Sender<Value>,
    ice: Arc<Mutex<IceConfig>>,
    _guard: TaskGuard,
}

impl VdsHost {
    pub fn ice(&self) -> IceConfig {
        self.ice.lock().map(|i| i.clone()).unwrap_or_default()
    }

    pub async fn alias(&self, code_room: &str, ttl_secs: u64) {
        let _ = self
            .out
            .send(json!({"op": "alias", "code": code_room, "ttl": ttl_secs}))
            .await;
    }

    pub async fn unalias(&self, code_room: &str) {
        let _ = self.out.send(json!({"op": "unalias", "code": code_room})).await;
    }

    pub async fn report_failure(&self, peer: &str) {
        if let Some((_, id)) = peer.rsplit_once("vds:") {
            let _ = self.out.send(json!({"op": "fail", "peer": id})).await;
        }
    }
}

pub async fn host(
    addr: &VdsAddr,
    pin: Option<&str>,
    identity: &Identity,
) -> io::Result<(VdsHost, mpsc::Receiver<HostEvent>, mpsc::Receiver<()>)> {
    let ws = open(addr, pin).await?;
    let mut conn = spawn_conn(ws);
    let room = identity.room();
    conn.tx
        .send(json!({"op": "host", "room": room, "pk": b64(&identity.public()), "v": 2}))
        .await
        .map_err(io_err)?;
    let challenge = expect(&mut conn, "challenge").await?;
    let nonce = challenge
        .get("nonce")
        .and_then(|n| n.as_str())
        .and_then(unb64)
        .ok_or_else(|| io_err("bad challenge"))?;
    let sig = identity.sign(&[b"shockingvrc/2/room", room.as_bytes(), &nonce]);
    conn.tx
        .send(json!({"op": "prove", "sig": b64(&sig)}))
        .await
        .map_err(io_err)?;
    let hosting = expect(&mut conn, "hosting").await?;
    let ice = Arc::new(Mutex::new(hosting.get("ice").map(|i| parse_ice(i, pin)).unwrap_or_default()));
    let owned_pin = pin.map(str::to_string);

    let (ev_tx, ev_rx) = mpsc::channel(16);
    let (closed_tx, closed_rx) = mpsc::channel(1);
    let _ = ev_tx.try_send(HostEvent::Online {
        ice: ice.lock().map(|i| i.clone()).unwrap_or_default(),
    });
    let out = conn.tx.clone();
    let ice_store = Arc::clone(&ice);
    let mut rx = std::mem::replace(&mut conn.rx, mpsc::channel(1).1);
    let refresh_out = conn.tx.clone();
    let refresher = tokio::spawn(async move {
        let mut t = tokio::time::interval(Duration::from_secs(600));
        t.tick().await;
        loop {
            t.tick().await;
            if refresh_out.send(json!({"op": "ice"})).await.is_err() {
                break;
            }
        }
    });
    let router_out = conn.tx.clone();
    let router = tokio::spawn(async move {
        let mut peers: HashMap<String, mpsc::Sender<SignalMsg>> = HashMap::new();
        while let Some(v) = rx.recv().await {
            let op = v.get("op").and_then(|o| o.as_str()).unwrap_or_default();
            let peer = v
                .get("peer")
                .or_else(|| v.get("from"))
                .and_then(|p| p.as_str())
                .unwrap_or_default()
                .to_string();
            match op {
                "peer" => {
                    let (tx, rx) = mpsc::channel(64);
                    peers.insert(peer.clone(), tx);
                    let ip = v.get("ip").and_then(|i| i.as_str()).unwrap_or_default().to_string();
                    let signaler = PeerSignaler {
                        peer,
                        ip,
                        out: router_out.clone(),
                        rx,
                        _conn: None,
                    };
                    if ev_tx
                        .send(HostEvent::Peer {
                            signaler: Box::new(signaler),
                        })
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
                "signal" => {
                    if let (Some(tx), Some(m)) = (
                        peers.get(&peer),
                        v.get("data")
                            .and_then(|d| serde_json::from_value::<SignalMsg>(d.clone()).ok()),
                    ) {
                        let _ = tx.try_send(m);
                    }
                }
                "left" => {
                    if let Some(tx) = peers.remove(&peer) {
                        let _ = tx.try_send(SignalMsg::Bye);
                    }
                }
                "ice" => {
                    if let (Some(i), Ok(mut store)) = (v.get("ice"), ice_store.lock()) {
                        *store = parse_ice(i, owned_pin.as_deref());
                    }
                }
                _ => {}
            }
            peers.retain(|_, tx| !tx.is_closed());
        }
        let _ = closed_tx.try_send(());
    });
    let mut guard = conn._guard;
    guard.0.push(refresher);
    guard.0.push(router);
    Ok((
        VdsHost {
            out,
            ice,
            _guard: guard,
        },
        ev_rx,
        closed_rx,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_addresses_parse() {
        assert_eq!(
            VdsAddr::parse("https://vds.example.com/").unwrap().ws_url(),
            "wss://vds.example.com:443/ws"
        );
        assert_eq!(VdsAddr::parse("203.0.113.5").unwrap().ws_url(), "wss://203.0.113.5:443/ws");
        assert_eq!(
            VdsAddr::parse("http://127.0.0.1:8080").unwrap().ws_url(),
            "ws://127.0.0.1:8080/ws"
        );
        assert_eq!(VdsAddr::parse("wss://[::1]:9000").unwrap().ws_url(), "wss://[::1]:9000/ws");
        assert!(VdsAddr::parse("").is_none());
    }
}
