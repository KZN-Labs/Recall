use std::sync::Arc;
use tonic::{Request, Response, Status};

use recall_core::ids::{AgentId, ContentHash, WorkspaceId};
use recall_proto::common as common_proto;
use recall_proto::memory as mem_proto;
use recall_proto::controlplane::v1::{
    workspace_service_server::WorkspaceService,
    CreateWorkspaceRequest, CreateWorkspaceResponse,
    GetWorkspaceRequest, RollbackWorkspaceRequest, RollbackWorkspaceResponse,
    SnapshotWorkspaceRequest, SnapshotWorkspaceResponse,
};
use recall_receipt::{action_kind, builder::ReceiptBuilder};

use crate::state::AppState;

pub struct WorkspaceServiceImpl {
    state: Arc<AppState>,
}

impl WorkspaceServiceImpl {
    pub fn new(state: Arc<AppState>) -> Self {
        Self { state }
    }
}

#[tonic::async_trait]
impl WorkspaceService for WorkspaceServiceImpl {
    async fn create_workspace(
        &self,
        request: Request<CreateWorkspaceRequest>,
    ) -> Result<Response<CreateWorkspaceResponse>, Status> {
        let req = request.into_inner();
        let workspace = recall_memory::workspace::new_workspace(
            &req.name,
            req.topology_mode,
            &req.constitution_version,
        );

        let ws_id = WorkspaceId(
            workspace
                .id
                .as_ref()
                .map(|w| w.value.clone())
                .unwrap_or_default(),
        );

        let cp_agent = AgentId("00000000-0000-0000-0000-000000000001".to_string());
        let cp_passport = ContentHash("cp_passport".to_string());

        let receipt = ReceiptBuilder::new(
            action_kind::WORKSPACE_CREATE,
            &ws_id,
            &cp_passport,
            &cp_agent,
        )
        .build(&self.state.cp_keypair);

        let receipt_id = receipt.id.as_ref().map(|h| h.hex.clone()).unwrap_or_default();
        let _ = self.state.receipt_store.append(receipt);

        Ok(Response::new(CreateWorkspaceResponse {
            workspace: Some(workspace),
            receipt_id: Some(common_proto::Hash { hex: receipt_id }),
        }))
    }

    async fn get_workspace(
        &self,
        _request: Request<GetWorkspaceRequest>,
    ) -> Result<Response<mem_proto::Workspace>, Status> {
        Err(Status::unimplemented("persistent workspace store requires Walrus backend"))
    }

    async fn snapshot_workspace(
        &self,
        request: Request<SnapshotWorkspaceRequest>,
    ) -> Result<Response<SnapshotWorkspaceResponse>, Status> {
        let req = request.into_inner();
        let ws_id = req
            .workspace_id
            .map(|w| WorkspaceId(w.value))
            .ok_or_else(|| Status::invalid_argument("missing workspace_id"))?;

        let cp_agent = AgentId("00000000-0000-0000-0000-000000000001".to_string());
        let cp_passport = ContentHash("cp_passport".to_string());

        let receipt = ReceiptBuilder::new(
            action_kind::WORKSPACE_SNAPSHOT,
            &ws_id,
            &cp_passport,
            &cp_agent,
        )
        .build(&self.state.cp_keypair);

        let receipt_id = receipt.id.as_ref().map(|h| h.hex.clone()).unwrap_or_default();
        let _ = self.state.receipt_store.append(receipt);

        Ok(Response::new(SnapshotWorkspaceResponse {
            receipt_id: Some(common_proto::Hash { hex: receipt_id }),
            snapshot_blob: None,
        }))
    }

    async fn rollback_workspace(
        &self,
        request: Request<RollbackWorkspaceRequest>,
    ) -> Result<Response<RollbackWorkspaceResponse>, Status> {
        let req = request.into_inner();
        let ws_id = req
            .workspace_id
            .map(|w| WorkspaceId(w.value))
            .ok_or_else(|| Status::invalid_argument("missing workspace_id"))?;

        let cp_agent = AgentId("00000000-0000-0000-0000-000000000001".to_string());
        let cp_passport = ContentHash("cp_passport".to_string());

        let receipt = ReceiptBuilder::new(
            action_kind::WORKSPACE_ROLLBACK,
            &ws_id,
            &cp_passport,
            &cp_agent,
        )
        .build(&self.state.cp_keypair);

        let receipt_id = receipt.id.as_ref().map(|h| h.hex.clone()).unwrap_or_default();
        let _ = self.state.receipt_store.append(receipt);

        Ok(Response::new(RollbackWorkspaceResponse {
            receipt_id: Some(common_proto::Hash { hex: receipt_id }),
        }))
    }
}
