use anyhow::Result;
use recall_capability::store::CapabilityStore;
use recall_conflict::ConflictStore;
use recall_crypto::RecallKeypair;
use recall_memory::store::MemoryStore;
use recall_passport::store::PassportStore;
use recall_receipt::store::ReceiptStore;
use recall_registry::RegistryStore;
use recall_transport::subscribe::SubscribeHub;
use std::collections::HashMap;
use std::sync::RwLock;
use sui_governance::SuiGovernanceClient;
use walrus_memory::WalrusMemoryBackend;

use crate::workspace_store::WorkspaceStore;

/// Shared application state, wrapped in Arc for service handlers.
pub struct AppState {
    pub cp_keypair:       RecallKeypair,
    pub passport_store:   PassportStore,
    pub capability_store: CapabilityStore,
    pub receipt_store:    ReceiptStore,
    pub memory_store:     MemoryStore,
    pub conflict_store:   ConflictStore,
    pub registry_store:   RegistryStore,
    pub subscribe_hub:    SubscribeHub,
    pub governance:       SuiGovernanceClient,
    pub enforcement:      crate::enforcement::EnforcementEngine,
    /// Walrus memory backend — Some when a publisher URL is configured.
    pub walrus:           Option<WalrusMemoryBackend>,
    /// Explicit workspace registry (created via CreateWorkspace or auto-registered on write).
    pub workspace_store:  WorkspaceStore,
    /// In-process cache of just-published registry package blobs keyed by
    /// Walrus blob ID. Avoids the publisher→aggregator propagation lag when
    /// the same control plane that published a profile is the one importing
    /// it. Production deployments replace this with Redis or similar.
    pub registry_blob_cache: RwLock<HashMap<String, Vec<u8>>>,
}

pub struct AppStateConfig {
    pub sui_rpc_url:          Option<String>,
    pub policy_object_id:     Option<String>,
    pub record_object_id:     Option<String>,
    /// Walrus publisher URL. If None, memory is not written to Walrus.
    /// Set to walrus_memory::WALRUS_TESTNET_PUBLISHER for testnet.
    pub walrus_publisher_url: Option<String>,
    /// Walrus aggregator URL.
    pub walrus_aggregator_url: Option<String>,
}

impl AppState {
    pub fn new(cfg: AppStateConfig) -> Result<Self> {
        let governance = match cfg.sui_rpc_url {
            Some(url) => SuiGovernanceClient::new(
                url,
                cfg.policy_object_id.unwrap_or_default(),
                cfg.record_object_id.unwrap_or_default(),
            ),
            None => SuiGovernanceClient::offline(),
        };

        let walrus = cfg.walrus_publisher_url.map(|pub_url| {
            let agg_url = cfg.walrus_aggregator_url
                .unwrap_or_else(|| walrus_memory::WALRUS_TESTNET_AGGREGATOR.to_string());
            WalrusMemoryBackend::new(&agg_url, &pub_url)
        });

        Ok(Self {
            cp_keypair:       RecallKeypair::generate(),
            passport_store:   PassportStore::default(),
            capability_store: CapabilityStore::default(),
            receipt_store:    ReceiptStore::default(),
            memory_store:     MemoryStore::default(),
            conflict_store:   ConflictStore::default(),
            registry_store:   RegistryStore::default(),
            subscribe_hub:    SubscribeHub::default(),
            governance,
            enforcement:      crate::enforcement::EnforcementEngine::default(),
            walrus,
            workspace_store:  WorkspaceStore::default(),
            registry_blob_cache: RwLock::new(HashMap::new()),
        })
    }
}
