use std::sync::Arc;
use tonic::{Request, Response, Status};

use recall_core::ids::{AgentId, ContentHash, WorkspaceId};
use tracing::{info, warn};
use recall_proto::controlplane::v1::{
    memory_service_server::MemoryService,
    GetConflictsRequest, GetConflictsResponse,
    ListMemoryRequest, ListMemoryResponse,
    ReadMemoryRequest, ReadMemoryResponse,
    ResolveConflictRequest, ResolveConflictResponse,
    WriteMemoryRequest, WriteMemoryResponse,
};
use recall_proto::common as common_proto;
use recall_receipt::{action_kind, builder::ReceiptBuilder};
use sui_governance::WriteAccessParams;

use crate::state::AppState;

pub struct MemoryServiceImpl {
    state: Arc<AppState>,
}

impl MemoryServiceImpl {
    pub fn new(state: Arc<AppState>) -> Self {
        Self { state }
    }
}

#[tonic::async_trait]
impl MemoryService for MemoryServiceImpl {
    async fn write_memory(
        &self,
        request: Request<WriteMemoryRequest>,
    ) -> Result<Response<WriteMemoryResponse>, Status> {
        let req = request.into_inner();
        let entry = req.entry.ok_or_else(|| Status::invalid_argument("missing entry"))?;
        let _cap_id_str = req
            .capability_id
            .as_ref()
            .map(|h| h.hex.clone())
            .ok_or_else(|| Status::invalid_argument("missing capability_id"))?;

        let workspace_id = entry
            .workspace_id
            .as_ref()
            .map(|w| WorkspaceId(w.value.clone()))
            .ok_or_else(|| Status::invalid_argument("missing workspace_id"))?;

        let actor_passport_id = entry
            .passport_id
            .as_ref()
            .map(|h| ContentHash(h.hex.clone()))
            .ok_or_else(|| Status::invalid_argument("missing passport_id"))?;

        let actor_agent_id = entry
            .agent_id
            .as_ref()
            .map(|a| AgentId(a.value.clone()))
            .ok_or_else(|| Status::invalid_argument("missing agent_id"))?;

        // ── Enforcement block check (local fast-path) ─────────────────────────
        // The on-chain record is the authority; this is a local cache check
        // that avoids an RPC round-trip when the control plane already knows
        // the agent is quarantined.
        if self.state.enforcement.is_blocked(&actor_agent_id) {
            return Err(Status::permission_denied("agent_quarantined"));
        }

        // ── Sui Move governance check ─────────────────────────────────────────
        let gov_params = WriteAccessParams {
            passport_id: actor_passport_id.0.clone(),
            workspace_id: workspace_id.0.clone(),
            trust_level: entry.trust_level as u8,
            role: 2, // WRITER — admission service validates role before this point
            enforcement_stage: self
                .state
                .enforcement
                .get_stage(&actor_agent_id)
                .as_str()
                .to_string(),
            entry_tags: entry.tags.clone(),
            entry_scope: entry.scope.clone(),
            has_supervisor_countersign: false,
            estimated_cost_usd_cents: entry
                .cost_annotation
                .as_ref()
                .map(|c| c.usd_cents)
                .unwrap_or(0),
        };

        let decision = self.state.governance.check_write_access(&gov_params).await;

        if !decision.allowed {
            // Record deny for the local enforcement loop.
            let new_stage = self.state.enforcement.record_deny(&actor_agent_id);

            let cp_agent_id = AgentId("00000000-0000-0000-0000-000000000001".to_string());
            let cp_passport = ContentHash("cp_passport".to_string());

            let deny_receipt = ReceiptBuilder::new(
                action_kind::GOVERNANCE_CHECK_DENY,
                &workspace_id,
                &cp_passport,
                &cp_agent_id,
            )
            .with_deny_reason(
                decision.deny_reason.as_deref().unwrap_or("governance deny"),
                vec![],
            )
            .build_unsigned();

            let _ = self.state.receipt_store.append(deny_receipt);

            let _ = new_stage; // escalation handled by enforcement engine

            return Err(Status::permission_denied(
                decision
                    .deny_reason
                    .unwrap_or_else(|| "governance.check.deny".into()),
            ));
        }

        // Emit governance.check.pass receipt.
        {
            let cp_agent_id = AgentId("00000000-0000-0000-0000-000000000001".to_string());
            let cp_passport = ContentHash("cp_passport".to_string());
            let pass_receipt = ReceiptBuilder::new(
                action_kind::GOVERNANCE_CHECK_PASS,
                &workspace_id,
                &cp_passport,
                &cp_agent_id,
            )
            .build_unsigned();
            let _ = self.state.receipt_store.append(pass_receipt);
        }

        // ── Walrus write — required, must succeed before we persist anything ──
        // Every memory write MUST land on Walrus. If Walrus is misconfigured or
        // unreachable, the write is rejected — we never silently degrade to
        // in-process-only storage.
        let walrus_blob_id: String = match self.state.walrus.as_ref() {
            Some(walrus) => match walrus.write_memory_entry(&entry).await {
                Ok(blob) => {
                    info!("Walrus blob stored: {}", blob.0);
                    blob.0
                }
                Err(e) => {
                    warn!("Walrus write failed: {e}");
                    return Err(Status::unavailable(format!(
                        "Walrus write failed: {e} — memory not stored on chain"
                    )));
                }
            },
            None => {
                return Err(Status::failed_precondition(
                    "Walrus backend not configured — set MEMWAL_PRIVATE_KEY and MEMWAL_ACCOUNT_ID",
                ));
            }
        };

        // ── Store the memory entry with the Walrus blob ref attached ─────────
        let memory_id = entry.id.clone();
        let mut entry = entry;
        entry.walrus_blob = Some(common_proto::WalrusBlobRef {
            blob_id: walrus_blob_id.clone(),
        });
        self.state.memory_store.insert(entry.clone());

        // Auto-register workspace so HTTP list reflects gRPC writes too.
        self.state.workspace_store.ensure_exists(&workspace_id.0);

        // Detect conflicts against existing entries for the same entity.
        let existing = self
            .state
            .memory_store
            .get_by_entity(&workspace_id.0, &entry.entity);

        for existing_entry in &existing {
            if existing_entry.id != entry.id
                && recall_conflict::detect_conflict(existing_entry, &entry)
            {
                let conflict_receipt_id = ContentHash(recall_crypto::sha256_hex(
                    format!("{}:{}", existing_entry.id, entry.id).as_bytes(),
                ));
                let conflict = recall_conflict::build_conflict_record(
                    existing_entry,
                    &entry,
                    &conflict_receipt_id,
                );
                self.state.conflict_store.insert(conflict);
            }
        }

        // ── Emit memory.write receipt ─────────────────────────────────────────
        let receipt = ReceiptBuilder::new(
            action_kind::MEMORY_WRITE,
            &workspace_id,
            &actor_passport_id,
            &actor_agent_id,
        )
        .with_cost_annotation(
            entry.model_provider.as_str(),
            entry.model_name.as_str(),
            entry.cost_annotation.as_ref().map(|c| c.tokens_in).unwrap_or(0),
            entry.cost_annotation.as_ref().map(|c| c.tokens_out).unwrap_or(0),
            entry.cost_annotation.as_ref().map(|c| c.usd_cents).unwrap_or(0),
        )
        .build(&self.state.cp_keypair);

        let receipt_id = receipt
            .id
            .as_ref()
            .map(|h| h.hex.clone())
            .unwrap_or_default();

        let _ = self.state.receipt_store.append(receipt);

        Ok(Response::new(WriteMemoryResponse {
            memory_id,
            receipt_id: Some(common_proto::Hash { hex: receipt_id }),
            walrus_blob: Some(common_proto::WalrusBlobRef { blob_id: walrus_blob_id }),
        }))
    }

