use recall_core::ids::{AgentId, ContentHash, WorkspaceId};
use recall_receipt::{action_kind, builder::ReceiptBuilder};
use recall_crypto::RecallKeypair;

/// Run the receipt conformance suite.
/// Returns the computed receipt IDs so they can be embedded in the canonical test vectors.
pub fn run_receipt_conformance() -> Vec<(String, String)> {
    let _kp = RecallKeypair::generate();
    let workspace = WorkspaceId("ws_acme-customer-ops".to_string());
    let passport = ContentHash("pp_support-agent-001".to_string());
    let agent = AgentId("550e8400-e29b-41d4-a716-446655440000".to_string());

    let receipt = ReceiptBuilder::new(
        action_kind::MEMORY_WRITE,
        &workspace,
        &passport,
        &agent,
    )
    .with_cost_annotation("anthropic", "claude-sonnet-4-6", 1240, 88, 2)
    .build_unsigned();

    let id = receipt
        .id
        .as_ref()
        .map(|h| h.hex.clone())
        .unwrap_or_default();

    vec![
        ("vec-001".to_string(), id.clone()),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use recall_receipt::compute_receipt_id;

    #[test]
    fn receipt_id_is_deterministic_for_same_timestamp() {
        // Build two receipts with identical fields (except the timestamp is fixed).
        // They must produce the same ID.
        let _workspace = WorkspaceId("ws_test".to_string());
        let _passport = ContentHash("pp_test".to_string());
        let _agent = AgentId("00000000-0000-0000-0000-000000000001".to_string());

        use recall_proto::{common as common_proto, receipt as receipt_proto};

        let fixed_ts = prost_types::Timestamp { seconds: 1716028320, nanos: 0 };

        let proto_a = receipt_proto::Receipt {
            id: None,
            action_kind: action_kind::MEMORY_WRITE.to_string(),
            workspace_id: Some(common_proto::WorkspaceId { value: "ws_test".to_string() }),
            actor_passport_id: Some(common_proto::Hash { hex: "pp_test".to_string() }),
            actor_agent_id: Some(common_proto::AgentId { value: "00000000-0000-0000-0000-000000000001".to_string() }),
            timestamp: Some(fixed_ts.clone()),
            causal_predecessors: vec![],
            evidence_digest: Some(common_proto::Hash {
                hex: "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855".to_string(),
            }),
            signatures: vec![],
            walrus_blob: None,
            seal_status: 1,
            cost_annotation: None,
            evidence: None,
            deny_reason: String::new(),
            unmet_caveats: vec![],
            reputation_delta: 0.0,
        };

        let proto_b = proto_a.clone();

        let id_a = compute_receipt_id(&proto_a);
        let id_b = compute_receipt_id(&proto_b);

        assert_eq!(id_a.0, id_b.0, "identical inputs must produce identical receipt IDs");
        assert_eq!(id_a.0.len(), 64, "receipt ID must be 64-char hex (SHA-256)");
    }

    #[test]
    fn different_action_kinds_produce_different_ids() {
        use recall_proto::{common as common_proto, receipt as receipt_proto};

        let base = receipt_proto::Receipt {
            id: None,
            action_kind: action_kind::MEMORY_WRITE.to_string(),
            workspace_id: Some(common_proto::WorkspaceId { value: "ws_test".to_string() }),
            actor_passport_id: Some(common_proto::Hash { hex: "pp_test".to_string() }),
            actor_agent_id: Some(common_proto::AgentId { value: "agent-001".to_string() }),
            timestamp: Some(prost_types::Timestamp { seconds: 1716028320, nanos: 0 }),
            causal_predecessors: vec![],
            evidence_digest: Some(common_proto::Hash {
                hex: "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855".to_string(),
            }),
            signatures: vec![],
            walrus_blob: None,
            seal_status: 1,
            cost_annotation: None,
            evidence: None,
            deny_reason: String::new(),
            unmet_caveats: vec![],
            reputation_delta: 0.0,
        };

        let mut read = base.clone();
        read.action_kind = action_kind::MEMORY_READ.to_string();

        let id_write = compute_receipt_id(&base);
        let id_read = compute_receipt_id(&read);

        assert_ne!(id_write.0, id_read.0, "different action kinds must produce different IDs");
    }

    #[test]
    fn signatures_do_not_affect_receipt_id() {
        use recall_proto::{common as common_proto, receipt as receipt_proto};

        let mut proto = receipt_proto::Receipt {
            id: None,
            action_kind: action_kind::MEMORY_WRITE.to_string(),
            workspace_id: Some(common_proto::WorkspaceId { value: "ws_test".to_string() }),
            actor_passport_id: Some(common_proto::Hash { hex: "pp_test".to_string() }),
            actor_agent_id: Some(common_proto::AgentId { value: "agent-001".to_string() }),
            timestamp: Some(prost_types::Timestamp { seconds: 1716028320, nanos: 0 }),
            causal_predecessors: vec![],
            evidence_digest: Some(common_proto::Hash {
                hex: "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855".to_string(),
            }),
            signatures: vec![],
            walrus_blob: None,
            seal_status: 1,
            cost_annotation: None,
            evidence: None,
            deny_reason: String::new(),
            unmet_caveats: vec![],
            reputation_delta: 0.0,
        };

        let id_before = compute_receipt_id(&proto);

        // Add a signature and recompute — ID must be identical.
        proto.signatures.push(common_proto::Signature {
            bytes: vec![1u8; 64],
            role: "ACTOR".to_string(),
            signer_public_key: vec![2u8; 32],
        });

        // compute_receipt_id clears signatures, so the ID must not change.
        // But wait: compute_receipt_id clears signatures in a clone.
        let id_after = compute_receipt_id(&proto);

        assert_eq!(id_before.0, id_after.0, "signatures must not affect receipt ID");
    }

    #[test]
    fn all_receipt_ids_are_64_char_hex() {
        let vectors = super::run_receipt_conformance();
        for (name, id) in vectors {
            assert_eq!(id.len(), 64, "vector {} has wrong length", name);
            assert!(id.chars().all(|c| c.is_ascii_hexdigit()), "vector {} is not hex", name);
        }
    }
}
