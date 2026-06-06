use axum;
use clap::Parser;
use std::net::SocketAddr;
use tonic::transport::Server;
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod anchor_scheduler;
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

    /// Anchor scheduler interval in seconds. Each tick batches all
    /// receipts written since the last anchor, computes a Merkle root,
    /// and submits it to the `receipt_anchor` Move package on Sui.
    ///
    /// Set to `0` to disable the scheduler entirely (no anchors emitted).
    #[arg(long, env = "RECALL_ANCHOR_INTERVAL_SECS", default_value_t = 30)]
    anchor_interval_secs: u64,
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

    // ── MemWal credentials required ──────────────────────────────────────────
    // RECALL is built on Walrus Memory. Every memory write MUST become a
    // permanent Walrus blob. Without these credentials the system would silently
    // degrade to in-process storage, which is wrong for the Walrus track.
    let memwal_key     = std::env::var("MEMWAL_PRIVATE_KEY").unwrap_or_default();
    let memwal_account = std::env::var("MEMWAL_ACCOUNT_ID").unwrap_or_default();

    if memwal_key.is_empty() || memwal_account.is_empty() {
        eprintln!();
        eprintln!("  ERROR: MemWal credentials not configured.");
        eprintln!();
        eprintln!("  RECALL requires Walrus Memory to store agent memory blobs.");
        eprintln!("  Every memory write must be a permanent Walrus blob.");
        eprintln!();
        eprintln!("  Set these env vars before starting:");
        eprintln!("    export MEMWAL_PRIVATE_KEY=\"your-ed25519-private-key\"");
        eprintln!("    export MEMWAL_ACCOUNT_ID=\"your-memwal-account-id\"");
        eprintln!();
        eprintln!("  Get credentials at: https://memory.walrus.xyz/");
        eprintln!();
        std::process::exit(1);
    }

    // Resolve Walrus endpoints
    let walrus_publisher = if args.walrus_testnet && args.walrus_publisher.is_none() {
        Some(walrus_memory::WALRUS_TESTNET_PUBLISHER.to_string())
    } else {
        args.walrus_publisher.clone()
    };
    let walrus_aggregator = if args.walrus_testnet && args.walrus_aggregator.is_none() {
        Some(walrus_memory::WALRUS_TESTNET_AGGREGATOR.to_string())
    } else {
        args.walrus_aggregator
    };

    // Warn (don't exit) if no Walrus publisher endpoint is configured at all.
    let walrus_configured = args.walrus_testnet
        || args.walrus_publisher.is_some()
        || std::env::var("WALRUS_PUBLISHER_URL").is_ok();
    if !walrus_configured {
        eprintln!("WARNING: --walrus-testnet not set and WALRUS_PUBLISHER_URL not set.");
        eprintln!("  Memory writes will not be stored on Walrus.");
        eprintln!("  Run with: cargo run -p recall-control-plane -- --walrus-testnet");
    }

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

    // ── Tatum RPC integration ────────────────────────────────────────────────
    // When TATUM_API_KEY is set the sui-anchor and sui-governance backends
    // route every Sui RPC call through Tatum's Sui gateway with the key
    // attached as the `x-api-key` header.
    let tatum_active = std::env::var("TATUM_API_KEY")
        .map(|k| !k.is_empty())
        .unwrap_or(false);
    let sui_network = std::env::var("SUI_NETWORK").unwrap_or_else(|_| "testnet".into());

    if tatum_active {
        info!("Sui RPC: Tatum ({} network)", sui_network);
    } else {
        info!("Sui RPC: public fullnode (set TATUM_API_KEY for Tatum)");
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

    // Spawn the anchor scheduler — seals batches of receipts under a Merkle
    // root and submits each root to the receipt_anchor Move package on Sui.
    anchor_scheduler::spawn(
        app_state.clone(),
        std::time::Duration::from_secs(args.anchor_interval_secs),
    );

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
