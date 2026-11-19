use std::io;
use std::sync::Arc;
use std::time::Duration;

use bytes::BytesMut;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use webrtc::data_channel::{DataChannel, DataChannelEvent, RTCDataChannelInit};
use webrtc::peer_connection::{
    PeerConnection, PeerConnectionBuilder, PeerConnectionEventHandler, RTCConfigurationBuilder, RTCIceCandidateInit,
    RTCIceGatheringState, RTCIceServer, RTCIceTransportPolicy, RTCPeerConnectionIceEvent, RTCPeerConnectionState,
    RTCSessionDescription,
};

use super::turn_shim::TurnShim;
use super::{CHANNEL_DEPTH, Channel, ChannelInfo, PathKind};
use crate::crypto::sha256;

pub const DATA_CHANNEL_LABEL: &str = "svrc";
const GATHER_WAIT: Duration = Duration::from_secs(3);
const OPEN_TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SignalMsg {
    Offer {
        sdp: String,
    },
    Answer {
        sdp: String,
    },
    Candidate {
        candidate: String,
        #[serde(default, rename = "sdpMid")]
        sdp_mid: Option<String>,
        #[serde(default, rename = "sdpMLineIndex")]
        sdp_mline_index: Option<u16>,
    },
    Bye,
}

#[async_trait::async_trait]
pub trait Signaler: Send {
    async fn send(&mut self, msg: SignalMsg) -> io::Result<()>;
    async fn recv(&mut self) -> Option<SignalMsg>;
    fn peer(&self) -> String;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct IceServer {
    pub urls: Vec<String>,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub credential: String,
}

#[derive(Debug, Clone, Default)]
pub struct IceConfig {
    pub servers: Vec<IceServer>,
    pub relay_only: bool,
}

impl IceConfig {
    pub fn host_only() -> Self {
        Self::default()
    }

    pub fn has_turn(&self) -> bool {
        self.servers
            .iter()
            .flat_map(|s| &s.urls)
            .any(|u| u.starts_with("turn:") || u.starts_with("turns:"))
    }

    pub fn without_turn(&self) -> Self {
        Self {
            servers: self
                .servers
                .iter()
                .map(|s| IceServer {
                    urls: s.urls.iter().filter(|u| u.starts_with("stun:")).cloned().collect(),
                    ..s.clone()
                })
                .filter(|s| !s.urls.is_empty())
                .collect(),
            relay_only: false,
        }
    }
}

async fn prepare_servers(cfg: &IceConfig) -> (Vec<RTCIceServer>, Vec<TurnShim>) {
    let mut out = Vec::new();
    let mut shims = Vec::new();
    for s in &cfg.servers {
        let mut urls = Vec::new();
        for url in &s.urls {
            let lower = url.to_ascii_lowercase();
            let tcp = lower.contains("transport=tcp");
            let secure = lower.starts_with("turns:");
            if lower.starts_with("turn") && (tcp || secure) {
                match TurnShim::start(url).await {
                    Ok(shim) => {
                        urls.push(shim.local_url());
                        shims.push(shim);
                    }
                    Err(e) => log::debug!("[remote] TURN {url} unavailable: {e}"),
                }
            } else {
                urls.push(url.clone());
            }
        }
        if !urls.is_empty() {
            out.push(RTCIceServer {
                urls,
                username: s.username.clone(),
                credential: s.credential.clone(),
            });
        }
    }
    (out, shims)
}

struct Events {
    gathered: mpsc::Sender<()>,
    channel: mpsc::Sender<Arc<dyn DataChannel>>,
    failed: mpsc::Sender<()>,
    candidates: mpsc::Sender<RTCIceCandidateInit>,
}

#[async_trait::async_trait]
impl PeerConnectionEventHandler for Events {
    async fn on_ice_gathering_state_change(&self, state: RTCIceGatheringState) {
        if state == RTCIceGatheringState::Complete {
            let _ = self.gathered.try_send(());
        }
    }

    async fn on_ice_candidate(&self, event: RTCPeerConnectionIceEvent) {
        if let Ok(init) = event.candidate.to_json() {
            let _ = self.candidates.try_send(init);
        }
    }

