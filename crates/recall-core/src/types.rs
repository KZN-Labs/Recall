use serde::{Deserialize, Serialize};

/// Trust level: 1=low, 2=medium, 3=high
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[repr(i32)]
pub enum TrustLevel {
    Low = 1,
    Medium = 2,
    High = 3,
}

impl TrustLevel {
    pub fn from_i32(v: i32) -> Option<Self> {
        match v {
            1 => Some(Self::Low),
            2 => Some(Self::Medium),
            3 => Some(Self::High),
            _ => None,
        }
    }
}

/// Agent role in workspace
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgentRole {
    Reader,
    Writer,
    Admin,
}

/// Workspace topology
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TopologyMode {
    Closed,
    Open,
}

/// Seal encryption status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SealStatus {
    Unsealed,
    Sealed,
}

/// Enforcement stage
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EnforcementStage {
    None,
    Detect,
    Coach,
    Quarantine,
    Evict,
}

impl EnforcementStage {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::None => "NONE",
            Self::Detect => "DETECT",
            Self::Coach => "COACH",
            Self::Quarantine => "QUARANTINE",
            Self::Evict => "EVICT",
        }
    }
}

/// Conflict resolution status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConflictStatus {
    Pending,
    AutoResolved,
    ManuallyResolved,
}

/// Signature role on a receipt
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SignatureRole {
    Actor,
    ControlPlane,
    Supervisor,
    BatchRoot,
}

impl SignatureRole {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Actor => "ACTOR",
            Self::ControlPlane => "CONTROL_PLANE",
            Self::Supervisor => "SUPERVISOR",
            Self::BatchRoot => "BATCH_ROOT",
        }
    }
}
