pub mod builder;
pub mod merkle;
pub mod store;

pub use builder::ReceiptBuilder;

use prost::Message;
use recall_core::ids::ContentHash;
use recall_crypto::{sha256_hex, RecallKeypair};
use recall_proto::{common as common_proto, receipt as receipt_proto};

/// Canonical action kind strings. Use these constants everywhere.
pub mod action_kind {
    pub const MEMORY_WRITE: &str = "memory.write";
    pub const MEMORY_READ: &str = "memory.read";
    pub const MEMORY_CONFLICT_DETECTED: &str = "memory.conflict.detected";
    pub const MEMORY_CONFLICT_RESOLVED: &str = "memory.conflict.resolved";
    pub const WORKSPACE_CREATE: &str = "workspace.create";
    pub const WORKSPACE_SNAPSHOT: &str = "workspace.snapshot";
    pub const WORKSPACE_ROLLBACK: &str = "workspace.rollback";
    pub const HANDOFF_CAPSULE_CREATE: &str = "handoff.capsule.create";
    pub const HANDOFF_CAPSULE_DELIVER: &str = "handoff.capsule.deliver";
    pub const REGISTRY_PUBLISH: &str = "registry.publish";
    pub const REGISTRY_IMPORT: &str = "registry.import";
    pub const AGENT_REGISTER: &str = "agent.register";
    pub const AGENT_REVOKE: &str = "agent.revoke";
    pub const AGENT_OPERATOR_REVOKE: &str = "agent.operator_revoke";
    pub const AGENT_ROTATE_KEY: &str = "agent.rotate_key";
    pub const CAPABILITY_ISSUE: &str = "capability.issue";
    pub const CAPABILITY_ATTENUATE: &str = "capability.attenuate";
    pub const CAPABILITY_REVOKE: &str = "capability.revoke";
    pub const CAPABILITY_CHECK_PASS: &str = "capability.check.pass";
    pub const CAPABILITY_CHECK_DENY: &str = "capability.check.deny";
    pub const GOVERNANCE_CHECK_PASS: &str = "governance.check.pass";
    pub const GOVERNANCE_CHECK_DENY: &str = "governance.check.deny";
    pub const ENFORCEMENT_DETECT: &str = "enforcement.detect";
    pub const ENFORCEMENT_COACH: &str = "enforcement.coach";
    pub const ENFORCEMENT_QUARANTINE: &str = "enforcement.quarantine";
    pub const ENFORCEMENT_EVICT: &str = "enforcement.evict";
    pub const ENFORCEMENT_REVERSE: &str = "enforcement.reverse";
    pub const ANCHOR_COMMIT: &str = "anchor.commit";
}

/// Compute a receipt's content address: SHA-256 of canonical proto bytes
/// with all signature fields cleared. Two implementations observing the same
/// action MUST produce bit-identical IDs.
pub fn compute_receipt_id(proto_msg: &receipt_proto::Receipt) -> ContentHash {
    let mut copy = proto_msg.clone();
    copy.id = None;
    copy.signatures.clear();
    let mut buf = Vec::new();
    copy.encode(&mut buf).expect("prost encode");
    ContentHash(sha256_hex(&buf))
}

/// Sign a receipt's canonical bytes with a keypair and return the updated proto.
pub fn sign_receipt(
    mut proto_msg: receipt_proto::Receipt,
    keypair: &RecallKeypair,
    role: &str,
) -> receipt_proto::Receipt {
    let mut copy = proto_msg.clone();
    copy.id = None;
    copy.signatures.clear();
    let mut buf = Vec::new();
    copy.encode(&mut buf).expect("prost encode");

    let sig_bytes = keypair.sign(&buf).to_bytes();
    proto_msg.signatures.push(common_proto::Signature {
        bytes: sig_bytes.to_vec(),
        role: role.to_string(),
        signer_public_key: keypair.public_key().to_bytes().to_vec(),
    });
    proto_msg
}
