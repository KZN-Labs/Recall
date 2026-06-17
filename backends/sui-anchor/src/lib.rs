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
        use std::str::FromStr;
        use sui_sdk_types::{
            Address, Argument, Command, GasPayment, Identifier, Input, MoveCall,
            ProgrammableTransaction, SharedInput, Transaction,
            TransactionExpiration, TransactionKind,
        };

        // ── 1. Resolve sender, gas coin, gas price, shared object versions ───
        //
        // The convenience builder RPC used to do all of this for us. With a
        // client-built PTB we fetch each piece explicitly.
        let sender_addr = Address::from_str(sender)
            .map_err(|e| anyhow!("invalid sender address {sender}: {e}"))?;
        let package_addr = Address::from_str(package_id)
            .map_err(|e| anyhow!("invalid package id {package_id}: {e}"))?;
        let registry_addr = Address::from_str(registry_id)
            .map_err(|e| anyhow!("invalid registry id {registry_id}: {e}"))?;
        let clock_addr = Address::from_str(CLOCK_OBJECT_ID)
            .map_err(|e| anyhow!("invalid clock id: {e}"))?;

        let gas_coin = self.fetch_gas_coin(sender).await?;
        let gas_price = self.fetch_reference_gas_price().await?;
        let registry_version = self.fetch_shared_initial_version(registry_id).await?;
        // The 0x6 Clock object has initial_shared_version = 1 on every Sui
        // network (it is created in genesis). Avoid a needless RPC.
        let clock_version = 1u64;

        // ── 2. Build the ProgrammableTransaction ─────────────────────────────
        //
        // The Move contract decodes merkle_root / walrus_blob_id / workspace_id
        // with `string::utf8(...)`, so we pass UTF-8 hex strings (not raw hash
        // bytes — those are invalid UTF-8 and the contract aborts).
        //
        // Pure inputs are BCS-encoded scalars/vectors. bcs::to_bytes of a
        // Vec<u8> emits a ULEB128 length prefix followed by the bytes, which
        // is exactly what Sui expects for `vector<u8>` pure args.
        let merkle_bytes_v: Vec<u8> = merkle_root.hex.as_bytes().to_vec();
        let walrus_blob_v: Vec<u8>  = Vec::new();
        let workspace_v:   Vec<u8>  = b"recall".to_vec();
        let receipt_count: u64      = batch.receipt_ids.len() as u64;

        let inputs = vec![
            // Index 0 — &mut AnchorRegistry (shared, mutable)
            Input::Shared(SharedInput::new(registry_addr, registry_version, true)),
            // Index 1 — vector<u8> merkle_root
            Input::Pure(bcs::to_bytes(&merkle_bytes_v)?),
            // Index 2 — vector<u8> walrus_blob_id (empty)
            Input::Pure(bcs::to_bytes(&walrus_blob_v)?),
            // Index 3 — vector<u8> workspace_id
            Input::Pure(bcs::to_bytes(&workspace_v)?),
            // Index 4 — u64 receipt_count
            Input::Pure(bcs::to_bytes(&receipt_count)?),
            // Index 5 — &Clock (shared, immutable)
            Input::Shared(SharedInput::new(clock_addr, clock_version, false)),
        ];

        let move_call = MoveCall {
            package: package_addr,
            module:  Identifier::new("receipt_anchor")
                .map_err(|e| anyhow!("invalid module name: {e}"))?,
            function: Identifier::new("commit_anchor")
                .map_err(|e| anyhow!("invalid function name: {e}"))?,
            type_arguments: vec![],
            arguments: vec![
                Argument::Input(0),
                Argument::Input(1),
                Argument::Input(2),
                Argument::Input(3),
                Argument::Input(4),
                Argument::Input(5),
            ],
        };

        let pt = ProgrammableTransaction {
            inputs,
            commands: vec![Command::MoveCall(move_call)],
        };

        // ── 3. Wrap in Transaction with gas + expiration ─────────────────────
        let tx = Transaction {
            kind: TransactionKind::ProgrammableTransaction(pt),
            sender: sender_addr,
            gas_payment: GasPayment {
                objects: vec![gas_coin],
                owner:   sender_addr,
                price:   gas_price,
                budget:  GAS_BUDGET,
            },
            expiration: TransactionExpiration::None,
        };

        // ── 4. BCS-serialize for submission, intent-hash for signing ─────────
        let tx_bytes = bcs::to_bytes(&tx)
            .map_err(|e| anyhow!("BCS encode transaction: {e}"))?;
        let tx_bytes_b64 = base64::engine::general_purpose::STANDARD.encode(&tx_bytes);

        // signing_digest() handles the intent prefix (TransactionData/V0/Sui) +
        // BCS-encode + BLAKE2b-256 hash, replacing our hand-rolled
        // 3-byte-prefix + manual hashing path used in the convenience-RPC flow.
        let intent_digest = tx.signing_digest();

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

    // ── PTB build helpers ─────────────────────────────────────────────────────

    /// Pick the first owned SUI coin object for the sender, returned as an
    /// ObjectReference suitable for use in GasPayment.
    async fn fetch_gas_coin(&self, sender: &str)
        -> Result<sui_sdk_types::ObjectReference>
    {
        use std::str::FromStr;
        let payload = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 10,
            "method": "suix_getCoins",
            "params": [sender, "0x2::sui::SUI", null, 1]
        });
        let resp: serde_json::Value = self
            .client
            .post(&self.sui_rpc_url)
            .json(&payload)
            .send()
            .await
            .map_err(|e| anyhow!("getCoins POST: {e}"))?
            .json()
            .await
            .map_err(|e| anyhow!("getCoins parse: {e}"))?;
        if let Some(err) = resp.get("error") {
            return Err(anyhow!("suix_getCoins error: {err}"));
        }
        let coin = resp["result"]["data"]
            .as_array()
            .and_then(|a| a.first())
            .ok_or_else(|| anyhow!("no SUI coins owned by {sender}"))?;
        let obj_id = coin["coinObjectId"].as_str()
            .ok_or_else(|| anyhow!("coin missing coinObjectId: {coin}"))?;
        let version_s = coin["version"].as_str()
            .ok_or_else(|| anyhow!("coin missing version: {coin}"))?;
        let digest_s = coin["digest"].as_str()
            .ok_or_else(|| anyhow!("coin missing digest: {coin}"))?;
        let version: u64 = version_s.parse()
            .map_err(|e| anyhow!("parse coin version {version_s}: {e}"))?;
        Ok(sui_sdk_types::ObjectReference::new(
            sui_sdk_types::Address::from_str(obj_id)
                .map_err(|e| anyhow!("parse coin address {obj_id}: {e}"))?,
            version,
            sui_sdk_types::Digest::from_str(digest_s)
                .map_err(|e| anyhow!("parse coin digest {digest_s}: {e}"))?,
        ))
    }

    /// Network's reference gas price (RGP). PTB gas_price must be ≥ this.
    async fn fetch_reference_gas_price(&self) -> Result<u64> {
        let payload = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 11,
            "method": "suix_getReferenceGasPrice",
            "params": []
        });
        let resp: serde_json::Value = self
            .client
            .post(&self.sui_rpc_url)
            .json(&payload)
            .send()
            .await
            .map_err(|e| anyhow!("getReferenceGasPrice POST: {e}"))?
            .json()
            .await
            .map_err(|e| anyhow!("getReferenceGasPrice parse: {e}"))?;
        if let Some(err) = resp.get("error") {
            return Err(anyhow!("suix_getReferenceGasPrice error: {err}"));
        }
        // RPC returns the price as a JSON string (decimal).
        let s = resp["result"].as_str()
            .ok_or_else(|| anyhow!("no gas price in response: {resp}"))?;
        s.parse::<u64>()
            .map_err(|e| anyhow!("parse RGP {s}: {e}"))
    }

    /// initial_shared_version for a shared object. Required by the BCS
    /// encoding of every Shared input — the fullnode rejects the tx if it
    /// disagrees with the network.
    async fn fetch_shared_initial_version(&self, object_id: &str) -> Result<u64> {
        let payload = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 12,
            "method": "sui_getObject",
            "params": [object_id, { "showOwner": true }]
        });
        let resp: serde_json::Value = self
            .client
            .post(&self.sui_rpc_url)
            .json(&payload)
            .send()
            .await
            .map_err(|e| anyhow!("getObject POST: {e}"))?
            .json()
            .await
            .map_err(|e| anyhow!("getObject parse: {e}"))?;
        if let Some(err) = resp.get("error") {
            return Err(anyhow!("sui_getObject error: {err}"));
        }
        // Owner shape: { "Shared": { "initial_shared_version": <u64-or-string> } }
        // Sui RPC may return this as either a JSON number (testnet/mainnet) or
        // a decimal string (older nodes / some explorers). Handle both.
        let raw = &resp["result"]["data"]["owner"]["Shared"]["initial_shared_version"];
        if let Some(n) = raw.as_u64() {
            return Ok(n);
        }
        if let Some(s) = raw.as_str() {
            return s.parse::<u64>()
                .map_err(|e| anyhow!("parse initial_shared_version {s}: {e}"));
        }
        Err(anyhow!(
            "object {object_id} is not a shared object or response missing initial_shared_version: {resp}"
        ))
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
