/// RECALL on-chain workspace governance.
///
/// All write-access decisions are made here — not in the control plane.
/// The control plane reads these objects and honours the results; it cannot
/// override them because the rules live on-chain and are enforced by the
/// Move verifier.
///
/// Governance rules (in priority order):
///   1. Quarantined / evicted agents are always blocked.
///   2. Agent role must be WRITER (2), SUPERVISOR (3), or ADMIN (4).
///   3. Agent trust level must meet the workspace minimum.
///   4. PII-tagged entries cannot be written to external scope.
///   5. Low-trust agents or entries matching supervisor_required_tags need countersign.
#[allow(unused_const)]
module recall_workspace_governance::governance {
    use sui::event;
    use sui::clock::{Self, Clock};
    use std::string::{Self, String};

    // ── Role constants ────────────────────────────────────────────────────────
    const ROLE_READER: u8     = 1;
    const ROLE_WRITER: u8     = 2;
    const ROLE_SUPERVISOR: u8 = 3;
    const ROLE_ADMIN: u8      = 4;

    // ── Enforcement stage constants ───────────────────────────────────────────
    const STAGE_NONE:       u8 = 0;
    const STAGE_DETECT:     u8 = 1;
    const STAGE_COACH:      u8 = 2;
    const STAGE_QUARANTINE: u8 = 3;
    const STAGE_EVICT:      u8 = 4;

    // ── Error codes ───────────────────────────────────────────────────────────
    const E_QUARANTINED:            u64 = 1;
    const E_INSUFFICIENT_ROLE:      u64 = 2;
    const E_INSUFFICIENT_TRUST:     u64 = 3;
    const E_PII_EXTERNAL_FORBIDDEN: u64 = 4;
    const E_SUPERVISOR_REQUIRED:    u64 = 5;
    const E_CAPABILITY_EXPIRED:     u64 = 6;
    const E_NOT_OPERATOR:           u64 = 7;

    // ─────────────────────────────────────────────────────────────────────────
    // Objects
    // ─────────────────────────────────────────────────────────────────────────

    /// Per-workspace governance policy. Shared object created by the workspace
    /// operator. Contains rules the control plane must honour.
    public struct WorkspacePolicy has key, store {
        id: UID,
        workspace_id: String,
        /// Minimum trust level required to write (1–3).
        min_trust_level_for_write: u8,
        /// Tags that always require supervisor countersign.
        supervisor_required_tags: vector<String>,
        /// Maximum allowed attenuation depth for capabilities.
        max_attenuation_depth: u8,
        /// Operator address — the only address allowed to mutate this policy.
        operator: address,
    }

    /// Per-agent enforcement state. Shared object updated by the control plane.
    public struct AgentEnforcementRecord has key, store {
        id: UID,
        passport_id: String,
        workspace_id: String,
        /// 0=NONE 1=DETECT 2=COACH 3=QUARANTINE 4=EVICT
        stage: u8,
        /// Running count of governance-check denials.
        deny_count: u64,
        /// Reputation score [0, 1_000_000] where 1_000_000 = 1.0
        reputation_millionths: u64,
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Events
    // ─────────────────────────────────────────────────────────────────────────

    public struct PolicyCreated has copy, drop {
        workspace_id: String,
        min_trust_level: u8,
        operator: address,
    }

    public struct EnforcementUpdated has copy, drop {
        passport_id: String,
        workspace_id: String,
        old_stage: u8,
        new_stage: u8,
        deny_count: u64,
    }

    public struct GovernanceCheckDenied has copy, drop {
        passport_id: String,
        workspace_id: String,
        reason_code: u64,
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Entry functions
    // ─────────────────────────────────────────────────────────────────────────

    /// Create a workspace governance policy. Called once by the workspace operator.
    public fun create_policy(
        workspace_id: vector<u8>,
        min_trust_level_for_write: u8,
        max_attenuation_depth: u8,
        ctx: &mut TxContext,
    ) {
        let ws_str = string::utf8(workspace_id);
        let op = tx_context::sender(ctx);

        event::emit(PolicyCreated {
            workspace_id: ws_str,
            min_trust_level: min_trust_level_for_write,
            operator: op,
        });

        let policy = WorkspacePolicy {
            id: object::new(ctx),
            workspace_id: ws_str,
            min_trust_level_for_write,
            supervisor_required_tags: vector[],
            max_attenuation_depth,
            operator: op,
        };
        transfer::share_object(policy);
    }

