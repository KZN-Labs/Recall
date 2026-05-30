use thiserror::Error;

#[derive(Debug, Error)]
pub enum RecallError {
    #[error("invalid hash: {0}")]
    InvalidHash(String),

    #[error("invalid signature: {0}")]
    InvalidSignature(String),

    #[error("capability denied: {0}")]
    CapabilityDenied(String),

    #[error("governance denied: reason={reason}")]
    GovernanceDenied { reason: String },

    #[error("passport not found: {0}")]
    PassportNotFound(String),

    #[error("capability not found: {0}")]
    CapabilityNotFound(String),

    #[error("capability revoked: {0}")]
    CapabilityRevoked(String),

    #[error("capability expired")]
    CapabilityExpired,

    #[error("attenuation depth exceeded: max=8, got={0}")]
    AttenuationDepthExceeded(i32),

    #[error("serialization error: {0}")]
    Serialization(String),

    #[error("storage error: {0}")]
    Storage(String),

    #[error("conflict detected: entity={entity}, signal_a={signal_a}, signal_b={signal_b}")]
    ConflictDetected {
        entity: String,
        signal_a: String,
        signal_b: String,
    },

    #[error("enforcement: agent {agent_id} is {stage}")]
    EnforcementBlocked { agent_id: String, stage: String },

    #[error("agent quarantined: {0}")]
    AgentQuarantined(String),

    #[error("missing supervisor countersign")]
    MissingSupervisorCountersign,

    #[error("internal error: {0}")]
    Internal(String),
}
