use axum;
use clap::Parser;
use std::net::SocketAddr;
use tonic::transport::Server;
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod enforcement;
mod http;
mod services;
mod workspace_store;
mod state;

use services::{
    admission::AdmissionServiceImpl,
    capability::CapabilityServiceImpl,
    envelope::EnvelopeServiceImpl,
    handoff::HandoffServiceImpl,
    inspector::InspectorServiceImpl,
    memory::MemoryServiceImpl,
    registry::RegistryServiceImpl,
    workspace::WorkspaceServiceImpl,
};

use recall_proto::controlplane::v1::{
    admission_service_server::AdmissionServiceServer,
    capability_service_server::CapabilityServiceServer,
    envelope_service_server::EnvelopeServiceServer,
    handoff_service_server::HandoffServiceServer,
    inspector_service_server::InspectorServiceServer,
    memory_service_server::MemoryServiceServer,
    registry_service_server::RegistryServiceServer,
    workspace_service_server::WorkspaceServiceServer,
};

use state::AppStateConfig;

#[derive(Parser, Debug)]
#[command(name = "recall-control-plane", about = "RECALL gRPC control plane server")]
struct Args {
    #[arg(long, env = "RECALL_BIND_ADDR", default_value = "0.0.0.0:9090")]
    bind_addr: SocketAddr,

    #[arg(long, env = "RECALL_HTTP_ADDR", default_value = "0.0.0.0:8080")]
    http_addr: SocketAddr,

    /// Sui RPC endpoint for on-chain governance. Leave unset for offline mode.
    #[arg(long, env = "RECALL_SUI_RPC_URL")]
    sui_rpc_url: Option<String>,

    #[arg(long, env = "RECALL_GOVERNANCE_POLICY_ID")]
    governance_policy_id: Option<String>,

    #[arg(long, env = "RECALL_GOVERNANCE_RECORD_ID")]
    governance_record_id: Option<String>,

    /// Walrus publisher URL. Set to enable real Walrus writes.
    /// Default testnet: https://publisher.walrus-testnet.walrus.space
    #[arg(long, env = "RECALL_WALRUS_PUBLISHER")]
    walrus_publisher: Option<String>,

    /// Walrus aggregator URL (for reads).
    /// Default testnet: https://aggregator.walrus-testnet.walrus.space
    #[arg(long, env = "RECALL_WALRUS_AGGREGATOR")]
    walrus_aggregator: Option<String>,

    /// Enable Walrus testnet with default endpoints (no URL flags needed).
    #[arg(long, env = "RECALL_WALRUS_TESTNET")]
    walrus_testnet: bool,

    #[arg(long, env = "RECALL_LOG_FORMAT", default_value = "text")]
    log_format: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "info".into()),
        ))
        .with(tracing_subscriber::fmt::layer())
        .init();

    // Resolve Walrus endpoints
    let walrus_publisher = if args.walrus_testnet && args.walrus_publisher.is_none() {
        Some(walrus_memory::WALRUS_TESTNET_PUBLISHER.to_string())
    } else {
        args.walrus_publisher
    };
    let walrus_aggregator = if args.walrus_testnet && args.walrus_aggregator.is_none() {
        Some(walrus_memory::WALRUS_TESTNET_AGGREGATOR.to_string())
    } else {
        args.walrus_aggregator
    };

    if let Some(ref pub_url) = walrus_publisher {
        info!("Walrus: ENABLED  publisher={pub_url}");
    } else {
        info!("Walrus: offline (pass --walrus-testnet to enable)");
    }

    if args.sui_rpc_url.is_some() {
        info!("Governance: on-chain ({})", args.sui_rpc_url.as_deref().unwrap());
    } else {
        info!("Governance: offline");
    }

    let app_state = std::sync::Arc::new(state::AppState::new(AppStateConfig {
        sui_rpc_url:           args.sui_rpc_url,
        policy_object_id:      args.governance_policy_id,
        record_object_id:      args.governance_record_id,
        walrus_publisher_url:  walrus_publisher,
        walrus_aggregator_url: walrus_aggregator,
    })?);

    info!("RECALL control plane  gRPC={}", args.bind_addr);
    info!("RECALL REST API        HTTP={}", args.http_addr);

    let http_state = app_state.clone();
    let http_addr  = args.http_addr;
    tokio::spawn(async move {
        let router   = http::router(http_state);
        let listener = tokio::net::TcpListener::bind(http_addr).await
            .expect("failed to bind HTTP listener");
        axum::serve(listener, router).await.expect("HTTP server error");
    });

    Server::builder()
        .add_service(AdmissionServiceServer::new(AdmissionServiceImpl::new(app_state.clone())))
        .add_service(CapabilityServiceServer::new(CapabilityServiceImpl::new(app_state.clone())))
        .add_service(MemoryServiceServer::new(MemoryServiceImpl::new(app_state.clone())))
        .add_service(HandoffServiceServer::new(HandoffServiceImpl::new(app_state.clone())))
        .add_service(WorkspaceServiceServer::new(WorkspaceServiceImpl::new(app_state.clone())))
        .add_service(RegistryServiceServer::new(RegistryServiceImpl::new(app_state.clone())))
        .add_service(InspectorServiceServer::new(InspectorServiceImpl::new(app_state.clone())))
        .add_service(EnvelopeServiceServer::new(EnvelopeServiceImpl::new(app_state.clone())))
        .serve(args.bind_addr)
        .await?;

    Ok(())
}
