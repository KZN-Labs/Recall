use anyhow::Result;
use prost::Message;
use recall_core::ids::WalrusBlobId;
use recall_crypto::sha256_hex;
use recall_proto::memory as mem_proto;

/// MemWal integration: read/write memory blobs to Walrus.
///
/// Production: replace with the real MemWal SDK client.
/// Walrus is append-only — every blob written is permanent.
#[allow(dead_code)]
pub struct WalrusMemoryBackend {
    /// Walrus aggregator endpoint, e.g. "https://aggregator.walrus-testnet.walrus.space"
    aggregator_url: String,
    /// Walrus publisher endpoint
    publisher_url: String,
    client: reqwest::Client,
}

impl WalrusMemoryBackend {
    pub fn new(aggregator_url: &str, publisher_url: &str) -> Self {
        Self {
            aggregator_url: aggregator_url.to_string(),
            publisher_url: publisher_url.to_string(),
            client: reqwest::Client::new(),
        }
    }

    /// Write a memory entry blob to Walrus. Returns the assigned blob ID.
    /// Every call to this function results in a permanent Walrus blob.
    pub async fn write_memory_entry(
        &self,
        entry: &mem_proto::MemoryEntry,
    ) -> Result<WalrusBlobId> {
        let mut buf = Vec::new();
        entry.encode(&mut buf)?;

        // In production: POST buf to Walrus publisher endpoint.
        // The blob ID is returned in the response.
        // Here we derive a deterministic pseudo blob ID for testing.
        let blob_id = format!("0x{}", sha256_hex(&buf));

        Ok(WalrusBlobId(blob_id))
    }

    /// Read a memory entry blob from Walrus by blob ID.
    pub async fn read_memory_entry(&self, blob_id: &WalrusBlobId) -> Result<mem_proto::MemoryEntry> {
        // In production: GET from Walrus aggregator endpoint.
        Err(anyhow::anyhow!(
            "Walrus backend not connected — blob_id: {}",
            blob_id.0
        ))
    }

    /// Write a handoff capsule blob to Walrus.
    pub async fn write_capsule(
        &self,
        capsule: &mem_proto::HandoffCapsule,
    ) -> Result<WalrusBlobId> {
        let mut buf = Vec::new();
        capsule.encode(&mut buf)?;
        let blob_id = format!("0x{}", sha256_hex(&buf));
        Ok(WalrusBlobId(blob_id))
    }
}
