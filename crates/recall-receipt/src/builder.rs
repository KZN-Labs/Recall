use chrono::Utc;
use prost_types::Timestamp;
use recall_core::ids::{AgentId, ContentHash, WalrusBlobId, WorkspaceId};
use recall_proto::{common as common_proto, receipt as receipt_proto};
use recall_crypto::{sha256_hex, RecallKeypair};

use crate::compute_receipt_id;

/// Fluent builder for receipt proto messages.
pub struct ReceiptBuilder {
    inner: receipt_proto::Receipt,
}

impl ReceiptBuilder {
    pub fn new(action_kind: &str, workspace_id: &WorkspaceId, actor_passport_id: &ContentHash, actor_agent_id: &AgentId) -> Self {
        let now = Utc::now();
        let inner = receipt_proto::Receipt {
            id: None,
            action_kind: action_kind.to_string(),
            workspace_id: Some(common_proto::WorkspaceId { value: workspace_id.0.clone() }),
            actor_passport_id: Some(common_proto::Hash { hex: actor_passport_id.0.clone() }),
            actor_agent_id: Some(common_proto::AgentId { value: actor_agent_id.0.clone() }),
            timestamp: Some(Timestamp { seconds: now.timestamp(), nanos: 0 }),
            causal_predecessors: vec![],
            evidence_digest: Some(common_proto::Hash { hex: sha256_hex(b"") }),
            signatures: vec![],
            walrus_blob: None,
            seal_status: 1, // UNSEALED
            cost_annotation: None,
            evidence: None,
            deny_reason: String::new(),
            unmet_caveats: vec![],
            reputation_delta: 0.0,
        };
        Self { inner }
    }

    pub fn with_causal_predecessor(mut self, predecessor_id: &ContentHash) -> Self {
        self.inner.causal_predecessors.push(common_proto::CausalRef {
            receipt_id: Some(common_proto::Hash { hex: predecessor_id.0.clone() }),
        });
        self
    }

    pub fn with_evidence_digest(mut self, digest: &ContentHash) -> Self {
        self.inner.evidence_digest = Some(common_proto::Hash { hex: digest.0.clone() });
        self
    }

    pub fn with_cost_annotation(mut self, provider: &str, model: &str, tokens_in: i64, tokens_out: i64, usd_cents: i64) -> Self {
        self.inner.cost_annotation = Some(common_proto::CostAnnotation {
            model_provider: provider.to_string(),
            model_name: model.to_string(),
            tokens_in,
            tokens_out,
            usd_cents,
        });
        self
    }

    pub fn with_deny_reason(mut self, reason: &str, unmet: Vec<String>) -> Self {
        self.inner.deny_reason = reason.to_string();
        self.inner.unmet_caveats = unmet;
        self
    }

    pub fn with_reputation_delta(mut self, delta: f64) -> Self {
        self.inner.reputation_delta = delta;
        self
    }

    pub fn with_walrus_blob(mut self, blob_id: &WalrusBlobId) -> Self {
        self.inner.walrus_blob = Some(common_proto::WalrusBlobRef { blob_id: blob_id.0.clone() });
        self
    }

    /// Finalize: compute receipt ID, sign with actor keypair.
    pub fn build(mut self, actor_keypair: &RecallKeypair) -> receipt_proto::Receipt {
        // Compute ID (signatures must be cleared, which they are).
        let id = compute_receipt_id(&self.inner);
        self.inner.id = Some(common_proto::Hash { hex: id.0 });

        // Sign with ACTOR key.
        crate::sign_receipt(self.inner, actor_keypair, "ACTOR")
    }

    /// Finalize without signing (for control-plane receipts that sign separately).
    pub fn build_unsigned(mut self) -> receipt_proto::Receipt {
        let id = compute_receipt_id(&self.inner);
        self.inner.id = Some(common_proto::Hash { hex: id.0 });
        self.inner
    }
}
