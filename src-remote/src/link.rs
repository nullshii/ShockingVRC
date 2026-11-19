use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Nonce};
use serde::{Deserialize, Serialize};

use crate::crypto::{self, Identity, b64, unb64};

pub const SCHEME: &str = "shockingvrc://";
pub const LINK_VERSION: u8 = 2;
const SIG_LABEL: &[u8] = b"shockingvrc/2/link";

const KDF_NONE: u8 = 0;
const KDF_ARGON2ID: u8 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KdfParams {
    pub m_kib: u32,
    pub t: u8,
    pub p: u8,
}

pub const KDF_DEFAULT: KdfParams = KdfParams {
    m_kib: 19_456,
    t: 2,
    p: 1,
};

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum LinkError {
    #[error("not a ShockingVRC link")]
    Scheme,
    #[error("this link needs a newer version of ShockingVRC (link version {0})")]
    Version(u8),
    #[error("the link is damaged or incomplete")]
    Malformed,
    #[error("this link is protected by a passphrase")]
    NeedsPassphrase,
    #[error("wrong passphrase, or the link was changed")]
    Decrypt,
    #[error("the link's signature does not match its sender")]
    Signature,
    #[error("this link has expired")]
    Expired,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum OfferedRole {
    #[default]
    Receiver,
    Controller,
    Both,
}

impl OfferedRole {
    pub const ALL: [OfferedRole; 3] = [OfferedRole::Receiver, OfferedRole::Controller, OfferedRole::Both];

    pub fn label(self) -> &'static str {
        match self {
            OfferedRole::Receiver => "They control me",
            OfferedRole::Controller => "I control them",
            OfferedRole::Both => "Both ways",
        }
    }

    pub fn inviter_receives(self) -> bool {
        matches!(self, OfferedRole::Receiver | OfferedRole::Both)
    }

    pub fn inviter_controls(self) -> bool {
        matches!(self, OfferedRole::Controller | OfferedRole::Both)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct RouteHints {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub lan: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub public: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tls_pin: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vds: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vds_pin: Option<String>,
    pub room: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct LinkBody {
    pub v: u8,
    pub pk: String,
    pub fp: String,
    pub name: String,
    pub routes: RouteHints,
    pub invite: String,
    pub secret: String,
    pub role: OfferedRole,
    pub exp: u64,
    pub uses: u32,
}

impl LinkBody {
    pub fn public_key(&self) -> Option<[u8; 32]> {
        crypto::unb64_array(&self.pk)
    }

    pub fn secret_bytes(&self) -> Option<Vec<u8>> {
        unb64(&self.secret)
    }

    pub fn is_expired(&self, now_unix: u64) -> bool {
        self.exp != 0 && now_unix > self.exp
    }
}

#[derive(Serialize, Deserialize)]
struct Signed {
    body: String,
    sig: String,
}

fn derive_key(passphrase: &str, salt: &[u8], k: KdfParams) -> Result<[u8; 32], LinkError> {
    let params = argon2::Params::new(k.m_kib, k.t as u32, k.p as u32, Some(32)).map_err(|_| LinkError::Malformed)?;
    let argon = argon2::Argon2::new(argon2::Algorithm::Argon2id, argon2::Version::V0x13, params);
    let mut out = [0u8; 32];
    argon
        .hash_password_into(passphrase.as_bytes(), salt, &mut out)
        .map_err(|_| LinkError::Malformed)?;
    Ok(out)
}

pub fn encode_link(body: &LinkBody, signer: &Identity, passphrase: Option<&str>) -> String {
    encode_link_with(body, signer, passphrase, KDF_DEFAULT)
}

pub fn encode_link_with(body: &LinkBody, signer: &Identity, passphrase: Option<&str>, kdf: KdfParams) -> String {
    let body_json = serde_json::to_string(body).unwrap_or_default();
    let sig = signer.sign(&[SIG_LABEL, body_json.as_bytes()]);
    let plain = serde_json::to_vec(&Signed {
        body: body_json,
        sig: b64(&sig),
    })
    .unwrap_or_default();

    let mut header = vec![LINK_VERSION];
    let key = match passphrase.filter(|p| !p.is_empty()) {
        Some(pass) => {
            let salt: [u8; 16] = crypto::random_bytes();
            header.push(KDF_ARGON2ID);
            header.extend_from_slice(&kdf.m_kib.to_be_bytes());
            header.push(kdf.t);
            header.push(kdf.p);
            header.extend_from_slice(&salt);
            derive_key(pass, &salt, kdf).unwrap_or([0; 32])
        }
        None => {
            let key: [u8; 32] = crypto::random_bytes();
            header.push(KDF_NONE);
            header.extend_from_slice(&key);
            key
        }
    };
    let nonce: [u8; 12] = crypto::random_bytes();
    header.extend_from_slice(&nonce);
    let cipher = Aes256Gcm::new_from_slice(&key).expect("32-byte key");
    let ct = cipher
        .encrypt(
            Nonce::from_slice(&nonce),
            Payload {
                msg: &plain,
                aad: &header,
            },
        )
        .unwrap_or_default();
    let mut blob = header;
    blob.extend(ct);
    format!("{SCHEME}{LINK_VERSION}/{}", b64(&blob))
}

fn payload(link: &str) -> Result<Vec<u8>, LinkError> {
    let s = link.trim();
    let rest = s
        .strip_prefix(SCHEME)
        .or_else(|| s.strip_prefix("shockingvrc:"))
        .ok_or(LinkError::Scheme)?;
    let rest = rest.trim_start_matches('/');
    let (ver, data) = rest.split_once('/').ok_or(LinkError::Malformed)?;
    let ver: u8 = ver.parse().map_err(|_| LinkError::Malformed)?;
    if ver != LINK_VERSION {
        return Err(LinkError::Version(ver));
    }
    let blob = unb64(data.trim_end_matches('/')).ok_or(LinkError::Malformed)?;
    if blob.first() != Some(&LINK_VERSION) {
        return Err(LinkError::Malformed);
    }
    Ok(blob)
}

pub fn needs_passphrase(link: &str) -> Result<bool, LinkError> {
    let blob = payload(link)?;
    match blob.get(1) {
        Some(&KDF_NONE) => Ok(false),
        Some(&KDF_ARGON2ID) => Ok(true),
        _ => Err(LinkError::Malformed),
    }
}

pub fn decode_link(link: &str, passphrase: Option<&str>, now_unix: u64) -> Result<LinkBody, LinkError> {
    let blob = payload(link)?;
    let (key, header_len) = match blob.get(1) {
        Some(&KDF_NONE) => {
            let key: [u8; 32] = blob.get(2..34).ok_or(LinkError::Malformed)?.try_into().unwrap();
            (key, 34)
        }
        Some(&KDF_ARGON2ID) => {
            let pass = passphrase.filter(|p| !p.is_empty()).ok_or(LinkError::NeedsPassphrase)?;
            let h = blob.get(2..24).ok_or(LinkError::Malformed)?;
            let kdf = KdfParams {
                m_kib: u32::from_be_bytes([h[0], h[1], h[2], h[3]]),
                t: h[4],
                p: h[5],
            };
            if kdf.m_kib > 262_144 || kdf.t > 16 || kdf.p > 8 || kdf.p == 0 {
                return Err(LinkError::Malformed);
            }
            (derive_key(pass, &h[6..22], kdf)?, 24)
        }
        _ => return Err(LinkError::Malformed),
    };
    let total = header_len + 12;
    if blob.len() < total + 16 {
        return Err(LinkError::Malformed);
    }
    let cipher = Aes256Gcm::new_from_slice(&key).map_err(|_| LinkError::Malformed)?;
    let plain = cipher
        .decrypt(
            Nonce::from_slice(&blob[header_len..total]),
            Payload {
                msg: &blob[total..],
                aad: &blob[..total],
            },
        )
        .map_err(|_| LinkError::Decrypt)?;
    let signed: Signed = serde_json::from_slice(&plain).map_err(|_| LinkError::Malformed)?;
    let body: LinkBody = serde_json::from_str(&signed.body).map_err(|_| LinkError::Malformed)?;
    let pk = body.public_key().ok_or(LinkError::Malformed)?;
    let sig = unb64(&signed.sig).ok_or(LinkError::Malformed)?;
    if !crypto::verify(&pk, &[SIG_LABEL, signed.body.as_bytes()], &sig) {
        return Err(LinkError::Signature);
    }
    if body.v != LINK_VERSION {
        return Err(LinkError::Version(body.v));
    }
    if body.is_expired(now_unix) {
        return Err(LinkError::Expired);
    }
    Ok(body)
}

const CODE_ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

pub fn new_control_code() -> String {
    let bytes: [u8; 16] = crypto::random_bytes();
    let chars: Vec<char> = bytes
        .iter()
        .map(|b| CODE_ALPHABET[(*b & 31) as usize] as char)
        .collect();
    chars
        .chunks(4)
        .map(|c| c.iter().collect::<String>())
        .collect::<Vec<_>>()
        .join("-")
}

pub fn normalize_code(code: &str) -> Option<String> {
    let s: String = code
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '-')
        .map(|c| match c.to_ascii_uppercase() {
            'O' => '0',
            'I' | 'L' => '1',
            'U' => 'V',
            other => other,
        })
        .collect();
    (s.len() == 16 && s.bytes().all(|b| CODE_ALPHABET.contains(&b))).then_some(s)
}

pub fn code_room(normalized: &str) -> String {
    format!("c-{}", &normalized[..4])
}

pub fn code_invite_id(normalized: &str) -> String {
    format!("code-{}", &normalized[..4])
}

#[cfg(test)]
mod tests {
    use super::*;

    const FAST: KdfParams = KdfParams { m_kib: 64, t: 1, p: 1 };

    fn body(id: &Identity) -> LinkBody {
        LinkBody {
            v: LINK_VERSION,
            pk: b64(&id.public()),
            fp: id.fingerprint(),
            name: "Tester".into(),
            routes: RouteHints {
                lan: vec!["192.168.1.20:47390".into()],
                room: id.room(),
                ..RouteHints::default()
            },
            invite: "inv1".into(),
            secret: b64(&[7u8; 32]),
            role: OfferedRole::Receiver,
            exp: 2_000_000_000,
            uses: 1,
        }
    }

    #[test]
    fn a_plain_link_round_trips() {
        let id = Identity::generate();
        let link = encode_link_with(&body(&id), &id, None, FAST);
        assert!(link.starts_with("shockingvrc://2/"));
        assert_eq!(needs_passphrase(&link), Ok(false));
        assert_eq!(decode_link(&link, None, 1_000).unwrap(), body(&id));
    }

    #[test]
    fn every_link_is_new_even_for_the_same_invite() {
        let id = Identity::generate();
        let a = encode_link_with(&body(&id), &id, None, FAST);
        let b = encode_link_with(&body(&id), &id, None, FAST);
        assert_ne!(a, b);
    }

    #[test]
    fn a_passphrase_link_needs_the_passphrase() {
        let id = Identity::generate();
        let link = encode_link_with(&body(&id), &id, Some("correct horse"), FAST);
        assert_eq!(needs_passphrase(&link), Ok(true));
        assert_eq!(decode_link(&link, None, 0), Err(LinkError::NeedsPassphrase));
        assert_eq!(decode_link(&link, Some("wrong"), 0), Err(LinkError::Decrypt));
        assert_eq!(decode_link(&link, Some("correct horse"), 0).unwrap().name, "Tester");
    }

    #[test]
    fn tampering_and_expiry_are_detected() {
        let id = Identity::generate();
        let link = encode_link_with(&body(&id), &id, None, FAST);
        let mut chars: Vec<char> = link.chars().collect();
        let k = chars.len() - 5;
        chars[k] = if chars[k] == 'A' { 'B' } else { 'A' };
        let broken: String = chars.into_iter().collect();
        assert!(matches!(
            decode_link(&broken, None, 0),
            Err(LinkError::Decrypt | LinkError::Malformed)
        ));
        assert_eq!(decode_link(&link, None, 2_000_000_001), Err(LinkError::Expired));
        assert_eq!(decode_link("https://example.com", None, 0), Err(LinkError::Scheme));
        assert_eq!(decode_link("shockingvrc://3/abc", None, 0), Err(LinkError::Version(3)));
    }

    #[test]
    fn a_link_signed_by_someone_else_is_rejected() {
        let id = Identity::generate();
        let mallory = Identity::generate();
        let link = encode_link_with(&body(&id), &mallory, None, FAST);
        assert_eq!(decode_link(&link, None, 0), Err(LinkError::Signature));
    }

    #[test]
    fn control_codes_normalise() {
        let code = new_control_code();
        assert_eq!(code.len(), 19);
        let n = normalize_code(&code.to_lowercase()).unwrap();
        assert_eq!(n.len(), 16);
        assert_eq!(normalize_code("o0il-1111-2222-3333").unwrap(), "0011111122223333");
        assert!(normalize_code("short").is_none());
        assert_eq!(code_room(&n).len(), 6);
    }
}
