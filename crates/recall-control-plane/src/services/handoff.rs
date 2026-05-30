use std::sync::Arc;
use tonic::{Request, Response, Status};

use recall_core::ids::{AgentId, ContentHash, WorkspaceId};
use recall_proto::common as common_proto;
use recall_proto::controlplane::v1::{
    handoff_service_server::HandoffService,
    CreateHandoffRequest, CreateHandoffResponse,
    DeliverHandoffRequest, DeliverHandoffResponse,
};
use recall_receipt::{action_kind, builder::ReceiptBuilder};

use crate::state::AppState;

pub struct HandoffServiceImpl {
    state: Arc<AppState>,
}

impl HandoffServiceImpl {
    pub fn new(state: Arc<AppState>) -> Self {
        Self { state }
    }
}

#[tonic::async_trait]
impl HandoffService for HandoffServiceImpl {
    async fn create_handoff(
        &self,
        request: Request<CreateHandoffRequest>,
    ) -> Result<Response<CreateHandoffResponse>, Status> {
        let req = request.into_inner();

        let from_agent = req
            .from_agent_id
            .map(|a| AgentId(a.value))
            .ok_or_else(|| Status::invalid_argument("missing from_agent_id"))?;

        let to_agent = req
            .to_agent_id
            .map(|a| AgentId(a.value))
            .ok_or_else(|| Status::invalid_argument("missing to_agent_id"))?;

        let workspace_id = req
            .workspace_id
            .map(|w| WorkspaceId(w.value))
            .ok_or_else(|| Status::invalid_argument("missing workspace_id"))?;

        let memory_entries = self
            .state
            .memory_store
            .get_by_entity(&workspace_id.0, &req.entity);

        let capsule = recall_memory::handoff::build_capsule(
            &from_agent,
            &to_agent,
            &req.entity,
            &workspace_id,
            memory_entries,
            &self.state.cp_keypair,
        );

        let cp_agent = AgentId("00000000-0000-0000-0000-000000000001".to_string());
        let cp_passport = ContentHash("cp_passport".to_string());

        let receipt = ReceiptBuilder::new(
            action_kind::HANDOFF_CAPSULE_CREATE,
            &workspace_id,
            &cp_passport,
            &cp_agent,
        )
        .build(&self.state.cp_keypair);

        let receipt_id = receipt.id.as_ref().map(|h| h.hex.clone()).unwrap_or_default();
        let _ = self.state.receipt_store.append(receipt);

        Ok(Response::new(CreateHandoffResponse {
            capsule: Some(capsule),
            receipt_id: Some(common_proto::Hash { hex: receipt_id }),
        }))
    }

    async fn deliver_handoff(
        &self,
        _request: Request<DeliverHandoffRequest>,
    ) -> Result<Response<DeliverHandoffResponse>, Status> {
        let cp_agent = AgentId("00000000-0000-0000-0000-000000000001".to_string());
        let cp_passport = ContentHash("cp_passport".to_string());
        let ws = WorkspaceId("global".to_string());

        let receipt = ReceiptBuilder::new(
            action_kind::HANDOFF_CAPSULE_DELIVER,
            &ws,
            &cp_passport,
            &cp_agent,
        )
        .build(&self.state.cp_keypair);

        let receipt_id = receipt.id.as_ref().map(|h| h.hex.clone()).unwrap_or_default();
        let _ = self.state.receipt_store.append(receipt);

        Ok(Response::new(DeliverHandoffResponse {
            receipt_id: Some(common_proto::Hash { hex: receipt_id }),
        }))
    }
}