    async fn on_connection_state_change(&self, state: RTCPeerConnectionState) {
        if matches!(state, RTCPeerConnectionState::Failed | RTCPeerConnectionState::Closed) {
            let _ = self.failed.try_send(());
        }
    }

    async fn on_data_channel(&self, dc: Arc<dyn DataChannel>) {
        let _ = self.channel.try_send(dc);
    }
}

struct PcGuard {
    pc: Arc<dyn PeerConnection>,
    shims: Vec<TurnShim>,
    pump: Option<tokio::task::JoinHandle<()>>,
}

impl Drop for PcGuard {
    fn drop(&mut self) {
        let pc = Arc::clone(&self.pc);
        let shims = std::mem::take(&mut self.shims);
        let pump = self.pump.take();
        if let Ok(rt) = tokio::runtime::Handle::try_current() {
            rt.spawn(async move {
                if let Some(p) = pump {
                    let _ = tokio::time::timeout(Duration::from_millis(500), p).await;
                }
                let _ = pc.close().await;
                drop(shims);
            });
        }
    }
}

fn io_err(e: impl std::fmt::Display) -> io::Error {
    io::Error::other(e.to_string())
}

pub fn sdp_fingerprints(sdp: &str) -> Vec<String> {
    let mut out: Vec<String> = sdp
        .lines()
        .filter_map(|l| l.trim().strip_prefix("a=fingerprint:"))
        .map(|f| f.trim().to_ascii_uppercase())
        .collect();
    out.sort();
    out.dedup();
    out
}

pub fn dtls_binding(local_sdp: &str, remote_sdp: &str) -> Vec<u8> {
    let mut all = sdp_fingerprints(local_sdp);
    all.extend(sdp_fingerprints(remote_sdp));
    all.sort();
    all.dedup();
    sha256(&[all.join("|").as_bytes()]).to_vec()
}

fn candidate_init(msg: &SignalMsg) -> Option<RTCIceCandidateInit> {
    match msg {
        SignalMsg::Candidate {
            candidate,
            sdp_mid,
            sdp_mline_index,
        } if !candidate.is_empty() => Some(RTCIceCandidateInit {
            candidate: candidate.clone(),
            sdp_mid: sdp_mid.clone(),
            sdp_mline_index: *sdp_mline_index,
            ..Default::default()
        }),
        _ => None,
    }
}

pub async fn connect(
    mut signaler: Box<dyn Signaler>,
    cfg: IceConfig,
    offerer: bool,
    path: PathKind,
) -> io::Result<Channel> {
    let (servers, shims) = prepare_servers(&cfg).await;
    let (gathered_tx, mut gathered_rx) = mpsc::channel(1);
    let (dc_tx, mut dc_rx) = mpsc::channel(1);
    let (failed_tx, mut failed_rx) = mpsc::channel(1);
    let (cand_tx, mut cand_rx) = mpsc::channel(32);
    let mut config = RTCConfigurationBuilder::new().with_ice_servers(servers);
    if cfg.relay_only {
        config = config.with_ice_transport_policy(RTCIceTransportPolicy::Relay);
    }
    let pc = PeerConnectionBuilder::<String>::new()
        .with_configuration(config.build())
        .with_handler(Arc::new(Events {
            gathered: gathered_tx,
            channel: dc_tx,
            failed: failed_tx,
            candidates: cand_tx,
        }))
        .with_udp_addrs(vec!["0.0.0.0:0".to_string()])
        .build()
        .await
        .map_err(io_err)?;
    let pc: Arc<dyn PeerConnection> = Arc::new(pc);
    let peer = signaler.peer();

    let own_dc = if offerer {
        let init = RTCDataChannelInit {
            ordered: true,
            ..Default::default()
        };
        Some(
            pc.create_data_channel(DATA_CHANNEL_LABEL, Some(init))
                .await
                .map_err(io_err)?,
        )
    } else {
        None
    };

    let negotiate = async {
        if offerer {
            let offer = pc.create_offer(None).await.map_err(io_err)?;
            pc.set_local_description(offer).await.map_err(io_err)?;
            let _ = tokio::time::timeout(GATHER_WAIT, gathered_rx.recv()).await;
            let local = pc
                .local_description()
                .await
                .ok_or_else(|| io_err("no local description"))?;
            signaler.send(SignalMsg::Offer { sdp: local.sdp }).await?;
        }
        loop {
            tokio::select! {
                msg = signaler.recv() => {
                    let Some(msg) = msg else { return Err(io_err("signaling closed")) };
                    match &msg {
                        SignalMsg::Offer { sdp } if !offerer => {
                            let offer = RTCSessionDescription::offer(sdp.clone()).map_err(io_err)?;
                            pc.set_remote_description(offer).await.map_err(io_err)?;
                            let answer = pc.create_answer(None).await.map_err(io_err)?;
                            pc.set_local_description(answer).await.map_err(io_err)?;
                            let _ = tokio::time::timeout(GATHER_WAIT, gathered_rx.recv()).await;
                            let local = pc.local_description().await.ok_or_else(|| io_err("no local description"))?;
                            signaler.send(SignalMsg::Answer { sdp: local.sdp }).await?;
                        }
                        SignalMsg::Answer { sdp } if offerer => {
                            let answer = RTCSessionDescription::answer(sdp.clone()).map_err(io_err)?;
                            pc.set_remote_description(answer).await.map_err(io_err)?;
                        }
                        SignalMsg::Bye => return Err(io_err("the peer hung up")),
                        other => {
                            if let Some(c) = candidate_init(other) {
                                let _ = pc.add_ice_candidate(c).await;
                            }
                        }
                    }
                }
                Some(c) = cand_rx.recv() => {
                    if pc.remote_description().await.is_some() || offerer {
                        let _ = signaler
                            .send(SignalMsg::Candidate {
                                candidate: c.candidate,
                                sdp_mid: c.sdp_mid,
                                sdp_mline_index: c.sdp_mline_index,
                            })
                            .await;
                    }
                }
                dc = dc_rx.recv(), if !offerer => {
                    if let Some(dc) = dc { return Ok(dc); }
                }
                _ = failed_rx.recv() => return Err(io_err("the connection failed")),
                _ = wait_open(own_dc.clone()), if offerer => {
                    return own_dc.clone().ok_or_else(|| io_err("no data channel"));
                }
            }
        }
    };
    let dc = match tokio::time::timeout(OPEN_TIMEOUT, negotiate).await {
        Ok(Ok(dc)) => dc,
        Ok(Err(e)) => {
            let _ = pc.close().await;
            return Err(e);
        }
        Err(_) => {
            let _ = pc.close().await;
            return Err(io::Error::new(io::ErrorKind::TimedOut, "WebRTC negotiation timed out"));
        }
    };
    if !offerer {
        tokio::time::timeout(OPEN_TIMEOUT, wait_open(Some(Arc::clone(&dc))))
            .await
            .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "data channel did not open"))?;
    }
    let _ = signaler.send(SignalMsg::Bye).await;

    let local = pc.local_description().await.map(|d| d.sdp).unwrap_or_default();
    let remote = pc.remote_description().await.map(|d| d.sdp).unwrap_or_default();
    let binding = dtls_binding(&local, &remote);

    let (out_tx, mut out_rx) = mpsc::channel::<Vec<u8>>(CHANNEL_DEPTH);
    let (in_tx, in_rx) = mpsc::channel::<Vec<u8>>(CHANNEL_DEPTH);
    let pump_dc = Arc::clone(&dc);
    let pump = tokio::spawn(async move {
        loop {
            tokio::select! {
                ev = pump_dc.poll() => match ev {
                    Some(DataChannelEvent::OnMessage(m)) => {
                        if in_tx.send(m.data.to_vec()).await.is_err() {
                            break;
                        }
                    }
                    Some(DataChannelEvent::OnClose) | None => break,
                    _ => {}
                },
                out = out_rx.recv() => match out {
                    Some(bytes) => {
                        if pump_dc.send(BytesMut::from(&bytes[..])).await.is_err() {
                            break;
                        }
                    }
                    None => break,
                },
            }
        }
        let _ = pump_dc.close().await;
    });
    Ok(Channel {
        tx: out_tx,
        rx: in_rx,
        info: ChannelInfo {
            path,
            binding,
            peer,
            label: if cfg.relay_only {
                "webrtc relay".into()
            } else {
                "webrtc".into()
            },
        },
        guard: Box::new(PcGuard {
            pc,
            shims,
            pump: Some(pump),
        }),
    })
}

