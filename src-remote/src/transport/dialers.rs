use std::io;

use super::direct::WsSignaler;
use super::rtc::{self, IceConfig};
use super::signal::{self, VdsAddr};
use super::{Channel, PathKind, RemoteTransport};

pub struct VdsRtcTransport {
    pub vds: VdsAddr,
    pub pin: Option<String>,
    pub room: String,
    pub relay: bool,
}

#[async_trait::async_trait]
impl RemoteTransport for VdsRtcTransport {
    fn name(&self) -> String {
        format!("{} via {}", if self.relay { "relay" } else { "p2p" }, self.vds.host)
    }

    fn path(&self) -> PathKind {
        if self.relay { PathKind::Relay } else { PathKind::P2p }
    }

    async fn dial(&self) -> io::Result<Channel> {
        let (signaler, ice) = signal::join(&self.vds, self.pin.as_deref(), &self.room).await?;
        let cfg = if self.relay {
            if !ice.has_turn() {
                return Err(io::Error::other("the rendezvous server offers no TURN relay"));
            }
            IceConfig {
                relay_only: true,
                ..ice
            }
        } else {
            ice.without_turn()
        };
        rtc::connect(signaler, cfg, true, self.path()).await
    }
}

pub struct LanIceTransport {
    pub addr: String,
    pub pin: String,
}

#[async_trait::async_trait]
impl RemoteTransport for LanIceTransport {
    fn name(&self) -> String {
        format!("lan ice {}", self.addr)
    }

    fn path(&self) -> PathKind {
        PathKind::Lan
    }

    async fn dial(&self) -> io::Result<Channel> {
        let signaler = WsSignaler::connect(&self.addr, &self.pin).await?;
        rtc::connect(Box::new(signaler), IceConfig::host_only(), true, PathKind::Lan).await
    }
}
