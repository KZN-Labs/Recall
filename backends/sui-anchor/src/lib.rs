use anyhow::{anyhow, Result};
use recall_proto::receipt as receipt_proto;
use std::env;
use tracing::{info, warn};

const CLOCK_OBJECT_ID: &str =
    "0x0000000000000000000000000000000000000000000000000000000000000006";
const GAS_BUDGET: u64 = 10_000_000; // 0.01 SUI (actual anchor cost ~0.003 SUI)

/// Resolve the Sui RPC endpoint. `SUI_NETWORK` selects testnet (default),
/// mainnet, or devnet. `RECALL_SUI_RPC_URL` overrides if set.
fn get_sui_rpc_url() -> String {
    if let Ok(url) = env::var("RECALL_SUI_RPC_URL") {
        if !url.is_empty() {
            return url;
        }
    }
    let network = env::var("SUI_NETWORK").unwrap_or_else(|_| "testnet".to_string());
    match network.as_str() {
        "mainnet" => "https://fullnode.mainnet.sui.io:443".to_string(),
        "devnet"  => "https://fullnode.devnet.sui.io:443".to_string(),
        _         => "https://fullnode.testnet.sui.io:443".to_string(),
    }
}

fn build_rpc_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .unwrap_or_default()
}

pub struct SuiAnchorDriver {
    sui_rpc_url: String,
    client: reqwest::Client,
}

impl SuiAnchorDriver {
    pub fn new(sui_rpc_url: &str, _package_id: &str, _sender_address: &str) -> Self {
        Self {
            sui_rpc_url: sui_rpc_url.to_string(),
            client: build_rpc_client(),
        }
    }

    pub fn testnet() -> Self {
        Self::new(&get_sui_rpc_url(), "", "")
    }

    /// Anchor a receipt batch to Sui. Returns one of:
    /// - a real Sui transaction digest (base58, ~44 chars), on successful
    ///   on-chain submission;
    /// - an `UNANCHORED:<reason>` string when env vars are missing, keys are
    ///   invalid, or submission fails. The prefix is deliberately ugly so a
    ///   synthetic value can never be mistaken for a confirmed on-chain tx.
    pub async fn anchor_batch(&self, batch: &receipt_proto::ReceiptBatch) -> Result<String> {
        let merkle_root = batch
            .merkle_root
            .as_ref()
            .ok_or_else(|| anyhow!("batch missing merkle root"))?;

        let private_key_str = match env::var("RECALL_SUI_PRIVATE_KEY") {
            Ok(k) if !k.is_empty() => k,
            _ => {
                warn!("RECALL_SUI_PRIVATE_KEY not set; emitting UNANCHORED");
                return Ok(unanchored("no_private_key"));
            }
        };
        let package_id = match env::var("RECALL_RECEIPT_ANCHOR_PACKAGE_ID") {
            Ok(id) if !id.is_empty() => id,
            _ => {
                warn!("RECALL_RECEIPT_ANCHOR_PACKAGE_ID not set; emitting UNANCHORED");
                return Ok(unanchored("no_package_id"));
            }
        };
        let registry_id = match env::var("RECALL_ANCHOR_REGISTRY_ID") {
            Ok(id) if !id.is_empty() => id,
            _ => {
                warn!("RECALL_ANCHOR_REGISTRY_ID not set; emitting UNANCHORED");
                return Ok(unanchored("no_registry_id"));
            }
        };
        let signing_key = match parse_private_key(&private_key_str) {
            Ok(k) => k,
            Err(e) => {
                warn!("Invalid RECALL_SUI_PRIVATE_KEY: {e}; emitting UNANCHORED");
                return Ok(unanchored("bad_private_key"));
            }
        };

        let sender = sui_address_from_ed25519(&signing_key.verifying_key().to_bytes());

        match self
            .submit_anchor(&sender, &signing_key, &package_id, &registry_id, merkle_root, batch)
            .await
        {
            Ok(digest) => {
                info!("Sui anchor committed: {digest}");
                Ok(digest)
            }
            Err(e) => {
                warn!("Sui anchor submission failed: {e}; emitting UNANCHORED");
                Ok(unanchored(&format!("submit_failed:{e}")))
            }
        }
    }

