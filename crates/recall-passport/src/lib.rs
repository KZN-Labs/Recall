use chrono::{DateTime, Utc};
use prost::Message;
use recall_core::{
    error::RecallError,
    ids::{AgentId, ContentHash, WorkspaceId},
    types::{AgentRole, TrustLevel},
};
use recall_crypto::{sha256_hex, RecallKeypair, RecallPublicKey};
use recall_proto::passport as proto;
use recall_proto::common as common_proto;

pub mod store;

/// Passport lifecycle states.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PassportState {
    Active,
    Revoked { reason: String },
}

/// In-memory representation of a RECALL agent passport.
#[derive(Debug, Clone)]
pub struct Passport {
    pub passport_id: ContentHash,
    pub agent_id: AgentId,
    pub workspace_id: WorkspaceId,
    pub trust_level: TrustLevel,
    pub role: AgentRole,
    pub model_provider: String,
    pub model_name: String,
    pub expires_at: Option<DateTime<Utc>>,
    pub public_key_bytes: Vec<u8>,
    pub state: PassportState,
}

impl Passport {
    /// Build and self-sign a new passport. The agent generates its own keypair.
    pub fn create(
        keypair: &RecallKeypair,
        agent_id: AgentId,
        workspace_id: WorkspaceId,
        trust_level: TrustLevel,
        role: AgentRole,
        model_provider: String,
        model_name: String,
        expires_at: Option<DateTime<Utc>>,
    ) -> Self {
        let public_key_bytes = keypair.public_key().to_bytes().to_vec();

        // Build proto message without signature to compute canonical bytes.
        let mut proto_msg = proto::Passport {
            agent_id: Some(common_proto::AgentId { value: agent_id.0.clone() }),
            agent_public_key: public_key_bytes.clone(),
            workspace_id: Some(common_proto::WorkspaceId { value: workspace_id.0.clone() }),
            trust_level: trust_level as i32,
            role: role_to_proto(role),
            model_provider: model_provider.clone(),
            model_name: model_name.clone(),
            expires_at: expires_at.map(|dt| prost_types::Timestamp {
                seconds: dt.timestamp(),
                nanos: 0,
            }),
            signature: None,
            passport_id: None,
        };

        // Sign the canonical bytes (signatures cleared = None here).
        let canonical = canonical_passport_bytes(&proto_msg);
        let sig_bytes = keypair.sign(&canonical).to_bytes();

        proto_msg.signature = Some(common_proto::Signature {
            bytes: sig_bytes.to_vec(),
            role: "ACTOR".to_string(),
            signer_public_key: public_key_bytes.clone(),
        });

        // Content address = SHA-256 of canonical bytes (sig cleared).
        let passport_id = ContentHash(sha256_hex(&canonical));
        proto_msg.passport_id = Some(common_proto::Hash { hex: passport_id.0.clone() });

        Self {
            passport_id,
            agent_id,
            workspace_id,
            trust_level,
            role,
            model_provider,
            model_name,
            expires_at,
            public_key_bytes,
            state: PassportState::Active,
        }
    }

    /// Verify the self-signature on a passport proto message.
    pub fn verify(proto_msg: &proto::Passport) -> Result<ContentHash, RecallError> {
        let sig_entry = proto_msg
            .signature
            .as_ref()
            .ok_or_else(|| RecallError::InvalidSignature("missing signature".into()))?;

        let pub_key = RecallPublicKey::from_bytes(&sig_entry.signer_public_key)
            .map_err(|e| RecallError::InvalidSignature(e.to_string()))?;

        // Compute canonical bytes with signature cleared.
        let mut msg_copy = proto_msg.clone();
        msg_copy.signature = None;
        msg_copy.passport_id = None;
        let canonical = canonical_passport_bytes(&msg_copy);

        let sig = recall_crypto::RecallSignature::from_bytes(&sig_entry.bytes)
            .map_err(|e| RecallError::InvalidSignature(e.to_string()))?;

        pub_key
            .verify(&canonical, &sig)
            .map_err(|e| RecallError::InvalidSignature(e.to_string()))?;

        Ok(ContentHash(sha256_hex(&canonical)))
    }

    pub fn is_expired(&self) -> bool {
        self.expires_at
            .map(|exp| Utc::now() > exp)
            .unwrap_or(false)
    }
}

fn canonical_passport_bytes(msg: &proto::Passport) -> Vec<u8> {
    let mut buf = Vec::new();
    msg.encode(&mut buf).expect("prost encode");
    buf
}

fn role_to_proto(role: AgentRole) -> i32 {
    match role {
        AgentRole::Reader => 1,
        AgentRole::Writer => 2,
        AgentRole::Admin => 3,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passport_create_and_verify() {
        let kp = RecallKeypair::generate();
        let passport = Passport::create(
            &kp,
            AgentId::new(),
            WorkspaceId::new("test-ws"),
            TrustLevel::Medium,
            AgentRole::Writer,
            "anthropic".into(),
            "claude-sonnet-4".into(),
            None,
        );
        assert_eq!(passport.state, PassportState::Active);
        assert!(!passport.is_expired());
    }
}
