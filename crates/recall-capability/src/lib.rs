pub mod caveat;
pub mod check;
pub mod store;

use chrono::Utc;
use prost::Message;
use recall_core::{
    error::RecallError,
    ids::{ContentHash, WorkspaceId},
};
use recall_crypto::{sha256_hex, RecallKeypair};
use recall_proto::{capability as cap_proto, common as common_proto};

/// Maximum attenuation depth (reject chains deeper than this).
pub const MAX_ATTENUATION_DEPTH: i32 = 8;

/// Issue a new root capability. Only the operator/control-plane calls this.
pub fn issue_capability(
    issuer_passport_id: &ContentHash,
    holder_passport_id: &ContentHash,
    workspace_id: &WorkspaceId,
    scope: cap_proto::CapabilityScope,
    caveats: Vec<cap_proto::Caveat>,
    valid_until: prost_types::Timestamp,
    issuer_keypair: &RecallKeypair,
) -> Result<cap_proto::Capability, RecallError> {
    let now = Utc::now();
    let mut cap = cap_proto::Capability {
        id: None,
        issuer_passport_id: Some(common_proto::Hash { hex: issuer_passport_id.0.clone() }),
        holder_passport_id: Some(common_proto::Hash { hex: holder_passport_id.0.clone() }),
        workspace_id: Some(common_proto::WorkspaceId { value: workspace_id.0.clone() }),
        scope: Some(scope),
        caveats,
        valid_from: Some(prost_types::Timestamp {
            seconds: now.timestamp(),
            nanos: 0,
        }),
        valid_until: Some(valid_until),
        parent_capability_id: None,
        attenuation_depth: 0,
        signature: None,
    };

    // Compute content address with signatures cleared.
    let canonical = canonical_capability_bytes(&cap);
    let id = ContentHash(sha256_hex(&canonical));
    cap.id = Some(common_proto::Hash { hex: id.0.clone() });

    let sig_bytes = issuer_keypair.sign(&canonical).to_bytes();
    cap.signature = Some(common_proto::Signature {
        bytes: sig_bytes.to_vec(),
        role: "ACTOR".to_string(),
        signer_public_key: issuer_keypair.public_key().to_bytes().to_vec(),
    });

    Ok(cap)
}

/// Attenuate a capability: the holder mints a child that narrows (never broadens) scope.
pub fn attenuate(
    parent: &cap_proto::Capability,
    new_holder_passport_id: &ContentHash,
    new_scope: cap_proto::CapabilityScope,
    additional_caveats: Vec<cap_proto::Caveat>,
    new_valid_until: prost_types::Timestamp,
    holder_keypair: &RecallKeypair,
) -> Result<cap_proto::Capability, RecallError> {
    let parent_depth = parent.attenuation_depth;
    if parent_depth >= MAX_ATTENUATION_DEPTH {
        return Err(RecallError::AttenuationDepthExceeded(parent_depth));
    }

    let parent_id = parent
        .id
        .as_ref()
        .ok_or_else(|| RecallError::Internal("parent capability missing ID".into()))?;

    let mut child = cap_proto::Capability {
        id: None,
        issuer_passport_id: parent.holder_passport_id.clone(),
        holder_passport_id: Some(common_proto::Hash { hex: new_holder_passport_id.0.clone() }),
        workspace_id: parent.workspace_id.clone(),
        scope: Some(new_scope),
        caveats: {
            let mut all = parent.caveats.clone();
            all.extend(additional_caveats);
            all
        },
        valid_from: parent.valid_from.clone(),
        valid_until: Some(new_valid_until),
        parent_capability_id: Some(common_proto::Hash { hex: parent_id.hex.clone() }),
        attenuation_depth: parent_depth + 1,
        signature: None,
    };

    let canonical = canonical_capability_bytes(&child);
    let id = ContentHash(sha256_hex(&canonical));
    child.id = Some(common_proto::Hash { hex: id.0 });

    let sig_bytes = holder_keypair.sign(&canonical).to_bytes();
    child.signature = Some(common_proto::Signature {
        bytes: sig_bytes.to_vec(),
        role: "ACTOR".to_string(),
        signer_public_key: holder_keypair.public_key().to_bytes().to_vec(),
    });

    Ok(child)
}

fn canonical_capability_bytes(cap: &cap_proto::Capability) -> Vec<u8> {
    let mut copy = cap.clone();
    copy.id = None;
    copy.signature = None;
    let mut buf = Vec::new();
    copy.encode(&mut buf).expect("prost encode");
    buf
}
