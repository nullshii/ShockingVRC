use std::time::Duration;

use crate::crypto::{self, Identity, TranscriptInput, b64, unb64};
use crate::protocol::{Challenge, ClientKind, Hello, Message, Proof};
use crate::transport::Channel;

pub const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum HandshakeError {
    #[error("the connection closed during the handshake")]
    Closed,
    #[error("the partner took too long to answer")]
    Timeout,
    #[error("unexpected {0} message")]
    Unexpected(&'static str),
    #[error("malformed handshake: {0}")]
    Malformed(String),
    #[error("the partner is not who the link says")]
    WrongIdentity,
    #[error("the partner's proof did not check out")]
    BadProof,
    #[error("refused: {0}")]
    Denied(String),
    #[error("the partner refused: {0}")]
    Rejected(String, Option<u64>),
}

impl HandshakeError {
    pub fn is_auth_failure(&self) -> bool {
        matches!(self, HandshakeError::BadProof | HandshakeError::Denied(_))
    }
}

async fn next(ch: &mut Channel, timeout: Duration) -> Result<Message, HandshakeError> {
    match tokio::time::timeout(timeout, ch.recv()).await {
        Err(_) => Err(HandshakeError::Timeout),
        Ok(None) => Err(HandshakeError::Closed),
        Ok(Some(Err(e))) => Err(HandshakeError::Malformed(e.to_string())),
        Ok(Some(Ok(m))) => Ok(m),
    }
}

async fn send(ch: &Channel, msg: &Message) -> Result<(), HandshakeError> {
    ch.send(msg).await.map_err(|_| HandshakeError::Closed)
}

fn grant_label(h: &Hello) -> String {
    match (&h.resume, &h.invite) {
        (Some(r), _) => format!("resume:{r}"),
        (None, Some(i)) => format!("invite:{i}"),
        (None, None) => "trusted".into(),
    }
}

#[derive(Debug, Clone)]
pub enum JoinGrant {
    Secret { id: String, secret: Vec<u8> },
    Resume { id: String, secret: Vec<u8> },
    Trusted,
}

#[derive(Debug, Clone)]
pub struct JoinerCtx {
    pub name: String,
    pub identity: Option<Identity>,
    pub client: ClientKind,
    pub expect: [u8; 32],
    pub grant: JoinGrant,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Joined {
    pub listener_name: String,
}

pub async fn join(ch: &mut Channel, ctx: &JoinerCtx) -> Result<Joined, HandshakeError> {
    let nonce: [u8; 32] = crypto::random_bytes();
    let (invite, resume, key) = match &ctx.grant {
        JoinGrant::Secret { id, secret } => (Some(id.clone()), None, Some(crypto::secret_key(secret))),
        JoinGrant::Resume { id, secret } => (None, Some(id.clone()), Some(crypto::secret_key(secret))),
        JoinGrant::Trusted => (None, None, None),
    };
    let my_pk = ctx.identity.as_ref().map(|i| i.public());
    let hello = Hello {
        name: ctx.name.clone(),
        client: ctx.client,
        identity: my_pk.map(|p| b64(&p)),
        invite,
        resume,
        nonce: b64(&nonce),
        path: Some(ch.info.path),
    };
    send(ch, &Message::Hello(hello.clone())).await?;
    let ch_msg = match next(ch, HANDSHAKE_TIMEOUT).await? {
        Message::Challenge(c) => c,
        Message::Reject { reason, retry_after_ms } => return Err(HandshakeError::Rejected(reason, retry_after_ms)),
        other => return Err(HandshakeError::Unexpected(other.kind())),
    };
    let listener_pk = crypto::unb64_array::<32>(&ch_msg.identity)
        .ok_or_else(|| HandshakeError::Malformed("listener identity".into()))?;
    if !crypto::ct_eq(&listener_pk, &ctx.expect) {
        return Err(HandshakeError::WrongIdentity);
    }
    let their_nonce = unb64(&ch_msg.nonce).ok_or_else(|| HandshakeError::Malformed("nonce".into()))?;
    let grant = grant_label(&hello);
    let t = crypto::transcript(&TranscriptInput {
        binding: &ch.info.binding,
        joiner_nonce: &nonce,
        listener_nonce: &their_nonce,
        listener_key: &listener_pk,
        joiner_key: my_pk.as_ref().map(|p| &p[..]).unwrap_or(&[]),
        grant: grant.as_bytes(),
    });
    let sig = unb64(&ch_msg.sig).unwrap_or_default();
    if !crypto::verify(&listener_pk, &[b"L", &t], &sig) {
        return Err(HandshakeError::BadProof);
    }
    if let Some(k) = &key {
        let mac = ch_msg.mac.as_deref().and_then(unb64).unwrap_or_default();
        if !crypto::hmac_verify(k, &[b"L", &t], &mac) {
            return Err(HandshakeError::BadProof);
        }
    }
    let proof = Proof {
        mac: key.map(|k| b64(&crypto::hmac(&k, &[b"D", &t]))),
        sig: ctx.identity.as_ref().map(|i| b64(&i.sign(&[b"D", &t]))),
    };
    send(ch, &Message::Proof(proof)).await?;
    Ok(Joined {
        listener_name: ch_msg.name,
    })
}

pub async fn await_verdict(
    ch: &mut Channel,
    consent_timeout: Duration,
) -> Result<crate::protocol::Welcome, HandshakeError> {
    let deadline = tokio::time::Instant::now() + consent_timeout;
    loop {
        let left = deadline.saturating_duration_since(tokio::time::Instant::now());
        match next(ch, left.max(Duration::from_millis(1))).await? {
            Message::Welcome(w) => return Ok(w),
            Message::Pending { .. } => continue,
            Message::Reject { reason, retry_after_ms } => return Err(HandshakeError::Rejected(reason, retry_after_ms)),
            Message::Ping { .. } | Message::Pong { .. } => continue,
            other => return Err(HandshakeError::Unexpected(other.kind())),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrantFound {
    pub key: Option<[u8; 32]>,
    pub needs_identity: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Accepted {
    pub hello: Hello,
    pub joiner_key: Option<[u8; 32]>,
}

pub async fn accept(
    ch: &mut Channel,
    identity: &Identity,
    name: &str,
    lookup: impl FnOnce(&Hello, Option<&[u8; 32]>) -> Result<GrantFound, String>,
) -> Result<Accepted, HandshakeError> {
    let hello = match next(ch, HANDSHAKE_TIMEOUT).await? {
        Message::Hello(h) => h,
        other => return Err(HandshakeError::Unexpected(other.kind())),
    };
    let joiner_key = match &hello.identity {
        Some(s) => Some(crypto::unb64_array::<32>(s).ok_or_else(|| HandshakeError::Malformed("identity".into()))?),
        None => None,
    };
    let their_nonce = unb64(&hello.nonce)
        .filter(|n| n.len() >= 16)
        .ok_or_else(|| HandshakeError::Malformed("nonce".into()))?;
    let found = match lookup(&hello, joiner_key.as_ref()) {
        Ok(f) => f,
        Err(reason) => {
            let _ = send(
                ch,
                &Message::Reject {
                    reason: reason.clone(),
                    retry_after_ms: None,
                },
            )
            .await;
            return Err(HandshakeError::Denied(reason));
        }
    };
    if found.needs_identity && joiner_key.is_none() {
        let _ = send(
            ch,
            &Message::Reject {
                reason: "identity required".into(),
                retry_after_ms: None,
            },
        )
        .await;
        return Err(HandshakeError::Denied("identity required".into()));
    }
    let nonce: [u8; 32] = crypto::random_bytes();
    let my_pk = identity.public();
    let grant = grant_label(&hello);
    let t = crypto::transcript(&TranscriptInput {
        binding: &ch.info.binding,
        joiner_nonce: &their_nonce,
        listener_nonce: &nonce,
        listener_key: &my_pk,
        joiner_key: joiner_key.as_ref().map(|p| &p[..]).unwrap_or(&[]),
        grant: grant.as_bytes(),
    });
    let challenge = Challenge {
        identity: b64(&my_pk),
        name: name.to_string(),
        nonce: b64(&nonce),
        sig: b64(&identity.sign(&[b"L", &t])),
        mac: found.key.map(|k| b64(&crypto::hmac(&k, &[b"L", &t]))),
    };
    send(ch, &Message::Challenge(challenge)).await?;
    let proof = match next(ch, HANDSHAKE_TIMEOUT).await? {
        Message::Proof(p) => p,
        other => return Err(HandshakeError::Unexpected(other.kind())),
    };
    if let Some(k) = &found.key {
        let mac = proof.mac.as_deref().and_then(unb64).unwrap_or_default();
        if !crypto::hmac_verify(k, &[b"D", &t], &mac) {
            return Err(HandshakeError::BadProof);
        }
    }
    match (&joiner_key, &proof.sig) {
        (Some(pk), Some(sig)) => {
            let sig = unb64(sig).unwrap_or_default();
            if !crypto::verify(pk, &[b"D", &t], &sig) {
                return Err(HandshakeError::BadProof);
            }
        }
        (Some(_), None) => return Err(HandshakeError::BadProof),
        (None, _) if found.key.is_none() => return Err(HandshakeError::BadProof),
        (None, _) => {}
    }
    if hello.client == ClientKind::App && joiner_key.is_none() {
        return Err(HandshakeError::Malformed("apps always carry an identity".into()));
    }
    Ok(Accepted { hello, joiner_key })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::Welcome;
    use crate::transport::{PathKind, memory};

    fn invite_lookup(
        id: &'static str,
        secret: &'static [u8],
    ) -> impl FnOnce(&Hello, Option<&[u8; 32]>) -> Result<GrantFound, String> {
        move |h, _| {
            if h.invite.as_deref() == Some(id) {
                Ok(GrantFound {
                    key: Some(crypto::secret_key(secret)),
                    needs_identity: false,
                })
            } else {
                Err("unknown invite".into())
            }
        }
    }

    fn joiner(listener: &Identity, grant: JoinGrant) -> JoinerCtx {
        JoinerCtx {
            name: "Joiner".into(),
            identity: Some(Identity::generate()),
            client: ClientKind::App,
            expect: listener.public(),
            grant,
        }
    }

    #[tokio::test]
    async fn an_invite_handshake_authenticates_both_sides() {
        let (mut a, mut b) = memory::pair(PathKind::Lan);
        let listener = Identity::generate();
        let ctx = joiner(
            &listener,
            JoinGrant::Secret {
                id: "inv".into(),
                secret: b"sekrit".to_vec(),
            },
        );
        let id2 = listener.clone();
        let server = tokio::spawn(async move {
            let acc = accept(&mut b, &id2, "Listener", invite_lookup("inv", b"sekrit")).await;
            if acc.is_ok() {
                b.send(&Message::Welcome(Welcome::default())).await.unwrap();
            }
            acc
        });
        let joined = join(&mut a, &ctx).await.unwrap();
        assert_eq!(joined.listener_name, "Listener");
        await_verdict(&mut a, Duration::from_secs(1)).await.unwrap();
        let acc = server.await.unwrap().unwrap();
        assert_eq!(acc.joiner_key, ctx.identity.map(|i| i.public()));
    }

    #[tokio::test]
    async fn a_wrong_secret_fails_on_both_sides() {
        let (mut a, mut b) = memory::pair(PathKind::Lan);
        let listener = Identity::generate();
        let ctx = joiner(
            &listener,
            JoinGrant::Secret {
                id: "inv".into(),
                secret: b"guess".to_vec(),
            },
        );
        let id2 = listener.clone();
        let server = tokio::spawn(async move { accept(&mut b, &id2, "L", invite_lookup("inv", b"sekrit")).await });
        assert_eq!(
            join(&mut a, &ctx).await,
            Err(HandshakeError::BadProof),
            "the listener's MAC fails"
        );
        drop(a);
        assert!(server.await.unwrap().is_err());
    }

    #[tokio::test]
    async fn an_impostor_listener_is_caught_by_the_pinned_key() {
        let (mut a, mut b) = memory::pair(PathKind::Lan);
        let real = Identity::generate();
        let impostor = Identity::generate();
        let ctx = joiner(
            &real,
            JoinGrant::Secret {
                id: "inv".into(),
                secret: b"s".to_vec(),
            },
        );
        tokio::spawn(async move { accept(&mut b, &impostor, "L", invite_lookup("inv", b"s")).await });
        assert_eq!(join(&mut a, &ctx).await, Err(HandshakeError::WrongIdentity));
    }

    #[tokio::test]
    async fn a_spliced_channel_fails_the_binding() {
        let (mut a, mut b) = memory::pair(PathKind::Lan);
        b.info.binding = b"a different channel".to_vec();
        let listener = Identity::generate();
        let ctx = joiner(
            &listener,
            JoinGrant::Secret {
                id: "inv".into(),
                secret: b"s".to_vec(),
            },
        );
        let id2 = listener.clone();
        tokio::spawn(async move { accept(&mut b, &id2, "L", invite_lookup("inv", b"s")).await });
        assert_eq!(join(&mut a, &ctx).await, Err(HandshakeError::BadProof));
    }

    #[tokio::test]
    async fn trusted_partners_need_their_identity() {
        let (mut a, mut b) = memory::pair(PathKind::Lan);
        let listener = Identity::generate();
        let ctx = joiner(&listener, JoinGrant::Trusted);
        let known = ctx.identity.as_ref().unwrap().public();
        let id2 = listener.clone();
        let server = tokio::spawn(async move {
            accept(&mut b, &id2, "L", |_, pk| {
                if pk == Some(&known) {
                    Ok(GrantFound {
                        key: None,
                        needs_identity: true,
                    })
                } else {
                    Err("not a partner".into())
                }
            })
            .await
        });
        join(&mut a, &ctx).await.unwrap();
        assert!(server.await.unwrap().is_ok());

        let (mut a, mut b) = memory::pair(PathKind::Lan);
        let stranger = joiner(&listener, JoinGrant::Trusted);
        let id3 = listener.clone();
        tokio::spawn(async move {
            accept(&mut b, &id3, "L", |_, pk| {
                if pk == Some(&known) {
                    Ok(GrantFound {
                        key: None,
                        needs_identity: true,
                    })
                } else {
                    Err("not a partner".into())
                }
            })
            .await
        });
        assert!(matches!(join(&mut a, &stranger).await, Err(HandshakeError::Rejected(..))));
    }
}
