use std::collections::{HashMap, VecDeque};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, oneshot, watch};
use tokio::task::JoinHandle;

use crate::crypto::{self, b64, unb64};
use crate::handshake::{self, GrantFound, HandshakeError, JoinGrant, JoinerCtx};
use crate::jitter::JitterBuffer;
use crate::limiter::FailLimiter;
use crate::link::{self, LinkBody, OfferedRole, RouteHints};
use crate::protocol::{
    ClientKind, FeedbackKind, Message, OutputReport, PolicySnapshot, RemoteFrame, SlotFrame, Welcome,
};
use crate::store::{self, Lifetime, PartnerRecord, TrustStore};
use crate::tls;
use crate::transport::dialers::{LanIceTransport, VdsRtcTransport};
use crate::transport::direct::{self, DirectTransport, Incoming};
use crate::transport::race::{self, Attempt, Strategy};
use crate::transport::rtc::{self, IceConfig};
use crate::transport::signal::{self, HostEvent, VdsAddr, VdsHost};
use crate::transport::{Channel, PathKind, RemoteTransport, lan};

pub const HEARTBEAT: Duration = Duration::from_millis(250);
pub const FAILSAFE: Duration = Duration::from_millis(1000);
pub const DEAD_AFTER: Duration = Duration::from_secs(6);
const FRAME_TICK: Duration = Duration::from_millis(25);
const OUTPUT_EVERY: Duration = Duration::from_millis(200);
const CONSENT_TIMEOUT: Duration = Duration::from_secs(60);
const TOKEN_TTL: Duration = Duration::from_secs(300);
const RESUME_WINDOW: Duration = Duration::from_secs(30);
const MAX_FRAMES_PER_SEC: f32 = 100.0;
const LOG_LEN: usize = 500;
const BAD_MESSAGE_LOG_EVERY: Duration = Duration::from_secs(5);
const FEEDBACK_MIN_GAP_MS: u64 = 250;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ConnectionMode {
    #[default]
    Automatic,
    Internet,
    LanOnly,
    DirectHybrid,
}

