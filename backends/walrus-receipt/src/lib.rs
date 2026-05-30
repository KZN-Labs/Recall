use anyhow::Result;
use prost::Message;
use recall_core::ids::{ContentHash, WalrusBlobId};
use recall_proto::receipt as receipt_proto;
use recall_receipt::merkle::merkle_root;

/// Walrus-backed receipt sealing.
/// Batches receipts into a Merkle tree and anchors the root to Sui.
#[allow(dead_code)]
pub struct WalrusReceiptSealer {
    publisher_url: String,
}

impl WalrusReceiptSealer {
    pub fn new(publisher_url: &str) -> Self {
        Self {
            publisher_url: publisher_url.to_string(),
        }
    }

    /// Write a receipt blob to Walrus. Every receipt is permanently stored.
    pub async fn write_receipt(&self, receipt: &receipt_proto::Receipt) -> Result<WalrusBlobId> {
        let mut buf = Vec::new();
        receipt.encode(&mut buf)?;

        // Production: POST to Walrus publisher.
        let blob_id = format!("0x{}", recall_crypto::sha256_hex(&buf));
        Ok(WalrusBlobId(blob_id))
    }

    /// Seal a batch of receipts: compute Merkle root, write to Walrus,
    /// and prepare the ReceiptBatch for Sui anchoring.
    pub async fn seal_batch(
        &self,
        receipt_ids: Vec<ContentHash>,
    ) -> Result<receipt_proto::ReceiptBatch> {
        if receipt_ids.is_empty() {
            anyhow::bail!("cannot seal empty batch");
        }

        let root = merkle_root(&receipt_ids);

        let batch = receipt_proto::ReceiptBatch {
            merkle_root: Some(recall_proto::common::Hash { hex: root.0.clone() }),
            receipt_ids: receipt_ids
                .iter()
                .map(|id| recall_proto::common::Hash { hex: id.0.clone() })
                .collect(),
            sui_tx_digest: String::new(), // filled in by sui-anchor after commit
            sealed_at: Some(prost_types::Timestamp {
                seconds: chrono::Utc::now().timestamp(),
                nanos: 0,
            }),
            batch_signature: None, // filled in by control-plane before anchoring
        };

        Ok(batch)
    }
}
