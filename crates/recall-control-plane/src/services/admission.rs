use std::sync::Arc;
use tonic::{Request, Response, Status};

use recall_core::ids::{AgentId, ContentHash};
use recall_passport::Passport;
use recall_proto::controlplane::v1::{
    admission_service_server::AdmissionService,
    GetPassportRequest,
    RegisterAgentRequest, RegisterAgentResponse,
    RevokeAgentResponse, RotateKeyResponse,
};
use recall_proto::common as common_proto;
use recall_proto::passport as passport_proto;
use recall_receipt::{action_kind, builder::ReceiptBuilder};
use recall_core::ids::WorkspaceId;

use crate::state::AppState;

pub struct AdmissionServiceImpl {
    state: Arc<AppState>,
}

impl AdmissionServiceImpl {
    pub fn new(state: Arc<AppState>) -> Self {
        Self { state }
    }
}

#[tonic::async_trait]
impl AdmissionService for AdmissionServiceImpl {
    async fn register_agent(
        &self,
        request: Request<RegisterAgentRequest>,
    ) -> Result<Response<RegisterAgentResponse>, Status> {
        let req = request.into_inner();
        let proto = req.passport.ok_or_else(|| Status::invalid_argument("missing passport"))?;

        let passport_id = Passport::verify(&proto)
            .map_err(|e| Status::invalid_argument(format!("invalid passport: {}", e)))?;

        let ws_id = proto
            .workspace_id
            .as_ref()
            .map(|w| WorkspaceId(w.value.clone()))
            .ok_or_else(|| Status::invalid_argument("missing workspace_id"))?;

        let agent_id = proto
            .agent_id
            .as_ref()
            .map(|a| AgentId(a.value.clone()))
            .ok_or_else(|| Status::invalid_argument("missing agent_id"))?;

        // Store the passport.
        let passport = recall_passport::Passport {
            passport_id: passport_id.clone(),
            agent_id: agent_id.clone(),
            workspace_id: ws_id.clone(),
            trust_level: recall_core::types::TrustLevel::from_i32(proto.trust_level)
                .unwrap_or(recall_core::types::TrustLevel::Low),
            role: recall_core::types::AgentRole::Writer,
            model_provider: proto.model_provider.clone(),
            model_name: proto.model_name.clone(),
            expires_at: proto.expires_at.map(|ts| {
                chrono::DateTime::from_timestamp(ts.seconds, 0)
                    .unwrap_or_default()
                    .into()
            }),
            public_key_bytes: proto.agent_public_key.clone(),
            state: recall_passport::PassportState::Active,
        };
        self.state.passport_store.register(passport).map_err(|e| Status::internal(e.to_string()))?;

        // Emit agent.register receipt.
        let cp_agent = AgentId("00000000-0000-0000-0000-000000000001".to_string());
        let cp_passport = ContentHash("cp_passport".to_string());

        let receipt = ReceiptBuilder::new(
            action_kind::AGENT_REGISTER,
            &ws_id,
            &cp_passport,
            &cp_agent,
        )
        .build(&self.state.cp_keypair);

        let receipt_id = receipt.id.as_ref().map(|h| h.hex.clone()).unwrap_or_default();
        let _ = self.state.receipt_store.append(receipt);

        Ok(Response::new(RegisterAgentResponse {
            passport_id: Some(common_proto::Hash { hex: passport_id.0 }),
            receipt_id: Some(common_proto::Hash { hex: receipt_id }),
        }))
    }