    /// Add a tag that always requires supervisor countersign on write.
    public fun add_supervisor_required_tag(
        policy: &mut WorkspacePolicy,
        tag: vector<u8>,
        ctx: &mut TxContext,
    ) {
        assert!(tx_context::sender(ctx) == policy.operator, E_NOT_OPERATOR);
        vector::push_back(&mut policy.supervisor_required_tags, string::utf8(tag));
    }

    /// Create the enforcement record for a newly admitted agent.
    public fun register_agent(
        passport_id: vector<u8>,
        workspace_id: vector<u8>,
        ctx: &mut TxContext,
    ) {
        let record = AgentEnforcementRecord {
            id: object::new(ctx),
            passport_id: string::utf8(passport_id),
            workspace_id: string::utf8(workspace_id),
            stage: STAGE_NONE,
            deny_count: 0,
            reputation_millionths: 1_000_000,
        };
        transfer::share_object(record);
    }

    /// Control plane calls this after a governance-check denial.
    /// Increments deny_count and may escalate the enforcement stage.
    public fun record_deny(
        record: &mut AgentEnforcementRecord,
        deny_threshold: u64,
        ctx: &mut TxContext,
    ) {
        let old_stage = record.stage;
        record.deny_count = record.deny_count + 1;

        if (record.stage < STAGE_EVICT && record.deny_count % deny_threshold == 0) {
            record.stage = record.stage + 1;
        };

        // Reputation degrades by 5% per deny (floored at 0).
        let delta = record.reputation_millionths / 20;
        if (record.reputation_millionths > delta) {
            record.reputation_millionths = record.reputation_millionths - delta;
        } else {
            record.reputation_millionths = 0;
        };

        event::emit(EnforcementUpdated {
            passport_id: record.passport_id,
            workspace_id: record.workspace_id,
            old_stage,
            new_stage: record.stage,
            deny_count: record.deny_count,
        });

        let _ = ctx;
    }

