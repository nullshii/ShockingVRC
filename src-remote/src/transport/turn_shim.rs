use std::collections::HashMap;
use std::io;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::{TcpStream, UdpSocket};
use tokio::sync::mpsc;

use super::direct::TaskGuard;
use super::lan;

const MAX_MESSAGE: usize = 65_535 + 4;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnUrl {
    pub secure: bool,
    pub host: String,
    pub port: u16,
}

pub fn parse_turn_url(url: &str) -> Option<TurnUrl> {
    let (secure, rest) = if let Some(r) = url.strip_prefix("turns:") {
        (true, r)
    } else {
        (false, url.strip_prefix("turn:")?)
    };
    let hostport = rest.split('?').next()?.trim_start_matches("//");
    let (host, port) = match hostport.rsplit_once(':') {
        Some((h, p)) if !h.ends_with(']') || h.starts_with('[') => (h.to_string(), p.parse().ok()?),
        _ => (hostport.to_string(), if secure { 5349 } else { 3478 }),
    };
    let host = host.trim_start_matches('[').trim_end_matches(']').to_string();
    (!host.is_empty()).then_some(TurnUrl { secure, host, port })
}

pub fn stream_frame_len(buf: &[u8]) -> Option<usize> {
    if buf.len() < 4 {
        return None;
    }
    let len = u16::from_be_bytes([buf[2], buf[3]]) as usize;
    let total = match buf[0] >> 6 {
        0 => 20 + len,
        1 => 4 + len.div_ceil(4) * 4,
        _ => 4 + len,
    };
    (buf.len() >= total).then_some(total)
}

pub fn pad_for_stream(datagram: &[u8]) -> Vec<u8> {
    let mut out = datagram.to_vec();
    if datagram.first().is_some_and(|b| b >> 6 == 1) {
        while out.len() % 4 != 0 {
            out.push(0);
        }
    }
    out
}

pub fn unpad_for_datagram(msg: &[u8]) -> &[u8] {
    if msg.len() >= 4 && msg[0] >> 6 == 1 {
        let len = u16::from_be_bytes([msg[2], msg[3]]) as usize;
        &msg[..(4 + len).min(msg.len())]
    } else {
        msg
    }
}

trait Stream: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> Stream for T {}

async fn open_stream(url: &TurnUrl, pin: Option<&str>) -> io::Result<Box<dyn Stream>> {
    let tcp = TcpStream::connect((url.host.as_str(), url.port)).await?;
    let _ = tcp.set_nodelay(true);
    if !url.secure {
        return Ok(Box::new(tcp));
    }
    let mut cfg = (*crate::tls::client_for(pin).map_err(io::Error::other)?).clone();
    cfg.alpn_protocols = vec![b"stun.turn".to_vec()];
    let name = rustls::pki_types::ServerName::try_from(url.host.clone()).map_err(io::Error::other)?;
    let tls = tokio_rustls::TlsConnector::from(Arc::new(cfg))
        .connect(name, tcp)
        .await?;
    Ok(Box::new(tls))
}

pub struct TurnShim {
    local: SocketAddr,
    advertised: IpAddr,
    _task: TaskGuard,
}

impl TurnShim {
    pub async fn start(url: &str) -> io::Result<Self> {
        Self::start_pinned(url, None).await
    }

    pub async fn start_pinned(url: &str, pin: Option<String>) -> io::Result<Self> {
        let target = parse_turn_url(url).ok_or_else(|| io::Error::other("bad TURN url"))?;
        drop(open_stream(&target, pin.as_deref()).await?);
        let advertised = lan::local_ipv4()
            .first()
            .map(|v4| IpAddr::V4(*v4))
            .unwrap_or(IpAddr::V4(Ipv4Addr::LOCALHOST));
        let bind_ip = if advertised.is_loopback() {
            IpAddr::V4(Ipv4Addr::LOCALHOST)
        } else {
            IpAddr::V4(Ipv4Addr::UNSPECIFIED)
        };
        let socket = Arc::new(UdpSocket::bind(SocketAddr::new(bind_ip, 0)).await?);
        let local = socket.local_addr()?;
        let task = tokio::spawn(run(socket, target, pin));
        Ok(Self {
            local,
            advertised,
            _task: TaskGuard(vec![task]),
        })
    }

