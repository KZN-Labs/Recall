//! Sui-based governance client for RECALL.
//!
//! The `workspace_governance` Move module on Sui owns all write-access rules.
//! This crate provides a `SuiGovernanceClient` that the control plane uses to
//! evaluate governance decisions before any memory write is processed.
//!
//! ## Modes
//!
//! - **Offline** (`SuiGovernanceClient::offline()`): applies the same rules
//!   locally without a Sui node — used in tests and single-node deployments.
//! - **On-chain** (`SuiGovernanceClient::new(rpc_url, ...)`): calls the Sui RPC
//!   `sui_devInspectTransactionBlock` endpoint to execute `check_write_access`
//!   as a dry-run read-only transaction.

use serde::{Deserialize, Serialize};
use tracing::{debug, warn};

// ── Decision type ─────────────────────────────────────────────────────────────

/// Result of a governance check.
#[derive(Debug, Clone)]
pub struct GovernanceDecision {
    pub allowed: bool,
    pub deny_reason: Option<String>,
}

impl GovernanceDecision {
    pub fn allow() -> Self {
        Self { allowed: true, deny_reason: None }
    }
    pub fn deny(reason: impl Into<String>) -> Self {
        Self { allowed: false, deny_reason: Some(reason.into()) }
    }
}

// ── Input parameters ─────────────────────────────────────────────────────────

/// All fields needed for a `check_write_access` governance call.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WriteAccessParams {
    pub passport_id: String,
    pub workspace_id: String,
    /// 1 = BASIC, 2 = STANDARD, 3 = HIGH
    pub trust_level: u8,
    /// 1 = READER, 2 = WRITER, 3 = SUPERVISOR, 4 = ADMIN
    pub role: u8,
    /// Current enforcement stage string: "NONE", "DETECT", "COACH", "QUARANTINE", "EVICT"
    pub enforcement_stage: String,
    pub entry_tags: Vec<String>,
    pub entry_scope: String,
    pub has_supervisor_countersign: bool,
    pub estimated_cost_usd_cents: i64,
}

// ── Client ────────────────────────────────────────────────────────────────────

/// Governance client.  Use `offline()` for tests; `new(...)` for production.
pub struct SuiGovernanceClient {
    rpc_url: Option<String>,
    /// Object ID of the `WorkspacePolicy` shared object on Sui.
    policy_object_id: Option<String>,
    /// Object ID of the `AgentEnforcementRecord` shared object on Sui.
    record_object_id: Option<String>,
}

impl SuiGovernanceClient {
    /// Build a client that evaluates rules against a live Sui node.
    pub fn new(
        rpc_url: impl Into<String>,
        policy_object_id: impl Into<String>,
        record_object_id: impl Into<String>,
    ) -> Self {
        Self {
            rpc_url: Some(rpc_url.into()),
            policy_object_id: Some(policy_object_id.into()),
            record_object_id: Some(record_object_id.into()),
        }
    }

    /// Build an offline client — applies the same rules locally without a Sui
    /// node. Intended for tests and single-node deployments where the control
    /// plane already tracks enforcement state.
    pub fn offline() -> Self {
        Self {
            rpc_url: None,
            policy_object_id: None,
            record_object_id: None,
        }
    }

    /// Evaluate `workspace_governance::governance::check_write_access` for a
    /// proposed memory write.
    ///
    /// Production path: calls Sui `devInspectTransactionBlock` RPC which
    /// executes the Move view function without committing a transaction.
    ///
    /// Offline path: evaluates the identical rules in Rust, mirroring the Move
    /// logic exactly so the two implementations stay in sync.
    pub async fn check_write_access(&self, params: &WriteAccessParams) -> GovernanceDecision {
        if self.rpc_url.is_some() {
            self.check_on_chain(params).await
        } else {
            self.check_offline(params)
        }
    }

    // ── On-chain path ─────────────────────────────────────────────────────────

