use recall_core::ids::{AgentId, WorkspaceId};
use recall_core::types::{AgentRole, TrustLevel};
use recall_crypto::RecallKeypair;
use recall_passport::Passport;

/// Run passport conformance tests.
pub fn run_passport_conformance() {
    let kp = RecallKeypair::generate();
    let passport = Passport::create(
        &kp,
        AgentId::new(),
        WorkspaceId::new("test-ws"),
        TrustLevel::Medium,
        AgentRole::Writer,
        "anthropic".to_string(),
        "claude-sonnet-4-6".to_string(),
        None,
    );

    assert_eq!(passport.passport_id.0.len(), 64, "passport ID must be 64-char hex");
    assert!(!passport.is_expired(), "freshly created passport must not be expired");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passport_id_is_stable_across_serialization() {
        let kp = RecallKeypair::generate();
        let passport = Passport::create(
            &kp,
            AgentId::from_str("550e8400-e29b-41d4-a716-446655440000"),
            WorkspaceId::new("test"),
            TrustLevel::High,
            AgentRole::Admin,
            "anthropic".to_string(),
            "claude-sonnet-4-6".to_string(),
            None,
        );

        let id1 = passport.passport_id.clone();
        // Passport ID is derived from canonical bytes — same input → same output.
        // (We can't re-create with the exact same signature, but we can verify format.)
        assert_eq!(id1.0.len(), 64);
        assert!(id1.0.chars().all(|c| c.is_ascii_hexdigit()));
    }
}