    pub fn local_url(&self) -> String {
        format!("turn:{}:{}?transport=udp", self.advertised, self.local.port())
    }
}

async fn run(socket: Arc<UdpSocket>, target: TurnUrl, pin: Option<String>) {
    let mut writers: HashMap<SocketAddr, mpsc::Sender<Vec<u8>>> = HashMap::new();
    let mut buf = vec![0u8; MAX_MESSAGE];
    let mut tasks = Vec::new();
    loop {
        let Ok((n, from)) = socket.recv_from(&mut buf).await else {
            break;
        };
        if !lan::is_local_address(&from.ip()) {
            continue;
        }
        writers.retain(|_, tx| !tx.is_closed());
        let tx = match writers.get(&from) {
            Some(tx) => tx.clone(),
            None => {
                let (tx, rx) = mpsc::channel::<Vec<u8>>(256);
                tasks.push(TaskGuard(vec![tokio::spawn(relay(
                    Arc::clone(&socket),
                    from,
                    target.clone(),
                    pin.clone(),
                    rx,
                ))]));
                writers.insert(from, tx.clone());
                tx
            }
        };
        let _ = tx.try_send(pad_for_stream(&buf[..n]));
    }
}

async fn relay(
    socket: Arc<UdpSocket>,
    client: SocketAddr,
    target: TurnUrl,
    pin: Option<String>,
    mut rx: mpsc::Receiver<Vec<u8>>,
) {
    let Ok(stream) = open_stream(&target, pin.as_deref()).await else {
        return;
    };
    let (mut rd, mut wr) = tokio::io::split(stream);
    let up = async {
        while let Some(bytes) = rx.recv().await {
            if wr.write_all(&bytes).await.is_err() {
                break;
            }
        }
    };
    let down = async {
        let mut acc: Vec<u8> = Vec::with_capacity(4096);
        let mut chunk = vec![0u8; 16 * 1024];
        loop {
            let Ok(n) = rd.read(&mut chunk).await else { break };
            if n == 0 {
                break;
            }
            acc.extend_from_slice(&chunk[..n]);
            while let Some(len) = stream_frame_len(&acc) {
                let _ = socket.send_to(unpad_for_datagram(&acc[..len]), client).await;
                acc.drain(..len);
            }
            if acc.len() > MAX_MESSAGE {
                break;
            }
        }
    };
    tokio::select! {
        _ = up => {}
        _ = down => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_parse_with_defaults() {
        assert_eq!(
            parse_turn_url("turns:vds.example:443?transport=tcp"),
            Some(TurnUrl {
                secure: true,
                host: "vds.example".into(),
                port: 443
            })
        );
        assert_eq!(parse_turn_url("turn:1.2.3.4").map(|u| u.port), Some(3478));
        assert_eq!(parse_turn_url("turns:h").map(|u| u.port), Some(5349));
        assert!(parse_turn_url("stun:h:1").is_none());
    }

    #[test]
    fn stream_framing_handles_stun_and_padded_channel_data() {
        let mut stun = vec![0x00, 0x01, 0x00, 0x04];
        stun.extend_from_slice(&[0u8; 16]);
        stun.extend_from_slice(&[9, 9, 9, 9]);
        assert_eq!(stream_frame_len(&stun), Some(24));
        assert_eq!(stream_frame_len(&stun[..10]), None);

        let data = [0x40, 0x00, 0x00, 0x05, 1, 2, 3, 4, 5];
        let padded = pad_for_stream(&data);
        assert_eq!(padded.len(), 12);
        assert_eq!(stream_frame_len(&padded), Some(12));
        assert_eq!(unpad_for_datagram(&padded), &data[..]);
    }
}