    async fn check_on_chain(&self, params: &WriteAccessParams) -> GovernanceDecision {
        let rpc = self.rpc_url.as_deref().unwrap();

        // Build a devInspect call to `recall_workspace_governance::governance::check_write_access`.
        // The Sui JSON-RPC devInspectTransactionBlock executes view functions
        // without gas or state mutation.
        let role_byte = params.role;
        let trust_byte = params.trust_level;
        let scope_bytes = params.entry_scope.as_bytes().to_vec();
        let tags_bytes: Vec<Vec<u8>> = params
            .entry_tags
            .iter()
            .map(|t| t.as_bytes().to_vec())
            .collect();

        let payload = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "sui_devInspectTransactionBlock",
            "params": [
                "0x0", // sender (any address for read-only)
                {
                    "kind": "ProgrammableTransaction",
                    "inputs": [
                        { "type": "object", "objectId": self.policy_object_id },
                        { "type": "object", "objectId": self.record_object_id },
                        { "type": "pure", "value": trust_byte },
                        { "type": "pure", "value": role_byte },
                        { "type": "pure", "value": tags_bytes },
                        { "type": "pure", "value": scope_bytes },
                        { "type": "pure", "value": params.has_supervisor_countersign },
                    ],
                    "transactions": [{
                        "MoveCall": {
                            "package": "recall_workspace_governance",
                            "module": "governance",
                            "function": "check_write_access",
                            "arguments": ["Input(0)", "Input(1)", "Input(2)", "Input(3)", "Input(4)", "Input(5)", "Input(6)"]
                        }
                    }]
                },
                null,
                null
            ]
        });

        let client = reqwest::Client::new();
        let resp = match client.post(rpc).json(&payload).send().await {
            Ok(r) => r,
            Err(e) => {
                warn!("Sui governance RPC call failed: {}; defaulting to offline check", e);
                return self.check_offline(params);
            }
        };

        let body: serde_json::Value = match resp.json().await {
            Ok(b) => b,
            Err(e) => {
                warn!("Sui governance RPC parse error: {}; defaulting to offline check", e);
                return self.check_offline(params);
            }
        };

        // Parse the bool return value from the devInspect response.
        // Response path: body["result"]["results"][0]["returnValues"][0][0] = [1] (true) or [0] (false)
        if let Some(val) = body
            .pointer("/result/results/0/returnValues/0/0/0")
            .and_then(|v| v.as_u64())
        {
            if val == 1 {
                debug!("Sui governance check PASS for {}", params.passport_id);
                GovernanceDecision::allow()
            } else {
                debug!("Sui governance check DENY for {}", params.passport_id);
                GovernanceDecision::deny("governance.check.deny: on-chain rule violation")
            }
        } else {
            warn!("Sui governance RPC unexpected response shape; defaulting to offline check");
            self.check_offline(params)
        }
    }

    // ── Offline path — mirrors governance.move exactly ────────────────────────

    fn check_offline(&self, params: &WriteAccessParams) -> GovernanceDecision {
        // Rule 1: quarantine / evict.
        let stage = params.enforcement_stage.as_str();
        if stage == "QUARANTINE" || stage == "EVICT" {
            return GovernanceDecision::deny("agent_quarantined");
        }

        // Rule 2: role must be WRITER (2) or above.
        if params.role < 2 {
            return GovernanceDecision::deny("insufficient_role: must be WRITER or above");
        }

        // Rule 3: trust level must be at least 1.
        // (Workspace minimum is enforced on-chain; offline default is 1.)
        if params.trust_level < 1 {
            return GovernanceDecision::deny("insufficient_trust_level");
        }

        // Rule 4: PII must never reach external scope.
        let is_external = params.entry_scope == "external";
        let has_pii = params.entry_tags.iter().any(|t| t == "pii");
        if is_external && has_pii {
            return GovernanceDecision::deny(
                "pii_external_forbidden: PII-tagged entries cannot be written to external scope",
            );
        }

        // Rule 5: supervisor countersign required for trust_level < 2 agents.
        let needs_supervisor = params.trust_level < 2
            || params.entry_tags.iter().any(|t| t == "high_value" || t == "clinical");
        if needs_supervisor && !params.has_supervisor_countersign {
            return GovernanceDecision::deny(
                "supervisor_countersign_required: entry requires supervisor approval",
            );
        }

        GovernanceDecision::allow()
    }
}

// ── Default: offline client ───────────────────────────────────────────────────

impl Default for SuiGovernanceClient {
    fn default() -> Self {
        Self::offline()
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn base_params() -> WriteAccessParams {
        WriteAccessParams {
            passport_id: "pp_test".into(),
            workspace_id: "ws_test".into(),
            trust_level: 2,
            role: 2, // WRITER
            enforcement_stage: "NONE".into(),
            entry_tags: vec!["customer".into()],
            entry_scope: "internal".into(),
            has_supervisor_countersign: false,
            estimated_cost_usd_cents: 2,
        }
    }

    #[tokio::test]
    async fn normal_write_is_allowed() {
        let client = SuiGovernanceClient::offline();
        let result = client.check_write_access(&base_params()).await;
        assert!(result.allowed, "normal write should be allowed");
    }

    #[tokio::test]
    async fn quarantined_agent_is_blocked() {
        let client = SuiGovernanceClient::offline();
        let mut params = base_params();
        params.enforcement_stage = "QUARANTINE".into();
        let result = client.check_write_access(&params).await;
        assert!(!result.allowed);
        assert!(result.deny_reason.as_deref().unwrap_or("").contains("quarantined"));
    }

    #[tokio::test]
    async fn evicted_agent_is_blocked() {
        let client = SuiGovernanceClient::offline();
        let mut params = base_params();
        params.enforcement_stage = "EVICT".into();
        let result = client.check_write_access(&params).await;
        assert!(!result.allowed);
    }

    #[tokio::test]
    async fn reader_role_cannot_write() {
        let client = SuiGovernanceClient::offline();
        let mut params = base_params();
        params.role = 1; // READER
        let result = client.check_write_access(&params).await;
        assert!(!result.allowed);
        assert!(result.deny_reason.as_deref().unwrap_or("").contains("role"));
    }

    #[tokio::test]
    async fn pii_to_external_scope_is_denied() {
        let client = SuiGovernanceClient::offline();
        let mut params = base_params();
        params.entry_tags = vec!["pii".into()];
        params.entry_scope = "external".into();
        let result = client.check_write_access(&params).await;
        assert!(!result.allowed);
        assert!(result.deny_reason.as_deref().unwrap_or("").contains("pii"));
    }

    #[tokio::test]
    async fn pii_to_internal_scope_is_allowed() {
        let client = SuiGovernanceClient::offline();
        let mut params = base_params();
        params.entry_tags = vec!["pii".into()];
        params.entry_scope = "internal".into();
        let result = client.check_write_access(&params).await;
        assert!(result.allowed);
    }

    #[tokio::test]
    async fn low_trust_requires_supervisor() {
        let client = SuiGovernanceClient::offline();
        let mut params = base_params();
        params.trust_level = 1;
        params.has_supervisor_countersign = false;
        let result = client.check_write_access(&params).await;
        assert!(!result.allowed);
        assert!(result.deny_reason.as_deref().unwrap_or("").contains("supervisor"));
    }

    #[tokio::test]
    async fn low_trust_with_supervisor_is_allowed() {
        let client = SuiGovernanceClient::offline();
        let mut params = base_params();
        params.trust_level = 1;
        params.has_supervisor_countersign = true;
        let result = client.check_write_access(&params).await;
        assert!(result.allowed);
    }
}
