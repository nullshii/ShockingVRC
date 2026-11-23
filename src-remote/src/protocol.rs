use serde::{Deserialize, Serialize};
use shocking_vrc_core::graph::device::{ChannelFrame, MAX_CHANNELS, SEGMENTS};
use shocking_vrc_core::graph::safety::RemoteChannelPolicy;

pub const PROTOCOL_VERSION: u8 = 2;
pub const MAGIC: [u8; 2] = *b"SV";
pub const HEADER_LEN: usize = 8;
pub const MAX_BODY: usize = 64 * 1024;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ProtocolError {
    #[error("message is too short")]
    Short,
    #[error("not a ShockingVRC message")]
    Magic,
    #[error("unsupported protocol version {0}")]
    Version(u8),
    #[error("message of {0} bytes is too large")]
    TooLarge(usize),
    #[error("message length does not match its header")]
    Length,
    #[error("malformed message: {0}")]
    Body(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ClientKind {
    #[default]
    App,
    Web,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Hello {
    pub name: String,
    #[serde(default)]
    pub client: ClientKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identity: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub invite: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resume: Option<String>,
    pub nonce: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<crate::transport::PathKind>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Challenge {
    pub identity: String,
    pub name: String,
    pub nonce: String,
    pub sig: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mac: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Proof {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mac: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sig: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Welcome {
    pub session: String,
    pub ttl_ms: u64,
    pub name: String,
    pub you_control: bool,
    pub you_receive: bool,
    pub trusted: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy: Option<PolicySnapshot>,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub ends_at_ms: u64,
}

fn is_zero(v: &u64) -> bool {
    *v == 0
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
pub struct WireChannel {
    #[serde(deserialize_with = "lenient_segments")]
    pub p: [f32; SEGMENTS],
    #[serde(deserialize_with = "lenient_segments")]
    pub f: [f32; SEGMENTS],
    #[serde(deserialize_with = "lenient_segments")]
    pub i: [f32; SEGMENTS],
    #[serde(default = "default_max", deserialize_with = "lenient_max")]
    pub m: f32,
}

fn default_max() -> f32 {
    200.0
}

fn lenient_segments<'de, D: serde::Deserializer<'de>>(d: D) -> Result<[f32; SEGMENTS], D::Error> {
    let raw: [Option<f32>; SEGMENTS] = Deserialize::deserialize(d)?;
    Ok(raw.map(|v| v.unwrap_or(f32::NAN)))
}

fn lenient_max<'de, D: serde::Deserializer<'de>>(d: D) -> Result<f32, D::Error> {
    Ok(Option::<f32>::deserialize(d)?.unwrap_or_else(default_max))
}

impl WireChannel {
    pub fn from_frame(c: &ChannelFrame) -> Self {
        let r = |v: f32| (v * 1000.0).round() / 1000.0;
        Self {
            p: c.power_seg.map(r),
            f: c.freq.map(|v| v.round()),
            i: c.intensity.map(|v| v.round()),
            m: c.limit_max.round(),
        }
    }

    pub fn to_frame(self) -> ChannelFrame {
        let fin = |v: f32| if v.is_finite() { v } else { 0.0 };
        let p = self.p.map(|v| fin(v).clamp(0.0, 1.0));
        ChannelFrame {
            power: p.iter().copied().fold(0.0, f32::max),
            power_seg: p,
            freq: self.f.map(fin),
            intensity: self.i.map(fin),
            limit_min: 0.0,
            limit_max: fin(self.m).clamp(0.0, 200.0),
            ceiling: shocking_vrc_core::graph::device::CEILING_OFF,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct SlotFrame {
    pub slot: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub a: Option<WireChannel>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub b: Option<WireChannel>,
}

impl SlotFrame {
    pub fn channels(&self) -> [Option<ChannelFrame>; MAX_CHANNELS] {
        [self.a.map(WireChannel::to_frame), self.b.map(WireChannel::to_frame)]
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct RemoteFrame {
    pub seq: u64,
    pub ts: u64,
    pub slots: Vec<SlotFrame>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct PolicySnapshot {
    pub slots: Vec<SlotInfo>,
    pub channels: [RemoteChannelPolicy; MAX_CHANNELS],
}

impl PolicySnapshot {
    pub fn slot_channels(&self, slot: u8) -> [RemoteChannelPolicy; MAX_CHANNELS] {
        self.slots
            .iter()
            .find(|s| s.slot == slot)
            .and_then(|s| s.channels)
            .unwrap_or(self.channels)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct SlotInfo {
    pub slot: u8,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channels: Option<[RemoteChannelPolicy; MAX_CHANNELS]>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct OutputReport {
    pub slots: Vec<(u8, [u8; MAX_CHANNELS])>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FeedbackKind {
    More,
    Less,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum Message {
    Hello(Hello),
    Challenge(Challenge),
    Proof(Proof),
    Welcome(Welcome),
    Pending {
        reason: String,
    },
    Reject {
        reason: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        retry_after_ms: Option<u64>,
    },
    Frame(RemoteFrame),
    Policy(PolicySnapshot),
    Output(OutputReport),
    Feedback {
        kind: FeedbackKind,
    },
    Ping {
        seq: u64,
        ts: u64,
    },
    Pong {
        seq: u64,
        ts: u64,
    },
    Stop {
        reason: String,
    },
    Bye,
}

impl Message {
    pub fn kind(&self) -> &'static str {
        match self {
            Message::Hello(_) => "hello",
            Message::Challenge(_) => "challenge",
            Message::Proof(_) => "proof",
            Message::Welcome(_) => "welcome",
            Message::Pending { .. } => "pending",
            Message::Reject { .. } => "reject",
            Message::Frame(_) => "frame",
            Message::Policy(_) => "policy",
            Message::Output(_) => "output",
            Message::Feedback { .. } => "feedback",
            Message::Ping { .. } => "ping",
            Message::Pong { .. } => "pong",
            Message::Stop { .. } => "stop",
            Message::Bye => "bye",
        }
    }
}

pub fn encode(msg: &Message) -> Vec<u8> {
    let body = serde_json::to_vec(msg).unwrap_or_default();
    let mut out = Vec::with_capacity(HEADER_LEN + body.len());
    out.extend_from_slice(&MAGIC);
    out.push(PROTOCOL_VERSION);
    out.push(0);
    out.extend_from_slice(&(body.len() as u32).to_be_bytes());
    out.extend_from_slice(&body);
    out
}

fn header(bytes: &[u8]) -> Result<usize, ProtocolError> {
    if bytes.len() < HEADER_LEN {
        return Err(ProtocolError::Short);
    }
    if bytes[..2] != MAGIC {
        return Err(ProtocolError::Magic);
    }
    if bytes[2] != PROTOCOL_VERSION {
        return Err(ProtocolError::Version(bytes[2]));
    }
    let len = u32::from_be_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]) as usize;
    if len > MAX_BODY {
        return Err(ProtocolError::TooLarge(len));
    }
    Ok(len)
}

pub fn decode(bytes: &[u8]) -> Result<Message, ProtocolError> {
    let len = header(bytes)?;
    if bytes.len() != HEADER_LEN + len {
        return Err(ProtocolError::Length);
    }
    serde_json::from_slice(&bytes[HEADER_LEN..]).map_err(|e| ProtocolError::Body(e.to_string()))
}

pub fn split(buf: &mut Vec<u8>) -> Result<Option<Message>, ProtocolError> {
    if buf.len() < HEADER_LEN {
        return Ok(None);
    }
    let len = header(buf)?;
    if buf.len() < HEADER_LEN + len {
        return Ok(None);
    }
    let msg = decode(&buf[..HEADER_LEN + len]);
    buf.drain(..HEADER_LEN + len);
    msg.map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_round_trip_with_a_versioned_header() {
        let msg = Message::Frame(RemoteFrame {
            seq: 7,
            ts: 1234,
            slots: vec![SlotFrame {
                slot: 0,
                a: Some(WireChannel {
                    p: [0.5; 4],
                    f: [100.0; 4],
                    i: [80.0; 4],
                    m: 60.0,
                }),
                b: None,
            }],
        });
        let bytes = encode(&msg);
        assert_eq!(&bytes[..3], b"SV\x02");
        assert_eq!(decode(&bytes), Ok(msg));
    }

    #[test]
    fn bad_headers_are_rejected() {
        let mut bytes = encode(&Message::Bye);
        assert_eq!(decode(&bytes[..4]), Err(ProtocolError::Short));
        bytes[2] = 9;
        assert_eq!(decode(&bytes), Err(ProtocolError::Version(9)));
        bytes[2] = PROTOCOL_VERSION;
        bytes[0] = b'X';
        assert_eq!(decode(&bytes), Err(ProtocolError::Magic));
        let mut big = encode(&Message::Bye);
        big[4..8].copy_from_slice(&(MAX_BODY as u32 + 1).to_be_bytes());
        assert!(matches!(decode(&big), Err(ProtocolError::TooLarge(_))));
    }

    #[test]
    fn a_stream_splits_into_messages() {
        let mut buf = encode(&Message::Ping { seq: 1, ts: 2 });
        buf.extend(encode(&Message::Bye));
        let tail = buf.split_off(buf.len() - 3);
        assert_eq!(split(&mut buf).unwrap(), Some(Message::Ping { seq: 1, ts: 2 }));
        assert_eq!(split(&mut buf).unwrap(), None, "the second one is incomplete");
        buf.extend(tail);
        assert_eq!(split(&mut buf).unwrap(), Some(Message::Bye));
        assert!(buf.is_empty());
    }

    #[test]
    fn wire_channels_are_sanitised_into_frames() {
        let w = WireChannel {
            p: [f32::NAN, 2.0, 0.25, -1.0],
            f: [f32::INFINITY, 50.0, 60.0, 70.0],
            i: [10.0; 4],
            m: 900.0,
        };
        let f = w.to_frame();
        assert_eq!(f.power_seg, [0.0, 1.0, 0.25, 0.0]);
        assert_eq!(f.power, 1.0);
        assert_eq!(f.freq[0], 0.0);
        assert_eq!(f.limit_max, 200.0);
    }

    #[test]
    fn feedback_round_trips() {
        let msg = Message::Feedback { kind: FeedbackKind::More };
        assert_eq!(decode(&encode(&msg)), Ok(msg));
        assert!(String::from_utf8_lossy(&encode(&Message::Feedback { kind: FeedbackKind::Less })).contains(r#""t":"feedback","kind":"less""#));
    }

    fn raw(body: &str) -> Vec<u8> {
        let mut out = b"SV\x02\x00".to_vec();
        out.extend_from_slice(&(body.len() as u32).to_be_bytes());
        out.extend_from_slice(body.as_bytes());
        out
    }

    #[test]
    fn a_null_from_the_browser_does_not_drop_the_whole_frame() {
        let body = r#"{"t":"frame","seq":3,"ts":10,"slots":[{"slot":0,
            "a":{"p":[0.5,null,0.5,0.5],"f":[18,18,18,18],"i":[100,100,100,100],"m":null},
            "b":{"p":[0,0,0,0],"f":[18,18,18,18],"i":[0,0,0,0]}}]}"#;
        let Ok(Message::Frame(f)) = decode(&raw(body)) else { panic!("frame did not decode") };
        let [a, b] = f.slots[0].channels();
        let a = a.unwrap();
        assert_eq!(a.power_seg, [0.5, 0.0, 0.5, 0.5]);
        assert_eq!(a.limit_max, 200.0, "a null max means no extra limit");
        assert_eq!(b.unwrap().limit_max, 200.0, "a missing max too");
    }

    #[test]
    fn policy_snapshot_resolves_per_slot_and_legacy_channels() {
        let mut dev1 = [RemoteChannelPolicy::default(); MAX_CHANNELS];
        dev1[0].cap = 45;
        let mut dev2 = [RemoteChannelPolicy::default(); MAX_CHANNELS];
        dev2[0].cap = 80;
        dev2[1].allowed = false;

        let snap = PolicySnapshot {
            slots: vec![
                SlotInfo {
                    slot: 0,
                    name: "One".into(),
                    channels: Some(dev1),
                },
                SlotInfo {
                    slot: 1,
                    name: "Two".into(),
                    channels: Some(dev2),
                },
            ],
            channels: dev1,
        };
        let bytes = encode(&Message::Policy(snap.clone()));
        assert_eq!(decode(&bytes), Ok(Message::Policy(snap.clone())));
        assert_eq!(snap.slot_channels(0)[0].cap, 45);
        assert_eq!(snap.slot_channels(1)[0].cap, 80);
        assert!(!snap.slot_channels(1)[1].allowed);

        let legacy_body = r#"{"t":"policy","slots":[{"slot":0,"name":"One"}],"channels":[{"allowed":true,"cap":55,"freq_bounds":[10,240],"int_bounds":[0,100],"ramp_per_s":40.0,"merge":"Max"},{"allowed":false,"cap":10,"freq_bounds":[10,240],"int_bounds":[0,100],"ramp_per_s":40.0,"merge":"Max"}]}"#;
        let Ok(Message::Policy(legacy)) = decode(&raw(legacy_body)) else {
            panic!("legacy policy did not decode")
        };
        assert_eq!(legacy.slot_channels(0)[0].cap, 55);
        assert!(!legacy.slot_channels(0)[1].allowed);
    }
}
