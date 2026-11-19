use std::io;
use std::sync::Mutex;

use tokio::sync::mpsc;

use super::{CHANNEL_DEPTH, Channel, ChannelInfo, PathKind, RemoteTransport};

pub fn pair(path: PathKind) -> (Channel, Channel) {
    let (a_tx, b_rx) = mpsc::channel(CHANNEL_DEPTH);
    let (b_tx, a_rx) = mpsc::channel(CHANNEL_DEPTH);
    let info = |peer: &str| ChannelInfo {
        path,
        binding: b"memory".to_vec(),
        peer: peer.into(),
        label: "memory".into(),
    };
    (
        Channel {
            tx: a_tx,
            rx: a_rx,
            info: info("b"),
            guard: Box::new(()),
        },
        Channel {
            tx: b_tx,
            rx: b_rx,
            info: info("a"),
            guard: Box::new(()),
        },
    )
}

pub struct MemoryTransport {
    pub path: PathKind,
    pub slot: Mutex<Option<Channel>>,
    pub delay: std::time::Duration,
}

#[async_trait::async_trait]
impl RemoteTransport for MemoryTransport {
    fn name(&self) -> String {
        format!("memory ({})", self.path)
    }

    fn path(&self) -> PathKind {
        self.path
    }

    async fn dial(&self) -> io::Result<Channel> {
        tokio::time::sleep(self.delay).await;
        self.slot
            .lock()
            .ok()
            .and_then(|mut s| s.take())
            .ok_or_else(|| io::Error::new(io::ErrorKind::ConnectionRefused, "no peer"))
    }
}
