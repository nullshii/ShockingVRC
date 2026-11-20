use std::io;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio_rustls::{TlsAcceptor, TlsConnector};
use tokio_tungstenite::tungstenite::Message as WsMessage;
use tokio_tungstenite::tungstenite::handshake::server::{Request, Response};

use super::lan::is_private;
use super::rtc::{SignalMsg, Signaler};
use super::{CHANNEL_DEPTH, Channel, ChannelInfo, PathKind, RemoteTransport};
use crate::crypto::{sha256, unb64};
use crate::store::TlsMaterial;
use crate::tls;

pub const SESSION_PATH: &str = "/svrc/2/session";
pub const ICE_PATH: &str = "/svrc/2/ice";
pub const DEFAULT_PORT: u16 = 47390;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(4);

pub struct TaskGuard(pub Vec<JoinHandle<()>>);

impl Drop for TaskGuard {
    fn drop(&mut self) {
        for t in &self.0 {
            t.abort();
        }
    }
}

fn io_err(e: impl std::fmt::Display) -> io::Error {
    io::Error::other(e.to_string())
}

fn pump<S>(ws: tokio_tungstenite::WebSocketStream<S>) -> (mpsc::Sender<Vec<u8>>, mpsc::Receiver<Vec<u8>>, TaskGuard)
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let (mut sink, mut stream) = ws.split();
    let (out_tx, mut out_rx) = mpsc::channel::<Vec<u8>>(CHANNEL_DEPTH);
    let (in_tx, in_rx) = mpsc::channel::<Vec<u8>>(CHANNEL_DEPTH);
    let _writer = tokio::spawn(async move {
        while let Some(bytes) = out_rx.recv().await {
            let sent = tokio::time::timeout(CONNECT_TIMEOUT, sink.send(WsMessage::Binary(bytes.into()))).await;
            if !matches!(sent, Ok(Ok(()))) {
                break;
            }
        }
        let _ = tokio::time::timeout(Duration::from_secs(1), sink.close()).await;
    });
    let reader = tokio::spawn(async move {
        while let Some(Ok(msg)) = stream.next().await {
            match msg {
                WsMessage::Binary(b) => {
                    if in_tx.send(b.to_vec()).await.is_err() {
                        break;
                    }
                }
                WsMessage::Text(t) => {
                    if in_tx.send(t.as_bytes().to_vec()).await.is_err() {
                        break;
                    }
                }
                WsMessage::Close(_) => break,
                _ => {}
            }
        }
    });
    (out_tx, in_rx, TaskGuard(vec![reader]))
}

pub enum Incoming {
    Session(Channel),
    Ice(Box<dyn Signaler>),
}

pub struct DirectListener {
    pub addr: SocketAddr,
    pub pin: String,
    _task: TaskGuard,
}

pub async fn listen(
    bind: SocketAddr,
    tls_material: &TlsMaterial,
) -> io::Result<(DirectListener, mpsc::Receiver<Incoming>)> {
    let config = tls::server_config(tls_material).map_err(io_err)?;
    let cert = unb64(&tls_material.cert_der).ok_or_else(|| io_err("bad certificate"))?;
    let binding = sha256(&[&cert]).to_vec();
    let pin = tls::pin_of(&cert);
    let listener = match TcpListener::bind(bind).await {
        Ok(l) => l,
        Err(e) if bind.port() != 0 => {
            log::warn!("[remote] Port {} is busy ({e}); using a random port", bind.port());
            TcpListener::bind(SocketAddr::new(bind.ip(), 0)).await?
        }
        Err(e) => return Err(e),
    };
    let addr = listener.local_addr()?;
    let (tx, rx) = mpsc::channel(8);
    let acceptor = TlsAcceptor::from(config);
    let task = tokio::spawn(async move {
        loop {
            let Ok((tcp, peer)) = listener.accept().await else {
                tokio::time::sleep(Duration::from_millis(100)).await;
                continue;
            };
            let acceptor = acceptor.clone();
            let tx = tx.clone();
            let binding = binding.clone();
            tokio::spawn(async move {
                let _ = tcp.set_nodelay(true);
                let tls = match tokio::time::timeout(CONNECT_TIMEOUT, acceptor.accept(tcp)).await {
                    Ok(Ok(s)) => s,
                    _ => return,
                };
                let path = Arc::new(Mutex::new(String::new()));
                let seen = Arc::clone(&path);
                #[allow(clippy::result_large_err)]
                let cb = move |req: &Request, resp: Response| {
                    if let Ok(mut p) = seen.lock() {
                        *p = req.uri().path().to_string();
                    }
                    Ok(resp)
                };
                let ws = match tokio::time::timeout(CONNECT_TIMEOUT, tokio_tungstenite::accept_hdr_async(tls, cb)).await
                {
                    Ok(Ok(ws)) => ws,
                    _ => return,
                };
                let path = path.lock().map(|p| p.clone()).unwrap_or_default();
                let (out_tx, in_rx, guard) = pump(ws);
                let incoming = if path == ICE_PATH {
                    Incoming::Ice(Box::new(WsSignaler {
                        tx: out_tx,
                        rx: in_rx,
                        _guard: guard,
                    }))
                } else if path == SESSION_PATH {
                    Incoming::Session(Channel {
                        tx: out_tx,
                        rx: in_rx,
                        info: ChannelInfo {
                            path: if is_private(&peer.ip()) {
                                PathKind::Lan
                            } else {
                                PathKind::P2p
                            },
                            binding,
                            peer: peer.ip().to_string(),
                            label: "direct".into(),
                        },
                        guard: Box::new(guard),
                    })
                } else {
                    return;
                };
                let _ = tx.send(incoming).await;
            });
        }
    });
    Ok((
        DirectListener {
            addr,
            pin,
            _task: TaskGuard(vec![task]),
        },
        rx,
    ))
}

