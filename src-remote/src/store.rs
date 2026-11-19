use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::crypto::{self, Identity, b64};
use crate::link::{OfferedRole, RouteHints};

pub const TRUST_FILE: &str = "remote_trust.json";

pub fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Lifetime {
    #[default]
    SingleUse,
    Minutes10,
    Hour1,
    Hours24,
    Days7,
    Indefinite,
}

impl Lifetime {
    pub const ALL: [Lifetime; 6] = [
        Lifetime::SingleUse,
        Lifetime::Minutes10,
        Lifetime::Hour1,
        Lifetime::Hours24,
        Lifetime::Days7,
        Lifetime::Indefinite,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Lifetime::SingleUse => "Single use",
            Lifetime::Minutes10 => "10 min",
            Lifetime::Hour1 => "1 hour",
            Lifetime::Hours24 => "24 hours",
            Lifetime::Days7 => "7 days",
            Lifetime::Indefinite => "Indefinite",
        }
    }

    pub fn secs(self) -> Option<u64> {
        match self {
            Lifetime::SingleUse => Some(24 * 3600),
            Lifetime::Minutes10 => Some(600),
            Lifetime::Hour1 => Some(3600),
            Lifetime::Hours24 => Some(24 * 3600),
            Lifetime::Days7 => Some(7 * 24 * 3600),
            Lifetime::Indefinite => None,
        }
    }

    pub fn uses(self) -> Option<u32> {
        match self {
            Lifetime::SingleUse | Lifetime::Indefinite => Some(1),
            _ => None,
        }
    }

    pub fn creates_pairing(self) -> bool {
        self == Lifetime::Indefinite
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InviteRecord {
    pub id: String,
    pub key: String,
    pub role: OfferedRole,
    pub lifetime: Lifetime,
    pub created: u64,
    pub expires: u64,
    #[serde(default)]
    pub uses_left: Option<u32>,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub passphrase: bool,
    #[serde(default)]
    pub web_code: bool,
}

impl InviteRecord {
    pub fn is_live(&self, now: u64) -> bool {
        (self.expires == 0 || now <= self.expires) && self.uses_left != Some(0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PartnerRecord {
    pub pk: String,
    pub name: String,
    pub my_role: OfferedRole,
    pub added: u64,
    #[serde(default)]
    pub auto_accept: bool,
    #[serde(default)]
    pub last_seen: u64,
    #[serde(default)]
    pub routes: Option<RouteHints>,
}

impl PartnerRecord {
    pub fn fingerprint(&self) -> String {
        crypto::unb64_array::<32>(&self.pk)
            .map(|pk| crypto::fingerprint(&pk))
            .unwrap_or_default()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TlsMaterial {
    pub cert_der: String,
    pub key_der: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct TrustData {
    #[serde(default)]
    identity: Option<String>,
    #[serde(default)]
    tls: Option<TlsMaterial>,
    #[serde(default)]
    invites: Vec<InviteRecord>,
    #[serde(default)]
    partners: Vec<PartnerRecord>,
}

#[derive(Debug)]
pub struct TrustStore {
    path: Option<PathBuf>,
    data: TrustData,
    identity: Identity,
}

impl TrustStore {
    pub fn open(path: &Path) -> Self {
        let data: TrustData = std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        let me = Self::from_data(Some(path.to_path_buf()), data);
        me.save();
        me
    }

    pub fn in_memory() -> Self {
        Self::from_data(None, TrustData::default())
    }

    fn from_data(path: Option<PathBuf>, mut data: TrustData) -> Self {
        let identity = data
            .identity
            .as_deref()
            .and_then(crypto::unb64_array::<32>)
            .map(Identity::from_secret)
            .unwrap_or_else(Identity::generate);
        data.identity = Some(b64(&identity.secret()));
        Self { path, data, identity }
    }

    pub fn save(&self) {
        let Some(path) = &self.path else { return };
        let Ok(json) = serde_json::to_string_pretty(&self.data) else {
            return;
        };
        let tmp = path.with_extension("json.tmp");
        if std::fs::write(&tmp, json).is_ok() && std::fs::rename(&tmp, path).is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
    }

    pub fn identity(&self) -> &Identity {
        &self.identity
    }

    pub fn tls(&self) -> Option<&TlsMaterial> {
        self.data.tls.as_ref()
    }

    pub fn set_tls(&mut self, tls: TlsMaterial) {
        self.data.tls = Some(tls);
        self.save();
    }

    pub fn invites(&self) -> &[InviteRecord] {
        &self.data.invites
    }

    pub fn partners(&self) -> &[PartnerRecord] {
        &self.data.partners
    }

    pub fn prune(&mut self, now: u64) {
        let before = self.data.invites.len();
        self.data.invites.retain(|i| i.is_live(now));
        if self.data.invites.len() != before {
            self.save();
        }
    }

    pub fn add_invite(
        &mut self,
        role: OfferedRole,
        lifetime: Lifetime,
        label: &str,
        passphrase: bool,
        now: u64,
    ) -> (InviteRecord, [u8; 32]) {
        let secret: [u8; 32] = crypto::random_bytes();
        let id = b64(&crypto::random_bytes::<12>());
        let rec = InviteRecord {
            id,
            key: b64(&crypto::secret_key(&secret)),
            role,
            lifetime,
            created: now,
            expires: lifetime.secs().map(|s| now + s).unwrap_or(0),
            uses_left: lifetime.uses(),
            label: label.to_string(),
            passphrase,
            web_code: false,
        };
        self.data.invites.push(rec.clone());
        self.save();
        (rec, secret)
    }

    pub fn add_web_code(&mut self, code: &str, role: OfferedRole, lifetime: Lifetime, now: u64) -> InviteRecord {
        let rec = InviteRecord {
            id: crate::link::code_invite_id(code),
            key: b64(&crypto::secret_key(code.as_bytes())),
            role,
            lifetime,
            created: now,
            expires: lifetime.secs().map(|s| now + s).unwrap_or(now + 600),
            uses_left: None,
            label: "Web control code".into(),
            passphrase: false,
            web_code: true,
        };
        self.data.invites.retain(|i| i.id != rec.id);
        self.data.invites.push(rec.clone());
        self.save();
        rec
    }

    pub fn invite(&self, id: &str, now: u64) -> Option<&InviteRecord> {
        self.data.invites.iter().find(|i| i.id == id && i.is_live(now))
    }

    pub fn consume_invite(&mut self, id: &str) {
        if let Some(i) = self.data.invites.iter_mut().find(|i| i.id == id)
            && let Some(n) = &mut i.uses_left
        {
            *n = n.saturating_sub(1);
        }
        self.data.invites.retain(|i| i.uses_left != Some(0));
        self.save();
    }

    pub fn revoke_invite(&mut self, id: &str) -> bool {
        let before = self.data.invites.len();
        self.data.invites.retain(|i| i.id != id);
        let changed = self.data.invites.len() != before;
        if changed {
            self.save();
        }
        changed
    }

    pub fn partner(&self, pk: &str) -> Option<&PartnerRecord> {
        self.data.partners.iter().find(|p| p.pk == pk)
    }

    pub fn upsert_partner(&mut self, rec: PartnerRecord) {
        match self.data.partners.iter_mut().find(|p| p.pk == rec.pk) {
            Some(p) => {
                let auto = p.auto_accept;
                *p = PartnerRecord {
                    auto_accept: auto || rec.auto_accept,
                    routes: rec.routes.clone().or_else(|| p.routes.clone()),
                    ..rec
                };
            }
            None => self.data.partners.push(rec),
        }
        self.save();
    }

    pub fn touch_partner(&mut self, pk: &str, now: u64) {
        if let Some(p) = self.data.partners.iter_mut().find(|p| p.pk == pk) {
            p.last_seen = now;
            self.save();
        }
    }

    pub fn set_auto_accept(&mut self, pk: &str, on: bool) {
        if let Some(p) = self.data.partners.iter_mut().find(|p| p.pk == pk) {
            p.auto_accept = on;
            self.save();
        }
    }

    pub fn revoke_partner(&mut self, pk: &str) -> bool {
        let before = self.data.partners.len();
        self.data.partners.retain(|p| p.pk != pk);
        let changed = self.data.partners.len() != before;
        if changed {
            self.save();
        }
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_hash_of_the_secret_is_stored() {
        let mut s = TrustStore::in_memory();
        let (rec, secret) = s.add_invite(OfferedRole::Receiver, Lifetime::Hour1, "x", false, 100);
        assert_ne!(rec.key, b64(&secret));
        assert_eq!(rec.key, b64(&crypto::secret_key(&secret)));
        assert_eq!(rec.expires, 3700);
        assert!(s.invite(&rec.id, 200).is_some());
        assert!(s.invite(&rec.id, 4000).is_none(), "expired");
    }

    #[test]
    fn single_use_invites_are_spent() {
        let mut s = TrustStore::in_memory();
        let (rec, _) = s.add_invite(OfferedRole::Both, Lifetime::SingleUse, "", false, 0);
        s.consume_invite(&rec.id);
        assert!(s.invite(&rec.id, 1).is_none());
        assert!(s.invites().is_empty());
    }

    #[test]
    fn the_store_persists_identity_and_partners() {
        let dir = std::env::temp_dir().join(format!("svrc-trust-{}", crypto::b64(&crypto::random_bytes::<6>())));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(TRUST_FILE);
        let (pk, id) = {
            let mut s = TrustStore::open(&path);
            s.upsert_partner(PartnerRecord {
                pk: "abc".into(),
                name: "Partner".into(),
                my_role: OfferedRole::Receiver,
                added: 1,
                auto_accept: false,
                last_seen: 0,
                routes: None,
            });
            (s.identity().public(), s.partners().len())
        };
        let s = TrustStore::open(&path);
        assert_eq!(s.identity().public(), pk);
        assert_eq!(s.partners().len(), id);
        let _ = std::fs::remove_dir_all(dir);
    }
}
