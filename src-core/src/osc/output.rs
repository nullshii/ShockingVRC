use std::collections::{BTreeMap, HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rosc::OscType;
use serde::{Deserialize, Serialize};
use tokio::net::UdpSocket;
use tokio::sync::{mpsc, oneshot};

use super::oscquery::{AvatarSchema, ParamType, VrchatAddress};
use super::scanner::{encode_and_send, is_sps_param};
use super::types::OscValue;

pub const FLOAT_EPSILON: f32 = 1.0 / 254.0;
pub const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(5);
pub const ADDRESS_DEBOUNCE: Duration = Duration::from_millis(400);
pub const REST_RETRY_DELAY: Duration = Duration::from_millis(100);

pub const CHAT_GLOBAL_MIN_INTERVAL: Duration = Duration::from_millis(1500);
pub const CHAT_LIVE_RESEND_INTERVAL: Duration = Duration::from_secs(20);
pub const CHAT_TRIGGER_HOLD: Duration = Duration::from_secs(3);
pub const CHAT_MAX_UTF16: usize = 144;
pub const CHAT_MAX_LINES: usize = 9;

const WORKER_QUEUE_CAP: usize = 128;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum OscOutType {
    #[default]
    Auto,
    Float,
    Int,
    Bool,
}

impl OscOutType {
    pub const ALL: [OscOutType; 4] = [OscOutType::Auto, OscOutType::Float, OscOutType::Int, OscOutType::Bool];

    pub fn label(self) -> &'static str {
        match self {
            OscOutType::Auto => "Auto",
            OscOutType::Float => "Float",
            OscOutType::Int => "Int",
            OscOutType::Bool => "Bool",
        }
    }

    pub fn as_param_type(self) -> Option<ParamType> {
        match self {
            OscOutType::Auto => None,
            OscOutType::Float => Some(ParamType::Float),
            OscOutType::Int => Some(ParamType::Int),
            OscOutType::Bool => Some(ParamType::Bool),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum ChatboxMode {
    #[default]
    Live,
    Trigger,
}

impl ChatboxMode {
    pub const ALL: [ChatboxMode; 2] = [ChatboxMode::Live, ChatboxMode::Trigger];

    pub fn label(self) -> &'static str {
        match self {
            ChatboxMode::Live => "Live",
            ChatboxMode::Trigger => "Trigger",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OutputSource {
    Local,
    Remote,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum OutputNodeState {
    Live,
    #[default]
    Idle,
    Offline,
    NoParam,
    ReadOnly,
    UnknownType,
    Refused,
    UsedByGraph,
    Muted,
    Limited,
}

impl OutputNodeState {
    pub fn label(self) -> &'static str {
        match self {
            OutputNodeState::Live => "live",
            OutputNodeState::Idle => "idle",
            OutputNodeState::Offline => "offline",
            OutputNodeState::NoParam => "no param",
            OutputNodeState::ReadOnly => "read-only",
            OutputNodeState::UnknownType => "unknown type",
            OutputNodeState::Refused => "refused",
            OutputNodeState::UsedByGraph => "used by Graph",
            OutputNodeState::Muted => "muted",
            OutputNodeState::Limited => "limited",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum SentValue {
    Float(f32),
    Int(i32),
    Bool(bool),
    Text(String),
}

impl SentValue {
    pub fn from_osc(v: OscValue) -> Self {
        match v {
            OscValue::Float(f) => SentValue::Float(f),
            OscValue::Int(i) => SentValue::Int(i),
            OscValue::Bool(b) => SentValue::Bool(b),
        }
    }

    pub fn display_short(&self) -> String {
        match self {
            SentValue::Float(f) => {
                let a = f.abs();
                if a >= 100.0 {
                    format!("{f:.1}")
                } else if a >= 10.0 {
                    format!("{f:.2}")
                } else {
                    format!("{f:.3}")
                }
            }
            SentValue::Int(i) => i.to_string(),
            SentValue::Bool(b) => if *b { "true" } else { "false" }.to_string(),
            SentValue::Text(s) => s.clone(),
        }
    }

    pub fn hash_into<H: Hasher>(&self, state: &mut H) {
        std::mem::discriminant(self).hash(state);
        match self {
            SentValue::Float(f) => f.to_bits().hash(state),
            SentValue::Int(i) => i.hash(state),
            SentValue::Bool(b) => b.hash(state),
            SentValue::Text(s) => s.hash(state),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct OutputNodeStatus {
    pub state: OutputNodeState,
    pub resolved_type: Option<ParamType>,
    pub last_sent: Option<SentValue>,
    pub warning: Option<String>,
}

impl OutputNodeStatus {
    pub fn hash_into<H: Hasher>(&self, state: &mut H) {
        self.state.hash(state);
        self.resolved_type.hash(state);
        self.last_sent.is_some().hash(state);
        if let Some(v) = &self.last_sent {
            v.hash_into(state);
        }
        self.warning.hash(state);
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct OscOutFrame {
    pub flat: u64,
    pub root: u64,
    pub address: String,
    pub ty: OscOutType,
    pub map: bool,
    pub min: f32,
    pub max: f32,
    pub rest: f32,
    pub rate_hz: f32,
    pub value: f32,
    pub enable: bool,
    pub active: bool,
    pub refused: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ChatboxFrame {
    pub flat: u64,
    pub root: u64,
    pub template: String,
    pub count: u8,
    pub mode: ChatboxMode,
    pub interval_ms: u32,
    pub notify: bool,
    pub clear_on_stop: bool,
    pub muted: bool,
    pub values: [f32; 4],
    pub enable: bool,
    pub fired: bool,
    pub active: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum OutgoingOsc {
    Param {
        address: String,
        value: OscValue,
    },
    Chatbox {
        text: String,
        immediate: bool,
        notify: bool,
    },
}

impl OutgoingOsc {
    pub fn into_parts(self) -> (String, Vec<OscType>) {
        match self {
            OutgoingOsc::Param { address, value } => {
                let arg = match value {
                    OscValue::Float(f) => OscType::Float(f),
                    OscValue::Int(i) => OscType::Int(i),
                    OscValue::Bool(b) => OscType::Bool(b),
                };
                (address, vec![arg])
            }
            OutgoingOsc::Chatbox {
                text,
                immediate,
                notify,
            } => (
                "/chatbox/input".to_string(),
                vec![OscType::String(text), OscType::Bool(immediate), OscType::Bool(notify)],
            ),
        }
    }
}

pub fn validate_osc_out_address(address: &str) -> Result<(), String> {
    let trimmed = address.trim();
    let Some(param) = trimmed.strip_prefix("/avatar/parameters/") else {
        return Err("Only /avatar/parameters/<name> addresses can be written — other OSC paths are refused".into());
    };
    if param.is_empty() {
        return Err("Enter a parameter name after /avatar/parameters/".into());
    }
    if param == "OGB_ENABLED" {
        return Err("OGB_ENABLED is reserved for the scanner keepalive and cannot be written".into());
    }
    if is_sps_param(param) {
        return Err(format!(
            "'{param}' is an SPS/contact parameter — writing it would echo back into device input"
        ));
    }
    Ok(())
}

pub fn convert_osc_value(raw: f32, ty: ParamType, map: bool, min: f32, max: f32, rest: f32) -> OscValue {
    let rest = if rest.is_finite() { rest } else { 0.0 };
    let v = if raw.is_finite() { raw } else { rest };
    if ty == ParamType::Bool {
        return OscValue::Bool(v > 0.5);
    }

    let min = if min.is_finite() { min } else { -1.0 };
    let max = if max.is_finite() { max } else { 1.0 };
    let (lo, hi) = if min <= max { (min, max) } else { (max, min) };

    let mapped = if map {
        let t = v.clamp(0.0, 1.0);
        let span = hi - lo;
        let out = if span.is_finite() {
            lo + t * span
        } else {
            lo + (hi * 0.5 - lo * 0.5) * (t * 2.0)
        };
        if out.is_finite() { out } else { lo }
    } else {
        v
    };

    match ty {
        ParamType::Float => {
            let clamped = mapped.clamp(lo, hi);
            OscValue::Float(if clamped.is_finite() { clamped } else { 0.0 })
        }
        ParamType::Int => {
            let eff_lo = lo.clamp(0.0, 255.0).ceil().min(255.0) as i32;
            let eff_hi = hi.clamp(0.0, 255.0).floor().max(0.0) as i32;
            let rounded = mapped.round().clamp(0.0, 255.0) as i32;
            let val = if eff_lo <= eff_hi {
                rounded.clamp(eff_lo, eff_hi)
            } else {
                mapped.clamp(lo, hi).round().clamp(0.0, 255.0) as i32
            };
            OscValue::Int(val)
        }
        ParamType::Bool => unreachable!(),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatboxRender {
    pub text: String,
    pub utf16_len: usize,
    pub truncated: bool,
    pub bad_placeholders: Vec<String>,
}

pub fn render_chatbox_template(template: &str, count: u8, values: [f32; 4]) -> ChatboxRender {
    let count = count.clamp(1, 4) as usize;
    let mut expanded = String::with_capacity(template.len() + 16);
    let mut bad_placeholders = Vec::new();
    let bytes = template.as_bytes();
    let mut idx = 0usize;

    while idx < template.len() {
        let ch = template[idx..].chars().next().expect("idx is on char boundary");
        let ch_len = ch.len_utf8();
        match ch {
            '\\' if idx + 1 < bytes.len() && bytes[idx + 1] == b'n' => {
                expanded.push('\n');
                idx += 2;
            }
            '{' => {
                if idx + 1 < bytes.len() && bytes[idx + 1] == b'{' {
                    expanded.push('{');
                    idx += 2;
                } else {
                    let rest = &template[idx + 1..];
                    let close = rest.find(['}', '{']);
                    match close {
                        Some(rel) if rest.as_bytes()[rel] == b'}' => {
                            let spec = &rest[..rel];
                            let full = &template[idx..idx + 1 + rel + 1];
                            if let Some(formatted) = format_placeholder(spec, count, values) {
                                expanded.push_str(&formatted);
                            } else {
                                expanded.push_str(full);
                                bad_placeholders.push(full.to_string());
                            }
                            idx += 1 + rel + 1;
                        }
                        _ => {
                            let literal = &template[idx..idx + 1];
                            expanded.push_str(literal);
                            bad_placeholders.push("{".to_string());
                            idx += 1;
                        }
                    }
                }
            }
            '}' => {
                if idx + 1 < bytes.len() && bytes[idx + 1] == b'}' {
                    expanded.push('}');
                    idx += 2;
                } else {
                    expanded.push('}');
                    idx += 1;
                }
            }
            _ => {
                expanded.push(ch);
                idx += ch_len;
            }
        }
    }

    let mut text = String::with_capacity(expanded.len().min(CHAT_MAX_UTF16 * 2));
    let mut utf16_len = 0usize;
    let mut lines = 1usize;
    let mut truncated = false;

    for ch in expanded.chars() {
        if ch.is_control() && ch != '\n' {
            continue;
        }
        if ch == '\n' {
            if lines >= CHAT_MAX_LINES {
                truncated = true;
                break;
            }
            lines += 1;
        }
        let cu = ch.len_utf16();
        if utf16_len + cu > CHAT_MAX_UTF16 {
            truncated = true;
            break;
        }
        text.push(ch);
        utf16_len += cu;
    }

    ChatboxRender {
        text,
        utf16_len,
        truncated,
        bad_placeholders,
    }
}

fn format_placeholder(spec: &str, count: usize, values: [f32; 4]) -> Option<String> {
    let (idx_str, fmt_opt) = match spec.split_once(':') {
        Some((i, f)) => (i, Some(f)),
        None => (spec, None),
    };
    let n: usize = idx_str.parse().ok()?;
    if n == 0 || n > count || n > 4 {
        return None;
    }
    let raw = values[n - 1];
    let v = if raw.is_finite() { raw } else { 0.0 };
    let clean_zero = |x: f32| if x == 0.0 { 0.0 } else { x };

    match fmt_opt {
        None => Some(format!("{:.2}", clean_zero(v))),
        Some("0") => Some(format!("{:.0}", clean_zero(v.round()))),
        Some("%") => Some(format!("{:.0}%", clean_zero((v * 100.0).round()))),
        Some(".0") => Some(format!("{:.0}", clean_zero(v.round()))),
        Some(".1") => Some(format!("{:.1}", clean_zero(v))),
        Some(".2") => Some(format!("{:.2}", clean_zero(v))),
        Some(".3") => Some(format!("{:.3}", clean_zero(v))),
        Some(".4") => Some(format!("{:.4}", clean_zero(v))),
        _ => None,
    }
}

#[derive(Debug, Clone, Copy)]
struct RestSpec {
    ty: ParamType,
    map: bool,
    min: f32,
    max: f32,
    rest: f32,
}

#[derive(Debug, Clone, Copy)]
struct PendingRest {
    value: OscValue,
    retry_at: Instant,
    sent_first: bool,
}

#[derive(Debug, Clone, Default)]
struct AddrSendState {
    owner: Option<(OutputSource, u64)>,
    last_sent: Option<OscValue>,
    last_sent_at: Option<Instant>,
    last_rest_spec: Option<RestSpec>,
    pending_rest: Option<PendingRest>,
}

#[derive(Debug, Clone)]
struct AddrTrack {
    address: String,
    changed_at: Instant,
}

#[derive(Debug, Default)]
struct SourceFrames {
    osc: Vec<OscOutFrame>,
    chat: Vec<ChatboxFrame>,
    active: bool,
}

#[derive(Debug, Clone)]
struct ShowingChat {
    source: OutputSource,
    flat: u64,
    text: String,
    is_live: bool,
    clear_on_stop: bool,
}

#[derive(Debug, Clone)]
struct PendingTrigger {
    source: OutputSource,
    flat: u64,
    text: String,
    notify: bool,
}

#[derive(Debug, Default)]
struct ChatboxState {
    last_sent_at: Option<Instant>,
    live_last_sent_at: Option<Instant>,
    showing: Option<ShowingChat>,
    pending_trigger: Option<PendingTrigger>,
    trigger_hold_until: Option<Instant>,
    pending_clear: bool,
}

pub struct OutputScheduler {
    vrchat_addr: Option<VrchatAddress>,
    schema: Arc<AvatarSchema>,
    stop_param: Option<String>,
    local: SourceFrames,
    remote: SourceFrames,
    addr_tracks: HashMap<(OutputSource, u64), AddrTrack>,
    addr_states: HashMap<String, AddrSendState>,
    node_last_sent: HashMap<(OutputSource, u64), SentValue>,
    chat: ChatboxState,
    statuses_local: BTreeMap<u64, OutputNodeStatus>,
    statuses_remote: BTreeMap<u64, OutputNodeStatus>,
}

impl Default for OutputScheduler {
    fn default() -> Self {
        Self::new()
    }
}

impl OutputScheduler {
    pub fn new() -> Self {
        Self {
            vrchat_addr: None,
            schema: Arc::new(AvatarSchema::default()),
            stop_param: None,
            local: SourceFrames::default(),
            remote: SourceFrames::default(),
            addr_tracks: HashMap::new(),
            addr_states: HashMap::new(),
            node_last_sent: HashMap::new(),
            chat: ChatboxState::default(),
            statuses_local: BTreeMap::new(),
            statuses_remote: BTreeMap::new(),
        }
    }

    pub fn vrchat_address(&self) -> Option<&VrchatAddress> {
        self.vrchat_addr.as_ref()
    }

    pub fn set_vrchat_address(&mut self, addr: Option<VrchatAddress>, now: Instant) {
        if self.vrchat_addr == addr {
            return;
        }
        self.vrchat_addr = addr;
        self.forget_avatar_state();
        self.chat.pending_trigger = None;
        self.chat.pending_clear = false;
        self.chat.showing = None;
        self.chat.trigger_hold_until = None;
        let _ = now;
    }

    pub fn set_schema(&mut self, schema: Arc<AvatarSchema>) {
        self.schema = schema;
    }

    pub fn set_stop_param(&mut self, stop_param: Option<String>) {
        self.stop_param = stop_param.and_then(|s| {
            let t = s.trim().to_string();
            (!t.is_empty()).then_some(t)
        });
    }

    pub fn on_avatar_change(&mut self) {
        self.forget_avatar_state();
    }

    fn forget_avatar_state(&mut self) {
        for st in self.addr_states.values_mut() {
            st.last_sent = None;
            st.last_sent_at = None;
            st.pending_rest = None;
            st.owner = None;
            st.last_rest_spec = None;
        }
        self.node_last_sent.retain(|_, v| matches!(v, SentValue::Text(_)));
    }

    pub fn statuses(&self, source: OutputSource) -> BTreeMap<u64, OutputNodeStatus> {
        match source {
            OutputSource::Local => self.statuses_local.clone(),
            OutputSource::Remote => self.statuses_remote.clone(),
        }
    }

    pub fn next_deadline(&self) -> Option<Instant> {
        self.vrchat_addr.as_ref()?;
        let mut earliest: Option<Instant> = None;
        let mut push = |t: Instant| {
            earliest = Some(match earliest {
                Some(cur) => cur.min(t),
                None => t,
            });
        };
        for st in self.addr_states.values() {
            if let Some(pr) = st.pending_rest {
                push(pr.retry_at);
            }
        }
        if self.chat.pending_clear || self.chat.pending_trigger.is_some() {
            let at = self
                .chat
                .last_sent_at
                .map(|t| t + CHAT_GLOBAL_MIN_INTERVAL)
                .unwrap_or_else(Instant::now);
            push(at);
        }
        earliest
    }

    pub fn submit(
        &mut self,
        source: OutputSource,
        osc: Vec<OscOutFrame>,
        chat: Vec<ChatboxFrame>,
        now: Instant,
        emit: impl FnMut(OutgoingOsc) -> bool,
    ) {
        let mut seen_flats = HashSet::with_capacity(osc.len());
        for f in &osc {
            seen_flats.insert(f.flat);
            let key = (source, f.flat);
            match self.addr_tracks.get_mut(&key) {
                Some(track) if track.address == f.address => {}
                Some(track) => {
                    track.address = f.address.clone();
                    track.changed_at = now;
                    self.node_last_sent.remove(&key);
                }
                None => {
                    self.addr_tracks.insert(
                        key,
                        AddrTrack {
                            address: f.address.clone(),
                            changed_at: now,
                        },
                    );
                }
            }
        }
        self.addr_tracks
            .retain(|(s, flat), _| *s != source || seen_flats.contains(flat));

        let slot = match source {
            OutputSource::Local => &mut self.local,
            OutputSource::Remote => &mut self.remote,
        };
        slot.osc = osc;
        slot.chat = chat;
        slot.active = true;

        self.run_step(now, false, emit);
    }

    pub fn release(&mut self, source: OutputSource, now: Instant, emit: impl FnMut(OutgoingOsc) -> bool) {
        let slot = match source {
            OutputSource::Local => &mut self.local,
            OutputSource::Remote => &mut self.remote,
        };
        slot.osc.clear();
        slot.chat.clear();
        slot.active = false;
        self.run_step(now, false, emit);
    }

    pub fn release_for_shutdown(&mut self, now: Instant, emit: impl FnMut(OutgoingOsc) -> bool) {
        self.local.osc.clear();
        self.local.chat.clear();
        self.local.active = false;
        self.remote.osc.clear();
        self.remote.chat.clear();
        self.remote.active = false;
        self.chat.pending_trigger = None;
        self.run_step(now, true, emit);
    }

    pub fn poll(&mut self, now: Instant, emit: impl FnMut(OutgoingOsc) -> bool) {
        self.run_step(now, false, emit);
    }

    fn run_step(&mut self, now: Instant, bypass_chat_limiter: bool, mut emit: impl FnMut(OutgoingOsc) -> bool) {
        self.step_osc(now, &mut emit);
        self.step_chat(now, bypass_chat_limiter, &mut emit);
    }

    fn evaluate_osc_frame(
        &self,
        source: OutputSource,
        frame: &OscOutFrame,
        now: Instant,
    ) -> Result<(ParamType, Option<String>, bool), OutputNodeStatus> {
        let last_sent = self.node_last_sent.get(&(source, frame.flat)).cloned();

        if let Some(reason) = &frame.refused {
            return Err(OutputNodeStatus {
                state: OutputNodeState::Refused,
                resolved_type: None,
                last_sent,
                warning: Some(reason.clone()),
            });
        }
        if let Some(sp) = &self.stop_param
            && frame.address == *sp
        {
            return Err(OutputNodeStatus {
                state: OutputNodeState::Refused,
                resolved_type: None,
                last_sent,
                warning: Some("Refused: matches remote stop parameter".into()),
            });
        }
        if self.vrchat_addr.is_none() {
            return Err(OutputNodeStatus {
                state: OutputNodeState::Offline,
                resolved_type: frame.ty.as_param_type(),
                last_sent,
                warning: None,
            });
        }

        let schema_info = self.schema.params.get(&frame.address);
        if let Some(info) = schema_info
            && !info.is_writable()
        {
            return Err(OutputNodeStatus {
                state: OutputNodeState::ReadOnly,
                resolved_type: info.ty,
                last_sent,
                warning: Some("Parameter is read-only in avatar schema".into()),
            });
        }

        let (resolved_ty, warning) = match frame.ty {
            OscOutType::Auto => match schema_info {
                None => {
                    return Err(OutputNodeStatus {
                        state: OutputNodeState::NoParam,
                        resolved_type: None,
                        last_sent,
                        warning: None,
                    });
                }
                Some(info) => match info.ty {
                    Some(ty) => (ty, None),
                    None => {
                        return Err(OutputNodeStatus {
                            state: OutputNodeState::UnknownType,
                            resolved_type: None,
                            last_sent,
                            warning: None,
                        });
                    }
                },
            },
            manual => {
                let ty = manual.as_param_type().expect("manual type");
                let warn = if schema_info.is_none() {
                    Some("Parameter not in avatar schema".into())
                } else {
                    None
                };
                (ty, warn)
            }
        };

        if !frame.active || !frame.enable {
            return Err(OutputNodeStatus {
                state: OutputNodeState::Idle,
                resolved_type: Some(resolved_ty),
                last_sent,
                warning,
            });
        }

        let settled = self
            .addr_tracks
            .get(&(source, frame.flat))
            .is_some_and(|t| now.saturating_duration_since(t.changed_at) >= ADDRESS_DEBOUNCE);

        Ok((resolved_ty, warning, settled))
    }

    fn step_osc(&mut self, now: Instant, emit: &mut impl FnMut(OutgoingOsc) -> bool) {
        self.statuses_local.clear();
        self.statuses_remote.clear();

        let mut winners: HashMap<String, (OutputSource, usize, ParamType, Option<String>, bool)> = HashMap::new();

        for (idx, frame) in self.local.osc.iter().enumerate() {
            match self.evaluate_osc_frame(OutputSource::Local, frame, now) {
                Ok((ty, warn, settled)) => {
                    winners
                        .entry(frame.address.clone())
                        .or_insert((OutputSource::Local, idx, ty, warn, settled));
                }
                Err(status) => {
                    self.statuses_local.insert(frame.flat, status);
                }
            }
        }

        for (idx, frame) in self.remote.osc.iter().enumerate() {
            match self.evaluate_osc_frame(OutputSource::Remote, frame, now) {
                Ok((ty, warn, settled)) => {
                    if let Some((OutputSource::Local, ..)) = winners.get(&frame.address) {
                        self.statuses_remote.insert(
                            frame.flat,
                            OutputNodeStatus {
                                state: OutputNodeState::UsedByGraph,
                                resolved_type: Some(ty),
                                last_sent: self.node_last_sent.get(&(OutputSource::Remote, frame.flat)).cloned(),
                                warning: warn,
                            },
                        );
                    } else {
                        winners
                            .entry(frame.address.clone())
                            .or_insert((OutputSource::Remote, idx, ty, warn, settled));
                    }
                }
                Err(status) => {
                    self.statuses_remote.insert(frame.flat, status);
                }
            }
        }

        let active_addrs: HashSet<String> = winners.keys().cloned().collect();
        for (addr, (src, idx, ty, warning, settled)) in winners {
            let frame = match src {
                OutputSource::Local => &self.local.osc[idx],
                OutputSource::Remote => &self.remote.osc[idx],
            };
            let key = (src, frame.flat);
            if !settled {
                let status = OutputNodeStatus {
                    state: OutputNodeState::Idle,
                    resolved_type: Some(ty),
                    last_sent: self.node_last_sent.get(&key).cloned(),
                    warning,
                };
                match src {
                    OutputSource::Local => {
                        self.statuses_local.insert(frame.flat, status);
                    }
                    OutputSource::Remote => {
                        self.statuses_remote.insert(frame.flat, status);
                    }
                }
                continue;
            }

            let st = self.addr_states.entry(addr.clone()).or_default();
            let took_over = st.owner.is_some() && st.owner != Some(key);
            st.owner = Some(key);
            st.pending_rest = None;
            st.last_rest_spec = Some(RestSpec {
                ty,
                map: frame.map,
                min: frame.min,
                max: frame.max,
                rest: frame.rest,
            });
            if took_over {
                st.last_sent_at = None;
            }

            let desired = convert_osc_value(frame.value, ty, frame.map, frame.min, frame.max, frame.rest);
            let min_interval =
                Duration::from_micros((1_000_000.0 / f64::from(frame.rate_hz.clamp(1.0, 40.0))).round() as u64);
            let should_send = match (st.last_sent, st.last_sent_at) {
                (None, _) => true,
                (Some(prev), Some(prev_at)) => match (prev, desired) {
                    (OscValue::Bool(a), OscValue::Bool(b)) => {
                        a != b || now.saturating_duration_since(prev_at) >= HEARTBEAT_INTERVAL
                    }
                    (OscValue::Int(a), OscValue::Int(b)) => {
                        a != b || now.saturating_duration_since(prev_at) >= HEARTBEAT_INTERVAL
                    }
                    (OscValue::Float(a), OscValue::Float(b)) => {
                        let elapsed = now.saturating_duration_since(prev_at);
                        if (b - a).abs() >= FLOAT_EPSILON {
                            elapsed >= min_interval
                        } else {
                            elapsed >= HEARTBEAT_INTERVAL
                        }
                    }
                    _ => true,
                },
                (Some(prev), None) => match (prev, desired) {
                    (OscValue::Bool(a), OscValue::Bool(b)) => a != b,
                    (OscValue::Int(a), OscValue::Int(b)) => a != b,
                    (OscValue::Float(a), OscValue::Float(b)) => (b - a).abs() >= FLOAT_EPSILON,
                    _ => true,
                },
            };

            if should_send {
                let msg = OutgoingOsc::Param {
                    address: addr.clone(),
                    value: desired,
                };
                if emit(msg) {
                    st.last_sent = Some(desired);
                    st.last_sent_at = Some(now);
                    self.node_last_sent.insert(key, SentValue::from_osc(desired));
                }
            }

            let status = OutputNodeStatus {
                state: OutputNodeState::Live,
                resolved_type: Some(ty),
                last_sent: self.node_last_sent.get(&key).cloned(),
                warning,
            };
            match src {
                OutputSource::Local => {
                    self.statuses_local.insert(frame.flat, status);
                }
                OutputSource::Remote => {
                    self.statuses_remote.insert(frame.flat, status);
                }
            }
        }

        let offline = self.vrchat_addr.is_none();
        let all_addrs: Vec<String> = self.addr_states.keys().cloned().collect();
        for addr in all_addrs {
            if active_addrs.contains(&addr) {
                continue;
            }
            let Some(st) = self.addr_states.get_mut(&addr) else {
                continue;
            };
            if offline {
                st.owner = None;
                st.pending_rest = None;
                continue;
            }
            if st.owner.take().is_some() {
                if st.last_sent.is_some()
                    && let Some(spec) = st.last_rest_spec
                {
                    let rest_val = convert_osc_value(spec.rest, spec.ty, spec.map, spec.min, spec.max, spec.rest);
                    let msg = OutgoingOsc::Param {
                        address: addr.clone(),
                        value: rest_val,
                    };
                    if emit(msg) {
                        st.last_sent = None;
                        st.last_sent_at = None;
                        st.pending_rest = Some(PendingRest {
                            value: rest_val,
                            retry_at: now + REST_RETRY_DELAY,
                            sent_first: true,
                        });
                    } else {
                        st.pending_rest = Some(PendingRest {
                            value: rest_val,
                            retry_at: now,
                            sent_first: false,
                        });
                    }
                }
            } else if let Some(pr) = st.pending_rest {
                if !pr.sent_first {
                    let msg = OutgoingOsc::Param {
                        address: addr.clone(),
                        value: pr.value,
                    };
                    if emit(msg) {
                        st.last_sent = None;
                        st.last_sent_at = None;
                        st.pending_rest = Some(PendingRest {
                            value: pr.value,
                            retry_at: now + REST_RETRY_DELAY,
                            sent_first: true,
                        });
                    }
                } else if now >= pr.retry_at {
                    let msg = OutgoingOsc::Param {
                        address: addr.clone(),
                        value: pr.value,
                    };
                    if emit(msg) {
                        st.pending_rest = None;
                    }
                }
            }
        }
    }

    fn step_chat(&mut self, now: Instant, bypass_chat_limiter: bool, emit: &mut impl FnMut(OutgoingOsc) -> bool) {
        let online = self.vrchat_addr.is_some();

        for (src, frames) in [
            (OutputSource::Local, &mut self.local.chat),
            (OutputSource::Remote, &mut self.remote.chat),
        ] {
            for f in frames.iter_mut() {
                if !f.fired {
                    continue;
                }
                f.fired = false;
                if online && f.active && !f.muted && f.mode == ChatboxMode::Trigger {
                    let rendered = render_chatbox_template(&f.template, f.count, f.values);
                    self.chat.pending_trigger = Some(PendingTrigger {
                        source: src,
                        flat: f.flat,
                        text: rendered.text,
                        notify: f.notify,
                    });
                    self.chat.pending_clear = false;
                }
            }
        }

        if online && let Some(trig) = self.chat.pending_trigger.clone() {
            let global_ok = self
                .chat
                .last_sent_at
                .is_none_or(|t| now.saturating_duration_since(t) >= CHAT_GLOBAL_MIN_INTERVAL);
            if global_ok {
                let msg = OutgoingOsc::Chatbox {
                    text: trig.text.clone(),
                    immediate: true,
                    notify: trig.notify,
                };
                if emit(msg) {
                    self.chat.last_sent_at = Some(now);
                    self.chat.trigger_hold_until = Some(now + CHAT_TRIGGER_HOLD);
                    self.chat.showing = Some(ShowingChat {
                        source: trig.source,
                        flat: trig.flat,
                        text: trig.text.clone(),
                        is_live: false,
                        clear_on_stop: false,
                    });
                    self.node_last_sent
                        .insert((trig.source, trig.flat), SentValue::Text(trig.text));
                    self.chat.pending_trigger = None;
                }
            }
        }

        let local_live_idx = if online {
            self.local
                .chat
                .iter()
                .position(|f| f.active && !f.muted && f.mode == ChatboxMode::Live && f.enable)
        } else {
            None
        };
        let remote_live_idx = if online {
            self.remote
                .chat
                .iter()
                .position(|f| f.active && !f.muted && f.mode == ChatboxMode::Live && f.enable)
        } else {
            None
        };

        let live_winner: Option<(OutputSource, usize)> = match (local_live_idx, remote_live_idx) {
            (Some(i), _) => Some((OutputSource::Local, i)),
            (None, Some(i)) => Some((OutputSource::Remote, i)),
            (None, None) => None,
        };

        if let Some(sh) = &mut self.chat.showing
            && sh.is_live
            && !sh.text.is_empty()
        {
            let still_winning_frame = live_winner.and_then(|(src, i)| {
                let f = match src {
                    OutputSource::Local => &self.local.chat[i],
                    OutputSource::Remote => &self.remote.chat[i],
                };
                (src == sh.source && f.flat == sh.flat).then_some(f)
            });
            if let Some(f) = still_winning_frame {
                sh.clear_on_stop = f.clear_on_stop;
            } else if sh.clear_on_stop {
                self.chat.pending_clear = true;
            }
        }

        if live_winner.is_some() {
            self.chat.pending_clear = false;
        }

        if online && self.chat.pending_clear && self.chat.pending_trigger.is_none() {
            let global_ok = bypass_chat_limiter
                || self
                    .chat
                    .last_sent_at
                    .is_none_or(|t| now.saturating_duration_since(t) >= CHAT_GLOBAL_MIN_INTERVAL);
            if global_ok {
                let msg = OutgoingOsc::Chatbox {
                    text: String::new(),
                    immediate: true,
                    notify: false,
                };
                if emit(msg) {
                    self.chat.last_sent_at = Some(now);
                    self.chat.live_last_sent_at = Some(now);
                    self.chat.showing = None;
                    self.chat.pending_clear = false;
                }
            }
        }

        let mut live_winner_limited = false;
        if online && let Some((win_src, win_idx)) = live_winner {
            let frame = match win_src {
                OutputSource::Local => &self.local.chat[win_idx],
                OutputSource::Remote => &self.remote.chat[win_idx],
            };
            let in_hold = self.chat.trigger_hold_until.is_some_and(|until| now < until);
            if self.chat.pending_trigger.is_some() || in_hold {
                live_winner_limited = true;
            } else {
                let rendered = render_chatbox_template(&frame.template, frame.count, frame.values);
                let same_live = self
                    .chat
                    .showing
                    .as_ref()
                    .is_some_and(|sh| sh.is_live && sh.source == win_src && sh.flat == frame.flat);
                let text_changed = !same_live || self.chat.showing.as_ref().is_none_or(|sh| sh.text != rendered.text);
                let interval =
                    Duration::from_millis(frame.interval_ms.clamp(1500, 30_000) as u64).max(CHAT_GLOBAL_MIN_INTERVAL);
                let global_ok = self
                    .chat
                    .last_sent_at
                    .is_none_or(|t| now.saturating_duration_since(t) >= CHAT_GLOBAL_MIN_INTERVAL);
                let node_ok = !same_live
                    || self
                        .chat
                        .live_last_sent_at
                        .is_none_or(|t| now.saturating_duration_since(t) >= interval);
                let need_resend = same_live
                    && !text_changed
                    && self
                        .chat
                        .live_last_sent_at
                        .is_some_and(|t| now.saturating_duration_since(t) >= CHAT_LIVE_RESEND_INTERVAL);

                if (text_changed && global_ok && node_ok) || (need_resend && global_ok) {
                    let msg = OutgoingOsc::Chatbox {
                        text: rendered.text.clone(),
                        immediate: true,
                        notify: false,
                    };
                    if emit(msg) {
                        self.chat.last_sent_at = Some(now);
                        self.chat.live_last_sent_at = Some(now);
                        self.chat.showing = Some(ShowingChat {
                            source: win_src,
                            flat: frame.flat,
                            text: rendered.text.clone(),
                            is_live: true,
                            clear_on_stop: frame.clear_on_stop,
                        });
                        self.node_last_sent
                            .insert((win_src, frame.flat), SentValue::Text(rendered.text));
                    } else {
                        live_winner_limited = true;
                    }
                } else if text_changed {
                    live_winner_limited = true;
                }
            }
        }

        for (src, frames, winner_local_idx) in [
            (OutputSource::Local, &self.local.chat, local_live_idx),
            (OutputSource::Remote, &self.remote.chat, remote_live_idx),
        ] {
            for (idx, f) in frames.iter().enumerate() {
                let key = (src, f.flat);
                let last_sent = self.node_last_sent.get(&key).cloned();
                let (state, warning) = if f.muted {
                    (OutputNodeState::Muted, None)
                } else if !online {
                    (OutputNodeState::Offline, None)
                } else if !f.active {
                    (OutputNodeState::Idle, None)
                } else {
                    match f.mode {
                        ChatboxMode::Live => {
                            if !f.enable {
                                (OutputNodeState::Idle, None)
                            } else if live_winner == Some((src, idx)) {
                                if live_winner_limited {
                                    (OutputNodeState::Limited, None)
                                } else {
                                    (OutputNodeState::Live, None)
                                }
                            } else if src == OutputSource::Remote && local_live_idx.is_some() {
                                (OutputNodeState::UsedByGraph, None)
                            } else if winner_local_idx.is_some() {
                                (
                                    OutputNodeState::Idle,
                                    Some("Another Live chatbox is earlier in the graph".into()),
                                )
                            } else {
                                (OutputNodeState::Idle, None)
                            }
                        }
                        ChatboxMode::Trigger => {
                            let is_pending = self
                                .chat
                                .pending_trigger
                                .as_ref()
                                .is_some_and(|t| t.source == src && t.flat == f.flat);
                            let is_holding = self.chat.trigger_hold_until.is_some_and(|u| now < u)
                                && self
                                    .chat
                                    .showing
                                    .as_ref()
                                    .is_some_and(|sh| !sh.is_live && sh.source == src && sh.flat == f.flat);
                            if is_pending {
                                (OutputNodeState::Limited, None)
                            } else if is_holding {
                                (OutputNodeState::Live, None)
                            } else {
                                (OutputNodeState::Idle, None)
                            }
                        }
                    }
                };
                let status = OutputNodeStatus {
                    state,
                    resolved_type: None,
                    last_sent,
                    warning,
                };
                match src {
                    OutputSource::Local => {
                        self.statuses_local.insert(f.flat, status);
                    }
                    OutputSource::Remote => {
                        self.statuses_remote.insert(f.flat, status);
                    }
                }
            }
        }
    }
}

enum WorkerMsg {
    Send(VrchatAddress, OutgoingOsc),
    Wake,
    Flush(oneshot::Sender<()>),
}

struct OutputInner {
    scheduler: Mutex<OutputScheduler>,
    tx: mpsc::Sender<WorkerMsg>,
    rx: Mutex<Option<mpsc::Receiver<WorkerMsg>>>,
    send_socket: Arc<tokio::sync::OnceCell<UdpSocket>>,
    shutting_down: AtomicBool,
}

#[derive(Clone)]
pub struct OscOutput {
    inner: Arc<OutputInner>,
}

impl OscOutput {
    pub fn new(send_socket: Arc<tokio::sync::OnceCell<UdpSocket>>) -> Self {
        let (tx, rx) = mpsc::channel(WORKER_QUEUE_CAP);
        Self {
            inner: Arc::new(OutputInner {
                scheduler: Mutex::new(OutputScheduler::new()),
                tx,
                rx: Mutex::new(Some(rx)),
                send_socket,
                shutting_down: AtomicBool::new(false),
            }),
        }
    }

    pub fn ensure_worker(&self) {
        if let Ok(handle) = tokio::runtime::Handle::try_current()
            && let Ok(mut slot) = self.inner.rx.lock()
            && let Some(rx) = slot.take()
        {
            let inner = Arc::clone(&self.inner);
            handle.spawn(async move {
                worker_loop(inner, rx).await;
            });
        }
    }

    pub fn reset_shutdown(&self) {
        self.inner.shutting_down.store(false, Ordering::Release);
    }

    pub fn set_vrchat_address(&self, addr: Option<VrchatAddress>) {
        if let Ok(mut sched) = self.inner.scheduler.lock() {
            sched.set_vrchat_address(addr, Instant::now());
        }
    }

    pub fn set_schema(&self, schema: Arc<AvatarSchema>) {
        if let Ok(mut sched) = self.inner.scheduler.lock() {
            sched.set_schema(schema);
        }
    }

    pub fn on_avatar_change(&self) {
        if let Ok(mut sched) = self.inner.scheduler.lock() {
            sched.on_avatar_change();
        }
    }

    pub fn submit(
        &self,
        source: OutputSource,
        osc: Vec<OscOutFrame>,
        chat: Vec<ChatboxFrame>,
        stop_param: Option<&str>,
    ) {
        if self.inner.shutting_down.load(Ordering::Acquire) {
            return;
        }
        self.ensure_worker();
        let now = Instant::now();
        let wake = {
            let Ok(mut sched) = self.inner.scheduler.lock() else {
                return;
            };
            sched.set_stop_param(stop_param.map(str::to_string));
            let addr = sched.vrchat_address().cloned();
            let tx = &self.inner.tx;
            sched.submit(source, osc, chat, now, |msg| match &addr {
                Some(a) => tx.try_send(WorkerMsg::Send(a.clone(), msg)).is_ok(),
                None => false,
            });
            sched.next_deadline().is_some()
        };
        if wake {
            let _ = self.inner.tx.try_send(WorkerMsg::Wake);
        }
    }

    pub fn release(&self, source: OutputSource) {
        self.ensure_worker();
        let now = Instant::now();
        let wake = {
            let Ok(mut sched) = self.inner.scheduler.lock() else {
                return;
            };
            let addr = sched.vrchat_address().cloned();
            let tx = &self.inner.tx;
            sched.release(source, now, |msg| match &addr {
                Some(a) => tx.try_send(WorkerMsg::Send(a.clone(), msg)).is_ok(),
                None => false,
            });
            sched.next_deadline().is_some()
        };
        if wake {
            let _ = self.inner.tx.try_send(WorkerMsg::Wake);
        }
    }

    pub fn statuses(&self, source: OutputSource) -> BTreeMap<u64, OutputNodeStatus> {
        self.inner
            .scheduler
            .lock()
            .map(|s| s.statuses(source))
            .unwrap_or_default()
    }

    pub async fn shutdown_and_flush(&self, budget: Duration) {
        self.inner.shutting_down.store(true, Ordering::Release);
        self.ensure_worker();
        let deadline = Instant::now() + budget;

        let has_retry = {
            let Ok(mut sched) = self.inner.scheduler.lock() else {
                return;
            };
            let addr = sched.vrchat_address().cloned();
            let tx = &self.inner.tx;
            sched.release_for_shutdown(Instant::now(), |msg| match &addr {
                Some(a) => tx.try_send(WorkerMsg::Send(a.clone(), msg)).is_ok(),
                None => false,
            });
            sched.next_deadline()
        };

        if let Some(retry_at) = has_retry {
            let wait_until = retry_at.min(deadline);
            let now = Instant::now();
            if wait_until > now {
                tokio::time::sleep(wait_until - now).await;
            }
            if let Ok(mut sched) = self.inner.scheduler.lock() {
                let addr = sched.vrchat_address().cloned();
                let tx = &self.inner.tx;
                sched.poll(Instant::now(), |msg| match &addr {
                    Some(a) => tx.try_send(WorkerMsg::Send(a.clone(), msg)).is_ok(),
                    None => false,
                });
            }
        }

        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return;
        }
        let (done_tx, done_rx) = oneshot::channel();
        if tokio::time::timeout(remaining, self.inner.tx.send(WorkerMsg::Flush(done_tx)))
            .await
            .is_ok()
        {
            let rem = deadline.saturating_duration_since(Instant::now());
            let _ = tokio::time::timeout(rem, done_rx).await;
        }
    }
}

async fn worker_loop(inner: Arc<OutputInner>, mut rx: mpsc::Receiver<WorkerMsg>) {
    loop {
        let next_deadline = inner.scheduler.lock().ok().and_then(|s| s.next_deadline());

        let msg = match next_deadline {
            Some(at) => {
                let sleep_dur = at.saturating_duration_since(Instant::now());
                tokio::select! {
                    m = rx.recv() => m,
                    _ = tokio::time::sleep(sleep_dur) => {
                        drive_pending(&inner);
                        continue;
                    }
                }
            }
            None => rx.recv().await,
        };

        let Some(msg) = msg else { break };
        match msg {
            WorkerMsg::Send(addr, out) => {
                let (osc_addr, args) = out.into_parts();
                if let Err(e) = encode_and_send(&inner.send_socket, &addr, osc_addr, args).await {
                    log::trace!("OSC output send failed: {e}");
                }
            }
            WorkerMsg::Wake => {}
            WorkerMsg::Flush(done) => {
                let _ = done.send(());
            }
        }
    }
}

fn drive_pending(inner: &OutputInner) {
    let Ok(mut sched) = inner.scheduler.lock() else {
        return;
    };
    let addr = sched.vrchat_address().cloned();
    let tx = &inner.tx;
    sched.poll(Instant::now(), |msg| match &addr {
        Some(a) => tx.try_send(WorkerMsg::Send(a.clone(), msg)).is_ok(),
        None => false,
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::osc::oscquery::ParamInfo;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};

    fn vrc(port: u16) -> VrchatAddress {
        VrchatAddress {
            osc_ip: "127.0.0.1".into(),
            osc_port: port,
            http_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port + 100),
        }
    }

    fn schema_with(entries: &[(&str, Option<ParamType>, Option<u8>)]) -> Arc<AvatarSchema> {
        let mut s = AvatarSchema {
            generation: 1,
            ..AvatarSchema::default()
        };
        for (addr, ty, access) in entries {
            s.params.insert(
                (*addr).to_string(),
                ParamInfo {
                    ty: *ty,
                    access: *access,
                    range: None,
                },
            );
        }
        Arc::new(s)
    }

    fn osc_frame(flat: u64, addr: &str, value: f32) -> OscOutFrame {
        OscOutFrame {
            flat,
            root: flat,
            address: addr.into(),
            ty: OscOutType::Auto,
            map: false,
            min: -1.0,
            max: 1.0,
            rest: 0.0,
            rate_hz: 10.0,
            value,
            enable: true,
            active: true,
            refused: None,
        }
    }

    fn chat_live(flat: u64, template: &str, v1: f32) -> ChatboxFrame {
        ChatboxFrame {
            flat,
            root: flat,
            template: template.into(),
            count: 1,
            mode: ChatboxMode::Live,
            interval_ms: 2000,
            notify: false,
            clear_on_stop: true,
            muted: false,
            values: [v1, 0.0, 0.0, 0.0],
            enable: true,
            fired: false,
            active: true,
        }
    }

    fn chat_trigger(flat: u64, template: &str, v1: f32, fired: bool, notify: bool) -> ChatboxFrame {
        ChatboxFrame {
            flat,
            root: flat,
            template: template.into(),
            count: 1,
            mode: ChatboxMode::Trigger,
            interval_ms: 2000,
            notify,
            clear_on_stop: false,
            muted: false,
            values: [v1, 0.0, 0.0, 0.0],
            enable: false,
            fired,
            active: true,
        }
    }

    #[test]
    fn value_conversion_handles_nan_inf_inverted_range_and_int_rounding() {
        assert_eq!(
            convert_osc_value(2.5, ParamType::Float, false, -1.0, 1.0, 0.0),
            OscValue::Float(1.0)
        );
        assert_eq!(
            convert_osc_value(0.5, ParamType::Float, true, 1.0, -1.0, 0.0),
            OscValue::Float(0.0)
        );
        assert_eq!(
            convert_osc_value(0.0, ParamType::Float, true, 1.0, -1.0, 0.0),
            OscValue::Float(-1.0)
        );
        assert_eq!(
            convert_osc_value(f32::NAN, ParamType::Float, false, -1.0, 1.0, 0.25),
            OscValue::Float(0.25)
        );
        assert_eq!(
            convert_osc_value(f32::INFINITY, ParamType::Float, false, -1.0, 1.0, -0.5),
            OscValue::Float(-0.5)
        );
        assert_eq!(
            convert_osc_value(f32::NEG_INFINITY, ParamType::Float, false, -1.0, 1.0, 0.75),
            OscValue::Float(0.75)
        );
        assert_eq!(
            convert_osc_value(127.6, ParamType::Int, false, 0.0, 255.0, 0.0),
            OscValue::Int(128)
        );
        assert_eq!(
            convert_osc_value(-5.0, ParamType::Int, false, -10.0, 10.0, 0.0),
            OscValue::Int(0)
        );
        assert_eq!(
            convert_osc_value(300.0, ParamType::Int, false, 0.0, 500.0, 0.0),
            OscValue::Int(255)
        );
        assert_eq!(convert_osc_value(0.5, ParamType::Int, true, 10.0, 20.0, 0.0), OscValue::Int(15));
        assert_eq!(
            convert_osc_value(0.51, ParamType::Bool, true, 100.0, 200.0, 0.0),
            OscValue::Bool(true)
        );
        assert_eq!(
            convert_osc_value(0.5, ParamType::Bool, true, 100.0, 200.0, 0.0),
            OscValue::Bool(false)
        );
        assert_eq!(
            convert_osc_value(f32::NAN, ParamType::Bool, false, 0.0, 1.0, 1.0),
            OscValue::Bool(true)
        );
    }

    #[test]
    fn template_grammar_and_escapes() {
        let r = render_chatbox_template(
            "A={1} B={2:.0} C={2:.1} D={2:.4} I={3:0} P={1:%} {{ok}}\\nline2",
            3,
            [0.356, 1.23456, 9.6, 0.0],
        );
        assert_eq!(r.text, "A=0.36 B=1 C=1.2 D=1.2346 I=10 P=36% {ok}\nline2");
        assert!(r.bad_placeholders.is_empty());
        assert!(!r.truncated);

        let bad = render_chatbox_template("x={2} y={1:.5} z={foo}", 1, [0.5, 0.0, 0.0, 0.0]);
        assert_eq!(bad.text, "x={2} y={1:.5} z={foo}");
        assert_eq!(bad.bad_placeholders, vec!["{2}", "{1:.5}", "{foo}"]);

        let ctrl = render_chatbox_template("a\x07b\rc\td\n2\n3\n4\n5\n6\n7\n8\n9\n10", 1, [0.0; 4]);
        assert_eq!(ctrl.text, "abcd\n2\n3\n4\n5\n6\n7\n8\n9");
        assert!(ctrl.truncated);
    }

    #[test]
    fn template_truncation_respects_utf16_and_emoji_surrogate_pairs() {
        let mut s = "a".repeat(143);
        s.push('😀');
        let r = render_chatbox_template(&s, 1, [0.0; 4]);
        assert_eq!(r.utf16_len, 143);
        assert_eq!(r.text.len(), 143);
        assert!(!r.text.contains('😀'));
        assert!(r.truncated);

        let mut s2 = "a".repeat(142);
        s2.push('😀');
        let r2 = render_chatbox_template(&s2, 1, [0.0; 4]);
        assert_eq!(r2.utf16_len, 144);
        assert!(r2.text.ends_with('😀'));
        assert!(!r2.truncated);

        let emojis = "⚡".repeat(10) + &"😀".repeat(80);
        let r3 = render_chatbox_template(&emojis, 1, [0.0; 4]);
        assert_eq!(r3.utf16_len, 144);
        assert!(r3.truncated);
    }

    #[test]
    fn address_debounce_rate_limit_epsilon_trailing_and_heartbeat() {
        let t0 = Instant::now();
        let mut s = OutputScheduler::new();
        s.set_vrchat_address(Some(vrc(9000)), t0);
        s.set_schema(schema_with(&[("/avatar/parameters/F", Some(ParamType::Float), Some(3))]));

        let mut sent = Vec::new();
        s.submit(
            OutputSource::Local,
            vec![osc_frame(1, "/avatar/parameters/F", 0.5)],
            vec![],
            t0,
            |m| {
                sent.push(m);
                true
            },
        );
        assert!(sent.is_empty(), "must wait 400 ms debounce");

        let t399 = t0 + Duration::from_millis(399);
        s.submit(
            OutputSource::Local,
            vec![osc_frame(1, "/avatar/parameters/F", 0.5)],
            vec![],
            t399,
            |m| {
                sent.push(m);
                true
            },
        );
        assert!(sent.is_empty());

        let t400 = t0 + Duration::from_millis(400);
        s.submit(
            OutputSource::Local,
            vec![osc_frame(1, "/avatar/parameters/F", 0.5)],
            vec![],
            t400,
            |m| {
                sent.push(m);
                true
            },
        );
        assert_eq!(sent.len(), 1);
        assert_eq!(
            sent[0],
            OutgoingOsc::Param {
                address: "/avatar/parameters/F".into(),
                value: OscValue::Float(0.5)
            }
        );

        let t450 = t400 + Duration::from_millis(50);
        s.submit(
            OutputSource::Local,
            vec![osc_frame(1, "/avatar/parameters/F", 0.8)],
            vec![],
            t450,
            |m| {
                sent.push(m);
                true
            },
        );
        assert_eq!(sent.len(), 1, "rate-limited");

        let t500 = t400 + Duration::from_millis(100);
        s.submit(
            OutputSource::Local,
            vec![osc_frame(1, "/avatar/parameters/F", 0.8)],
            vec![],
            t500,
            |m| {
                sent.push(m);
                true
            },
        );
        assert_eq!(sent.len(), 2);
        assert_eq!(
            sent[1],
            OutgoingOsc::Param {
                address: "/avatar/parameters/F".into(),
                value: OscValue::Float(0.8)
            }
        );

        let t650 = t500 + Duration::from_millis(150);
        s.submit(
            OutputSource::Local,
            vec![osc_frame(1, "/avatar/parameters/F", 0.801)],
            vec![],
            t650,
            |m| {
                sent.push(m);
                true
            },
        );
        assert_eq!(sent.len(), 2, "below epsilon");

        let t5500 = t500 + HEARTBEAT_INTERVAL;
        s.submit(
            OutputSource::Local,
            vec![osc_frame(1, "/avatar/parameters/F", 0.801)],
            vec![],
            t5500,
            |m| {
                sent.push(m);
                true
            },
        );
        assert_eq!(sent.len(), 3, "5 s heartbeat");
    }

    #[test]
    fn int_and_bool_send_immediately_on_change() {
        let t0 = Instant::now();
        let mut s = OutputScheduler::new();
        s.set_vrchat_address(Some(vrc(9000)), t0);
        s.set_schema(schema_with(&[
            ("/avatar/parameters/B", Some(ParamType::Bool), Some(3)),
            ("/avatar/parameters/I", Some(ParamType::Int), Some(3)),
        ]));
        let mut fb = osc_frame(1, "/avatar/parameters/B", 0.0);
        let mut fi = osc_frame(2, "/avatar/parameters/I", 1.0);
        fi.min = 0.0;
        fi.max = 255.0;
        fi.rate_hz = 1.0;

        let mut sent = Vec::new();
        s.submit(OutputSource::Local, vec![fb.clone(), fi.clone()], vec![], t0, |m| {
            sent.push(m);
            true
        });
        let t1 = t0 + ADDRESS_DEBOUNCE;
        s.submit(OutputSource::Local, vec![fb.clone(), fi.clone()], vec![], t1, |m| {
            sent.push(m);
            true
        });
        assert_eq!(sent.len(), 2);

        fb.value = 1.0;
        fi.value = 2.0;
        let t2 = t1 + Duration::from_millis(25);
        s.submit(OutputSource::Local, vec![fb, fi], vec![], t2, |m| {
            sent.push(m);
            true
        });
        assert_eq!(sent.len(), 4);
    }

    #[test]
    fn release_triggers_rest_with_retry_and_skips_never_active_and_avatar_change() {
        let t0 = Instant::now();
        let mut s = OutputScheduler::new();
        s.set_vrchat_address(Some(vrc(9000)), t0);
        s.set_schema(schema_with(&[
            ("/avatar/parameters/A", Some(ParamType::Float), Some(3)),
            ("/avatar/parameters/B", Some(ParamType::Float), Some(3)),
        ]));

        let mut never = osc_frame(99, "/avatar/parameters/B", 0.9);
        never.enable = false;
        let mut sent = Vec::new();
        s.submit(OutputSource::Local, vec![never], vec![], t0, |m| {
            sent.push(m);
            true
        });
        s.submit(OutputSource::Local, vec![], vec![], t0 + Duration::from_secs(1), |m| {
            sent.push(m);
            true
        });
        assert!(sent.is_empty(), "never-active node must send no rest");

        let mut fa = osc_frame(1, "/avatar/parameters/A", 0.7);
        fa.rest = -0.25;
        s.submit(OutputSource::Local, vec![fa.clone()], vec![], t0, |m| {
            sent.push(m);
            true
        });
        let t1 = t0 + ADDRESS_DEBOUNCE;
        s.submit(OutputSource::Local, vec![fa.clone()], vec![], t1, |m| {
            sent.push(m);
            true
        });
        assert_eq!(sent.len(), 1);

        fa.enable = false;
        let t2 = t1 + Duration::from_millis(50);
        s.submit(OutputSource::Local, vec![fa.clone()], vec![], t2, |m| {
            sent.push(m);
            true
        });
        assert_eq!(sent.len(), 2);
        assert_eq!(
            sent[1],
            OutgoingOsc::Param {
                address: "/avatar/parameters/A".into(),
                value: OscValue::Float(-0.25)
            }
        );

        s.poll(t2 + Duration::from_millis(50), |m| {
            sent.push(m);
            true
        });
        assert_eq!(sent.len(), 2);

        s.poll(t2 + REST_RETRY_DELAY, |m| {
            sent.push(m);
            true
        });
        assert_eq!(sent.len(), 3);
        assert_eq!(sent[2], sent[1]);

        s.poll(t2 + Duration::from_secs(1), |m| {
            sent.push(m);
            true
        });
        assert_eq!(sent.len(), 3);

        fa.enable = true;
        let t3 = t2 + Duration::from_secs(2);
        s.submit(OutputSource::Local, vec![fa.clone()], vec![], t3, |m| {
            sent.push(m);
            true
        });
        assert_eq!(sent.len(), 4);
        fa.address = "/avatar/parameters/B".into();
        let t4 = t3 + Duration::from_millis(50);
        s.submit(OutputSource::Local, vec![fa.clone()], vec![], t4, |m| {
            sent.push(m);
            true
        });
        assert_eq!(sent.len(), 5);
        assert_eq!(
            sent[4],
            OutgoingOsc::Param {
                address: "/avatar/parameters/A".into(),
                value: OscValue::Float(-0.25)
            },
            "old address receives rest on rename"
        );

        let t5 = t4 + ADDRESS_DEBOUNCE;
        s.submit(OutputSource::Local, vec![fa], vec![], t5, |m| {
            sent.push(m);
            true
        });
        assert_eq!(sent.len(), 7);
        s.on_avatar_change();
        let count_before = sent.len();
        s.release(OutputSource::Local, t5 + Duration::from_millis(25), |m| {
            sent.push(m);
            true
        });
        assert_eq!(sent.len(), count_before, "no rest after avatar change");
    }

    #[test]
    fn local_remote_arbitration_and_direct_takeover_without_rest_blink() {
        let t0 = Instant::now();
        let mut s = OutputScheduler::new();
        s.set_vrchat_address(Some(vrc(9000)), t0);
        s.set_schema(schema_with(&[("/avatar/parameters/X", Some(ParamType::Float), Some(3))]));

        let local_f = osc_frame(1, "/avatar/parameters/X", 0.9);
        let remote_f = osc_frame(2, "/avatar/parameters/X", 0.3);

        let mut sent = Vec::new();
        s.submit(OutputSource::Local, vec![local_f.clone()], vec![], t0, |m| {
            sent.push(m);
            true
        });
        s.submit(OutputSource::Remote, vec![remote_f.clone()], vec![], t0, |m| {
            sent.push(m);
            true
        });

        let t1 = t0 + ADDRESS_DEBOUNCE;
        s.submit(OutputSource::Local, vec![local_f], vec![], t1, |m| {
            sent.push(m);
            true
        });
        assert_eq!(sent.len(), 1);
        assert_eq!(
            sent[0],
            OutgoingOsc::Param {
                address: "/avatar/parameters/X".into(),
                value: OscValue::Float(0.9)
            }
        );
        assert_eq!(s.statuses(OutputSource::Local)[&1].state, OutputNodeState::Live);
        assert_eq!(s.statuses(OutputSource::Remote)[&2].state, OutputNodeState::UsedByGraph);

        let t2 = t1 + Duration::from_millis(50);
        s.release(OutputSource::Local, t2, |m| {
            sent.push(m);
            true
        });
        assert_eq!(sent.len(), 2, "only Remote's 0.3, no rest blink");
        assert_eq!(
            sent[1],
            OutgoingOsc::Param {
                address: "/avatar/parameters/X".into(),
                value: OscValue::Float(0.3)
            }
        );
        assert_eq!(s.statuses(OutputSource::Remote)[&2].state, OutputNodeState::Live);
    }

    #[test]
    fn full_queue_not_recorded_as_sent_and_resend_after_vrc_address_change() {
        let t0 = Instant::now();
        let mut s = OutputScheduler::new();
        s.set_vrchat_address(Some(vrc(9000)), t0);
        s.set_schema(schema_with(&[("/avatar/parameters/X", Some(ParamType::Float), Some(3))]));

        let f = osc_frame(1, "/avatar/parameters/X", 0.5);
        s.submit(OutputSource::Local, vec![f.clone()], vec![], t0, |_| false);

        let t1 = t0 + ADDRESS_DEBOUNCE;
        let mut attempts = 0;
        s.submit(OutputSource::Local, vec![f.clone()], vec![], t1, |_| {
            attempts += 1;
            false
        });
        assert_eq!(attempts, 1);
        assert!(s.statuses(OutputSource::Local)[&1].last_sent.is_none());

        let mut sent = Vec::new();
        let t2 = t1 + Duration::from_millis(25);
        s.submit(OutputSource::Local, vec![f.clone()], vec![], t2, |m| {
            sent.push(m);
            true
        });
        assert_eq!(sent.len(), 1);
        assert_eq!(s.statuses(OutputSource::Local)[&1].last_sent, Some(SentValue::Float(0.5)));

        let t3 = t2 + Duration::from_millis(25);
        s.set_vrchat_address(Some(vrc(9002)), t3);
        s.submit(OutputSource::Local, vec![f], vec![], t3, |m| {
            sent.push(m);
            true
        });
        assert_eq!(sent.len(), 2, "resent after VRChat address change");
    }

    #[test]
    fn auto_with_missing_readonly_unknown_and_manual_missing_plus_stop_param() {
        let t0 = Instant::now();
        let mut s = OutputScheduler::new();
        s.set_vrchat_address(Some(vrc(9000)), t0);
        s.set_stop_param(Some("/avatar/parameters/RemoteStop".into()));
        s.set_schema(schema_with(&[
            ("/avatar/parameters/RO", Some(ParamType::Float), Some(1)),
            ("/avatar/parameters/Unk", None, Some(3)),
        ]));

        let mut manual_missing = osc_frame(4, "/avatar/parameters/MissingManual", 0.6);
        manual_missing.ty = OscOutType::Float;
        let frames = vec![
            osc_frame(1, "/avatar/parameters/MissingAuto", 0.5),
            osc_frame(2, "/avatar/parameters/RO", 0.5),
            osc_frame(3, "/avatar/parameters/Unk", 0.5),
            manual_missing,
            osc_frame(5, "/avatar/parameters/RemoteStop", 1.0),
        ];

        let mut sent = Vec::new();
        s.submit(OutputSource::Local, frames.clone(), vec![], t0, |m| {
            sent.push(m);
            true
        });
        s.submit(OutputSource::Local, frames, vec![], t0 + ADDRESS_DEBOUNCE, |m| {
            sent.push(m);
            true
        });

        let st = s.statuses(OutputSource::Local);
        assert_eq!(st[&1].state, OutputNodeState::NoParam);
        assert_eq!(st[&2].state, OutputNodeState::ReadOnly);
        assert_eq!(st[&3].state, OutputNodeState::UnknownType);
        assert_eq!(st[&4].state, OutputNodeState::Live);
        assert!(st[&4].warning.is_some(), "manual type missing param warns");
        assert_eq!(st[&5].state, OutputNodeState::Refused, "stop_param refused");
        assert_eq!(sent.len(), 1, "only manual-missing sent");
    }

    #[test]
    fn chatbox_limiter_preemption_hold_depth1_queue_and_clear_on_stop() {
        let t0 = Instant::now();
        let mut s = OutputScheduler::new();
        s.set_vrchat_address(Some(vrc(9000)), t0);

        let mut sent = Vec::new();
        s.submit(OutputSource::Local, vec![], vec![chat_live(1, "P {1:%}", 0.5)], t0, |m| {
            sent.push(m);
            true
        });
        assert_eq!(
            sent,
            vec![OutgoingOsc::Chatbox {
                text: "P 50%".into(),
                immediate: true,
                notify: false,
            }]
        );

        let t1 = t0 + Duration::from_millis(300);
        s.submit(
            OutputSource::Local,
            vec![],
            vec![chat_live(1, "P {1:%}", 0.6), chat_trigger(2, "T1", 0.0, true, true)],
            t1,
            |m| {
                sent.push(m);
                true
            },
        );
        let t2 = t0 + Duration::from_millis(600);
        s.submit(
            OutputSource::Local,
            vec![],
            vec![chat_live(1, "P {1:%}", 0.7), chat_trigger(2, "T2", 0.0, true, true)],
            t2,
            |m| {
                sent.push(m);
                true
            },
        );
        assert_eq!(sent.len(), 1, "global 1500 ms limiter holds triggers");

        let t3 = t0 + CHAT_GLOBAL_MIN_INTERVAL;
        s.submit(
            OutputSource::Local,
            vec![],
            vec![chat_live(1, "P {1:%}", 0.8), chat_trigger(2, "T2", 0.0, false, true)],
            t3,
            |m| {
                sent.push(m);
                true
            },
        );
        assert_eq!(sent.len(), 2);
        assert_eq!(
            sent[1],
            OutgoingOsc::Chatbox {
                text: "T2".into(),
                immediate: true,
                notify: true,
            }
        );

        let t4 = t3 + Duration::from_millis(2500);
        s.submit(
            OutputSource::Local,
            vec![],
            vec![chat_live(1, "P {1:%}", 0.8), chat_trigger(2, "T2", 0.0, false, true)],
            t4,
            |m| {
                sent.push(m);
                true
            },
        );
        assert_eq!(sent.len(), 2, "trigger holds for 3 s");

        let t5 = t3 + CHAT_TRIGGER_HOLD;
        s.submit(
            OutputSource::Local,
            vec![],
            vec![chat_live(1, "P {1:%}", 0.8), chat_trigger(2, "T2", 0.0, false, true)],
            t5,
            |m| {
                sent.push(m);
                true
            },
        );
        assert_eq!(sent.len(), 3);
        assert_eq!(
            sent[2],
            OutgoingOsc::Chatbox {
                text: "P 80%".into(),
                immediate: true,
                notify: false,
            }
        );

        let t6 = t5 + CHAT_LIVE_RESEND_INTERVAL;
        s.submit(OutputSource::Local, vec![], vec![chat_live(1, "P {1:%}", 0.8)], t6, |m| {
            sent.push(m);
            true
        });
        assert_eq!(sent.len(), 4);

        let t7 = t6 + Duration::from_millis(100);
        let mut off = chat_live(1, "P {1:%}", 0.8);
        off.enable = false;
        s.submit(OutputSource::Local, vec![], vec![off], t7, |m| {
            sent.push(m);
            true
        });
        assert_eq!(sent.len(), 4, "normal clear respects 1500ms limiter");

        s.release_for_shutdown(t7 + Duration::from_millis(50), |m| {
            sent.push(m);
            true
        });
        assert_eq!(sent.len(), 5);
        assert_eq!(
            sent[4],
            OutgoingOsc::Chatbox {
                text: String::new(),
                immediate: true,
                notify: false,
            }
        );
    }

    #[test]
    fn chatbox_trigger_is_consumed_once_and_never_queued_offline() {
        let t0 = Instant::now();
        let mut s = OutputScheduler::new();
        let mut sent = Vec::new();

        s.submit(
            OutputSource::Local,
            vec![],
            vec![chat_trigger(1, "Offline", 0.0, true, false)],
            t0,
            |m| {
                sent.push(m);
                true
            },
        );
        assert!(sent.is_empty());

        s.set_vrchat_address(Some(vrc(9000)), t0 + Duration::from_millis(10));
        s.poll(t0 + Duration::from_millis(20), |m| {
            sent.push(m);
            true
        });
        assert!(sent.is_empty(), "offline trigger must not fire after coming online");

        let t1 = t0 + Duration::from_millis(50);
        s.submit(
            OutputSource::Local,
            vec![],
            vec![chat_trigger(1, "Once", 0.0, true, true)],
            t1,
            |m| {
                sent.push(m);
                true
            },
        );
        assert_eq!(sent.len(), 1);

        s.submit(OutputSource::Remote, vec![], vec![], t1 + Duration::from_millis(10), |m| {
            sent.push(m);
            true
        });
        s.poll(t1 + CHAT_GLOBAL_MIN_INTERVAL + Duration::from_millis(50), |m| {
            sent.push(m);
            true
        });
        assert_eq!(sent.len(), 1, "trigger must not re-fire on remote submit or poll");
    }
}
