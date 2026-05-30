use anyhow::Result;
use recall_capability::store::CapabilityStore;
use recall_conflict::ConflictStore;
use recall_crypto::RecallKeypair;
use recall_memory::store::MemoryStore;
use recall_passport::store::PassportStore;
use recall_receipt::store::ReceiptStore;
use recall_registry::RegistryStore;
use recall_transport::subscribe::SubscribeHub;
use sui_governance::SuiGovernanceClient;

/// Shared application state, wrapped in Arc for service handlers.
pub struct AppState {
    /// Control-plane's own keypair — it is an agent with extra privileges.
    pub cp_keypair: RecallKeypair,
    pub passport_store: PassportStore,
    pub capability_store: CapabilityStore,
    pub receipt_store: ReceiptStore,
    pub memory_store: MemoryStore,
    pub conflict_store: ConflictStore,
    pub registry_store: RegistryStore,
    pub subscribe_hub: SubscribeHub,
    /// On-chain governance client.
    /// Defaults to offline mode; configure with Sui RPC URL for production.
    pub governance: SuiGovernanceClient,
    pub enforcement: crate::enforcement::EnforcementEngine,
}

impl AppState {
    /// Create application state.
    ///
    /// `sui_rpc_url` — if Some, the governance client will call the Sui node
    /// at that URL. If None, offline (local) rule evaluation is used.
    pub fn new(
        sui_rpc_url: Option<String>,
        policy_object_id: Option<String>,
        record_object_id: Option<String>,
    ) -> Result<Self> {
        let governance = match sui_rpc_url {
            Some(url) => SuiGovernanceClient::new(
                url,
                policy_object_id.unwrap_or_default(),
                record_object_id.unwrap_or_default(),
            ),
            None => SuiGovernanceClient::offline(),
        };

        Ok(Self {
            cp_keypair: RecallKeypair::generate(),
            passport_store: PassportStore::default(),
            capability_store: CapabilityStore::default(),
            receipt_store: ReceiptStore::default(),
            memory_store: MemoryStore::default(),
            conflict_store: ConflictStore::default(),
            registry_store: RegistryStore::default(),
            subscribe_hub: SubscribeHub::default(),
            governance,
            enforcement: crate::enforcement::EnforcementEngine::default(),
        })
    }
}
