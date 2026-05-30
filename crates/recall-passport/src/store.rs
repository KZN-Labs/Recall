use recall_core::{error::RecallError, ids::ContentHash};
use std::collections::HashMap;
use std::sync::RwLock;

use crate::{Passport, PassportState};

/// In-process passport registry. Production code swaps this for a Postgres-backed store.
pub struct PassportStore {
    passports: RwLock<HashMap<String, Passport>>,
    revocations: RwLock<HashMap<String, String>>,
}

impl Default for PassportStore {
    fn default() -> Self {
        Self {
            passports: RwLock::new(HashMap::new()),
            revocations: RwLock::new(HashMap::new()),
        }
    }
}

impl PassportStore {
    pub fn register(&self, passport: Passport) -> Result<ContentHash, RecallError> {
        let id = passport.passport_id.clone();
        self.passports
            .write()
            .unwrap()
            .insert(id.0.clone(), passport);
        Ok(id)
    }

    pub fn get(&self, passport_id: &ContentHash) -> Option<Passport> {
        self.passports.read().unwrap().get(&passport_id.0).cloned()
    }

    pub fn revoke(&self, passport_id: &ContentHash, reason: &str) -> Result<(), RecallError> {
        let mut store = self.passports.write().unwrap();
        let passport = store
            .get_mut(&passport_id.0)
            .ok_or_else(|| RecallError::PassportNotFound(passport_id.0.clone()))?;
        passport.state = PassportState::Revoked {
            reason: reason.to_owned(),
        };
        self.revocations
            .write()
            .unwrap()
            .insert(passport_id.0.clone(), reason.to_owned());
        Ok(())
    }

    pub fn is_revoked(&self, passport_id: &ContentHash) -> bool {
        self.revocations.read().unwrap().contains_key(&passport_id.0)
    }
}
