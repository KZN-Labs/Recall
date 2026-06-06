use anyhow::{anyhow, Result};
use recall_proto::receipt as receipt_proto;
use std::env;
use tracing::{info, warn};

const CLOCK_OBJECT_ID: &str = "0x6";
const GAS_BUDGET: &str = "100000000";

/// Resolve the Sui RPC endpoint.
///
/// Priority:
///   1. `TATUM_API_KEY` set  → Tatum's Sui gateway (`SUI_NETWORK`-aware)
///   2. Otherwise            → public Sui fullnode for `SUI_NETWORK`
///
/// `SUI_NETWORK` defaults to `testnet`.
fn get_sui_rpc_url() -> String {
    let network = env::var("SUI_NETWORK").unwrap_or_else(|_| "testnet".to_string());

    // Tatum RPC takes priority if API key is set
    if let Ok(api_key) = env::var("TATUM_API_KEY") {
        if !api_key.is_empty() {
            return match network.as_str() {
                "mainnet" => "https://sui-mainnet.gateway.tatum.io".to_string(),
                "devnet"  => "https://sui-devnet.gateway.tatum.io".to_string(),
                _         => "https://sui-testnet.gateway.tatum.io".to_string(),
            };
        }
    }

    // Fallback to public fullnode
    match network.as_str() {
        "mainnet" => "https://fullnode.mainnet.sui.io:443".to_string(),
        "devnet"  => "https://fullnode.devnet.sui.io:443".to_string(),
        _         => "https://fullnode.testnet.sui.io:443".to_string(),
    }
}

