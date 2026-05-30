use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status};

use recall_core::ids::{AgentId, ContentHash};
use recall_proto::common as common_proto;
use recall_proto::controlplane::v1::{
    envelope_service_server::EnvelopeService,
    Envelope, EnvelopeRequest, EnvelopeResponse, SubscribeRequest,
};
use recall_receipt::builder::ReceiptBuilder;
use recall_core::ids::WorkspaceId;

use crate::state::AppState;

pub struct EnvelopeServiceImpl {
    state: Arc<AppState>,
}

impl EnvelopeServiceImpl {
    pub fn new(state: Arc<AppState>) -> Self {
        Self { state }
    }
}

#[tonic::async_trait]
impl EnvelopeService for EnvelopeServiceImpl {
    type SubscribeStream = ReceiverStream<Result<Envelope, Status>>;

    async fn send_envelope(
        &self,
        request: Request<EnvelopeRequest>,
    ) -> Result<Response<EnvelopeResponse>, Status> {
        let req = request.into_inner();
        let _envelope = req.envelope.ok_or_else(|| Status::invalid_argument("missing envelope"))?;

        let cp_agent = AgentId("00000000-0000-0000-0000-000000000001".to_string());
        let cp_passport = ContentHash("cp_passport".to_string());
        let ws = WorkspaceId("global".to_string());

        let receipt = ReceiptBuilder::new(
            "envelope.send",
            &ws,
            &cp_passport,
            &cp_agent,
        )
        .build(&self.state.cp_keypair);

        let receipt_id = receipt.id.as_ref().map(|h| h.hex.clone()).unwrap_or_default();
        let _ = self.state.receipt_store.append(receipt);

        Ok(Response::new(EnvelopeResponse {
            receipt_id: Some(common_proto::Hash { hex: receipt_id }),
        }))
    }

    async fn subscribe(
        &self,
        _request: Request<SubscribeRequest>,
    ) -> Result<Response<Self::SubscribeStream>, Status> {
        let (_tx, rx) = mpsc::channel(64);
        Ok(Response::new(ReceiverStream::new(rx)))
    }
}
