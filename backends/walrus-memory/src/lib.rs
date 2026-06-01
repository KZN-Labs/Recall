use anyhow::{anyhow, Result};
use prost::Message;
use recall_core::ids::WalrusBlobId;
use recall_proto::memory as mem_proto;

pub const WALRUS_TESTNET_PUBLISHER:  &str = "https://publisher.walrus-testnet.walrus.space";
pub const WALRUS_TESTNET_AGGREGATOR: &str = "https://aggregator.walrus-testnet.walrus.space";

/// MemWal integration: read/write memory blobs on Walrus.
pub struct WalrusMemoryBackend {
    aggregator_url: String,
    publisher_url:  String,
    client:         reqwest::Client,
}

impl WalrusMemoryBackend {
    pub fn new(aggregator_url: &str, publisher_url: &str) -> Self {
        Self {
            aggregator_url: aggregator_url.trim_end_matches('/').to_string(),
            publisher_url:  publisher_url.trim_end_matches('/').to_string(),
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .unwrap(),
        }
    }

    pub fn testnet() -> Self {
        Self::new(WALRUS_TESTNET_AGGREGATOR, WALRUS_TESTNET_PUBLISHER)
    }

    /// Write a memory entry to Walrus. Returns the permanent blob ID.
    pub async fn write_memory_entry(&self, entry: &mem_proto::MemoryEntry) -> Result<WalrusBlobId> {
        let mut buf = Vec::new();
        entry.encode(&mut buf)?;
        self.put_blob(&buf).await
    }

    /// Write a handoff capsule to Walrus.
    pub async fn write_capsule(&self, capsule: &mem_proto::HandoffCapsule) -> Result<WalrusBlobId> {
        let mut buf = Vec::new();
        capsule.encode(&mut buf)?;
        self.put_blob(&buf).await
    }

    /// Read and decode a memory entry from Walrus by blob ID.
    pub async fn read_memory_entry(&self, blob_id: &WalrusBlobId) -> Result<mem_proto::MemoryEntry> {
        let bytes = self.get_blob(&blob_id.0).await?;
        Ok(mem_proto::MemoryEntry::decode(bytes.as_slice())?)
    }

    // ── HTTP primitives ───────────────────────────────────────────────────────

    async fn put_blob(&self, data: &[u8]) -> Result<WalrusBlobId> {
        let resp = self.client
            .put(format!("{}/v1/blobs", self.publisher_url))
            .header("Content-Type", "application/octet-stream")
            .body(data.to_vec())
            .send()
            .await
            .map_err(|e| anyhow!("Walrus PUT failed: {e}"))?;

        let status = resp.status();
        let body: serde_json::Value = resp.json().await
            .map_err(|e| anyhow!("Walrus response parse failed: {e}"))?;

        if !status.is_success() {
            return Err(anyhow!("Walrus publisher returned {status}: {body}"));
        }

        // Handle both newlyCreated and alreadyCertified responses
        let blob_id = body["newlyCreated"]["blobObject"]["blobId"]
            .as_str()
            .or_else(|| body["alreadyCertified"]["blobId"].as_str())
            .ok_or_else(|| anyhow!("Walrus response missing blobId: {body}"))?
            .to_string();

        Ok(WalrusBlobId(blob_id))
    }

    async fn get_blob(&self, blob_id: &str) -> Result<Vec<u8>> {
        let resp = self.client
            .get(format!("{}/v1/{}", self.aggregator_url, blob_id))
            .send()
            .await
            .map_err(|e| anyhow!("Walrus GET failed: {e}"))?;

        if !resp.status().is_success() {
            return Err(anyhow!("Walrus aggregator returned {}: {blob_id}", resp.status()));
        }

        Ok(resp.bytes().await?.to_vec())
    }
}
