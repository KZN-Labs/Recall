use recall_core::ids::ContentHash;
use recall_proto::{common as common_proto, memory as mem_proto};
use uuid::Uuid;

/// Events that conflict when written by different agents about the same entity.
const CONFLICTING_EVENT_PAIRS: &[(&str, &str)] = &[
    ("credit_approved", "flag_suspicious"),
    ("credit_offered", "flag_suspicious"),
    ("account_verified", "flag_suspicious"),
    ("refund_approved", "refund_denied"),
];

/// Check whether two new memory entries conflict.
pub fn detect_conflict(
    a: &mem_proto::MemoryEntry,
    b: &mem_proto::MemoryEntry,
) -> bool {
    if a.entity != b.entity || a.workspace_id != b.workspace_id {
        return false;
    }
    if a.agent_id == b.agent_id {
        return false; // same agent updating its own memory is not a conflict
    }

    for (event_a, event_b) in CONFLICTING_EVENT_PAIRS {
        if (a.event == *event_a && b.event == *event_b)
            || (a.event == *event_b && b.event == *event_a)
        {
            return true;
        }
    }
    false
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
}