    async fn submit_anchor(
        &self,
        sender: &str,
        signing_key: &ed25519_dalek::SigningKey,
        package_id: &str,
        registry_id: &str,
        merkle_root: &recall_proto::common::Hash,
        batch: &receipt_proto::ReceiptBatch,
    ) -> Result<String> {
        use base64::Engine;
        use ed25519_dalek::Signer;

        // The Move contract converts merkle_root/walrus_blob_id/workspace_id
        // via `string::utf8(...)`, so we must pass UTF-8 encoded hex strings,
        // not raw hash bytes (which are invalid UTF-8).
        let merkle_bytes = merkle_root.hex.as_bytes().to_vec();
        let receipt_count = batch.receipt_ids.len() as u64;

        // Actual on-chain ABI (from sui_getNormalizedMoveModulesByPackage):
        //   commit_anchor(registry: &mut AnchorRegistry,
        //                 merkle_root: vector<u8>,
        //                 walrus_blob_id: vector<u8>,
        //                 workspace_id: vector<u8>,
        //                 receipt_count: u64,
        //                 clock: &Clock,
        //                 ctx: &mut TxContext)   ← injected, not passed
        let args = serde_json::json!([
            registry_id,                                   // &mut AnchorRegistry
            format!("0x{}", hex::encode(&merkle_bytes)),   // vector<u8>
            "0x",                                          // vector<u8> (empty)
            format!("0x{}", hex::encode(b"recall")),       // vector<u8>
            receipt_count.to_string(),                     // u64
            CLOCK_OBJECT_ID                                // &Clock
        ]);

        let build_payload = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "unsafe_moveCall",
            "params": [
                sender,
                package_id,
                "receipt_anchor",
                "commit_anchor",
                [],
                args,
                null,
                GAS_BUDGET.to_string(),
                "Commit"
            ]
        });

        let build_resp: serde_json::Value = self
            .client
            .post(&self.sui_rpc_url)
            .json(&build_payload)
            .send()
            .await
            .map_err(|e| anyhow!("build POST failed: {e}"))?
            .json()
            .await
            .map_err(|e| anyhow!("build parse failed: {e}"))?;

        if let Some(err) = build_resp.get("error") {
            return Err(anyhow!("Sui RPC build error: {err}"));
        }

        let tx_bytes_b64 = build_resp["result"]["txBytes"]
            .as_str()
            .ok_or_else(|| anyhow!("no txBytes in build response: {build_resp}"))?;

        let raw_tx = base64::engine::general_purpose::STANDARD
            .decode(tx_bytes_b64)
            .map_err(|e| anyhow!("base64 decode txBytes: {e}"))?;

        // Intent: [TransactionData=0, AppId::Sui=0, Version=0] ++ tx bytes.
        // Sui requires signing the BLAKE2b-256 digest of the intent message,
        // not the raw intent message itself.
        let mut intent_msg = vec![0u8, 0u8, 0u8];
        intent_msg.extend_from_slice(&raw_tx);

        use blake2::digest::consts::U32;
        use blake2::{Blake2b, Digest};
        let mut hasher = Blake2b::<U32>::new();
        hasher.update(&intent_msg);
        let intent_digest: [u8; 32] = hasher.finalize().into();

        let sig = signing_key.sign(&intent_digest);
        let pub_bytes = signing_key.verifying_key().to_bytes();

        // Sui compact signature: flag(0x00=Ed25519) ++ sig(64) ++ pubkey(32)
        let mut full_sig = Vec::with_capacity(97);
        full_sig.push(0x00u8);
        full_sig.extend_from_slice(&sig.to_bytes());
        full_sig.extend_from_slice(&pub_bytes);
        let sig_b64 = base64::engine::general_purpose::STANDARD.encode(&full_sig);

        let exec_payload = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "sui_executeTransactionBlock",
            "params": [
                tx_bytes_b64,
                [sig_b64],
                { "showEffects": true, "showEvents": true },
                "WaitForLocalExecution"
            ]
        });

        let exec_resp: serde_json::Value = self
            .client
            .post(&self.sui_rpc_url)
            .json(&exec_payload)
            .send()
            .await
            .map_err(|e| anyhow!("Sui execute POST failed: {e}"))?
            .json()
            .await
            .map_err(|e| anyhow!("Sui execute parse failed: {e}"))?;

        if let Some(err) = exec_resp.get("error") {
            return Err(anyhow!("Sui execute error: {err}"));
        }

        exec_resp["result"]["digest"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| anyhow!("no digest in exec response: {exec_resp}"))
    }
}

// ── Key parsing ───────────────────────────────────────────────────────────────

fn parse_private_key(key_str: &str) -> Result<ed25519_dalek::SigningKey> {
    if key_str.starts_with("suiprivkey") {
        parse_bech32_key(key_str)
    } else {
        let bytes = hex::decode(key_str)
            .map_err(|e| anyhow!("hex decode private key: {e}"))?;
        if bytes.len() != 32 {
            return Err(anyhow!("hex key must be 32 bytes, got {}", bytes.len()));
        }
        ed25519_dalek::SigningKey::try_from(bytes.as_slice())
            .map_err(|e| anyhow!("invalid Ed25519 key: {e}"))
    }
}

fn parse_bech32_key(key_str: &str) -> Result<ed25519_dalek::SigningKey> {
    use bech32::FromBase32;
    let (_hrp, data, _variant) =
        bech32::decode(key_str).map_err(|e| anyhow!("bech32 decode: {e}"))?;
    let bytes =
        Vec::<u8>::from_base32(&data).map_err(|e| anyhow!("bech32 base32 decode: {e}"))?;
    if bytes.len() < 33 {
        return Err(anyhow!("bech32 key payload too short: {} bytes", bytes.len()));
    }
    ed25519_dalek::SigningKey::try_from(&bytes[1..33])
        .map_err(|e| anyhow!("invalid Ed25519 key from bech32: {e}"))
}

fn sui_address_from_ed25519(pub_key: &[u8]) -> String {
    if let Ok(addr) = env::var("RECALL_SUI_SENDER_ADDRESS") {
        if !addr.is_empty() {
            return addr;
        }
    }
    use blake2::digest::consts::U32;
    use blake2::{Blake2b, Digest};
    let mut hasher = Blake2b::<U32>::new();
    hasher.update([0x00u8]);
    hasher.update(pub_key);
    format!("0x{}", hex::encode(hasher.finalize()))
}

/// Build an `UNANCHORED:<reason>` marker. The prefix is opaque enough that the
/// recall-ops CLI, downstream proto consumers, and human readers can never
/// mistake it for a real base58 Sui tx digest.
pub fn unanchored(reason: &str) -> String {
    // Sanitize: spaces and colons in the reason would interfere with display
    // and downstream parsing. Keep it short and shell-safe.
    let safe: String = reason
        .chars()
        .map(|c| match c {
            ' ' | '\t' | '\n' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .take(120)
        .collect();
    format!("UNANCHORED:{safe}")
}

/// True iff `digest` is a synthetic marker produced by [`unanchored`].
pub fn is_unanchored(digest: &str) -> bool {
    digest.starts_with("UNANCHORED:")
}
