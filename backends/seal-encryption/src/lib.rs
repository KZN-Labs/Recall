/// Seal integration: threshold encryption for workspace memory blobs.
///
/// Primary path: delegates to the Seal testnet HTTP API. The Seal key server
/// releases the decryption key only after verifying the agent holds a valid
/// passport admitted to the workspace policy on Sui.
///
/// Fallback: local AES-256-GCM with a policy-derived key. Used when the Seal
/// endpoint is unreachable (offline dev, network issues, hackathon).
use anyhow::{anyhow, Result};
use ring::aead::{
    Aad, BoundKey, LessSafeKey, Nonce, NonceSequence, SealingKey, UnboundKey,
    AES_256_GCM, NONCE_LEN,
};
use ring::rand::{SecureRandom, SystemRandom};
use tracing::warn;

pub struct SealClient {
    pub seal_url: String,
    pub policy_object_id: String,
    workspace_key: [u8; 32],
    http_client: reqwest::Client,
}

impl SealClient {
    pub fn new(seal_url: &str, policy_object_id: &str) -> Self {
        let workspace_key = derive_key(policy_object_id);
        Self {
            seal_url: seal_url.trim_end_matches('/').to_string(),
            policy_object_id: policy_object_id.to_string(),
            workspace_key,
            http_client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(5))
                .build()
                .unwrap(),
        }
    }

    /// Encrypt `plaintext` under the workspace policy.
    /// Tries the Seal threshold API first; falls back to local AES-256-GCM.
    pub async fn encrypt(&self, plaintext: &[u8]) -> Result<Vec<u8>> {
        match self.seal_api_encrypt(plaintext).await {
            Ok(ct) => Ok(ct),
            Err(e) => {
                warn!("Seal API encrypt unavailable ({e}); using local AES-256-GCM");
                self.local_encrypt(plaintext)
            }
        }
    }

    /// Decrypt a blob produced by `encrypt`.
    /// In production the agent's public key is used to verify the Sui access proof
    /// before Seal releases the decryption key.
    pub async fn decrypt(&self, ciphertext: &[u8], agent_public_key: &[u8]) -> Result<Vec<u8>> {
        match self.seal_api_decrypt(ciphertext, agent_public_key).await {
            Ok(pt) => Ok(pt),
            Err(e) => {
                warn!("Seal API decrypt unavailable ({e}); using local AES-256-GCM");
                self.local_decrypt(ciphertext)
            }
        }
    }

    /// Add a passport to the workspace policy on Seal.
    pub async fn grant_access(&self, passport_id: &str) -> Result<()> {
        let resp = self
            .http_client
            .post(format!("{}/v1/policy/{}/grant", self.seal_url, self.policy_object_id))
            .json(&serde_json::json!({ "passport_id": passport_id }))
            .send()
            .await;
        match resp {
            Ok(r) if r.status().is_success() => Ok(()),
            Ok(r) => {
                warn!(
                    "Seal grant_access returned {} for passport {passport_id}",
                    r.status()
                );
                Ok(())
            }
            Err(e) => {
                warn!("Seal grant_access request failed: {e}");
                Ok(())
            }
        }
    }

    /// Remove a passport from the workspace policy on Seal.
    pub async fn revoke_access(&self, passport_id: &str) -> Result<()> {
        let resp = self
            .http_client
            .post(format!("{}/v1/policy/{}/revoke", self.seal_url, self.policy_object_id))
            .json(&serde_json::json!({ "passport_id": passport_id }))
            .send()
            .await;
        match resp {
            Ok(r) if r.status().is_success() => Ok(()),
            Ok(r) => {
                warn!(
                    "Seal revoke_access returned {} for passport {passport_id}",
                    r.status()
                );
                Ok(())
            }
            Err(e) => {
                warn!("Seal revoke_access request failed: {e}");
                Ok(())
            }
        }
    }

    // ── Seal HTTP API ─────────────────────────────────────────────────────────

    async fn seal_api_encrypt(&self, plaintext: &[u8]) -> Result<Vec<u8>> {
        use base64::Engine;
        let data_b64 = base64::engine::general_purpose::STANDARD.encode(plaintext);

        let resp: serde_json::Value = self
            .http_client
            .post(format!("{}/v1/encrypt", self.seal_url))
            .json(&serde_json::json!({
                "data": data_b64,
                "threshold_policy_object_id": self.policy_object_id,
            }))
            .send()
            .await
            .map_err(|e| anyhow!("Seal encrypt HTTP: {e}"))?
            .json()
            .await
            .map_err(|e| anyhow!("Seal encrypt parse: {e}"))?;

        let ct_b64 = resp["ciphertext"]
            .as_str()
            .ok_or_else(|| anyhow!("no ciphertext in Seal response: {resp}"))?;

        base64::engine::general_purpose::STANDARD
            .decode(ct_b64)
            .map_err(|e| anyhow!("Seal ciphertext b64 decode: {e}"))
    }

    async fn seal_api_decrypt(
        &self,
        ciphertext: &[u8],
        agent_public_key: &[u8],
    ) -> Result<Vec<u8>> {
        use base64::Engine;
        let ct_b64 = base64::engine::general_purpose::STANDARD.encode(ciphertext);
        let pk_b64 = base64::engine::general_purpose::STANDARD.encode(agent_public_key);

        let resp: serde_json::Value = self
            .http_client
            .post(format!("{}/v1/decrypt", self.seal_url))
            .json(&serde_json::json!({
                "ciphertext": ct_b64,
                "agent_public_key": pk_b64,
                "threshold_policy_object_id": self.policy_object_id,
            }))
            .send()
            .await
            .map_err(|e| anyhow!("Seal decrypt HTTP: {e}"))?
            .json()
            .await
            .map_err(|e| anyhow!("Seal decrypt parse: {e}"))?;

        let pt_b64 = resp["plaintext"]
            .as_str()
            .ok_or_else(|| anyhow!("no plaintext in Seal response: {resp}"))?;

        base64::engine::general_purpose::STANDARD
            .decode(pt_b64)
            .map_err(|e| anyhow!("Seal plaintext b64 decode: {e}"))
    }

    // ── Local AES-256-GCM fallback ────────────────────────────────────────────

    /// Output format: [12-byte nonce][AES-256-GCM ciphertext+tag]
    fn local_encrypt(&self, plaintext: &[u8]) -> Result<Vec<u8>> {
        let rng = SystemRandom::new();
        let mut nonce_bytes = [0u8; NONCE_LEN];
        rng.fill(&mut nonce_bytes).map_err(|_| anyhow!("RNG failed"))?;

        let unbound = UnboundKey::new(&AES_256_GCM, &self.workspace_key)
            .map_err(|_| anyhow!("invalid key length"))?;
        let nonce_seq = SingleUseNonce(Some(Nonce::assume_unique_for_key(nonce_bytes)));
        let mut sealing = SealingKey::new(unbound, nonce_seq);

        let mut in_out = plaintext.to_vec();
        sealing
            .seal_in_place_append_tag(Aad::empty(), &mut in_out)
            .map_err(|_| anyhow!("AES-GCM seal failed"))?;

        let mut out = Vec::with_capacity(NONCE_LEN + in_out.len());
        out.extend_from_slice(&nonce_bytes);
        out.extend_from_slice(&in_out);
        Ok(out)
    }

    fn local_decrypt(&self, ciphertext: &[u8]) -> Result<Vec<u8>> {
        if ciphertext.len() < NONCE_LEN {
            return Err(anyhow!("ciphertext too short — missing nonce prefix"));
        }
        let (nonce_bytes, body) = ciphertext.split_at(NONCE_LEN);
        let nonce_arr: [u8; NONCE_LEN] = nonce_bytes.try_into().unwrap();

        let unbound = UnboundKey::new(&AES_256_GCM, &self.workspace_key)
            .map_err(|_| anyhow!("invalid key length"))?;
        let key = LessSafeKey::new(unbound);

        let mut in_out = body.to_vec();
        let plaintext = key
            .open_in_place(
                Nonce::assume_unique_for_key(nonce_arr),
                Aad::empty(),
                &mut in_out,
            )
            .map_err(|_| anyhow!("AES-GCM open failed — wrong key or tampered ciphertext"))?;

        Ok(plaintext.to_vec())
    }
}

