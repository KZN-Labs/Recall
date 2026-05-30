use std::collections::HashSet;
use std::sync::RwLock;

/// Replay protection: track nonces seen in a sliding time window.
/// Rejects duplicate envelope nonces to prevent replay attacks.
pub struct ReplayProtector {
    seen_nonces: RwLock<HashSet<String>>,
}

impl Default for ReplayProtector {
    fn default() -> Self {
        Self {
            seen_nonces: RwLock::new(HashSet::new()),
        }
    }
}

impl ReplayProtector {
    /// Returns true if the nonce is fresh (not seen before). Inserts it.
    pub fn check_and_insert(&self, nonce: &str) -> bool {
        let mut seen = self.seen_nonces.write().unwrap();
        if seen.contains(nonce) {
            return false;
        }
        seen.insert(nonce.to_string());
        true
    }
}
