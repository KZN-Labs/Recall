use recall_core::ids::ContentHash;
use recall_proto::capability as cap_proto;
use std::collections::{HashMap, HashSet};
use std::sync::RwLock;

pub struct CapabilityStore {
    caps: RwLock<HashMap<String, cap_proto::Capability>>,
    revocations: RwLock<HashSet<String>>,
}

impl Default for CapabilityStore {
    fn default() -> Self {
        Self {
            caps: RwLock::new(HashMap::new()),
            revocations: RwLock::new(HashSet::new()),
        }
    }
}

impl CapabilityStore {
    pub fn insert(&self, cap: cap_proto::Capability) -> ContentHash {
        let id = cap.id.as_ref().expect("capability missing id").hex.clone();
        self.caps.write().unwrap().insert(id.clone(), cap);
        ContentHash(id)
    }

    pub fn get(&self, id: &ContentHash) -> Option<cap_proto::Capability> {
        self.caps.read().unwrap().get(&id.0).cloned()
    }

    pub fn revoke(&self, id: &ContentHash) {
        self.revocations.write().unwrap().insert(id.0.clone());
    }

    pub fn is_revoked(&self, id: &ContentHash) -> bool {
        self.revocations.read().unwrap().contains(&id.0)
    }

    /// Revoke all capabilities issued by a specific passport (cascade revocation).
    pub fn revoke_all_by_issuer(&self, issuer_passport_id: &ContentHash) {
        let ids: Vec<String> = self
            .caps
            .read()
            .unwrap()
            .values()
            .filter(|c| {
                c.issuer_passport_id
                    .as_ref()
                    .map(|i| i.hex == issuer_passport_id.0)
                    .unwrap_or(false)
            })
            .filter_map(|c| c.id.as_ref().map(|id| id.hex.clone()))
            .collect();

        let mut revocations = self.revocations.write().unwrap();
        for id in ids {
            revocations.insert(id);
        }
    }
}