async fn connect_ws(
    addr: &str,
    pin: &str,
    path: &str,
) -> io::Result<(
    tokio_tungstenite::WebSocketStream<tokio_rustls::client::TlsStream<TcpStream>>,
    Vec<u8>,
)> {
    let config = tls::pinned_client(pin).map_err(io_err)?;
    let tcp = tokio::time::timeout(CONNECT_TIMEOUT, TcpStream::connect(addr))
        .await
        .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "connect timed out"))??;
    let _ = tcp.set_nodelay(true);
    let name = rustls::pki_types::ServerName::try_from("shockingvrc.local").map_err(io_err)?;
    let tls_stream = tokio::time::timeout(CONNECT_TIMEOUT, TlsConnector::from(config).connect(name, tcp))
        .await
        .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "TLS timed out"))??;
    let cert = tls_stream
        .get_ref()
        .1
        .peer_certificates()
        .and_then(|c| c.first())
        .map(|c| c.as_ref().to_vec())
        .ok_or_else(|| io_err("no certificate"))?;
    let url = format!("wss://shockingvrc.local{path}");
    let (ws, _) = tokio::time::timeout(CONNECT_TIMEOUT, tokio_tungstenite::client_async(url, tls_stream))
        .await
        .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "WebSocket timed out"))?
        .map_err(io_err)?;
    Ok((ws, sha256(&[&cert]).to_vec()))
}

pub struct DirectTransport {
    pub addr: String,
    pub pin: String,
}

#[async_trait::async_trait]
impl RemoteTransport for DirectTransport {
    fn name(&self) -> String {
        format!("direct {}", self.addr)
    }

    fn path(&self) -> PathKind {
        match self.addr.parse::<SocketAddr>() {
            Ok(a) if !is_private(&a.ip()) => PathKind::P2p,
            _ => PathKind::Lan,
        }
    }

    async fn dial(&self) -> io::Result<Channel> {
        let (ws, binding) = connect_ws(&self.addr, &self.pin, SESSION_PATH).await?;
        let (tx, rx, guard) = pump(ws);
        Ok(Channel {
            tx,
            rx,
            info: ChannelInfo {
                path: self.path(),
                binding,
                peer: self.addr.clone(),
                label: "direct".into(),
            },
            guard: Box::new(guard),
        })
    }
}

pub struct WsSignaler {
    tx: mpsc::Sender<Vec<u8>>,
    rx: mpsc::Receiver<Vec<u8>>,
    _guard: TaskGuard,
}

impl WsSignaler {
    pub async fn connect(addr: &str, pin: &str) -> io::Result<Self> {
        let (ws, _) = connect_ws(addr, pin, ICE_PATH).await?;
        let (tx, rx, guard) = pump(ws);
        Ok(Self { tx, rx, _guard: guard })
    }
}

#[async_trait::async_trait]
impl Signaler for WsSignaler {
    async fn send(&mut self, msg: SignalMsg) -> io::Result<()> {
        let bytes = serde_json::to_vec(&msg).map_err(io_err)?;
        self.tx
            .send(bytes)
            .await
            .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "signaling closed"))
    }

    async fn recv(&mut self) -> Option<SignalMsg> {
        loop {
            let bytes = self.rx.recv().await?;
            if let Ok(m) = serde_json::from_slice(&bytes) {
                return Some(m);
            }
        }
    }

    fn peer(&self) -> String {
        "lan".into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::Message;

    #[tokio::test]
    async fn a_pinned_direct_connection_carries_messages() {
        let material = tls::generate();
        let (listener, mut incoming) = listen("127.0.0.1:0".parse().unwrap(), &material).await.unwrap();
        let t = DirectTransport {
            addr: listener.addr.to_string(),
            pin: listener.pin.clone(),
        };
        let client = tokio::spawn(async move { t.dial().await });
        let Some(Incoming::Session(mut server)) = incoming.recv().await else {
            panic!("expected a session");
        };
        let mut client = client.await.unwrap().unwrap();
        assert_eq!(client.info.binding, server.info.binding, "both ends see the same certificate");
        client.send(&Message::Ping { seq: 1, ts: 2 }).await.unwrap();
        assert_eq!(server.recv().await.unwrap().unwrap(), Message::Ping { seq: 1, ts: 2 });
        server.send(&Message::Bye).await.unwrap();
        assert_eq!(client.recv().await.unwrap().unwrap(), Message::Bye);
    }

    #[tokio::test]
    async fn a_wrong_pin_is_refused() {
        let material = tls::generate();
        let other = tls::generate();
        let (listener, _incoming) = listen("127.0.0.1:0".parse().unwrap(), &material).await.unwrap();
        let t = DirectTransport {
            addr: listener.addr.to_string(),
            pin: tls::material_pin(&other).unwrap(),
        };
        assert!(t.dial().await.is_err());
    }
}