    /// Operator manually escalates or reverses an agent's enforcement stage.
    public fun set_enforcement_stage(
        record: &mut AgentEnforcementRecord,
        policy: &WorkspacePolicy,
        new_stage: u8,
        ctx: &mut TxContext,
    ) {
        assert!(tx_context::sender(ctx) == policy.operator, E_NOT_OPERATOR);
        let old_stage = record.stage;
        record.stage = new_stage;

        event::emit(EnforcementUpdated {
            passport_id: record.passport_id,
            workspace_id: record.workspace_id,
            old_stage,
            new_stage,
            deny_count: record.deny_count,
        });
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Pure view / check functions
    // ─────────────────────────────────────────────────────────────────────────

    /// Returns true if the agent is in a blocked enforcement stage.
    public fun is_quarantined(record: &AgentEnforcementRecord): bool {
        record.stage >= STAGE_QUARANTINE
    }

    /// Returns true if the agent has the minimum role to write.
    public fun has_write_role(role: u8): bool {
        role >= ROLE_WRITER
    }

    /// Returns true if the agent's trust level meets the workspace minimum.
    public fun meets_trust_requirement(
        policy: &WorkspacePolicy,
        trust_level: u8,
    ): bool {
        trust_level >= policy.min_trust_level_for_write
    }

    /// Returns true if writing a PII-tagged entry to external scope is attempted.
    public fun is_pii_external_violation(
        entry_tags: &vector<String>,
        entry_scope: &String,
    ): bool {
        let scope_external = string::utf8(b"external");
        if (*entry_scope != scope_external) {
            return false
        };
        let pii_tag = string::utf8(b"pii");
        vector::contains(entry_tags, &pii_tag)
    }

    /// Returns true if a supervisor countersign is required for this write.
    /// Triggered when:
    ///   - The agent trust level is < 2, OR
    ///   - Any of the entry tags match the policy's supervisor_required_tags.
    public fun requires_supervisor_countersign(
        policy: &WorkspacePolicy,
        trust_level: u8,
        entry_tags: &vector<String>,
    ): bool {
        if (trust_level < 2) {
            return true
        };
        let required = &policy.supervisor_required_tags;
        vector::any!(required, |tag| vector::contains(entry_tags, tag))
    }

    /// Returns true if the capability has not expired.
    public fun is_capability_valid(valid_until_ms: u64, clock: &Clock): bool {
        (clock::timestamp_ms(clock) < valid_until_ms)
    }

    /// Composite check: returns true only if ALL governance conditions pass.
    /// Emits a GovernanceCheckDenied event for the first failing condition.
    public fun check_write_access(
        policy: &WorkspacePolicy,
        record: &AgentEnforcementRecord,
        trust_level: u8,
        role: u8,
        entry_tags: &vector<String>,
        entry_scope: &String,
        has_supervisor_countersign: bool,
    ): bool {
        // Rule 1: quarantine / evict blocks all writes.
        if (is_quarantined(record)) {
            event::emit(GovernanceCheckDenied {
                passport_id: record.passport_id,
                workspace_id: record.workspace_id,
                reason_code: E_QUARANTINED,
            });
            return false
        };

        // Rule 2: role must allow writing.
        if (!has_write_role(role)) {
            event::emit(GovernanceCheckDenied {
                passport_id: record.passport_id,
                workspace_id: record.workspace_id,
                reason_code: E_INSUFFICIENT_ROLE,
            });
            return false
        };

        // Rule 3: trust level must meet workspace minimum.
        if (!meets_trust_requirement(policy, trust_level)) {
            event::emit(GovernanceCheckDenied {
                passport_id: record.passport_id,
                workspace_id: record.workspace_id,
                reason_code: E_INSUFFICIENT_TRUST,
            });
            return false
        };

        // Rule 4: PII must never reach external scope.
        if (is_pii_external_violation(entry_tags, entry_scope)) {
            event::emit(GovernanceCheckDenied {
                passport_id: record.passport_id,
                workspace_id: record.workspace_id,
                reason_code: E_PII_EXTERNAL_FORBIDDEN,
            });
            return false
        };

        // Rule 5: supervisor countersign required for certain writes.
        if (requires_supervisor_countersign(policy, trust_level, entry_tags) &&
            !has_supervisor_countersign)
        {
            event::emit(GovernanceCheckDenied {
                passport_id: record.passport_id,
                workspace_id: record.workspace_id,
                reason_code: E_SUPERVISOR_REQUIRED,
            });
            return false
        };

        true
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Accessors
    // ─────────────────────────────────────────────────────────────────────────

    public fun enforcement_stage(record: &AgentEnforcementRecord): u8 { record.stage }
    public fun deny_count(record: &AgentEnforcementRecord): u64 { record.deny_count }
    public fun reputation(record: &AgentEnforcementRecord): u64 { record.reputation_millionths }
    public fun policy_workspace_id(policy: &WorkspacePolicy): &String { &policy.workspace_id }
    public fun min_trust_level(policy: &WorkspacePolicy): u8 { policy.min_trust_level_for_write }
    public fun policy_operator(policy: &WorkspacePolicy): address { policy.operator }
    public fun supervisor_required_tags(policy: &WorkspacePolicy): &vector<String> {
        &policy.supervisor_required_tags
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Test-only helpers
    // ─────────────────────────────────────────────────────────────────────────

    #[test_only]
    public fun create_policy_for_testing(
        workspace_id: vector<u8>,
        min_trust_level_for_write: u8,
        max_attenuation_depth: u8,
        ctx: &mut TxContext,
    ): WorkspacePolicy {
        WorkspacePolicy {
            id: object::new(ctx),
            workspace_id: string::utf8(workspace_id),
            min_trust_level_for_write,
            supervisor_required_tags: vector[],
            max_attenuation_depth,
            operator: tx_context::sender(ctx),
        }
    }

    #[test_only]
    public fun create_record_for_testing(
        passport_id: vector<u8>,
        workspace_id: vector<u8>,
        stage: u8,
        ctx: &mut TxContext,
    ): AgentEnforcementRecord {
        AgentEnforcementRecord {
            id: object::new(ctx),
            passport_id: string::utf8(passport_id),
            workspace_id: string::utf8(workspace_id),
            stage,
            deny_count: 0,
            reputation_millionths: 1_000_000,
        }
    }

    #[test_only]
    public fun destroy_policy_for_testing(policy: WorkspacePolicy) {
        let WorkspacePolicy { id, workspace_id: _, min_trust_level_for_write: _,
            supervisor_required_tags: _, max_attenuation_depth: _, operator: _ } = policy;
        object::delete(id);
    }

    #[test_only]
    public fun destroy_record_for_testing(record: AgentEnforcementRecord) {
        let AgentEnforcementRecord { id, passport_id: _, workspace_id: _,
            stage: _, deny_count: _, reputation_millionths: _ } = record;
        object::delete(id);
    }

    // Allow tests to add tags directly to a policy without going through the entry function.
    #[test_only]
    public fun add_tag_for_testing(policy: &mut WorkspacePolicy, tag: vector<u8>) {
        vector::push_back(&mut policy.supervisor_required_tags, string::utf8(tag));
    }

    // Allow tests to set stage directly without needing operator auth.
    #[test_only]
    public fun set_stage_for_testing(record: &mut AgentEnforcementRecord, stage: u8) {
        record.stage = stage;
    }
}
