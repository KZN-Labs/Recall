use std::sync::Arc;
use tonic::{Request, Response, Status};

use recall_core::ids::{AgentId, ContentHash, WorkspaceId};
use recall_proto::common as common_proto;
use recall_proto::controlplane::v1::{
    capability_service_server::CapabilityService,
    CheckCapabilityRequest, CheckCapabilityResponse,
    GetCapabilityRequest,
    IssueCapabilityRequest, IssueCapabilityResponse,
    RevokeCapabilityRequest, RevokeCapabilityResponse,
};
use recall_proto::capability as cap_proto;
use recall_receipt::{action_kind, builder::ReceiptBuilder};

use crate::state::AppState;

pub struct CapabilityServiceImpl {
    state: Arc<AppState>,
}

impl CapabilityServiceImpl {
    pub fn new(state: Arc<AppState>) -> Self {
        Self { state }
    }
}

#[tonic::async_trait]
impl CapabilityService for CapabilityServiceImpl {
    async fn issue_capability(
        &self,
        request: Request<IssueCapabilityRequest>,
    ) -> Result<Response<IssueCapabilityResponse>, Status> {
        let req = request.into_inner();
        let scope = req.scope.unwrap_or_default();
        let valid_until = req.valid_until.ok_or_else(|| Status::invalid_argument("missing valid_until"))?;

        let issuer_id = req
            .issuer_passport_id
            .as_ref()
            .map(|h| ContentHash(h.hex.clone()))
            .ok_or_else(|| Status::invalid_argument("missing issuer_passport_id"))?;

        let holder_id = req
            .holder_passport_id
            .as_ref()
            .map(|h| ContentHash(h.hex.clone()))
            .ok_or_else(|| Status::invalid_argument("missing holder_passport_id"))?;

        let workspace_id = WorkspaceId("global".to_string());

        let cap = recall_capability::issue_capability(
            &issuer_id,
            &holder_id,
            &workspace_id,
            scope,
            req.caveats,
            valid_until,
            &self.state.cp_keypair,
        )
        .map_err(|e| Status::internal(e.to_string()))?;

        let _cap_id = cap.id.as_ref().map(|h| h.hex.clone()).unwrap_or_default();
        self.state.capability_store.insert(cap.clone());

        let cp_agent = AgentId("00000000-0000-0000-0000-000000000001".to_string());
        let cp_passport = ContentHash("cp_passport".to_string());

        let receipt = ReceiptBuilder::new(
            action_kind::CAPABILITY_ISSUE,
            &workspace_id,
            &cp_passport,
            &cp_agent,
        )
        .build(&self.state.cp_keypair);

        let receipt_id = receipt.id.as_ref().map(|h| h.hex.clone()).unwrap_or_default();
        let _ = self.state.receipt_store.append(receipt);

        Ok(Response::new(IssueCapabilityResponse {
            capability: Some(cap),
            receipt_id: Some(common_proto::Hash { hex: receipt_id }),
        }))
    }

    async fn attenuate_capability(
        &self,
        request: Request<cap_proto::AttenuateRequest>,
    ) -> Result<Response<IssueCapabilityResponse>, Status> {
        let req = request.into_inner();
        let parent_id = req
            .parent_id
            .as_ref()
            .map(|h| ContentHash(h.hex.clone()))
            .ok_or_else(|| Status::invalid_argument("missing parent_id"))?;

        let parent = self.state.capability_store.get(&parent_id)
            .ok_or_else(|| Status::not_found("parent capability not found"))?;

        let new_holder = req
            .new_holder_passport_id
            .as_ref()
            .map(|h| ContentHash(h.hex.clone()))
            .ok_or_else(|| Status::invalid_argument("missing new_holder_passport_id"))?;

        let new_valid_until = req.new_valid_until.ok_or_else(|| Status::invalid_argument("missing new_valid_until"))?;

        let child = recall_capability::attenuate(
            &parent,
            &new_holder,
            req.new_scope.unwrap_or_default(),
            req.additional_caveats,
            new_valid_until,
            &self.state.cp_keypair,
        )
        .map_err(|e| Status::invalid_argument(e.to_string()))?;

        self.state.capability_store.insert(child.clone());

        let cp_agent = AgentId("00000000-0000-0000-0000-000000000001".to_string());
        let cp_passport = ContentHash("cp_passport".to_string());
        let ws = WorkspaceId("global".to_string());

        let receipt = ReceiptBuilder::new(
            action_kind::CAPABILITY_ATTENUATE,
            &ws,
            &cp_passport,
            &cp_agent,
        )
        .build(&self.state.cp_keypair);

        let receipt_id = receipt.id.as_ref().map(|h| h.hex.clone()).unwrap_or_default();
        let _ = self.state.receipt_store.append(receipt);

        Ok(Response::new(IssueCapabilityResponse {
            capability: Some(child),
            receipt_id: Some(common_proto::Hash { hex: receipt_id }),
        }))
    }