    async fn revoke_agent(
        &self,
        request: Request<passport_proto::SelfRevokeRequest>,
    ) -> Result<Response<RevokeAgentResponse>, Status> {
        let req = request.into_inner();
        let passport_id = req
            .passport_id
            .as_ref()
            .map(|h| ContentHash(h.hex.clone()))
            .ok_or_else(|| Status::invalid_argument("missing passport_id"))?;

        self.state
            .passport_store
            .revoke(&passport_id, &req.reason)
            .map_err(|e| Status::not_found(e.to_string()))?;

        let cp_agent = AgentId("00000000-0000-0000-0000-000000000001".to_string());
        let cp_passport = ContentHash("cp_passport".to_string());
        let ws = WorkspaceId("global".to_string());

        let receipt = ReceiptBuilder::new(
            action_kind::AGENT_REVOKE,
            &ws,
            &cp_passport,
            &cp_agent,
        )
        .build(&self.state.cp_keypair);

        let receipt_id = receipt.id.as_ref().map(|h| h.hex.clone()).unwrap_or_default();
        let _ = self.state.receipt_store.append(receipt);

        Ok(Response::new(RevokeAgentResponse {
            receipt_id: Some(common_proto::Hash { hex: receipt_id }),
        }))
    }

    async fn operator_revoke_agent(
        &self,
        request: Request<passport_proto::OperatorRevokeRequest>,
    ) -> Result<Response<RevokeAgentResponse>, Status> {
        let req = request.into_inner();
        let passport_id = req
            .passport_id
            .as_ref()
            .map(|h| ContentHash(h.hex.clone()))
            .ok_or_else(|| Status::invalid_argument("missing passport_id"))?;

        self.state
            .passport_store
            .revoke(&passport_id, &req.reason)
            .map_err(|e| Status::not_found(e.to_string()))?;

        if req.cascade_capabilities {
            self.state.capability_store.revoke_all_by_issuer(&passport_id);
        }

        let cp_agent = AgentId("00000000-0000-0000-0000-000000000001".to_string());
        let cp_passport = ContentHash("cp_passport".to_string());
        let ws = WorkspaceId("global".to_string());

        let receipt = ReceiptBuilder::new(
            action_kind::AGENT_OPERATOR_REVOKE,
            &ws,
            &cp_passport,
            &cp_agent,
        )
        .build(&self.state.cp_keypair);

        let receipt_id = receipt.id.as_ref().map(|h| h.hex.clone()).unwrap_or_default();
        let _ = self.state.receipt_store.append(receipt);

        Ok(Response::new(RevokeAgentResponse {
            receipt_id: Some(common_proto::Hash { hex: receipt_id }),
        }))
    }

    async fn rotate_key(
        &self,
        _request: Request<passport_proto::KeyRotationRequest>,
    ) -> Result<Response<RotateKeyResponse>, Status> {
        let cp_agent = AgentId("00000000-0000-0000-0000-000000000001".to_string());
        let cp_passport = ContentHash("cp_passport".to_string());
        let ws = WorkspaceId("global".to_string());

        let receipt = ReceiptBuilder::new(
            action_kind::AGENT_ROTATE_KEY,
            &ws,
            &cp_passport,
            &cp_agent,
        )
        .build(&self.state.cp_keypair);

        let receipt_id = receipt.id.as_ref().map(|h| h.hex.clone()).unwrap_or_default();
        let _ = self.state.receipt_store.append(receipt);

        Ok(Response::new(RotateKeyResponse {
            receipt_id: Some(common_proto::Hash { hex: receipt_id }),
        }))
    }

    async fn get_passport(
        &self,
        request: Request<GetPassportRequest>,
    ) -> Result<Response<passport_proto::Passport>, Status> {
        let req = request.into_inner();
        let passport_id = req
            .passport_id
            .as_ref()
            .map(|h| ContentHash(h.hex.clone()))
            .ok_or_else(|| Status::invalid_argument("missing passport_id"))?;

        self.state
            .passport_store
            .get(&passport_id)
            .map(|p| {
                Response::new(passport_proto::Passport {
                    agent_id: Some(common_proto::AgentId { value: p.agent_id.0 }),
                    agent_public_key: p.public_key_bytes,
                    workspace_id: Some(common_proto::WorkspaceId { value: p.workspace_id.0 }),
                    trust_level: p.trust_level as i32,
                    role: p.role as i32,
                    model_provider: p.model_provider,
                    model_name: p.model_name,
                    expires_at: None,
                    signature: None,
                    passport_id: Some(common_proto::Hash { hex: p.passport_id.0 }),
                })
            })
            .ok_or_else(|| Status::not_found("passport not found"))
    }
}