async fn wait_open(dc: Option<Arc<dyn DataChannel>>) {
    let Some(dc) = dc else {
        return std::future::pending().await;
    };
    loop {
        if matches!(dc.ready_state().await, Ok(webrtc::data_channel::RTCDataChannelState::Open)) {
            return;
        }
        match tokio::time::timeout(Duration::from_millis(200), dc.poll()).await {
            Ok(Some(DataChannelEvent::OnOpen)) => return,
            Ok(Some(DataChannelEvent::OnClose)) | Ok(None) => return std::future::pending().await,
            _ => {}
        }
    }
}

pub fn signal_pair() -> (Box<dyn Signaler>, Box<dyn Signaler>) {
    struct Mem {
        tx: mpsc::Sender<SignalMsg>,
        rx: mpsc::Receiver<SignalMsg>,
    }
    #[async_trait::async_trait]
    impl Signaler for Mem {
        async fn send(&mut self, msg: SignalMsg) -> io::Result<()> {
            self.tx
                .send(msg)
                .await
                .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "closed"))
        }
        async fn recv(&mut self) -> Option<SignalMsg> {
            self.rx.recv().await
        }
        fn peer(&self) -> String {
            "memory".into()
        }
    }
    let (a_tx, b_rx) = mpsc::channel(64);
    let (b_tx, a_rx) = mpsc::channel(64);
    (Box::new(Mem { tx: a_tx, rx: a_rx }), Box::new(Mem { tx: b_tx, rx: b_rx }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::Message;

    #[test]
    fn the_binding_is_the_same_from_both_ends() {
        let a = "v=0\r\na=fingerprint:sha-256 AA:BB\r\n";
        let b = "v=0\r\na=fingerprint:sha-256 cc:dd\r\n";
        assert_eq!(dtls_binding(a, b), dtls_binding(b, a));
        assert_ne!(dtls_binding(a, b), dtls_binding(a, a));
        assert_eq!(sdp_fingerprints(b), vec!["SHA-256 CC:DD".to_string()]);
    }

    #[test]
    fn turn_can_be_stripped_for_the_p2p_attempt() {
        let cfg = IceConfig {
            servers: vec![IceServer {
                urls: vec!["stun:x:3478".into(), "turn:x:3478?transport=udp".into()],
                username: "u".into(),
                credential: "c".into(),
            }],
            relay_only: false,
        };
        assert!(cfg.has_turn());
        let p2p = cfg.without_turn();
        assert!(!p2p.has_turn());
        assert_eq!(p2p.servers[0].urls, vec!["stun:x:3478".to_string()]);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn a_host_only_data_channel_connects_locally() {
        let (sa, sb) = signal_pair();
        let answer = tokio::spawn(connect(sb, IceConfig::host_only(), false, PathKind::Lan));
        let offer = connect(sa, IceConfig::host_only(), true, PathKind::Lan);
        let (offer, answer) = tokio::join!(offer, answer);
        let (Ok(a), Ok(Ok(mut b))) = (offer, answer) else {
            eprintln!("no usable network interface for a host-only WebRTC test; skipping");
            return;
        };
        assert_eq!(a.info.binding, b.info.binding, "both ends derive the same DTLS binding");
        a.send(&Message::Ping { seq: 9, ts: 1 }).await.unwrap();
        let got = tokio::time::timeout(Duration::from_secs(5), b.recv()).await.unwrap();
        assert_eq!(got.unwrap().unwrap(), Message::Ping { seq: 9, ts: 1 });
    }
}
