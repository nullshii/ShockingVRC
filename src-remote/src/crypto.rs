use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

type HmacSha256 = Hmac<Sha256>;

pub const SECRET_LABEL: &[u8] = b"shockingvrc/2/secret";
pub const TRANSCRIPT_LABEL: &[u8] = b"shockingvrc/2/handshake";

pub fn b64(bytes: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(bytes)
}

pub fn unb64(s: &str) -> Option<Vec<u8>> {
    URL_SAFE_NO_PAD.decode(s.trim().trim_end_matches('=')).ok()
}

pub fn unb64_array<const N: usize>(s: &str) -> Option<[u8; N]> {
    unb64(s)?.try_into().ok()
}

pub fn random_bytes<const N: usize>() -> [u8; N] {
    let mut out = [0u8; N];
    rand::fill(&mut out[..]);
    out
}

pub fn sha256(parts: &[&[u8]]) -> [u8; 32] {
    let mut h = Sha256::new();
    for p in parts {
        h.update(p);
    }
    h.finalize().into()
}

pub fn secret_key(secret: &[u8]) -> [u8; 32] {
    sha256(&[SECRET_LABEL, secret])
}

pub fn hmac(key: &[u8], parts: &[&[u8]]) -> [u8; 32] {
    let mut m = <HmacSha256 as Mac>::new_from_slice(key).expect("HMAC takes any key length");
    for p in parts {
        m.update(p);
    }
    m.finalize().into_bytes().into()
}

pub fn hmac_verify(key: &[u8], parts: &[&[u8]], tag: &[u8]) -> bool {
    let mut m = <HmacSha256 as Mac>::new_from_slice(key).expect("HMAC takes any key length");
    for p in parts {
        m.update(p);
    }
    m.verify_slice(tag).is_ok()
}

pub fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && bool::from(a.ct_eq(b))
}

pub fn fingerprint(public: &[u8; 32]) -> String {
    let h = sha256(&[b"shockingvrc/2/fp", public]);
    h[..8]
        .chunks(2)
        .map(|c| format!("{:02X}{:02X}", c[0], c[1]))
        .collect::<Vec<_>>()
        .join("-")
}

pub fn room_for(public: &[u8; 32]) -> String {
    let h = sha256(&[b"shockingvrc/2/room", public]);
    h[..16].iter().map(|b| format!("{b:02x}")).collect()
}

#[derive(Clone)]
pub struct Identity {
    key: SigningKey,
}

impl std::fmt::Debug for Identity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Identity({})", self.fingerprint())
    }
}

impl Identity {
    pub fn generate() -> Self {
        Self::from_secret(random_bytes())
    }

    pub fn from_secret(secret: [u8; 32]) -> Self {
        Self {
            key: SigningKey::from_bytes(&secret),
        }
    }

    pub fn secret(&self) -> [u8; 32] {
        self.key.to_bytes()
    }

    pub fn public(&self) -> [u8; 32] {
        self.key.verifying_key().to_bytes()
    }

    pub fn fingerprint(&self) -> String {
        fingerprint(&self.public())
    }

    pub fn room(&self) -> String {
        room_for(&self.public())
    }

    pub fn sign(&self, parts: &[&[u8]]) -> [u8; 64] {
        let mut msg = Vec::new();
        for p in parts {
            msg.extend_from_slice(p);
        }
        self.key.sign(&msg).to_bytes()
    }
}

pub fn verify(public: &[u8], parts: &[&[u8]], sig: &[u8]) -> bool {
    let (Ok(pk), Ok(sig)) = (<[u8; 32]>::try_from(public), <[u8; 64]>::try_from(sig)) else {
        return false;
    };
    let Ok(key) = VerifyingKey::from_bytes(&pk) else {
        return false;
    };
    let mut msg = Vec::new();
    for p in parts {
        msg.extend_from_slice(p);
    }
    key.verify_strict(&msg, &Signature::from_bytes(&sig)).is_ok()
}

fn put(out: &mut Vec<u8>, part: &[u8]) {
    out.extend_from_slice(&(part.len() as u32).to_be_bytes());
    out.extend_from_slice(part);
}

#[derive(Debug, Clone, Default)]
pub struct TranscriptInput<'a> {
    pub binding: &'a [u8],
    pub joiner_nonce: &'a [u8],
    pub listener_nonce: &'a [u8],
    pub listener_key: &'a [u8],
    pub joiner_key: &'a [u8],
    pub grant: &'a [u8],
}

pub fn transcript(t: &TranscriptInput<'_>) -> [u8; 32] {
    let mut buf = Vec::with_capacity(256);
    put(&mut buf, TRANSCRIPT_LABEL);
    for part in [
        t.binding,
        t.joiner_nonce,
        t.listener_nonce,
        t.listener_key,
        t.joiner_key,
        t.grant,
    ] {
        put(&mut buf, part);
    }
    sha256(&[&buf])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signatures_verify_and_reject_tampering() {
        let id = Identity::generate();
        let sig = id.sign(&[b"hello", b" world"]);
        assert!(verify(&id.public(), &[b"hello world"], &sig));
        assert!(!verify(&id.public(), &[b"hello there"], &sig));
        assert!(!verify(&Identity::generate().public(), &[b"hello world"], &sig));
        assert!(!verify(b"short", &[b"x"], &sig));
    }

    #[test]
    fn hmac_round_trips_in_constant_time() {
        let key = secret_key(b"s3cret");
        let tag = hmac(&key, &[b"a", b"b"]);
        assert!(hmac_verify(&key, &[b"ab"], &tag));
        assert!(!hmac_verify(&key, &[b"ab"], &tag[..31]));
        assert!(!hmac_verify(&secret_key(b"other"), &[b"ab"], &tag));
    }

    #[test]
    fn transcripts_depend_on_every_field() {
        let base = TranscriptInput {
            binding: b"b",
            joiner_nonce: b"n1",
            listener_nonce: b"n2",
            listener_key: b"k1",
            joiner_key: b"",
            grant: b"g",
        };
        let t0 = transcript(&base);
        let t1 = transcript(&TranscriptInput {
            binding: b"c",
            ..base.clone()
        });
        let t2 = transcript(&TranscriptInput {
            joiner_nonce: b"n",
            listener_nonce: b"1n2",
            ..base
        });
        assert_ne!(t0, t1);
        assert_ne!(t0, t2, "length prefixes keep fields apart");
    }

    #[test]
    fn identities_persist_and_fingerprints_are_stable() {
        let id = Identity::generate();
        let back = Identity::from_secret(id.secret());
        assert_eq!(back.public(), id.public());
        assert_eq!(id.fingerprint().len(), 19);
        assert_eq!(id.room().len(), 32);
        assert_eq!(unb64_array::<32>(&b64(&id.public())), Some(id.public()));
    }
}
