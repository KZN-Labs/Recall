use recall_core::error::RecallError;
use recall_proto::registry as reg_proto;
use std::collections::HashMap;
use std::sync::RwLock;

/// Registry profiles are immutable once published.
pub struct RegistryStore {
    profiles: RwLock<HashMap<String, reg_proto::RegistryProfile>>,
}

impl Default for RegistryStore {
    fn default() -> Self {
        Self {
            profiles: RwLock::new(HashMap::new()),
        }
    }
}

impl RegistryStore {
    /// Publish a profile. Once published, it is immutable — re-publishing the same name+version is an error.
    pub fn publish(&self, profile: reg_proto::RegistryProfile) -> Result<(), RecallError> {
        let key = format!("{}@{}", profile.name, profile.version);
        let mut store = self.profiles.write().unwrap();
        if store.contains_key(&key) {
            return Err(RecallError::Internal(format!(
                "registry profile {} is immutable — already published",
                key
            )));
        }
        store.insert(key, profile);
        Ok(())
    }

    pub fn get(&self, name: &str, version: &str) -> Option<reg_proto::RegistryProfile> {
        self.profiles
            .read()
            .unwrap()
            .get(&format!("{}@{}", name, version))
            .cloned()
    }

    pub fn list(&self, category: Option<&str>, name_prefix: Option<&str>) -> Vec<reg_proto::RegistryProfile> {
        self.profiles
            .read()
            .unwrap()
            .values()
            .filter(|p| {
                category.map(|c| p.category == c).unwrap_or(true)
                    && name_prefix.map(|n| p.name.starts_with(n)).unwrap_or(true)
            })
            .cloned()
            .collect()
    }
}
