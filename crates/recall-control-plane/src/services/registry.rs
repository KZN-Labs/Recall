use std::sync::Arc;
use tonic::{Request, Response, Status};

use recall_core::ids::{AgentId, ContentHash, WorkspaceId};
use recall_proto::common as common_proto;
use recall_proto::registry as reg_proto;
use recall_proto::controlplane::v1::{
    registry_service_server::RegistryService, GetProfileRequest,
};
use recall_receipt::{action_kind, builder::ReceiptBuilder};

use crate::state::AppState;

pub struct RegistryServiceImpl {
    state: Arc<AppState>,
}

impl RegistryServiceImpl {
    pub fn new(state: Arc<AppState>) -> Self {
        Self { state }
    }
}

#[tonic::async_trait]
impl RegistryService for RegistryServiceImpl {
    async fn publish_profile(
        &self,
        request: Request<reg_proto::PublishRequest>,
    ) -> Result<Response<reg_proto::PublishResponse>, Status> {
        let req = request.into_inner();

        let profile = reg_proto::RegistryProfile {
            name: req.name.clone(),
            version: req.version.clone(),
            author: String::new(),
            category: String::new(),
            description: req.description,
            memory_count: 0,
            artifact_count: 0,
            import_count: 0,
            recommended_system_prompt: String::new(),
            workspace_config: None,
            walrus_blob: None,
            published_at: Some(prost_types::Timestamp {
                seconds: chrono::Utc::now().timestamp(),
                nanos: 0,
            }),
            immutable: true,
            component_blob_ids: None,
            publisher_passport_id: String::new(),
        };

        self.state
            .registry_store
            .publish(profile.clone())
            .map_err(|e| Status::already_exists(e.to_string()))?;

        let cp_agent = AgentId("00000000-0000-0000-0000-000000000001".to_string());
        let cp_passport = ContentHash("cp_passport".to_string());
        let ws = WorkspaceId("global".to_string());

        let receipt = ReceiptBuilder::new(
            action_kind::REGISTRY_PUBLISH,
            &ws,
            &cp_passport,
            &cp_agent,
        )
        .build(&self.state.cp_keypair);

        let receipt_id = receipt.id.as_ref().map(|h| h.hex.clone()).unwrap_or_default();
        let _ = self.state.receipt_store.append(receipt);

        Ok(Response::new(reg_proto::PublishResponse {
            profile: Some(profile),
            receipt_id: Some(common_proto::Hash { hex: receipt_id }),
        }))
    }

    async fn import_profile(
        &self,
        request: Request<reg_proto::ImportRequest>,
    ) -> Result<Response<reg_proto::ImportResponse>, Status> {
        let req = request.into_inner();
        let profile = self.state.registry_store.get(&req.name, &req.version)
            .ok_or_else(|| Status::not_found("profile not found"))?;

        let cp_agent = AgentId("00000000-0000-0000-0000-000000000001".to_string());
        let cp_passport = ContentHash("cp_passport".to_string());
        let ws = WorkspaceId("global".to_string());

        let receipt = ReceiptBuilder::new(
            action_kind::REGISTRY_IMPORT,
            &ws,
            &cp_passport,
            &cp_agent,
        )
        .build(&self.state.cp_keypair);

        let receipt_id = receipt.id.as_ref().map(|h| h.hex.clone()).unwrap_or_default();
        let _ = self.state.receipt_store.append(receipt);

        Ok(Response::new(reg_proto::ImportResponse {
            profile: Some(profile),
            receipt_id: Some(common_proto::Hash { hex: receipt_id }),
        }))
    }

    async fn list_profiles(
        &self,
        request: Request<reg_proto::ListProfilesRequest>,
    ) -> Result<Response<reg_proto::ListProfilesResponse>, Status> {
        let req = request.into_inner();
        let profiles = self.state.registry_store.list(
            if req.category.is_empty() { None } else { Some(req.category.as_str()) },
            if req.name_prefix.is_empty() { None } else { Some(req.name_prefix.as_str()) },
        );
        Ok(Response::new(reg_proto::ListProfilesResponse {
            profiles,
            next_page_token: String::new(),
        }))
    }

    async fn get_profile(
        &self,
        request: Request<GetProfileRequest>,
    ) -> Result<Response<reg_proto::RegistryProfile>, Status> {
        let req = request.into_inner();
        self.state
            .registry_store
            .get(&req.name, &req.version)
            .map(Response::new)
            .ok_or_else(|| Status::not_found("profile not found"))
    }
}
