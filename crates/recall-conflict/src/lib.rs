use recall_core::ids::ContentHash;
use recall_proto::{common as common_proto, memory as mem_proto};
use uuid::Uuid;

/// Built-in default pairs. RECALL ships with these so first-time workspaces
/// detect the obvious cross-agent conflicts out of the box. Workspaces can
/// extend or replace this set at creation time via [`ConflictPolicy`].
///
/// Note: this is a **policy**, not a general semantic engine. RECALL does not
/// reason about event meaning — it matches event-name pairs you opt into.
pub const DEFAULT_CONFLICT_PAIRS: &[(&str, &str)] = &[
    ("credit_approved",  "flag_suspicious"),
    ("credit_offered",   "flag_suspicious"),
    ("account_verified", "flag_suspicious"),
    ("refund_approved",  "refund_denied"),
];

/// Configurable conflict policy owned by a workspace.
///
/// Two memory entries from different agents about the same entity conflict
/// iff their `(event_a, event_b)` pair (in either order) appears in `pairs`.
///
/// Workspaces start with `ConflictPolicy::default()` (the 4 built-in pairs).
/// Add pairs with [`ConflictPolicy::with_pair`] or replace the whole set with
/// [`ConflictPolicy::with_pairs`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConflictPolicy {
    pairs: Vec<(String, String)>,
}

impl Default for ConflictPolicy {
    fn default() -> Self {
        Self {
            pairs: DEFAULT_CONFLICT_PAIRS
                .iter()
                .map(|(a, b)| (a.to_string(), b.to_string()))
                .collect(),
        }
    }
}

impl ConflictPolicy {
    /// Empty policy — no events ever conflict. Useful in tests and when a
    /// workspace wants to opt out of conflict detection entirely.
    pub fn empty() -> Self {
        Self { pairs: Vec::new() }
    }

    /// Build a policy from a list of event-name pairs. Order within a pair is
    /// not significant — conflicts fire for either ordering.
    pub fn with_pairs<I, A, B>(pairs: I) -> Self
    where
        I: IntoIterator<Item = (A, B)>,
        A: Into<String>,
        B: Into<String>,
    {
        Self {
            pairs: pairs.into_iter().map(|(a, b)| (a.into(), b.into())).collect(),
        }
    }

    /// Add one pair to an existing policy. Builder-style — returns self.
    pub fn with_pair<A: Into<String>, B: Into<String>>(mut self, a: A, b: B) -> Self {
        self.pairs.push((a.into(), b.into()));
        self
    }

    /// True iff two event names (in either order) appear together in any
    /// policy pair.
    pub fn matches(&self, event_a: &str, event_b: &str) -> bool {
        self.pairs.iter().any(|(pa, pb)| {
            (event_a == pa && event_b == pb) || (event_a == pb && event_b == pa)
        })
    }

    /// Number of configured pairs. Mostly useful for diagnostics and tests.
    pub fn len(&self) -> usize {
        self.pairs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.pairs.is_empty()
    }
}

/// Check whether two new memory entries conflict, using the **default**
/// conflict policy. Provided for back-compat and quick checks; prefer
/// [`detect_conflict_with`] when a workspace has a customized policy.
pub fn detect_conflict(
    a: &mem_proto::MemoryEntry,
    b: &mem_proto::MemoryEntry,
) -> bool {
    detect_conflict_with(a, b, &ConflictPolicy::default())
}

/// Check whether two new memory entries conflict under the given policy.
///
/// Two entries conflict iff:
///   - they are about the same entity in the same workspace,
///   - they were written by different agents, AND
///   - their event-name pair is configured in `policy`.
pub fn detect_conflict_with(
    a: &mem_proto::MemoryEntry,
    b: &mem_proto::MemoryEntry,
    policy: &ConflictPolicy,
) -> bool {
    if a.entity != b.entity || a.workspace_id != b.workspace_id {
        return false;
    }
    if a.agent_id == b.agent_id {
        return false; // same agent updating its own memory is not a conflict
    }
    policy.matches(&a.event, &b.event)
}

/// Trust-ranked auto-resolution: prefer the signal from the higher-trust agent.
/// Returns "SIGNAL_A_PREFERRED", "SIGNAL_B_PREFERRED", or "MANUAL_REVIEW_REQUIRED".
pub fn auto_resolve(a: &mem_proto::MemoryEntry, b: &mem_proto::MemoryEntry) -> &'static str {
    if a.trust_level > b.trust_level {
        "SIGNAL_A_PREFERRED"
    } else if b.trust_level > a.trust_level {
        "SIGNAL_B_PREFERRED"
    } else {
        "MANUAL_REVIEW_REQUIRED"
    }
}

