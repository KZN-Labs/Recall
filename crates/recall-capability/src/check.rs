use chrono::Utc;

use crate::caveat::{evaluate_all, CaveatContext};
use crate::store::CapabilityStore;

pub struct CheckResult {
    pub allowed: bool,
    pub deny_reason: Option<String>,
    pub unmet_caveats: Vec<String>,
}

/// Capability check at the control plane. Called for every consequential action.
/// Never runs in agent code or SDK adapters.
pub fn check_capability<'a>(
    store: &CapabilityStore,
    capability_id: &recall_core::ids::ContentHash,
    action_kind: &str,
    entity: &str,
    context: CaveatContext<'a>,
) -> CheckResult {
    let cap = match store.get(capability_id) {
        Some(c) => c,
        None => {
            return CheckResult {
                allowed: false,
                deny_reason: Some(format!("capability {} not found", capability_id.0)),
                unmet_caveats: vec![],
            }
        }
    };

    // Revocation check (immediate, consult revocation list).
    if store.is_revoked(capability_id) {
        return CheckResult {
            allowed: false,
            deny_reason: Some("capability is revoked".into()),
            unmet_caveats: vec![],
        };
    }

    // Expiry check.
    if let Some(valid_until) = &cap.valid_until {
        let now = Utc::now().timestamp();
        if now > valid_until.seconds {
            return CheckResult {
                allowed: false,
                deny_reason: Some("capability expired".into()),
                unmet_caveats: vec![],
            };
        }
    }

    // Scope check: action kind.
    if let Some(scope) = &cap.scope {
        if !scope.unrestricted_actions && !scope.permitted_action_kinds.iter().any(|k| k == action_kind) {
            return CheckResult {
                allowed: false,
                deny_reason: Some(format!("action '{}' not in permitted_action_kinds", action_kind)),
                unmet_caveats: vec![],
            };
        }

        // Entity scope: check forbidden patterns.
        for forbidden in &scope.forbidden_entity_scopes {
            if glob_match(forbidden, entity) {
                return CheckResult {
                    allowed: false,
                    deny_reason: Some(format!("entity '{}' matches forbidden scope '{}'", entity, forbidden)),
                    unmet_caveats: vec![],
                };
            }
        }

        // Entity scope: check permitted patterns (must match at least one if non-empty).
        if !scope.permitted_entity_scopes.is_empty() {
            let permitted = scope.permitted_entity_scopes.iter().any(|p| glob_match(p, entity));
            if !permitted {
                return CheckResult {
                    allowed: false,
                    deny_reason: Some(format!("entity '{}' not in permitted_entity_scopes", entity)),
                    unmet_caveats: vec![],
                };
            }
        }
    }

    // Caveat evaluation.
    if let Err(failures) = evaluate_all(&cap.caveats, &context) {
        return CheckResult {
            allowed: false,
            deny_reason: Some("caveat evaluation failed".into()),
            unmet_caveats: failures,
        };
    }

    CheckResult {
        allowed: true,
        deny_reason: None,
        unmet_caveats: vec![],
    }
}

/// Minimal glob matching: supports '*' as wildcard segment.
fn glob_match(pattern: &str, value: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    if let Some(prefix) = pattern.strip_suffix(".*") {
        return value == prefix || value.starts_with(&format!("{}.", prefix));
    }
    pattern == value
}
