pub mod dialers;
pub mod direct;
pub mod lan;
pub mod memory;
pub mod race;
pub mod rtc;
pub mod signal;
pub mod turn_shim;

use std::fmt;
use std::io;

use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

pub const CHANNEL_DEPTH: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum PathKind {
    #[default]
    Lan,
    P2p,
    Relay,
}

impl PathKind {
    pub fn label(self) -> &'static str {
        match self {
            PathKind::Lan => "LAN",
            PathKind::P2p => "P2P",
            PathKind::Relay => "Relay",
        }
    }
}

impl fmt::Display for PathKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

#[derive(Debug, Clone, Default)]
pub struct ChannelInfo {
    pub path: PathKind,
    pub binding: Vec<u8>,
    pub peer: String,
    pub label: String,
}

pub trait Guard: Send + Sync {}

impl<T: Send + Sync> Guard for T {}

pub struct Channel {
    pub tx: mpsc::Sender<Vec<u8>>,
    pub rx: mpsc::Receiver<Vec<u8>>,
    pub info: ChannelInfo,
    pub guard: Box<dyn Guard>,
}

impl fmt::Debug for Channel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Channel").field("info", &self.info).finish()
    }
}

impl Channel {
    pub async fn send(&self, msg: &crate::protocol::Message) -> io::Result<()> {
        self.tx
            .send(crate::protocol::encode(msg))
            .await
            .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "channel closed"))
    }

    pub fn try_send(&self, msg: &crate::protocol::Message) -> bool {
        self.tx.try_send(crate::protocol::encode(msg)).is_ok()
    }

    pub async fn recv(&mut self) -> Option<Result<crate::protocol::Message, crate::protocol::ProtocolError>> {
        let bytes = self.rx.recv().await?;
        Some(crate::protocol::decode(&bytes))
    }
}

#[async_trait::async_trait]
pub trait RemoteTransport: Send + Sync {
    fn name(&self) -> String;
    fn path(&self) -> PathKind;
    async fn dial(&self) -> io::Result<Channel>;
}