    async fn read_memory(
        &self,
        request: Request<ReadMemoryRequest>,
    ) -> Result<Response<ReadMemoryResponse>, Status> {
        let req = request.into_inner();
        let workspace_id = req
            .workspace_id
            .as_ref()
            .map(|w| w.value.clone())
            .ok_or_else(|| Status::invalid_argument("missing workspace_id"))?;

        let entries = self
            .state
            .memory_store
            .get_by_entity(&workspace_id, &req.entity);

        let cp_agent = AgentId("00000000-0000-0000-0000-000000000001".to_string());
        let cp_passport = ContentHash("cp_passport".to_string());
        let ws = WorkspaceId(workspace_id);

        let receipt = ReceiptBuilder::new(
            action_kind::MEMORY_READ,
            &ws,
            &cp_passport,
            &cp_agent,
        )
        .build(&self.state.cp_keypair);

        let receipt_id = receipt
            .id
            .as_ref()
            .map(|h| h.hex.clone())
            .unwrap_or_default();
        let _ = self.state.receipt_store.append(receipt);

        Ok(Response::new(ReadMemoryResponse {
            entries,
            receipt_id: Some(common_proto::Hash { hex: receipt_id }),
        }))
    }

    async fn list_memory(
        &self,
        _request: Request<ListMemoryRequest>,
    ) -> Result<Response<ListMemoryResponse>, Status> {
        Ok(Response::new(ListMemoryResponse {
            entries: vec![],
            next_page_token: String::new(),
        }))
    }

    async fn get_conflicts(
        &self,
        request: Request<GetConflictsRequest>,
    ) -> Result<Response<GetConflictsResponse>, Status> {
        let req = request.into_inner();
        let workspace_id = req
            .workspace_id
            .as_ref()
            .map(|w| w.value.clone())
            .ok_or_else(|| Status::invalid_argument("missing workspace_id"))?;

        let conflicts = self.state.conflict_store.list_pending(&workspace_id);
        Ok(Response::new(GetConflictsResponse { conflicts }))
    }

    async fn resolve_conflict(
        &self,
        request: Request<ResolveConflictRequest>,
    ) -> Result<Response<ResolveConflictResponse>, Status> {
        let req = request.into_inner();
        self.state
            .conflict_store
            .resolve(&req.conflict_id, &req.resolution);

        let cp_agent = AgentId("00000000-0000-0000-0000-000000000001".to_string());
        let cp_passport = ContentHash("cp_passport".to_string());
        let ws = WorkspaceId("global".to_string());

        let receipt = ReceiptBuilder::new(
            action_kind::MEMORY_CONFLICT_RESOLVED,
            &ws,
            &cp_passport,
            &cp_agent,
        )
        .build(&self.state.cp_keypair);

        let receipt_id = receipt
            .id
            .as_ref()
            .map(|h| h.hex.clone())
            .unwrap_or_default();
        let _ = self.state.receipt_store.append(receipt);

        Ok(Response::new(ResolveConflictResponse {
            receipt_id: Some(common_proto::Hash { hex: receipt_id }),
        }))
    }
}