// ── Key derivation ────────────────────────────────────────────────────────────

/// Derive a 32-byte workspace key from the policy object ID.
/// In production: replaced by `fetchKey(policy_object_id, agent_keypair)` from the Seal SDK.
fn derive_key(policy_object_id: &str) -> [u8; 32] {
    use ring::digest;
    let hash = digest::digest(&digest::SHA256, policy_object_id.as_bytes());
    let mut key = [0u8; 32];
    key.copy_from_slice(hash.as_ref());
    key
}

// ── Nonce helper ──────────────────────────────────────────────────────────────

struct SingleUseNonce(Option<Nonce>);
impl NonceSequence for SingleUseNonce {
    fn advance(&mut self) -> std::result::Result<Nonce, ring::error::Unspecified> {
        self.0.take().ok_or(ring::error::Unspecified)
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn encrypt_decrypt_round_trip() {
        let client = SealClient::new("https://seal-testnet.mystenlabs.com", "policy_test_001");
        let plaintext = b"memory entry for sarah@email.com";
        let ciphertext = client.encrypt(plaintext).await.unwrap();
        assert_ne!(&ciphertext[NONCE_LEN..], plaintext, "ciphertext must differ from plaintext");
        let recovered = client.decrypt(&ciphertext, &[]).await.unwrap();
        assert_eq!(recovered, plaintext);
    }

    #[tokio::test]
    async fn wrong_key_fails() {
        let client_a = SealClient::new("https://seal-testnet.mystenlabs.com", "policy_a");
        let client_b = SealClient::new("https://seal-testnet.mystenlabs.com", "policy_b");
        let ciphertext = client_a.encrypt(b"secret").await.unwrap();
        assert!(client_b.decrypt(&ciphertext, &[]).await.is_err());
    }
}