/// Build a ConflictRecord proto from two conflicting entries.
pub fn build_conflict_record(
    a: &mem_proto::MemoryEntry,
    b: &mem_proto::MemoryEntry,
    receipt_id: &ContentHash,
) -> mem_proto::ConflictRecord {
    let auto_res = auto_resolve(a, b);

    mem_proto::ConflictRecord {
        id: format!("conflict_{}", Uuid::now_v7()),
        receipt_id: Some(common_proto::Hash { hex: receipt_id.0.clone() }),
        workspace_id: a.workspace_id.clone(),
        entity: a.entity.clone(),
        signal_a: Some(mem_proto::ConflictSignal {
            memory_id: a.id.clone(),
            agent_id: a.agent_id.clone(),
            trust_level: a.trust_level,
            event: a.event.clone(),
            timestamp: a.timestamp.clone(),
        }),
        signal_b: Some(mem_proto::ConflictSignal {
            memory_id: b.id.clone(),
            agent_id: b.agent_id.clone(),
            trust_level: b.trust_level,
            event: b.event.clone(),
            timestamp: b.timestamp.clone(),
        }),
        status: 1, // PENDING
        auto_resolution: auto_res.to_string(),
        detected_at: Some(prost_types::Timestamp {
            seconds: chrono::Utc::now().timestamp(),
            nanos: 0,
        }),
        resolved_at: None,
        resolution: String::new(),
        walrus_blob: None,
    }
}

/// Store for conflict records.
pub struct ConflictStore {
    records: std::sync::RwLock<std::collections::HashMap<String, mem_proto::ConflictRecord>>,
}

impl Default for ConflictStore {
    fn default() -> Self {
        Self {
            records: std::sync::RwLock::new(std::collections::HashMap::new()),
        }
    }
}

impl ConflictStore {
    pub fn insert(&self, record: mem_proto::ConflictRecord) {
        self.records.write().unwrap().insert(record.id.clone(), record);
    }

    pub fn get(&self, id: &str) -> Option<mem_proto::ConflictRecord> {
        self.records.read().unwrap().get(id).cloned()
    }

    pub fn list_pending(&self, workspace_id: &str) -> Vec<mem_proto::ConflictRecord> {
        self.records
            .read()
            .unwrap()
            .values()
            .filter(|r| {
                r.workspace_id
                    .as_ref()
                    .map(|w| w.value == workspace_id)
                    .unwrap_or(false)
                    && r.status == 1 // PENDING
            })
            .cloned()
            .collect()
    }

    pub fn resolve(&self, id: &str, resolution: &str) {
        if let Some(record) = self.records.write().unwrap().get_mut(id) {
            record.status = 3; // MANUALLY_RESOLVED
            record.resolution = resolution.to_string();
            record.resolved_at = Some(prost_types::Timestamp {
                seconds: chrono::Utc::now().timestamp(),
                nanos: 0,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_entry(event: &str, trust: i32, agent: &str) -> mem_proto::MemoryEntry {
        mem_proto::MemoryEntry {
            id: format!("mem_{}", agent),
            entity: "sarah@email.com".to_string(),
            workspace_id: Some(common_proto::WorkspaceId { value: "ws_test".to_string() }),
            agent_id: Some(common_proto::AgentId { value: agent.to_string() }),
            trust_level: trust,
            event: event.to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn detects_credit_vs_fraud_conflict() {
        let a = make_entry("credit_approved", 2, "support-agent");
        let b = make_entry("flag_suspicious", 3, "fraud-agent");
        assert!(detect_conflict(&a, &b));
    }

    #[test]
    fn no_conflict_same_event() {
        let a = make_entry("credit_offered", 2, "support-agent");
        let b = make_entry("credit_offered", 2, "billing-agent");
        assert!(!detect_conflict(&a, &b));
    }

    #[test]
    fn auto_resolve_prefers_higher_trust() {
        let a = make_entry("credit_approved", 2, "support-agent");
        let b = make_entry("flag_suspicious", 3, "fraud-agent");
        assert_eq!(auto_resolve(&a, &b), "SIGNAL_B_PREFERRED");
    }

    #[test]
    fn default_policy_has_four_pairs() {
        assert_eq!(ConflictPolicy::default().len(), DEFAULT_CONFLICT_PAIRS.len());
    }

    #[test]
    fn custom_pair_fires_with_explicit_policy() {
        // A pair the default policy knows nothing about.
        let policy = ConflictPolicy::empty()
            .with_pair("shipment_delivered", "package_lost");

        let a = make_entry("shipment_delivered", 2, "fulfillment-agent");
        let b = make_entry("package_lost",       3, "support-agent");

        assert!(!detect_conflict(&a, &b),
            "the default policy must NOT fire on a custom pair");
        assert!(detect_conflict_with(&a, &b, &policy),
            "a workspace policy containing the pair MUST fire (in either order)");

        // Reversed order must also fire.
        let a2 = make_entry("package_lost",       3, "support-agent");
        let b2 = make_entry("shipment_delivered", 2, "fulfillment-agent");
        assert!(detect_conflict_with(&a2, &b2, &policy));
    }

    #[test]
    fn empty_policy_never_conflicts() {
        let a = make_entry("credit_approved", 2, "support-agent");
        let b = make_entry("flag_suspicious", 3, "fraud-agent");
        assert!(!detect_conflict_with(&a, &b, &ConflictPolicy::empty()));
    }
}
