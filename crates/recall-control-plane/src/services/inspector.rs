use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status};

use recall_core::ids::ContentHash;
use recall_proto::controlplane::v1::{
    inspector_service_server::InspectorService,
    GetReceiptBatchRequest, GetReceiptRequest,
    ListReceiptsRequest, ListReceiptsResponse,
    StreamReceiptsRequest,
};
use recall_proto::receipt as receipt_proto;

use crate::state::AppState;

pub struct InspectorServiceImpl {
    state: Arc<AppState>,
}

impl InspectorServiceImpl {
    pub fn new(state: Arc<AppState>) -> Self {
        Self { state }
    }
}

#[tonic::async_trait]
impl InspectorService for InspectorServiceImpl {
    type StreamReceiptsStream = ReceiverStream<Result<receipt_proto::Receipt, Status>>;

    async fn get_receipt(
        &self,
        request: Request<GetReceiptRequest>,
    ) -> Result<Response<receipt_proto::Receipt>, Status> {
        let req = request.into_inner();
        let id = req
            .receipt_id
            .as_ref()
            .map(|h| ContentHash(h.hex.clone()))
            .ok_or_else(|| Status::invalid_argument("missing receipt_id"))?;

        self.state
            .receipt_store
            .get(&id)
            .map(Response::new)
            .ok_or_else(|| Status::not_found("receipt not found"))
    }

    async fn list_receipts(
        &self,
        request: Request<ListReceiptsRequest>,
    ) -> Result<Response<ListReceiptsResponse>, Status> {
        let req = request.into_inner();
        let workspace_id = req
            .workspace_id
            .as_ref()
            .map(|w| w.value.clone())
            .ok_or_else(|| Status::invalid_argument("missing workspace_id"))?;

        let mut receipts = self.state.receipt_store.list_by_workspace(&workspace_id);
        if !req.action_kind_filter.is_empty() {
            receipts.retain(|r| r.action_kind == req.action_kind_filter);
        }

        Ok(Response::new(ListReceiptsResponse {
            receipts,
            next_page_token: String::new(),
        }))
    }

    async fn get_receipt_batch(
        &self,
        _request: Request<GetReceiptBatchRequest>,
    ) -> Result<Response<receipt_proto::ReceiptBatch>, Status> {
        Err(Status::unimplemented("receipt batch anchoring requires Walrus backend"))
    }

    async fn stream_receipts(
        &self,
        request: Request<StreamReceiptsRequest>,
    ) -> Result<Response<Self::StreamReceiptsStream>, Status> {
        let req = request.into_inner();
        let workspace_id = req
            .workspace_id
            .as_ref()
            .map(|w| w.value.clone())
            .ok_or_else(|| Status::invalid_argument("missing workspace_id"))?;

        let (tx, rx) = mpsc::channel(64);
        let mut broadcast_rx = self.state.subscribe_hub.subscribe(&workspace_id);
        let action_filter = req.action_kind_filter.clone();

        tokio::spawn(async move {
            loop {
                match broadcast_rx.recv().await {
                    Ok(receipt) => {
                        if action_filter.is_empty() || receipt.action_kind == action_filter {
                            if tx.send(Ok(receipt)).await.is_err() {
                                break;
                            }
                        }
                    }
                    Err(_) => break,
                }
            }
        });

        Ok(Response::new(ReceiverStream::new(rx)))
    }
}
