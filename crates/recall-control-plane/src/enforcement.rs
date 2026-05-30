use recall_core::ids::AgentId;
use std::collections::HashMap;
use std::sync::RwLock;

/// Enforcement stage for an agent (reconstructable from receipt stream).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum Stage {
    None,
    Detect,
    Coach,
    Quarantine,
    Evict,
}

impl Stage {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::None => "NONE",
            Self::Detect => "DETECT",
            Self::Coach => "COACH",
            Self::Quarantine => "QUARANTINE",
            Self::Evict => "EVICT",
        }
    }

    #[allow(dead_code)]
    pub fn next(self) -> Self {
        match self {
            Self::None => Self::Detect,
            Self::Detect => Self::Coach,
            Self::Coach => Self::Quarantine,
            Self::Quarantine => Self::Evict,
            Self::Evict => Self::Evict,
        }
    }

    #[allow(dead_code)]
    pub fn previous(self) -> Self {
        match self {
            Self::None | Self::Detect => Self::None,
            Self::Coach => Self::Detect,
            Self::Quarantine => Self::Coach,
            Self::Evict => Self::Quarantine,
        }
    }
}

#[derive(Debug, Default)]
struct AgentEnforcementState {
    stage: Stage,
    deny_count: u32,
    reputation: f64,
}

impl Default for Stage {
    fn default() -> Self {
        Self::None
    }
}

/// In-process enforcement engine.
/// State is reconstructable from enforcement receipts (sum reputation_delta fields).
pub struct EnforcementEngine {
    states: RwLock<HashMap<String, AgentEnforcementState>>,
    /// How many denies before triggering Detect stage.
    deny_threshold: u32,
}

impl Default for EnforcementEngine {
    fn default() -> Self {
        Self {
            states: RwLock::new(HashMap::new()),
            deny_threshold: 3,
        }
    }
}

impl EnforcementEngine {
    /// Record a constitution deny event for an agent. Returns the new stage if escalation occurs.
    pub fn record_deny(&self, agent_id: &AgentId) -> Option<Stage> {
        let mut states = self.states.write().unwrap();
        let state = states.entry(agent_id.0.clone()).or_default();
        state.deny_count += 1;
        if state.deny_count >= self.deny_threshold && state.stage == Stage::None {
            state.stage = Stage::Detect;
            state.reputation -= 0.1;
            return Some(Stage::Detect);
        }
        None
    }

    #[allow(dead_code)]
    pub fn escalate(&self, agent_id: &AgentId) -> Stage {
        let mut states = self.states.write().unwrap();
        let state = states.entry(agent_id.0.clone()).or_default();
        state.stage = state.stage.next();
        state.reputation -= 0.2;
        state.stage
    }

    #[allow(dead_code)]
    pub fn reverse(&self, agent_id: &AgentId) -> Stage {
        let mut states = self.states.write().unwrap();
        let state = states.entry(agent_id.0.clone()).or_default();
        state.stage = state.stage.previous();
        // Residue: reputation doesn't fully recover.
        state.reputation -= 0.05;
        state.stage
    }

    pub fn get_stage(&self, agent_id: &AgentId) -> Stage {
        self.states
            .read()
            .unwrap()
            .get(&agent_id.0)
            .map(|s| s.stage)
            .unwrap_or(Stage::None)
    }

    pub fn get_reputation(&self, agent_id: &AgentId) -> f64 {
        self.states
            .read()
            .unwrap()
            .get(&agent_id.0)
            .map(|s| s.reputation)
            .unwrap_or(1.0)
    }

    pub fn is_blocked(&self, agent_id: &AgentId) -> bool {
        matches!(
            self.get_stage(agent_id),
            Stage::Quarantine | Stage::Evict
        )
    }
}