impl ConnectionMode {
    pub const ALL: [ConnectionMode; 4] = [
        ConnectionMode::Automatic,
        ConnectionMode::Internet,
        ConnectionMode::LanOnly,
        ConnectionMode::DirectHybrid,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ConnectionMode::Automatic => "Automatic",
            ConnectionMode::Internet => "Internet",
            ConnectionMode::LanOnly => "LAN only",
            ConnectionMode::DirectHybrid => "Direct hybrid",
        }
    }

    fn uses_direct(self) -> bool {
        !matches!(self, ConnectionMode::Internet)
    }

    fn uses_vds(self) -> bool {
        !matches!(self, ConnectionMode::LanOnly)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct RemoteConfig {
    pub mode: ConnectionMode,
    pub name: String,
    pub vds_url: String,
    pub vds_pin: String,
    pub lan_port: u16,
    pub public_addr: String,
    pub jitter_ms: u32,
}

impl Default for RemoteConfig {
    fn default() -> Self {
        Self {
            mode: ConnectionMode::Automatic,
            name: String::new(),
            vds_url: String::new(),
            vds_pin: String::new(),
            lan_port: direct::DEFAULT_PORT,
            public_addr: String::new(),
            jitter_ms: 100,
        }
    }
}

impl RemoteConfig {
    fn vds(&self) -> Option<VdsAddr> {
        VdsAddr::parse(&self.vds_url)
    }

    fn vds_pin(&self) -> Option<String> {
        Some(self.vds_pin.trim().to_string()).filter(|p| !p.is_empty())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum VdsState {
    #[default]
    Off,
    Connecting,
    Online,
    Error(String),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionStatus {
    pub id: u64,
    pub partner: String,
    pub fingerprint: String,
    pub client: ClientKind,
    pub path: PathKind,
    pub latency_ms: Option<u32>,
    pub secs: u64,
    pub i_control: bool,
    pub i_receive: bool,
    pub live: bool,
    pub partner_policy: Option<PolicySnapshot>,
    pub partner_output: OutputReport,
    pub ends_at_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConsentRequest {
    pub id: u64,
    pub partner: String,
    pub fingerprint: String,
    pub client: ClientKind,
    pub role: OfferedRole,
    pub known: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InviteInfo {
    pub id: String,
    pub label: String,
    pub role: OfferedRole,
    pub lifetime: Lifetime,
    pub expires: u64,
    pub uses_left: Option<u32>,
    pub web_code: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PartnerInfo {
    pub pk: String,
    pub name: String,
    pub fingerprint: String,
    pub my_role: OfferedRole,
    pub auto_accept: bool,
    pub can_dial: bool,
    pub last_seen: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogEntry {
    pub at: u64,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct RemoteStatus {
    pub fingerprint: String,
    pub room: String,
    pub listening: Option<String>,
    pub vds: VdsState,
    pub sessions: Vec<SessionStatus>,
    pub consent: Option<ConsentRequest>,
    pub connecting: Option<String>,
    pub last_error: Option<String>,
    pub invites: Vec<InviteInfo>,
    pub partners: Vec<PartnerInfo>,
    pub log: Vec<LogEntry>,
}

impl RemoteStatus {
    pub fn receiving(&self) -> Option<&SessionStatus> {
        self.sessions.iter().find(|s| s.i_receive)
    }

    pub fn controlling(&self) -> Option<&SessionStatus> {
        self.sessions.iter().find(|s| s.i_control)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InviteRequest {
    pub role: OfferedRole,
    pub lifetime: Lifetime,
    pub passphrase: Option<String>,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreatedInvite {
    pub id: String,
    pub link: String,
    pub expires: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConsentAnswer {
    Decline,
    AcceptOnce,
    AcceptAndTrust,
}

struct SessionShared {
    id: u64,
    partner: String,
    partner_pk: Option<String>,
    client: ClientKind,
    path: PathKind,
    started: Instant,
    i_control: bool,
    i_receive: bool,
    last_rx_ms: AtomicU64,
    last_frame_ms: AtomicU64,
    latency_ms: AtomicU32,
    partner_policy: Mutex<Option<PolicySnapshot>>,
    partner_output: Mutex<OutputReport>,
    feedback_out: Mutex<Vec<FeedbackKind>>,
    feedback_more: AtomicU32,
    feedback_less: AtomicU32,
    last_feedback_ms: AtomicU64,
    stop: mpsc::Sender<String>,
    policy_tx: watch::Sender<Option<PolicySnapshot>>,
    frames: watch::Sender<Option<RemoteFrame>>,
    stopped_by_us: AtomicBool,
    deadline_ms: u64,
}

struct TokenGrant {
    key: [u8; 32],
    partner_pk: Option<String>,
    role: OfferedRole,
    expires: Instant,
    deadline_ms: u64,
}

#[cfg(test)]
const DROP_FOR_TEST: &str = "\u{0}drop";

fn past(deadline_ms: u64, now_ms: u64) -> bool {
    deadline_ms != 0 && now_ms >= deadline_ms
}

struct Resume {
    targets: Vec<Target>,
    expect: [u8; 32],
    id: String,
    secret: Vec<u8>,
}

struct PendingConsent {
    req: ConsentRequest,
    tx: oneshot::Sender<ConsentAnswer>,
}

struct Inner {
    cfg: RwLock<RemoteConfig>,
    store: Mutex<TrustStore>,
    epoch: Instant,
    next_id: AtomicU64,
    sessions: Mutex<Vec<Arc<SessionShared>>>,
    inbound: Mutex<JitterBuffer>,
    policy: RwLock<PolicySnapshot>,
    output: Mutex<OutputReport>,
    consent: Mutex<Option<PendingConsent>>,
    tokens: Mutex<HashMap<String, TokenGrant>>,
    limiter: Mutex<FailLimiter>,
    log: Mutex<VecDeque<LogEntry>>,
    vds: Mutex<Option<Arc<VdsHost>>>,
    vds_state: Mutex<VdsState>,
    listening: Mutex<Option<(SocketAddr, String)>>,
    connecting: Mutex<Option<String>>,
    last_error: Mutex<Option<String>>,
    tasks: Mutex<Vec<JoinHandle<()>>>,
    net_tasks: Mutex<Vec<JoinHandle<()>>>,
    advert: Mutex<Option<lan::Advertisement>>,
    seq: AtomicU64,
}

#[derive(Clone)]
pub struct RemoteService {
    inner: Arc<Inner>,
}

#[derive(Debug, Clone)]
enum Target {
    Direct {
        addr: String,
        pin: String,
    },
    LanIce {
        addr: String,
        pin: String,
    },
    Vds {
        vds: VdsAddr,
        pin: Option<String>,
        room: String,
        relay: bool,
    },
}

impl Target {
    fn transport(&self) -> Box<dyn RemoteTransport> {
        match self.clone() {
            Target::Direct { addr, pin } => Box::new(DirectTransport { addr, pin }),
            Target::LanIce { addr, pin } => Box::new(LanIceTransport { addr, pin }),
            Target::Vds { vds, pin, room, relay } => Box::new(VdsRtcTransport { vds, pin, room, relay }),
        }
    }
}

fn mode_plan(mode: ConnectionMode, targets: &[Target]) -> (Vec<Attempt>, Strategy) {
    let mut attempts = Vec::new();
    let pick = |f: &dyn Fn(&Target) -> bool| targets.iter().filter(|t| f(t)).cloned().collect::<Vec<_>>();
    let direct = pick(&|t| matches!(t, Target::Direct { .. }));
    let lan_ice = pick(&|t| matches!(t, Target::LanIce { .. }));
    let p2p = pick(&|t| matches!(t, Target::Vds { relay: false, .. }));
    let relay = pick(&|t| matches!(t, Target::Vds { relay: true, .. }));
    let add = |list: &mut Vec<Attempt>, ts: &[Target], delay_ms: u64, timeout_s: u64| {
        for t in ts {
            list.push(Attempt {
                transport: t.transport(),
                delay: Duration::from_millis(delay_ms),
                timeout: Duration::from_secs(timeout_s),
            });
        }
    };
    match mode {
        ConnectionMode::Automatic => {
            add(&mut attempts, &direct, 0, 100);
            add(&mut attempts, &p2p, 0, 100);
            add(&mut attempts, &relay, 2500, 100);
            (attempts, Strategy::Race)
        }
        ConnectionMode::Internet => {
            add(&mut attempts, &p2p, 0, 100);
            add(&mut attempts, &relay, 3000, 100);
            (attempts, Strategy::Race)
        }
        ConnectionMode::LanOnly => {
            add(&mut attempts, &lan_ice, 0, 100);
            (attempts, Strategy::Race)
        }
        ConnectionMode::DirectHybrid => {
            add(&mut attempts, &direct, 0, 6);
            add(&mut attempts, &p2p, 0, 100);
            add(&mut attempts, &relay, 0, 100);
            (attempts, Strategy::Sequential)
        }
    }
}

fn targets_for(routes: &RouteHints, mode: ConnectionMode, discovered: &[(SocketAddr, Option<String>)]) -> Vec<Target> {
    let mut out = Vec::new();
    let pin = routes.tls_pin.clone().unwrap_or_default();
    let mut addrs: Vec<String> = routes.lan.clone();
    for (a, p) in discovered {
        if p.as_deref().is_none_or(|p| p == pin) && !addrs.contains(&a.to_string()) {
            addrs.push(a.to_string());
        }
    }
    if !pin.is_empty() {
        for a in &addrs {
            if mode == ConnectionMode::LanOnly {
                out.push(Target::LanIce {
                    addr: a.clone(),
                    pin: pin.clone(),
                });
            } else {
                out.push(Target::Direct {
                    addr: a.clone(),
                    pin: pin.clone(),
                });
            }
        }
        if let Some(p) = routes.public.as_ref().filter(|_| mode != ConnectionMode::LanOnly) {
            out.push(Target::Direct {
                addr: p.clone(),
                pin: pin.clone(),
            });
        }
    }
    if mode.uses_vds()
        && let Some(vds) = routes.vds.as_deref().and_then(VdsAddr::parse)
    {
        for relay in [false, true] {
            out.push(Target::Vds {
                vds: vds.clone(),
                pin: routes.vds_pin.clone(),
                room: routes.room.clone(),
                relay,
            });
        }
    }
    out
}

fn their_role(offered: OfferedRole) -> OfferedRole {
    match offered {
        OfferedRole::Receiver => OfferedRole::Controller,
        OfferedRole::Controller => OfferedRole::Receiver,
        OfferedRole::Both => OfferedRole::Both,
    }
}

impl RemoteService {
    pub fn start(config: RemoteConfig, store_path: Option<PathBuf>) -> Self {
        let store = match &store_path {
            Some(p) => TrustStore::open(p),
            None => TrustStore::in_memory(),
        };
        let jitter = JitterBuffer::new(config.jitter_ms as i64);
        let me = Self {
            inner: Arc::new(Inner {
                cfg: RwLock::new(config),
                store: Mutex::new(store),
                epoch: Instant::now(),
                next_id: AtomicU64::new(1),
                sessions: Mutex::new(Vec::new()),
                inbound: Mutex::new(jitter),
                policy: RwLock::new(PolicySnapshot::default()),
                output: Mutex::new(OutputReport::default()),
                consent: Mutex::new(None),
                tokens: Mutex::new(HashMap::new()),
                limiter: Mutex::new(FailLimiter::default()),
                log: Mutex::new(VecDeque::new()),
                vds: Mutex::new(None),
                vds_state: Mutex::new(VdsState::Off),
                listening: Mutex::new(None),
                connecting: Mutex::new(None),
                last_error: Mutex::new(None),
                tasks: Mutex::new(Vec::new()),
                net_tasks: Mutex::new(Vec::new()),
                advert: Mutex::new(None),
                seq: AtomicU64::new(0),
            }),
        };
        me.ensure_tls();
        me.log(format!("Remote started — your fingerprint is {}", me.fingerprint()));
        me.restart_network();
        let house = me.clone();
        me.track(tokio::spawn(async move {
            let mut t = tokio::time::interval(Duration::from_secs(30));
            loop {
                t.tick().await;
                house.housekeeping();
            }
        }));
        me
    }

    fn track(&self, h: JoinHandle<()>) {
        if let Ok(mut t) = self.inner.tasks.lock() {
            t.retain(|h| !h.is_finished());
            t.push(h);
        }
    }

    fn track_net(&self, h: JoinHandle<()>) {
        if let Ok(mut t) = self.inner.net_tasks.lock() {
            t.retain(|h| !h.is_finished());
            t.push(h);
        }
    }

    fn now_ms(&self) -> u64 {
        self.inner.epoch.elapsed().as_millis() as u64
    }

    pub fn fingerprint(&self) -> String {
        self.inner
            .store
            .lock()
            .map(|s| s.identity().fingerprint())
            .unwrap_or_default()
    }

    fn identity(&self) -> crypto::Identity {
        self.inner
            .store
            .lock()
            .map(|s| s.identity().clone())
            .unwrap_or_else(|_| crypto::Identity::generate())
    }

    fn name(&self) -> String {
        let n = self.inner.cfg.read().map(|c| c.name.clone()).unwrap_or_default();
        if n.trim().is_empty() { "ShockingVRC".into() } else { n }
    }

    pub fn config(&self) -> RemoteConfig {
        self.inner.cfg.read().map(|c| c.clone()).unwrap_or_default()
    }

    fn ensure_tls(&self) {
        if let Ok(mut s) = self.inner.store.lock()
            && s.tls().is_none()
        {
            s.set_tls(tls::generate());
        }
    }

    pub fn log(&self, text: impl Into<String>) {
        let text = text.into();
        log::info!("[remote] {text}");
        if let Ok(mut l) = self.inner.log.lock() {
            l.push_back(LogEntry {
                at: store::now_unix(),
                text,
            });
            while l.len() > LOG_LEN {
                l.pop_front();
            }
        }
    }

    fn error(&self, text: impl Into<String>) {
        let text = text.into();
        self.log(text.clone());
        if let Ok(mut e) = self.inner.last_error.lock() {
            *e = Some(text);
        }
    }

    pub fn export_log(&self) -> String {
        self.inner
            .log
            .lock()
            .map(|l| l.iter().map(|e| format!("{}\t{}\n", e.at, e.text)).collect())
            .unwrap_or_default()
    }

    pub fn set_config(&self, config: RemoteConfig) {
        let changed_net = {
            let Ok(mut c) = self.inner.cfg.write() else { return };
            let net = c.mode != config.mode
                || c.vds_url != config.vds_url
                || c.vds_pin != config.vds_pin
                || c.lan_port != config.lan_port;
            *c = config.clone();
            net
        };
        if let Ok(mut j) = self.inner.inbound.lock() {
            *j = JitterBuffer::new(config.jitter_ms as i64);
        }
        if changed_net {
            self.restart_network();
        }
    }

    pub(crate) fn stop_network(&self) {
        if let Ok(mut t) = self.inner.net_tasks.lock() {
            for h in t.drain(..) {
                h.abort();
            }
        }
        if let Ok(mut a) = self.inner.advert.lock() {
            *a = None;
        }
        if let Ok(mut v) = self.inner.vds.lock() {
            *v = None;
        }
        if let Ok(mut l) = self.inner.listening.lock() {
            *l = None;
        }
        if let Ok(mut s) = self.inner.vds_state.lock() {
            *s = VdsState::Off;
        }
    }

    fn restart_network(&self) {
        self.stop_network();
        let cfg = self.config();
        if cfg.mode.uses_direct() {
            let me = self.clone();
            self.track_net(tokio::spawn(async move { me.run_direct_listener(cfg.lan_port).await }));
        }
        let cfg = self.config();
        if cfg.mode.uses_vds() && cfg.vds().is_some() {
            let me = self.clone();
            self.track_net(tokio::spawn(async move { me.run_vds_host().await }));
        }
    }

    async fn run_direct_listener(self, port: u16) {
        let Some(material) = self.inner.store.lock().ok().and_then(|s| s.tls().cloned()) else {
            return;
        };
        let bind = SocketAddr::from(([0, 0, 0, 0], port));
        let (listener, mut incoming) = match direct::listen(bind, &material).await {
            Ok(x) => x,
            Err(e) => {
                self.error(format!("LAN listener unavailable: {e}"));
                return;
            }
        };
        self.log(format!("Listening on the LAN at port {}", listener.addr.port()));
        if let Ok(mut l) = self.inner.listening.lock() {
            *l = Some((listener.addr, listener.pin.clone()));
        }
        let room = self.identity().room();
        let advert = lan::advertise(&room, listener.addr.port(), &listener.pin);
        if let Ok(mut a) = self.inner.advert.lock() {
            *a = advert;
        }
        while let Some(inc) = incoming.recv().await {
            let me = self.clone();
            tokio::spawn(async move {
                match inc {
                    Incoming::Session(ch) => me.handle_incoming(ch, None).await,
                    Incoming::Ice(sig) => match rtc::connect(sig, IceConfig::host_only(), false, PathKind::Lan).await {
                        Ok(ch) => me.handle_incoming(ch, None).await,
                        Err(e) => me.log(format!("LAN ICE setup failed: {e}")),
                    },
                }
            });
        }
        drop(listener);
    }

    async fn run_vds_host(self) {
        let mut backoff = Duration::from_secs(2);
        loop {
            let cfg = self.config();
            let Some(addr) = cfg.vds() else { return };
            self.set_vds_state(VdsState::Connecting);
            match signal::host(&addr, cfg.vds_pin().as_deref(), &self.identity()).await {
                Ok((host, mut events, mut closed)) => {
                    backoff = Duration::from_secs(2);
                    let host = Arc::new(host);
                    if let Ok(mut v) = self.inner.vds.lock() {
                        *v = Some(Arc::clone(&host));
                    }
                    self.re_alias(&host).await;
                    self.set_vds_state(VdsState::Online);
                    self.log(format!("Online on {}", addr.host));
                    loop {
                        tokio::select! {
                            ev = events.recv() => match ev {
                                Some(HostEvent::Peer { signaler }) => {
                                    let me = self.clone();
                                    let ice = host.ice();
                                    let host = Arc::clone(&host);
                                    tokio::spawn(async move {
                                        let peer = signaler.peer();
                                        if me.locked_out(peer.split('|').next().unwrap_or_default()) {
                                            host.report_failure(&peer).await;
                                            return;
                                        }
                                        match rtc::connect(signaler, ice, false, PathKind::P2p).await {
                                            Ok(ch) => me.handle_incoming(ch, Some(host)).await,
                                            Err(e) => log::debug!("[remote] Peer setup failed: {e}"),
                                        }
                                    });
                                }
                                Some(HostEvent::Online { .. }) => {}
                                None => break,
                            },
                            _ = closed.recv() => break,
                        }
                    }
                    if let Ok(mut v) = self.inner.vds.lock() {
                        *v = None;
                    }
                    self.set_vds_state(VdsState::Error("disconnected".into()));
                }
                Err(e) => {
                    self.set_vds_state(VdsState::Error(e.to_string()));
                    log::warn!("[remote] Rendezvous server: {e}");
                }
            }
            tokio::time::sleep(backoff).await;
            backoff = (backoff * 2).min(Duration::from_secs(60));
        }
    }

    async fn re_alias(&self, host: &VdsHost) {
        let codes: Vec<(String, u64)> = self
            .inner
            .store
            .lock()
            .map(|s| {
                let now = store::now_unix();
                s.invites()
                    .iter()
                    .filter(|i| i.web_code && i.is_live(now))
                    .map(|i| (i.id.replacen("code-", "c-", 1), i.expires.saturating_sub(now)))
                    .collect()
            })
            .unwrap_or_default();
        for (room, ttl) in codes {
            host.alias(&room, ttl).await;
        }
    }

    fn set_vds_state(&self, s: VdsState) {
        if let Ok(mut v) = self.inner.vds_state.lock() {
            *v = s;
        }
    }

    fn locked_out(&self, peer: &str) -> bool {
        self.inner
            .limiter
            .lock()
            .map(|l| l.check(peer, Instant::now()).is_err())
            .unwrap_or(false)
    }

    fn housekeeping(&self) {
        let now = store::now_unix();
        if let Ok(mut s) = self.inner.store.lock() {
            s.prune(now);
        }
        if let Ok(mut t) = self.inner.tokens.lock() {
            t.retain(|_, g| g.expires > Instant::now());
        }
    }

    fn routes(&self) -> RouteHints {
        let cfg = self.config();
        let listening = self.inner.listening.lock().ok().and_then(|l| l.clone());
        let pin = self
            .inner
            .store
            .lock()
            .ok()
            .and_then(|s| s.tls().and_then(tls::material_pin));
        let lan_addrs = match &listening {
            Some((addr, _)) if cfg.mode.uses_direct() => lan::local_ipv4()
                .into_iter()
                .filter(|ip| ip.is_private())
                .take(4)
                .map(|ip| format!("{ip}:{}", addr.port()))
                .collect(),
            _ => Vec::new(),
        };
        RouteHints {
            lan: lan_addrs,
            public: Some(cfg.public_addr.trim().to_string()).filter(|p| !p.is_empty()),
            tls_pin: pin,
            vds: cfg
                .vds()
                .filter(|_| cfg.mode.uses_vds())
                .map(|_| cfg.vds_url.trim().to_string()),
            vds_pin: cfg.vds_pin(),
            room: self.identity().room(),
        }
    }

    pub fn create_invite(&self, req: InviteRequest) -> Result<CreatedInvite, String> {
        let pass = req.passphrase.as_deref().map(str::trim).filter(|p| !p.is_empty());
        if req.lifetime.creates_pairing() && pass.is_none_or(|p| p.chars().count() < 6) {
            return Err("An indefinite pairing needs a passphrase of at least 6 characters".into());
        }
        let routes = self.routes();
        if routes.lan.is_empty() && routes.vds.is_none() && routes.public.is_none() {
            return Err("No way to be reached yet — wait for the LAN listener or set a rendezvous server".into());
        }
        let now = store::now_unix();
        let (rec, secret, identity) = {
            let mut s = self.inner.store.lock().map_err(|_| "store unavailable")?;
            let (rec, secret) = s.add_invite(req.role, req.lifetime, &req.label, pass.is_some(), now);
            (rec, secret, s.identity().clone())
        };
        let body = LinkBody {
            v: link::LINK_VERSION,
            pk: b64(&identity.public()),
            fp: identity.fingerprint(),
            name: self.name(),
            routes,
            invite: rec.id.clone(),
            secret: b64(&secret),
            role: req.role,
            exp: rec.expires,
            uses: rec.uses_left.unwrap_or(0),
        };
        let link = link::encode_link(&body, &identity, pass);
        self.log(format!(
            "Created a {} invite ({})",
            req.lifetime.label().to_lowercase(),
            req.role.label().to_lowercase()
        ));
        Ok(CreatedInvite {
            id: rec.id,
            link,
            expires: rec.expires,
        })
    }

    pub async fn create_web_code(&self, lifetime: Lifetime) -> Result<String, String> {
        let host = self
            .inner
            .vds
            .lock()
            .ok()
            .and_then(|v| v.clone())
            .ok_or("Connect to a rendezvous server first")?;
        let code = link::new_control_code();
        let norm = link::normalize_code(&code).ok_or("code generation failed")?;
        let lifetime = if lifetime == Lifetime::Indefinite {
            Lifetime::Hours24
        } else {
            lifetime
        };
        let rec = self.inner.store.lock().map_err(|_| "store unavailable")?.add_web_code(
            &norm,
            OfferedRole::Receiver,
            lifetime,
            store::now_unix(),
        );
        host.alias(&link::code_room(&norm), rec.expires.saturating_sub(store::now_unix()))
            .await;
        self.log("Created a web control code");
        Ok(code)
    }

    pub fn revoke_invite(&self, id: &str) {
        let was_code = id.starts_with("code-");
        if self
            .inner
            .store
            .lock()
            .map(|mut s| s.revoke_invite(id))
            .unwrap_or(false)
        {
            self.log("Revoked an invite");
            if was_code && let Some(host) = self.inner.vds.lock().ok().and_then(|v| v.clone()) {
                let room = id.replacen("code-", "c-", 1);
                tokio::spawn(async move { host.unalias(&room).await });
            }
        }
    }

    pub fn revoke_partner(&self, pk: &str) {
        if self
            .inner
            .store
            .lock()
            .map(|mut s| s.revoke_partner(pk))
            .unwrap_or(false)
        {
            self.log("Removed a trusted partner");
            let ids: Vec<u64> = self
                .sessions()
                .iter()
                .filter(|s| s.partner_pk.as_deref() == Some(pk))
                .map(|s| s.id)
                .collect();
            for id in ids {
                self.stop_session(id, "partner revoked");
            }
        }
    }

    pub fn set_auto_accept(&self, pk: &str, on: bool) {
        if let Ok(mut s) = self.inner.store.lock() {
            s.set_auto_accept(pk, on);
        }
    }

    fn lookup(&self, hello: &crate::protocol::Hello, pk: Option<&[u8; 32]>) -> Result<(GrantFound, Grant), String> {
        let now = store::now_unix();
        if let Some(rid) = &hello.resume {
            let tokens = self.inner.tokens.lock().map_err(|_| "busy")?;
            let t = tokens
                .get(rid)
                .filter(|t| t.expires > Instant::now() && !past(t.deadline_ms, store::now_unix_ms()))
                .ok_or("session expired")?;
            if t.partner_pk.as_deref() != pk.map(|p| b64(p)).as_deref() {
                return Err("session expired".into());
            }
            return Ok((
                GrantFound {
                    key: Some(t.key),
                    needs_identity: t.partner_pk.is_some(),
                },
                Grant::Resume {
                    role: t.role,
                    deadline_ms: t.deadline_ms,
                },
            ));
        }
        let store = self.inner.store.lock().map_err(|_| "busy")?;
        if let Some(id) = &hello.invite {
            let rec = store.invite(id, now).ok_or("invite not found or expired")?;
            let key = unb64(&rec.key)
                .and_then(|k| <[u8; 32]>::try_from(k).ok())
                .ok_or("invite damaged")?;
            return Ok((
                GrantFound {
                    key: Some(key),
                    needs_identity: rec.lifetime.creates_pairing(),
                },
                Grant::Invite {
                    id: rec.id.clone(),
                    role: rec.role,
                    pairing: rec.lifetime.creates_pairing(),
                },
            ));
        }
        let pk = pk.ok_or("identity required")?;
        let rec = store.partner(&b64(pk)).ok_or("not a trusted partner")?;
        Ok((
            GrantFound {
                key: None,
                needs_identity: true,
            },
            Grant::Trusted {
                role: rec.my_role,
                auto_accept: rec.auto_accept,
            },
        ))
    }

    async fn handle_incoming(self, mut ch: Channel, via: Option<Arc<VdsHost>>) {
        let full_peer = ch.info.peer.clone();
        let peer = full_peer.split('|').next().unwrap_or_default().to_string();
        let locked = self
            .inner
            .limiter
            .lock()
            .ok()
            .and_then(|l| l.check(&peer, Instant::now()).err());
        if let Some(wait) = locked {
            let _ = ch
                .send(&Message::Reject {
                    reason: "too many failed attempts".into(),
                    retry_after_ms: Some(wait.as_millis() as u64),
                })
                .await;
            return;
        }
        let identity = self.identity();
        let name = self.name();
        let mut grant = None;
        let result = handshake::accept(&mut ch, &identity, &name, |hello, pk| match self.lookup(hello, pk) {
            Ok((found, g)) => {
                grant = Some(g);
                Ok(found)
            }
            Err(e) => Err(e),
        })
        .await;
        let accepted = match result {
            Ok(a) => a,
            Err(e) => {
                if e.is_auth_failure() || matches!(e, HandshakeError::Malformed(_)) {
                    let lock = self
                        .inner
                        .limiter
                        .lock()
                        .ok()
                        .and_then(|mut l| l.fail(&peer, Instant::now()));
                    if let Some(h) = &via {
                        h.report_failure(&full_peer).await;
                    }
                    self.log(format!(
                        "Refused a connection from {peer}: {e}{}",
                        lock.map(|d| format!(" (locked out for {} s)", d.as_secs()))
                            .unwrap_or_default()
                    ));
                }
                return;
            }
        };
        if let Ok(mut l) = self.inner.limiter.lock() {
            l.succeed(&peer);
        }
        let Some(grant) = grant else { return };
        let joiner_pk = accepted.joiner_key.map(|k| b64(&k));
        let fp = accepted.joiner_key.map(|k| crypto::fingerprint(&k)).unwrap_or_default();
        let partner = if accepted.hello.name.trim().is_empty() {
            "Partner".to_string()
        } else {
            accepted.hello.name.chars().take(40).collect()
        };
        let role = grant.role();
        let (i_receive, i_control) = (role.inviter_receives(), role.inviter_controls());

        if let Err(reason) = self.make_room(i_receive, i_control, joiner_pk.as_deref()) {
            let _ = ch
                .send(&Message::Reject {
                    reason,
                    retry_after_ms: Some(5000),
                })
                .await;
            return;
        }

        let known = joiner_pk.as_deref().is_some_and(|pk| {
            self.inner
                .store
                .lock()
                .map(|s| s.partner(pk).is_some())
                .unwrap_or(false)
        });
        let needs_consent = match &grant {
            Grant::Resume { .. } => false,
            Grant::Trusted { auto_accept, .. } => !auto_accept,
            Grant::Invite { .. } => true,
        };
        let mut trust = matches!(grant, Grant::Invite { pairing: true, .. });
        if needs_consent {
            let _ = ch
                .send(&Message::Pending {
                    reason: "waiting for the receiver to accept".into(),
                })
                .await;
            let req = ConsentRequest {
                id: self.inner.next_id.fetch_add(1, Ordering::Relaxed),
                partner: partner.clone(),
                fingerprint: fp.clone(),
                client: accepted.hello.client,
                role,
                known,
            };
            self.log(format!("{partner} wants to connect — waiting for your answer"));
            match self.ask_consent(req, &mut ch).await {
                ConsentAnswer::Decline => {
                    let _ = ch
                        .send(&Message::Reject {
                            reason: "declined".into(),
                            retry_after_ms: None,
                        })
                        .await;
                    self.log(format!("Declined {partner}"));
                    return;
                }
                ConsentAnswer::AcceptOnce => {}
                ConsentAnswer::AcceptAndTrust => trust = joiner_pk.is_some(),
            }
            if let Err(reason) = self.make_room(i_receive, i_control, joiner_pk.as_deref()) {
                let _ = ch
                    .send(&Message::Reject {
                        reason,
                        retry_after_ms: Some(5000),
                    })
                    .await;
                return;
            }
        }

        let deadline_ms = match &grant {
            Grant::Invite { id, pairing, .. } => {
                let current = self
                    .inner
                    .store
                    .lock()
                    .ok()
                    .and_then(|s| s.invite(id, store::now_unix()).map(|r| r.expires));
                match current {
                    Some(_) if *pairing => 0,
                    Some(expires) => expires.saturating_mul(1000),
                    None => 1,
                }
            }
            other => other.deadline_ms(),
        };
        if past(deadline_ms, store::now_unix_ms()) {
            let _ = ch
                .send(&Message::Reject {
                    reason: "session expired".into(),
                    retry_after_ms: None,
                })
                .await;
            self.log(format!("Refused {partner}: the invite's time is up"));
            return;
        }
        let now = store::now_unix();
        if let Grant::Invite { id, .. } = &grant
            && let Ok(mut s) = self.inner.store.lock()
        {
            s.consume_invite(id);
        }
        if let Some(pk) = &joiner_pk
            && let Ok(mut s) = self.inner.store.lock()
        {
            if trust {
                s.upsert_partner(PartnerRecord {
                    pk: pk.clone(),
                    name: partner.clone(),
                    my_role: role,
                    added: now,
                    auto_accept: !matches!(grant, Grant::Invite { pairing: true, .. }) || known,
                    last_seen: now,
                    routes: None,
                });
            }
            s.touch_partner(pk, now);
        }
        let token_id = b64(&crypto::random_bytes::<12>());
        let token_secret: [u8; 32] = crypto::random_bytes();
        let left = if deadline_ms == 0 {
            TOKEN_TTL
        } else {
            Duration::from_millis(deadline_ms.saturating_sub(store::now_unix_ms())).min(TOKEN_TTL)
        };
        if let Ok(mut t) = self.inner.tokens.lock() {
            t.insert(
                token_id.clone(),
                TokenGrant {
                    key: crypto::secret_key(&token_secret),
                    partner_pk: joiner_pk.clone(),
                    role,
                    expires: Instant::now() + left,
                    deadline_ms,
                },
            );
        }
        let trusted = trust || known;
        let welcome = Welcome {
            session: format!("{token_id}.{}", b64(&token_secret)),
            ttl_ms: TOKEN_TTL.as_millis() as u64,
            name: self.name(),
            you_control: i_receive,
            you_receive: i_control,
            trusted,
            policy: i_receive.then(|| self.policy()),
            ends_at_ms: deadline_ms,
        };
        if ch.send(&Message::Welcome(welcome)).await.is_err() {
            return;
        }
        let path = accepted.hello.path.unwrap_or(ch.info.path);
        let shared = self.add_session(SessionSetup {
            partner,
            partner_pk: joiner_pk,
            client: accepted.hello.client,
            path,
            i_control,
            i_receive,
            partner_policy: None,
            deadline_ms,
        });
        self.run_session(ch, shared).await;
    }

    fn make_room(&self, i_receive: bool, i_control: bool, pk: Option<&str>) -> Result<(), String> {
        let now = self.now_ms();
        let sessions = self.sessions();
        for s in sessions {
            let clash = (i_receive && s.i_receive) || (i_control && s.i_control);
            if !clash {
                continue;
            }
            let quiet = now.saturating_sub(s.last_rx_ms.load(Ordering::Relaxed)) > 2000;
            let same = pk.is_some() && s.partner_pk.as_deref() == pk;
            if same && quiet {
                s.stopped_by_us.store(true, Ordering::Relaxed);
                let _ = s.stop.try_send("replaced by a new connection".into());
                continue;
            }
            return Err(if same {
                "already connected".into()
            } else {
                "busy with another partner".into()
            });
        }
        Ok(())
    }

    async fn ask_consent(&self, req: ConsentRequest, ch: &mut Channel) -> ConsentAnswer {
        let (tx, mut rx) = oneshot::channel();
        if let Ok(mut c) = self.inner.consent.lock() {
            if let Some(old) = c.take() {
                let _ = old.tx.send(ConsentAnswer::Decline);
            }
            *c = Some(PendingConsent { req: req.clone(), tx });
        }
        let deadline = tokio::time::sleep(CONSENT_TIMEOUT);
        tokio::pin!(deadline);
        let answer = loop {
            tokio::select! {
                a = &mut rx => break a.unwrap_or(ConsentAnswer::Decline),
                _ = &mut deadline => break ConsentAnswer::Decline,
                m = ch.recv() => match m {
                    Some(Ok(Message::Ping { seq, ts })) => { let _ = ch.send(&Message::Pong { seq, ts }).await; }
                    Some(Ok(Message::Bye)) | None => break ConsentAnswer::Decline,
                    _ => {}
                },
            }
        };
        if let Ok(mut c) = self.inner.consent.lock()
            && c.as_ref().is_some_and(|p| p.req.id == req.id)
        {
            *c = None;
        }
        answer
    }

    pub fn answer_consent(&self, answer: ConsentAnswer) {
        if let Some(p) = self.inner.consent.lock().ok().and_then(|mut c| c.take()) {
            let _ = p.tx.send(answer);
        }
    }

    pub fn policy(&self) -> PolicySnapshot {
        self.inner.policy.read().map(|p| p.clone()).unwrap_or_default()
    }

    pub fn set_policy(&self, policy: PolicySnapshot) {
        let changed = self
            .inner
            .policy
            .write()
            .map(|mut p| {
                let changed = *p != policy;
                *p = policy.clone();
                changed
            })
            .unwrap_or(false);
        if changed {
            for s in self.sessions() {
                if s.i_receive {
                    s.policy_tx.send_replace(Some(policy.clone()));
                }
            }
        }
    }

    pub fn set_output_report(&self, report: OutputReport) {
        if let Ok(mut o) = self.inner.output.lock() {
            *o = report;
        }
    }

    pub fn take_inbound(&self) -> Option<RemoteFrame> {
        let now = self.now_ms() as i64;
        self.inner.inbound.lock().ok()?.pop_due(now)
    }

    pub fn inbound_live(&self) -> bool {
        let now = self.now_ms();
        self.sessions().iter().any(|s| {
            s.i_receive && now.saturating_sub(s.last_frame_ms.load(Ordering::Relaxed)) <= FAILSAFE.as_millis() as u64
        })
    }

    pub fn receiving(&self) -> bool {
        self.sessions().iter().any(|s| s.i_receive)
    }

    pub fn controlling(&self) -> bool {
        self.sessions().iter().any(|s| s.i_control)
    }

    pub fn send_feedback(&self, kind: FeedbackKind) -> bool {
        let now = self.now_ms();
        let mut sent = false;
        for s in self.sessions() {
            if !s.i_receive {
                continue;
            }
            let last = s.last_feedback_ms.load(Ordering::Relaxed);
            if now.saturating_sub(last) < FEEDBACK_MIN_GAP_MS {
                continue;
            }
            s.last_feedback_ms.store(now, Ordering::Relaxed);
            if let Ok(mut q) = s.feedback_out.lock() {
                q.push(kind);
                sent = true;
            }
        }
        sent
    }

    pub fn partner_context(&self) -> shocking_vrc_core::graph::PartnerContext {
        let mut ctx = shocking_vrc_core::graph::PartnerContext::default();
        for s in self.sessions().iter().filter(|s| s.i_control) {
            ctx.live = true;
            ctx.more = ctx.more.wrapping_add(s.feedback_more.load(Ordering::Relaxed));
            ctx.less = ctx.less.wrapping_add(s.feedback_less.load(Ordering::Relaxed));
            if let Ok(o) = s.partner_output.lock() {
                for (slot, out) in &o.slots {
                    match ctx.outputs.iter_mut().find(|(k, _)| k == slot) {
                        Some((_, v)) => {
                            v[0] = v[0].max(out[0]);
                            v[1] = v[1].max(out[1]);
                        }
                        None => ctx.outputs.push((*slot, *out)),
                    }
                }
            }
        }
        ctx
    }

    pub fn send_frame(&self, slots: Vec<SlotFrame>) {
        let frame = RemoteFrame {
            seq: self.inner.seq.fetch_add(1, Ordering::Relaxed) + 1,
            ts: self.now_ms(),
            slots,
        };
        for s in self.sessions() {
            if s.i_control {
                s.frames.send_replace(Some(frame.clone()));
            }
        }
    }

    pub fn connect_link(&self, link_text: &str, passphrase: Option<String>) -> Result<(), String> {
        let body = link::decode_link(link_text, passphrase.as_deref(), store::now_unix()).map_err(|e| e.to_string())?;
        let expect = body.public_key().ok_or("link is damaged")?;
        let secret = body.secret_bytes().ok_or("link is damaged")?;
        if self.identity().public() == expect {
            return Err("That is your own link".into());
        }
        let grant = JoinGrant::Secret {
            id: body.invite.clone(),
            secret,
        };
        self.spawn_join(body.name.clone(), body.routes.clone(), expect, grant, Some(body.role));
        Ok(())
    }

    pub fn connect_partner(&self, pk: &str) -> Result<(), String> {
        let rec = self
            .inner
            .store
            .lock()
            .ok()
            .and_then(|s| s.partner(pk).cloned())
            .ok_or("not a trusted partner")?;
        let routes = rec
            .routes
            .clone()
            .ok_or("this partner connects to you, not the other way round")?;
        let expect = crypto::unb64_array::<32>(&rec.pk).ok_or("damaged partner record")?;
        self.spawn_join(rec.name.clone(), routes, expect, JoinGrant::Trusted, None);
        Ok(())
    }

    fn spawn_join(
        &self,
        name: String,
        routes: RouteHints,
        expect: [u8; 32],
        grant: JoinGrant,
        offered: Option<OfferedRole>,
    ) {
        if let Ok(mut c) = self.inner.connecting.lock() {
            *c = Some(name.clone());
        }
        if let Ok(mut e) = self.inner.last_error.lock() {
            *e = None;
        }
        let me = self.clone();
        self.track(tokio::spawn(async move {
            let result = me.join(name.clone(), routes.clone(), expect, grant, offered).await;
            if let Ok(mut c) = me.inner.connecting.lock() {
                *c = None;
            }
            if let Err(e) = result {
                me.error(format!("Could not connect to {name}: {e}"));
            }
        }));
    }

    async fn join(
        &self,
        name: String,
        routes: RouteHints,
        expect: [u8; 32],
        grant: JoinGrant,
        offered: Option<OfferedRole>,
    ) -> Result<(), String> {
        let mode = self.config().mode;
        let room = routes.room.clone();
        let discovered = if mode.uses_direct() && !room.is_empty() {
            tokio::task::spawn_blocking(move || lan::discover(&room, Duration::from_millis(800)))
                .await
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        let targets = targets_for(&routes, mode, &discovered);
        self.log(format!("Connecting to {name} ({} route(s), {})", targets.len(), mode.label()));
        let won = self.dial(&targets, mode, expect, grant).await?;
        let (ch, welcome, path) = won;
        let (session, resume) = self.register_joined(&welcome, path, name, Some(routes), targets, expect, offered);
        let me = self.clone();
        tokio::spawn(async move { me.session_loop(ch, session, resume).await });
        Ok(())
    }

    async fn session_loop(
        self,
        ch: Channel,
        session: (Arc<SessionShared>, mpsc::Receiver<String>),
        resume: Option<Resume>,
    ) {
        let mut current = Some((ch, session));
        let mut resume = resume;
        while let Some((ch, session)) = current.take() {
            let partner = session.0.partner.clone();
            let retry = self.run_session(ch, session).await;
            let Some(r) = resume.take().filter(|_| retry) else {
                break;
            };
            let Some((ch, welcome, path)) = self.try_resume(&r, &partner).await else {
                break;
            };
            let (session, next) =
                self.register_joined(&welcome, path, partner, None, r.targets.clone(), r.expect, None);
            resume = next;
            current = Some((ch, session));
        }
    }

    async fn dial(
        &self,
        targets: &[Target],
        mode: ConnectionMode,
        expect: [u8; 32],
        grant: JoinGrant,
    ) -> Result<(Channel, Welcome, PathKind), String> {
        let (attempts, strategy) = mode_plan(mode, targets);
        let ctx = JoinerCtx {
            name: self.name(),
            identity: Some(self.identity()),
            client: ClientKind::App,
            expect,
            grant,
        };
        let fail = |e: HandshakeError| {
            if matches!(
                e,
                HandshakeError::Rejected(..) | HandshakeError::BadProof | HandshakeError::Denied(_)
            ) {
                race::Failure::fatal(e.to_string())
            } else {
                race::Failure::from(e.to_string())
            }
        };
        let won = race::establish(attempts, strategy, |mut ch| {
            let ctx = ctx.clone();
            async move {
                handshake::join(&mut ch, &ctx).await.map_err(fail)?;
                let w = handshake::await_verdict(&mut ch, CONSENT_TIMEOUT + Duration::from_secs(5))
                    .await
                    .map_err(fail)?;
                Ok((ch, w))
            }
        })
        .await
        .map_err(|errs| errs.join("; "))?;
        let path = won.path;
        let (ch, w) = won.value;
        Ok((ch, w, path))
    }

    #[allow(clippy::too_many_arguments)]
    fn register_joined(
        &self,
        welcome: &Welcome,
        path: PathKind,
        name: String,
        routes: Option<RouteHints>,
        targets: Vec<Target>,
        expect: [u8; 32],
        offered: Option<OfferedRole>,
    ) -> ((Arc<SessionShared>, mpsc::Receiver<String>), Option<Resume>) {
        let partner = if welcome.name.trim().is_empty() {
            name
        } else {
            welcome.name.chars().take(40).collect()
        };
        let pk = b64(&expect);
        let my_role = offered
            .map(their_role)
            .unwrap_or_else(|| match (welcome.you_control, welcome.you_receive) {
                (true, true) => OfferedRole::Both,
                (true, false) => OfferedRole::Controller,
                _ => OfferedRole::Receiver,
            });
        if welcome.trusted
            && let Ok(mut s) = self.inner.store.lock()
        {
            let now = store::now_unix();
            s.upsert_partner(PartnerRecord {
                pk: pk.clone(),
                name: partner.clone(),
                my_role,
                added: now,
                auto_accept: false,
                last_seen: now,
                routes,
            });
        }
        let resume = welcome.session.split_once('.').and_then(|(id, s)| {
            unb64(s).map(|secret| Resume {
                targets,
                expect,
                id: id.to_string(),
                secret,
            })
        });
        let shared = self.add_session(SessionSetup {
            partner: partner.clone(),
            partner_pk: Some(pk),
            client: ClientKind::App,
            path,
            i_control: welcome.you_control,
            i_receive: welcome.you_receive,
            partner_policy: welcome.policy.clone(),
            deadline_ms: welcome.ends_at_ms,
        });
        self.log(format!("Connected to {partner} over {path}"));
        (shared, resume)
    }

    fn sessions(&self) -> Vec<Arc<SessionShared>> {
        self.inner.sessions.lock().map(|s| s.clone()).unwrap_or_default()
    }

    fn add_session(&self, s: SessionSetup) -> (Arc<SessionShared>, mpsc::Receiver<String>) {
        let (stop_tx, stop_rx) = mpsc::channel(2);
        let (policy_tx, _) = watch::channel(None);
        let (frames, _) = watch::channel(None);
        let shared = Arc::new(SessionShared {
            id: self.inner.next_id.fetch_add(1, Ordering::Relaxed),
            partner: s.partner,
            partner_pk: s.partner_pk,
            client: s.client,
            path: s.path,
            started: Instant::now(),
            i_control: s.i_control,
            i_receive: s.i_receive,
            last_rx_ms: AtomicU64::new(self.now_ms()),
            last_frame_ms: AtomicU64::new(0),
            latency_ms: AtomicU32::new(u32::MAX),
            partner_policy: Mutex::new(s.partner_policy),
            partner_output: Mutex::new(OutputReport::default()),
            feedback_out: Mutex::new(Vec::new()),
            feedback_more: AtomicU32::new(0),
            feedback_less: AtomicU32::new(0),
            last_feedback_ms: AtomicU64::new(0),
            stop: stop_tx,
            policy_tx,
            frames,
            stopped_by_us: AtomicBool::new(false),
            deadline_ms: s.deadline_ms,
        });
        if s.i_receive
            && let Ok(mut j) = self.inner.inbound.lock()
        {
            j.clear();
        }
        if let Ok(mut list) = self.inner.sessions.lock() {
            list.push(Arc::clone(&shared));
        }
        (shared, stop_rx)
    }

    async fn run_session(
        &self,
        mut ch: Channel,
        (shared, mut stop_rx): (Arc<SessionShared>, mpsc::Receiver<String>),
    ) -> bool {
        let mut policy_rx = shared.policy_tx.subscribe();
        let mut frames_rx = shared.frames.subscribe();
        frames_rx.mark_unchanged();
        let mut tick = tokio::time::interval(FRAME_TICK);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut last_ping = Instant::now();
        let mut last_output = Instant::now();
        let mut ping_seq = 0u64;
        let mut bucket = MAX_FRAMES_PER_SEC / 5.0;
        let mut bucket_at = Instant::now();
        let mut last_seq = 0u64;
        let mut last_bad_log: Option<Instant> = None;
        let reason: String;
        let mut remote_stop = false;
        loop {
            tokio::select! {
                msg = ch.recv() => {
                    let Some(msg) = msg else { reason = "connection lost".into(); break };
                    let msg = match msg {
                        Ok(m) => m,
                        Err(e) => {
                            if last_bad_log.is_none_or(|t| t.elapsed() >= BAD_MESSAGE_LOG_EVERY) {
                                last_bad_log = Some(Instant::now());
                                log::warn!("remote: ignored a message from {} that did not decode: {e}", shared.partner);
                            }
                            continue;
                        }
                    };
                    shared.last_rx_ms.store(self.now_ms(), Ordering::Relaxed);
                    match msg {
                        Message::Frame(f) if shared.i_receive => {
                            let now = Instant::now();
                            bucket = (bucket + now.duration_since(bucket_at).as_secs_f32() * MAX_FRAMES_PER_SEC)
                                .min(MAX_FRAMES_PER_SEC / 5.0);
                            bucket_at = now;
                            if bucket < 1.0 || f.seq <= last_seq || f.slots.len() > 16 {
                                continue;
                            }
                            bucket -= 1.0;
                            last_seq = f.seq;
                            shared.last_frame_ms.store(self.now_ms(), Ordering::Relaxed);
                            if let Ok(mut j) = self.inner.inbound.lock() {
                                j.push(f, self.now_ms() as i64);
                            }
                        }
                        Message::Policy(p) if shared.i_control => {
                            if let Ok(mut slot) = shared.partner_policy.lock() {
                                *slot = Some(p);
                            }
                        }
                        Message::Output(o) if shared.i_control => {
                            if let Ok(mut slot) = shared.partner_output.lock() {
                                *slot = o;
                            }
                        }
                        Message::Feedback { kind } if shared.i_control => {
                            let counter = match kind {
                                FeedbackKind::More => &shared.feedback_more,
                                FeedbackKind::Less => &shared.feedback_less,
                            };
                            counter.fetch_add(1, Ordering::Relaxed);
                            self.log(format!(
                                "{} asks for {}",
                                shared.partner,
                                if kind == FeedbackKind::More { "more" } else { "less" }
                            ));
                        }
                        Message::Ping { seq, ts } => {
                            let _ = ch.try_send(&Message::Pong { seq, ts });
                        }
                        Message::Pong { ts, .. } => {
                            let rtt = self.now_ms().saturating_sub(ts);
                            shared.latency_ms.store(rtt.min(60_000) as u32, Ordering::Relaxed);
                        }
                        Message::Stop { reason: r } => {
                            remote_stop = true;
                            reason = format!("stopped by {}: {r}", shared.partner);
                            break;
                        }
                        Message::Bye => { remote_stop = true; reason = "the partner left".into(); break }
                        _ => {}
                    }
                }
                r = stop_rx.recv() => {
                    let r = r.unwrap_or_else(|| "stopped".into());
                    #[cfg(test)]
                    if r == DROP_FOR_TEST {
                        reason = "connection lost".into();
                        break;
                    }
                    let _ = ch.send(&Message::Stop { reason: r.clone() }).await;
                    shared.stopped_by_us.store(true, Ordering::Relaxed);
                    reason = r;
                    break;
                }
                Ok(()) = policy_rx.changed() => {
                    let p = policy_rx.borrow_and_update().clone();
                    if let Some(p) = p {
                        let _ = ch.send(&Message::Policy(p)).await;
                    }
                }
                _ = tick.tick() => {
                    if past(shared.deadline_ms, store::now_unix_ms()) {
                        let r = "the invite's time is up".to_string();
                        let _ = ch.send(&Message::Stop { reason: r.clone() }).await;
                        shared.stopped_by_us.store(true, Ordering::Relaxed);
                        reason = r;
                        break;
                    }
                    let now = Instant::now();
                    let fresh = if shared.i_control && frames_rx.has_changed().unwrap_or(false) {
                        frames_rx.borrow_and_update().clone()
                    } else {
                        None
                    };
                    if let Some(f) = fresh.filter(|f| self.now_ms().saturating_sub(f.ts) < 500) {
                        let _ = ch.try_send(&Message::Frame(f));
                    }
                    if now.duration_since(last_ping) >= HEARTBEAT {
                        last_ping = now;
                        ping_seq += 1;
                        let _ = ch.try_send(&Message::Ping { seq: ping_seq, ts: self.now_ms() });
                    }
                    if shared.i_receive {
                        let queued: Vec<FeedbackKind> = shared
                            .feedback_out
                            .lock()
                            .map(|mut q| std::mem::take(&mut *q))
                            .unwrap_or_default();
                        for kind in queued {
                            let _ = ch.try_send(&Message::Feedback { kind });
                        }
                    }
                    if shared.i_receive && now.duration_since(last_output) >= OUTPUT_EVERY {
                        last_output = now;
                        let report = self.inner.output.lock().map(|o| o.clone()).unwrap_or_default();
                        let _ = ch.try_send(&Message::Output(report));
                    }
                    let quiet = self.now_ms().saturating_sub(shared.last_rx_ms.load(Ordering::Relaxed));
                    if quiet > DEAD_AFTER.as_millis() as u64 {
                        reason = "the partner stopped answering".into();
                        break;
                    }
                }
            }
        }
        drop(ch);
        self.end_session(&shared, &reason);
        let by_us = shared.stopped_by_us.load(Ordering::Relaxed);
        !remote_stop && !by_us
    }

    fn end_session(&self, shared: &SessionShared, reason: &str) {
        if let Ok(mut l) = self.inner.sessions.lock() {
            l.retain(|s| s.id != shared.id);
        }
        if shared.i_receive
            && let Ok(mut j) = self.inner.inbound.lock()
        {
            j.clear();
        }
        self.log(format!("Session with {} ended — {reason}", shared.partner));
    }

    async fn try_resume(&self, r: &Resume, partner: &str) -> Option<(Channel, Welcome, PathKind)> {
        let deadline = Instant::now() + RESUME_WINDOW;
        let mode = self.config().mode;
        self.log(format!("Reconnecting to {partner}…"));
        while Instant::now() < deadline {
            let grant = JoinGrant::Resume {
                id: r.id.clone(),
                secret: r.secret.clone(),
            };
            match self.dial(&r.targets, mode, r.expect, grant).await {
                Ok(won) => return Some(won),
                Err(e) => {
                    if e.contains("session expired") || e.contains("declined") {
                        break;
                    }
                    tokio::time::sleep(Duration::from_secs(2)).await;
                }
            }
        }
        self.log(format!("Could not reconnect to {partner}"));
        None
    }

    #[cfg(test)]
    pub(crate) fn set_invite_expiry(&self, id: &str, expires: u64) {
        if let Ok(mut s) = self.inner.store.lock() {
            s.set_invite_expiry(id, expires);
        }
    }

    #[cfg(test)]
    pub(crate) fn drop_connections(&self) {
        for s in self.sessions() {
            let _ = s.stop.try_send(DROP_FOR_TEST.to_string());
        }
    }

    pub fn stop_session(&self, id: u64, reason: &str) {
        for s in self.sessions() {
            if s.id == id {
                s.stopped_by_us.store(true, Ordering::Relaxed);
                let _ = s.stop.try_send(reason.to_string());
            }
        }
    }

    pub fn stop_all(&self, reason: &str) {
        let sessions = self.sessions();
        if !sessions.is_empty() {
            self.log(format!("Stop remote: {reason}"));
        }
        for s in sessions {
            s.stopped_by_us.store(true, Ordering::Relaxed);
            let _ = s.stop.try_send(reason.to_string());
        }
        if let Ok(mut j) = self.inner.inbound.lock() {
            j.clear();
        }
        self.answer_consent(ConsentAnswer::Decline);
    }

    pub fn shutdown(&self) {
        self.stop_all("app closing");
        self.stop_network();
        if let Ok(mut t) = self.inner.tasks.lock() {
            for h in t.drain(..) {
                h.abort();
            }
        }
    }

    pub fn status(&self) -> RemoteStatus {
        let now_ms = self.now_ms();
        let now = store::now_unix();
        let sessions = self
            .sessions()
            .iter()
            .map(|s| {
                let lat = s.latency_ms.load(Ordering::Relaxed);
                SessionStatus {
                    id: s.id,
                    partner: s.partner.clone(),
                    fingerprint: s
                        .partner_pk
                        .as_deref()
                        .and_then(crypto::unb64_array::<32>)
                        .map(|k| crypto::fingerprint(&k))
                        .unwrap_or_else(|| "web".into()),
                    client: s.client,
                    path: s.path,
                    latency_ms: (lat != u32::MAX).then_some(lat),
                    secs: s.started.elapsed().as_secs(),
                    i_control: s.i_control,
                    i_receive: s.i_receive,
                    live: s.i_receive
                        && now_ms.saturating_sub(s.last_frame_ms.load(Ordering::Relaxed))
                            <= FAILSAFE.as_millis() as u64,
                    partner_policy: s.partner_policy.lock().ok().and_then(|p| p.clone()),
                    partner_output: s.partner_output.lock().map(|o| o.clone()).unwrap_or_default(),
                    ends_at_ms: (s.deadline_ms != 0).then_some(s.deadline_ms),
                }
            })
            .collect();
        let (invites, partners, fingerprint, room) = self
            .inner
            .store
            .lock()
            .map(|s| {
                (
                    s.invites()
                        .iter()
                        .filter(|i| i.is_live(now))
                        .map(|i| InviteInfo {
                            id: i.id.clone(),
                            label: i.label.clone(),
                            role: i.role,
                            lifetime: i.lifetime,
                            expires: i.expires,
                            uses_left: i.uses_left,
                            web_code: i.web_code,
                        })
                        .collect(),
                    s.partners()
                        .iter()
                        .map(|p| PartnerInfo {
                            pk: p.pk.clone(),
                            name: p.name.clone(),
                            fingerprint: p.fingerprint(),
                            my_role: p.my_role,
                            auto_accept: p.auto_accept,
                            can_dial: p.routes.is_some(),
                            last_seen: p.last_seen,
                        })
                        .collect(),
                    s.identity().fingerprint(),
                    s.identity().room(),
                )
            })
            .unwrap_or_default();
        RemoteStatus {
            fingerprint,
            room,
            listening: self
                .inner
                .listening
                .lock()
                .ok()
                .and_then(|l| l.as_ref().map(|(a, _)| a.to_string())),
            vds: self.inner.vds_state.lock().map(|v| v.clone()).unwrap_or_default(),
            sessions,
            consent: self
                .inner
                .consent
                .lock()
                .ok()
                .and_then(|c| c.as_ref().map(|p| p.req.clone())),
            connecting: self.inner.connecting.lock().ok().and_then(|c| c.clone()),
            last_error: self.inner.last_error.lock().ok().and_then(|e| e.clone()),
            invites,
            partners,
            log: self
                .inner
                .log
                .lock()
                .map(|l| l.iter().rev().take(100).cloned().collect())
                .unwrap_or_default(),
        }
    }
}

struct SessionSetup {
    partner: String,
    partner_pk: Option<String>,
    client: ClientKind,
    path: PathKind,
    i_control: bool,
    i_receive: bool,
    partner_policy: Option<PolicySnapshot>,
    deadline_ms: u64,
}

enum Grant {
    Invite {
        id: String,
        role: OfferedRole,
        pairing: bool,
    },
    Trusted {
        role: OfferedRole,
        auto_accept: bool,
    },
    Resume {
        role: OfferedRole,
        deadline_ms: u64,
    },
}

impl Grant {
    fn deadline_ms(&self) -> u64 {
        match self {
            Grant::Resume { deadline_ms, .. } => *deadline_ms,
            Grant::Invite { .. } | Grant::Trusted { .. } => 0,
        }
    }

    fn role(&self) -> OfferedRole {
        match self {
            Grant::Invite { role, .. } | Grant::Trusted { role, .. } | Grant::Resume { role, .. } => *role,
        }
    }
}
