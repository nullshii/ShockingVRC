use std::future::Future;
use std::time::Duration;

use futures_util::StreamExt;
use futures_util::stream::FuturesUnordered;

use super::{Channel, PathKind, RemoteTransport};

pub struct Attempt {
    pub transport: Box<dyn RemoteTransport>,
    pub delay: Duration,
    pub timeout: Duration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Strategy {
    Race,
    Sequential,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    pub message: String,
    pub fatal: bool,
}

impl From<String> for Failure {
    fn from(message: String) -> Self {
        Self { message, fatal: false }
    }
}

impl Failure {
    pub fn fatal(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            fatal: true,
        }
    }
}

#[derive(Debug)]
pub struct Won<T> {
    pub value: T,
    pub path: PathKind,
    pub via: String,
}

async fn run_one<T, F, Fut>(a: &Attempt, handshake: &F) -> Result<Won<T>, Failure>
where
    F: Fn(Channel) -> Fut,
    Fut: Future<Output = Result<T, Failure>>,
{
    let name = a.transport.name();
    let work = async {
        let ch = a
            .transport
            .dial()
            .await
            .map_err(|e| Failure::from(format!("{name}: {e}")))?;
        let path = ch.info.path;
        let value = handshake(ch).await.map_err(|f| Failure {
            message: format!("{name}: {}", f.message),
            fatal: f.fatal,
        })?;
        Ok(Won {
            value,
            path,
            via: name.clone(),
        })
    };
    match tokio::time::timeout(a.timeout, work).await {
        Ok(r) => r,
        Err(_) => Err(Failure::from(format!("{name}: timed out"))),
    }
}

pub async fn establish<T, F, Fut>(
    attempts: Vec<Attempt>,
    strategy: Strategy,
    handshake: F,
) -> Result<Won<T>, Vec<String>>
where
    F: Fn(Channel) -> Fut,
    Fut: Future<Output = Result<T, Failure>>,
{
    let mut errors = Vec::new();
    if attempts.is_empty() {
        errors.push("no way to reach the partner is configured".to_string());
        return Err(errors);
    }
    match strategy {
        Strategy::Sequential => {
            for a in &attempts {
                match run_one(a, &handshake).await {
                    Ok(w) => return Ok(w),
                    Err(e) => {
                        errors.push(e.message);
                        if e.fatal {
                            break;
                        }
                    }
                }
            }
            Err(errors)
        }
        Strategy::Race => {
            let mut pending: FuturesUnordered<_> = attempts
                .iter()
                .map(|a| {
                    let h = &handshake;
                    async move {
                        tokio::time::sleep(a.delay).await;
                        run_one(a, h).await
                    }
                })
                .collect();
            while let Some(r) = pending.next().await {
                match r {
                    Ok(w) => return Ok(w),
                    Err(e) => {
                        errors.push(e.message);
                        if e.fatal {
                            break;
                        }
                    }
                }
            }
            Err(errors)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;
    use crate::transport::memory::{MemoryTransport, pair};

    fn attempt(path: PathKind, delay_ms: u64, live: bool) -> (Attempt, Option<Channel>) {
        let (a, b) = pair(path);
        let t = MemoryTransport {
            path,
            slot: Mutex::new(live.then_some(a)),
            delay: Duration::from_millis(delay_ms),
        };
        (
            Attempt {
                transport: Box::new(t),
                delay: Duration::ZERO,
                timeout: Duration::from_secs(2),
            },
            Some(b),
        )
    }

    #[tokio::test]
    async fn the_fastest_authenticated_path_wins() {
        let (slow, _k1) = attempt(PathKind::Relay, 80, true);
        let (fast, _k2) = attempt(PathKind::Lan, 5, true);
        let (dead, _k3) = attempt(PathKind::P2p, 0, false);
        let won = establish(vec![slow, fast, dead], Strategy::Race, |ch| async move {
            Ok::<_, Failure>(ch.info.path)
        })
        .await
        .unwrap();
        assert_eq!(won.value, PathKind::Lan);
    }

    #[tokio::test]
    async fn a_failed_handshake_falls_through_to_the_next_path() {
        let (lan, _k1) = attempt(PathKind::Lan, 0, true);
        let (relay, _k2) = attempt(PathKind::Relay, 0, true);
        let won = establish(vec![lan, relay], Strategy::Sequential, |ch| async move {
            if ch.info.path == PathKind::Lan {
                Err(Failure::from("bad proof".to_string()))
            } else {
                Ok(ch.info.path)
            }
        })
        .await
        .unwrap();
        assert_eq!(won.path, PathKind::Relay);
    }

    #[tokio::test]
    async fn every_error_is_reported_when_nothing_works() {
        let (a, _k) = attempt(PathKind::Lan, 0, false);
        let errs = establish(vec![a], Strategy::Race, |_| async { Ok::<_, Failure>(()) })
            .await
            .unwrap_err();
        assert_eq!(errs.len(), 1);
    }

    #[tokio::test]
    async fn a_refusal_ends_the_race() {
        let (first, _k1) = attempt(PathKind::Lan, 0, true);
        let (second, _k2) = attempt(PathKind::Relay, 0, true);
        let errs = establish(vec![first, second], Strategy::Sequential, |_| async {
            Err::<(), _>(Failure::fatal("declined"))
        })
        .await
        .unwrap_err();
        assert_eq!(errs.len(), 1, "the second route is not tried");
    }
}
