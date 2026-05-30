use recall_core::ids::{AgentId, WorkspaceId};
use recall_crypto::RecallKeypair;
use recall_proto::{common as common_proto, memory as mem_proto};
use prost::Message;
use uuid::Uuid;

/// Build and sign a handoff capsule (signed by the control plane).
pub fn build_capsule(
    from_agent_id: &AgentId,
    to_agent_id: &AgentId,
    entity: &str,
    workspace_id: &WorkspaceId,
    memory_snapshot: Vec<mem_proto::MemoryEntry>,
    cp_keypair: &RecallKeypair,
) -> mem_proto::HandoffCapsule {
    let capsule_id = format!("capsule_{}", Uuid::now_v7());

    let mut capsule = mem_proto::HandoffCapsule {
        id: capsule_id,
        from_agent_id: Some(common_proto::AgentId { value: from_agent_id.0.clone() }),
        to_agent_id: Some(common_proto::AgentId { value: to_agent_id.0.clone() }),
        entity: entity.to_string(),
        workspace_id: Some(common_proto::WorkspaceId { value: workspace_id.0.clone() }),
        memory_snapshot,
        created_at: Some(prost_types::Timestamp {
            seconds: chrono::Utc::now().timestamp(),
            nanos: 0,
        }),
        signature: None,
        walrus_blob: None,
    };

    // Sign with control-plane key.
    let mut canonical = capsule.clone();
    canonical.signature = None;
    let mut buf = Vec::new();
    canonical.encode(&mut buf).expect("prost encode");
    let sig_bytes = cp_keypair.sign(&buf).to_bytes();
    capsule.signature = Some(common_proto::Signature {
        bytes: sig_bytes.to_vec(),
        role: "CONTROL_PLANE".to_string(),
        signer_public_key: cp_keypair.public_key().to_bytes().to_vec(),
    });

    capsule
}