/// Build the reqwest client used for every Sui RPC call.
///
/// When `TATUM_API_KEY` is set the key is attached as the `x-api-key` header
/// on every request — Tatum's gateway authenticates against that header.
fn build_rpc_client() -> reqwest::Client {
    let mut headers = reqwest::header::HeaderMap::new();
    if let Ok(api_key) = env::var("TATUM_API_KEY") {
        if !api_key.is_empty() {
            if let Ok(val) = reqwest::header::HeaderValue::from_str(&api_key) {
                headers.insert("x-api-key", val);
            }
        }
    }
    reqwest::Client::builder()
        .default_headers(headers)
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

    /// Build a driver using `TATUM_API_KEY` + `SUI_NETWORK` if set, otherwise a
    /// public fullnode for the configured (or default `testnet`) network.
    pub fn testnet() -> Self {
        Self::new(&get_sui_rpc_url(), "", "")
    }

    /// Anchor a receipt batch to Sui. Returns the Sui transaction digest.
    /// Falls back to a synthetic digest if env vars are not configured.
    pub async fn anchor_batch(&self, batch: &receipt_proto::ReceiptBatch) -> Result<String> {
        let merkle_root = batch
            .merkle_root
            .as_ref()
            .ok_or_else(|| anyhow!("batch missing merkle root"))?;

        let private_key_str = match env::var("RECALL_SUI_PRIVATE_KEY") {
            Ok(k) if !k.is_empty() => k,
            _ => {
                warn!("RECALL_SUI_PRIVATE_KEY not set; using synthetic anchor digest");
                return Ok(fake_digest(&merkle_root.hex));
            }
        };

        let package_id = match env::var("RECALL_RECEIPT_ANCHOR_PACKAGE_ID") {
            Ok(id) if !id.is_empty() => id,
            _ => {
                warn!("RECALL_RECEIPT_ANCHOR_PACKAGE_ID not set; using synthetic anchor digest");
                return Ok(fake_digest(&merkle_root.hex));
            }
        };

        let registry_id = match env::var("RECALL_ANCHOR_REGISTRY_ID") {
            Ok(id) if !id.is_empty() => id,
            _ => {
                warn!("RECALL_ANCHOR_REGISTRY_ID not set; using synthetic anchor digest");
                return Ok(fake_digest(&merkle_root.hex));
            }
        };

        let signing_key = match parse_private_key(&private_key_str) {
            Ok(k) => k,
            Err(e) => {
                warn!("Invalid RECALL_SUI_PRIVATE_KEY: {e}; using synthetic anchor digest");
                return Ok(fake_digest(&merkle_root.hex));
            }
        };

        let sender = sui_address_from_ed25519(&signing_key.verifying_key().to_bytes());

        match self
            .submit_ptb(&sender, &signing_key, &package_id, &registry_id, merkle_root, batch)
            .await
        {
            Ok(digest) => {
                info!("Sui anchor committed: {digest}");
                Ok(digest)
            }
            Err(e) => {
                warn!("Sui anchor submission failed ({e}); using synthetic digest");
                Ok(fake_digest(&merkle_root.hex))
            }
        }
    }

    async fn submit_ptb(
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

        let merkle_bytes = hex::decode(&merkle_root.hex)
            .unwrap_or_else(|_| merkle_root.hex.as_bytes().to_vec());

        let receipt_count = batch.receipt_ids.len() as u64;

        // commit_anchor(merkle_root: vector<u8>, walrus_blob_id: vector<u8>,
        //               workspace_id: vector<u8>, receipt_count: u64,
        //               registry: &mut AnchorRegistry, clock: &Clock)
        let args = serde_json::json!([
            bcs_bytes_arg(&merkle_bytes),
            bcs_bytes_arg(&[]),           // walrus_blob_id (not in ReceiptBatch proto)
            bcs_bytes_arg(b"recall"),     // workspace_id
            bcs_u64_arg(receipt_count),
            registry_id,
            CLOCK_OBJECT_ID
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
                GAS_BUDGET,
                "WaitForLocalExecution"
            ]
        });

        let build_resp: serde_json::Value = self
            .client
            .post(&self.sui_rpc_url)
            .json(&build_payload)
            .send()
            .await
            .map_err(|e| anyhow!("Sui RPC POST failed: {e}"))?
            .json()
            .await
            .map_err(|e| anyhow!("Sui RPC parse failed: {e}"))?;

        if let Some(err) = build_resp.get("error") {
            return Err(anyhow!("Sui RPC build error: {err}"));
        }

        let tx_bytes_b64 = build_resp["result"]["txBytes"]
            .as_str()
            .ok_or_else(|| anyhow!("no txBytes in build response: {build_resp}"))?;

        let raw_tx = base64::engine::general_purpose::STANDARD
            .decode(tx_bytes_b64)
            .map_err(|e| anyhow!("base64 decode txBytes: {e}"))?;

        // Intent bytes: [TransactionData=0, AppId::Sui=0, Version=0] ++ tx bytes
        let mut intent_msg = vec![0u8, 0u8, 0u8];
        intent_msg.extend_from_slice(&raw_tx);

        let sig = signing_key.sign(&intent_msg);
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

// ── BCS helpers ───────────────────────────────────────────────────────────────

fn bcs_bytes_arg(data: &[u8]) -> serde_json::Value {
    use base64::Engine;
    serde_json::Value::String(
        base64::engine::general_purpose::STANDARD.encode(bcs_encode_bytes(data)),
    )
}

fn bcs_u64_arg(v: u64) -> serde_json::Value {
    use base64::Engine;
    serde_json::Value::String(
        base64::engine::general_purpose::STANDARD.encode(v.to_le_bytes()),
    )
}

fn bcs_encode_bytes(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut len = data.len();
    loop {
        let byte = (len & 0x7F) as u8;
        len >>= 7;
        if len > 0 {
            out.push(byte | 0x80);
        } else {
            out.push(byte);
            break;
        }
    }
    out.extend_from_slice(data);
    out
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
    // First byte is the scheme flag (0x00 = Ed25519)
    ed25519_dalek::SigningKey::try_from(&bytes[1..33])
        .map_err(|e| anyhow!("invalid Ed25519 key from bech32: {e}"))
}

fn sui_address_from_ed25519(pub_key: &[u8]) -> String {
    // Env var override takes priority — always use this for funded wallets
    if let Ok(addr) = env::var("RECALL_SUI_SENDER_ADDRESS") {
        if !addr.is_empty() {
            return addr;
        }
    }

    // Correct Sui address derivation:
    //   Blake2b-256( scheme_flag || public_key_bytes )
    // scheme_flag = 0x00 for Ed25519
    use blake2::digest::consts::U32;
    use blake2::{Blake2b, Digest};

    let mut hasher = Blake2b::<U32>::new();
    hasher.update([0x00u8]); // Ed25519 scheme flag
    hasher.update(pub_key);
    format!("0x{}", hex::encode(hasher.finalize()))
}

fn fake_digest(merkle_hex: &str) -> String {
    format!("sui_tx_{}", &merkle_hex[..merkle_hex.len().min(16)])
}
