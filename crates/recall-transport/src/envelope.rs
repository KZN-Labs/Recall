use recall_core::ids::AgentId;
use recall_proto::controlplane as cp_proto;
use uuid::Uuid;

/// Envelope types used in agent-to-agent messaging.
pub const ENVELOPE_INFORM: &str = "INFORM";
pub const ENVELOPE_HANDOFF: &str = "HANDOFF";
pub const ENVELOPE_ENFORCEMENT_COACH: &str = "ENFORCEMENT_COACH";

/// Build an INFORM envelope (used by enforcement coach stage).
pub fn build_inform_envelope(
    to_agent_id: &AgentId,
    from_agent_id: &AgentId,
    deny_reason: &str,
    guidance: &str,
) -> cp_proto::Envelope {
    cp_proto::Envelope {
        id: Uuid::now_v7().to_string(),
        envelope_type: ENVELOPE_ENFORCEMENT_COACH.to_string(),
        to_agent_id: Some(recall_proto::common::AgentId { value: to_agent_id.0.clone() }),
        from_agent_id: Some(recall_proto::common::AgentId { value: from_agent_id.0.clone() }),
        payload: serde_json::json!({
            "deny_reason": deny_reason,
            "guidance": guidance
        })
        .to_string()
        .into_bytes(),
        sent_at: Some(prost_types::Timestamp {
            seconds: chrono::Utc::now().timestamp(),
            nanos: 0,
        }),
        nonce: Uuid::now_v7().to_string(),
    }
}
