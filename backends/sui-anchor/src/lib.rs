use anyhow::Result;
use recall_proto::receipt as receipt_proto;

/// Sui receipt anchoring.
/// Commits the Merkle root of a receipt batch to the Sui chain via the receipt_anchor Move contract.
#[allow(dead_code)]
pub struct SuiAnchorDriver {
    sui_rpc_url: String,
    /// Package ID of the deployed receipt_anchor Move contract
    package_id: String,
    /// Wallet address used to sign Sui transactions
    sender_address: String,
}

impl SuiAnchorDriver {
    pub fn new(sui_rpc_url: &str, package_id: &str, sender_address: &str) -> Self {
        Self {
            sui_rpc_url: sui_rpc_url.to_string(),
            package_id: package_id.to_string(),
            sender_address: sender_address.to_string(),
        }
    }

    /// Anchor a receipt batch to Sui. Returns the Sui transaction digest.
    /// Emits an anchor.commit receipt on the control plane side.
    pub async fn anchor_batch(
        &self,
        batch: &receipt_proto::ReceiptBatch,
    ) -> Result<String> {
        let merkle_root = batch
            .merkle_root
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("batch missing merkle root"))?;

        // Production: build and submit a Sui PTB calling
        //   receipt_anchor::anchor(merkle_root_bytes, receipt_count, walrus_blob_id)
        // via the Sui Rust SDK.
        //
        // The tx digest is returned and stored on the ReceiptBatch.

        let fake_digest = format!(
            "sui_tx_{}",
            recall_core::ids::ContentHash(merkle_root.hex.clone()).0
        );

        Ok(fake_digest)
    }
}