    async fn revoke_capability(
        &self,
        request: Request<RevokeCapabilityRequest>,
    ) -> Result<Response<RevokeCapabilityResponse>, Status> {
        let req = request.into_inner();
        let cap_id = req
            .capability_id
            .as_ref()
            .map(|h| ContentHash(h.hex.clone()))
            .ok_or_else(|| Status::invalid_argument("missing capability_id"))?;

        self.state.capability_store.revoke(&cap_id);

        let cp_agent = AgentId("00000000-0000-0000-0000-000000000001".to_string());
        let cp_passport = ContentHash("cp_passport".to_string());
        let ws = WorkspaceId("global".to_string());

        let receipt = ReceiptBuilder::new(
            action_kind::CAPABILITY_REVOKE,
            &ws,
            &cp_passport,
            &cp_agent,
        )
        .build(&self.state.cp_keypair);

        let receipt_id = receipt.id.as_ref().map(|h| h.hex.clone()).unwrap_or_default();
        let _ = self.state.receipt_store.append(receipt);

        Ok(Response::new(RevokeCapabilityResponse {
            receipt_id: Some(common_proto::Hash { hex: receipt_id }),
        }))
    }

    async fn check_capability(
        &self,
        request: Request<CheckCapabilityRequest>,
    ) -> Result<Response<CheckCapabilityResponse>, Status> {
        let req = request.into_inner();
        let cap_id = req
            .capability_id
            .as_ref()
            .map(|h| ContentHash(h.hex.clone()))
            .ok_or_else(|| Status::invalid_argument("missing capability_id"))?;

        let caveat_ctx = recall_capability::caveat::CaveatContext {
            actions_in_window: 0,
            entry_tags: vec![],
            entry_scope: "internal",
            constitution_version: "1.0.0",
            has_supervisor_countersign: false,
        };

        let result = recall_capability::check::check_capability(
            &self.state.capability_store,
            &cap_id,
            &req.action_kind,
            &req.entity,
            caveat_ctx,
        );

        let action_kind_str = if result.allowed {
            action_kind::CAPABILITY_CHECK_PASS
        } else {
            action_kind::CAPABILITY_CHECK_DENY
        };

        let cp_agent = AgentId("00000000-0000-0000-0000-000000000001".to_string());
        let cp_passport = ContentHash("cp_passport".to_string());
        let ws = WorkspaceId("global".to_string());

        let receipt = ReceiptBuilder::new(action_kind_str, &ws, &cp_passport, &cp_agent)
            .with_deny_reason(
                result.deny_reason.as_deref().unwrap_or(""),
                result.unmet_caveats.clone(),
            )
            .build(&self.state.cp_keypair);

        let receipt_id = receipt.id.as_ref().map(|h| h.hex.clone()).unwrap_or_default();
        let _ = self.state.receipt_store.append(receipt);

        Ok(Response::new(CheckCapabilityResponse {
            allowed: result.allowed,
            deny_reason: result.deny_reason.unwrap_or_default(),
            unmet_caveats: result.unmet_caveats,
            receipt_id: Some(common_proto::Hash { hex: receipt_id }),
        }))
    }

    async fn get_capability(
        &self,
        request: Request<GetCapabilityRequest>,
    ) -> Result<Response<cap_proto::Capability>, Status> {
        let req = request.into_inner();
        let cap_id = req
            .capability_id
            .as_ref()
            .map(|h| ContentHash(h.hex.clone()))
            .ok_or_else(|| Status::invalid_argument("missing capability_id"))?;

        self.state
            .capability_store
            .get(&cap_id)
            .map(Response::new)
            .ok_or_else(|| Status::not_found("capability not found"))
    }
}
