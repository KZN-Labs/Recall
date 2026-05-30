#[test_only]
module recall_workspace_governance::governance_tests {
    use recall_workspace_governance::governance::{
        Self, WorkspacePolicy, AgentEnforcementRecord,
    };
    use sui::test_scenario::{Self};
    use sui::clock;
    use std::string;

    const OPERATOR: address = @0xDEAD;
    const AGENT:    address = @0xBEEF;

    // ── helpers ───────────────────────────────────────────────────────────────

    /// Build a default policy: min_trust=2, max_attenuation=8, no special tags.
    fun make_policy(scenario: &mut sui::test_scenario::Scenario): WorkspacePolicy {
        governance::create_policy_for_testing(
            b"ws_test", 2, 8, test_scenario::ctx(scenario),
        )
    }

    /// Build a record at STAGE_NONE.
    fun make_record(stage: u8, scenario: &mut sui::test_scenario::Scenario): AgentEnforcementRecord {
        governance::create_record_for_testing(
            b"pp_agent", b"ws_test", stage, test_scenario::ctx(scenario),
        )
    }

    /// Make a simple tag vector with one entry.
    fun tags(tag: vector<u8>): vector<string::String> {
        vector[string::utf8(tag)]
    }

    /// Make an empty tag vector.
    fun no_tags(): vector<string::String> {
        vector[]
    }

    // ── accessor sanity ───────────────────────────────────────────────────────

    #[test]
    fun test_policy_accessors() {
        let mut scenario = test_scenario::begin(OPERATOR);
        let policy = make_policy(&mut scenario);

        assert!(*governance::policy_workspace_id(&policy) == string::utf8(b"ws_test"), 0);
        assert!(governance::min_trust_level(&policy) == 2, 1);
        assert!(governance::policy_operator(&policy) == OPERATOR, 2);

        governance::destroy_policy_for_testing(policy);
        test_scenario::end(scenario);
    }

    #[test]
    fun test_record_accessors_initial_state() {
        let mut scenario = test_scenario::begin(OPERATOR);
        let record = make_record(0, &mut scenario);

        assert!(governance::enforcement_stage(&record) == 0, 3);
        assert!(governance::deny_count(&record) == 0, 4);
        assert!(governance::reputation(&record) == 1_000_000, 5);
        assert!(!governance::is_quarantined(&record), 6);

        governance::destroy_record_for_testing(record);
        test_scenario::end(scenario);
    }

    // ── is_quarantined ────────────────────────────────────────────────────────

    #[test]
    fun test_stage_none_not_quarantined() {
        let mut scenario = test_scenario::begin(OPERATOR);
        let record = make_record(0, &mut scenario);
        assert!(!governance::is_quarantined(&record), 7);
        governance::destroy_record_for_testing(record);
        test_scenario::end(scenario);
    }

    #[test]
    fun test_stage_detect_not_quarantined() {
        let mut scenario = test_scenario::begin(OPERATOR);
        let record = make_record(1, &mut scenario);
        assert!(!governance::is_quarantined(&record), 8);
        governance::destroy_record_for_testing(record);
        test_scenario::end(scenario);
    }

    #[test]
    fun test_stage_coach_not_quarantined() {
        let mut scenario = test_scenario::begin(OPERATOR);
        let record = make_record(2, &mut scenario);
        assert!(!governance::is_quarantined(&record), 9);
        governance::destroy_record_for_testing(record);
        test_scenario::end(scenario);
    }

    #[test]
    fun test_stage_quarantine_is_quarantined() {
        let mut scenario = test_scenario::begin(OPERATOR);
        let record = make_record(3, &mut scenario);
        assert!(governance::is_quarantined(&record), 10);
        governance::destroy_record_for_testing(record);
        test_scenario::end(scenario);
    }

    #[test]
    fun test_stage_evict_is_quarantined() {
        let mut scenario = test_scenario::begin(OPERATOR);
        let record = make_record(4, &mut scenario);
        assert!(governance::is_quarantined(&record), 11);
        governance::destroy_record_for_testing(record);
        test_scenario::end(scenario);
    }

    // ── has_write_role ────────────────────────────────────────────────────────

    #[test]
    fun test_reader_cannot_write() {
        assert!(!governance::has_write_role(1), 12);
    }

    #[test]
    fun test_writer_can_write() {
        assert!(governance::has_write_role(2), 13);
    }

    #[test]
    fun test_supervisor_can_write() {
        assert!(governance::has_write_role(3), 14);
    }

    #[test]
    fun test_admin_can_write() {
        assert!(governance::has_write_role(4), 15);
    }

    // ── meets_trust_requirement ───────────────────────────────────────────────

    #[test]
    fun test_trust_below_minimum_fails() {
        let mut scenario = test_scenario::begin(OPERATOR);
        let policy = make_policy(&mut scenario); // min=2
        assert!(!governance::meets_trust_requirement(&policy, 1), 16);
        governance::destroy_policy_for_testing(policy);
        test_scenario::end(scenario);
    }

    #[test]
    fun test_trust_at_minimum_passes() {
        let mut scenario = test_scenario::begin(OPERATOR);
        let policy = make_policy(&mut scenario); // min=2
        assert!(governance::meets_trust_requirement(&policy, 2), 17);
        governance::destroy_policy_for_testing(policy);
        test_scenario::end(scenario);
    }

    #[test]
    fun test_trust_above_minimum_passes() {
        let mut scenario = test_scenario::begin(OPERATOR);
        let policy = make_policy(&mut scenario); // min=2
        assert!(governance::meets_trust_requirement(&policy, 3), 18);
        governance::destroy_policy_for_testing(policy);
        test_scenario::end(scenario);
    }

    // ── is_pii_external_violation ─────────────────────────────────────────────

    #[test]
    fun test_pii_external_is_violation() {
        let t = tags(b"pii");
        let scope = string::utf8(b"external");
        assert!(governance::is_pii_external_violation(&t, &scope), 19);
    }

    #[test]
    fun test_pii_internal_is_not_violation() {
        let t = tags(b"pii");
        let scope = string::utf8(b"internal");
        assert!(!governance::is_pii_external_violation(&t, &scope), 20);
    }

    #[test]
    fun test_non_pii_external_is_not_violation() {
        let t = tags(b"customer");
        let scope = string::utf8(b"external");
        assert!(!governance::is_pii_external_violation(&t, &scope), 21);
    }

    #[test]
    fun test_empty_tags_external_is_not_violation() {
        let t = no_tags();
        let scope = string::utf8(b"external");
        assert!(!governance::is_pii_external_violation(&t, &scope), 22);
    }

    // ── requires_supervisor_countersign ──────────────────────────────────────

    #[test]
    fun test_low_trust_requires_supervisor() {
        let mut scenario = test_scenario::begin(OPERATOR);
        let policy = make_policy(&mut scenario);
        let t = no_tags();
        assert!(governance::requires_supervisor_countersign(&policy, 1, &t), 23);
        governance::destroy_policy_for_testing(policy);
        test_scenario::end(scenario);
    }

    #[test]
    fun test_standard_trust_no_tags_no_supervisor() {
        let mut scenario = test_scenario::begin(OPERATOR);
        let policy = make_policy(&mut scenario);
        let t = no_tags();
        assert!(!governance::requires_supervisor_countersign(&policy, 2, &t), 24);
        governance::destroy_policy_for_testing(policy);
        test_scenario::end(scenario);
    }

    #[test]
    fun test_supervisor_tag_triggers_requirement() {
        let mut scenario = test_scenario::begin(OPERATOR);
        let mut policy = make_policy(&mut scenario);
        governance::add_tag_for_testing(&mut policy, b"high_value");
        let t = tags(b"high_value");
        assert!(governance::requires_supervisor_countersign(&policy, 2, &t), 25);
        governance::destroy_policy_for_testing(policy);
        test_scenario::end(scenario);
    }

    #[test]
    fun test_non_supervisor_tag_no_requirement() {
        let mut scenario = test_scenario::begin(OPERATOR);
        let mut policy = make_policy(&mut scenario);
        governance::add_tag_for_testing(&mut policy, b"high_value");
        let t = tags(b"customer"); // different tag
        assert!(!governance::requires_supervisor_countersign(&policy, 2, &t), 26);
        governance::destroy_policy_for_testing(policy);
        test_scenario::end(scenario);
    }

    // ── is_capability_valid ───────────────────────────────────────────────────

    #[test]
    fun test_capability_not_yet_expired() {
        let mut scenario = test_scenario::begin(OPERATOR);
        let mut clk = clock::create_for_testing(test_scenario::ctx(&mut scenario));
        clock::set_for_testing(&mut clk, 1_000);
        assert!(governance::is_capability_valid(2_000, &clk), 27);
        clock::destroy_for_testing(clk);
        test_scenario::end(scenario);
    }

    #[test]
    fun test_capability_expired() {
        let mut scenario = test_scenario::begin(OPERATOR);
        let mut clk = clock::create_for_testing(test_scenario::ctx(&mut scenario));
        clock::set_for_testing(&mut clk, 5_000);
        assert!(!governance::is_capability_valid(4_999, &clk), 28);
        clock::destroy_for_testing(clk);
        test_scenario::end(scenario);
    }

    #[test]
    fun test_capability_expiry_at_exact_boundary_is_expired() {
        let mut scenario = test_scenario::begin(OPERATOR);
        let mut clk = clock::create_for_testing(test_scenario::ctx(&mut scenario));
        clock::set_for_testing(&mut clk, 5_000);
        // valid_until_ms == current_ms means expired (strict less-than)
        assert!(!governance::is_capability_valid(5_000, &clk), 29);
        clock::destroy_for_testing(clk);
        test_scenario::end(scenario);
    }

    // ── check_write_access (composite) ───────────────────────────────────────

    #[test]
    fun test_normal_write_allowed() {
        let mut scenario = test_scenario::begin(OPERATOR);
        let policy = make_policy(&mut scenario);   // min_trust=2
        let record = make_record(0, &mut scenario); // NONE
        let t = tags(b"customer");
        let scope = string::utf8(b"internal");

        let result = governance::check_write_access(
            &policy, &record,
            2,    // trust_level
            2,    // WRITER role
            &t, &scope,
            false, // no supervisor countersign needed
        );
        assert!(result, 30);

        governance::destroy_policy_for_testing(policy);
        governance::destroy_record_for_testing(record);
        test_scenario::end(scenario);
    }

    #[test]
    fun test_quarantined_agent_blocked() {
        let mut scenario = test_scenario::begin(OPERATOR);
        let policy = make_policy(&mut scenario);
        let record = make_record(3, &mut scenario); // QUARANTINE
        let t = no_tags();
        let scope = string::utf8(b"internal");

        let result = governance::check_write_access(
            &policy, &record, 2, 2, &t, &scope, false,
        );
        assert!(!result, 31);

        governance::destroy_policy_for_testing(policy);
        governance::destroy_record_for_testing(record);
        test_scenario::end(scenario);
    }

    #[test]
    fun test_evicted_agent_blocked() {
        let mut scenario = test_scenario::begin(OPERATOR);
        let policy = make_policy(&mut scenario);
        let record = make_record(4, &mut scenario); // EVICT
        let t = no_tags();
        let scope = string::utf8(b"internal");

        let result = governance::check_write_access(
            &policy, &record, 3, 4, &t, &scope, true,
        );
        assert!(!result, 32);

        governance::destroy_policy_for_testing(policy);
        governance::destroy_record_for_testing(record);
        test_scenario::end(scenario);
    }

    #[test]
    fun test_reader_role_blocked() {
        let mut scenario = test_scenario::begin(OPERATOR);
        let policy = make_policy(&mut scenario);
        let record = make_record(0, &mut scenario);
        let t = no_tags();
        let scope = string::utf8(b"internal");

        let result = governance::check_write_access(
            &policy, &record, 2, 1, &t, &scope, false, // role=READER
        );
        assert!(!result, 33);

        governance::destroy_policy_for_testing(policy);
        governance::destroy_record_for_testing(record);
        test_scenario::end(scenario);
    }

    #[test]
    fun test_insufficient_trust_blocked() {
        let mut scenario = test_scenario::begin(OPERATOR);
        let policy = make_policy(&mut scenario); // min_trust=2
        let record = make_record(0, &mut scenario);
        let t = no_tags();
        let scope = string::utf8(b"internal");

        let result = governance::check_write_access(
            &policy, &record, 1, 2, &t, &scope, true, // trust=1 < min=2
        );
        assert!(!result, 34);

        governance::destroy_policy_for_testing(policy);
        governance::destroy_record_for_testing(record);
        test_scenario::end(scenario);
    }

    #[test]
    fun test_pii_to_external_scope_blocked() {
        let mut scenario = test_scenario::begin(OPERATOR);
        let policy = make_policy(&mut scenario);
        let record = make_record(0, &mut scenario);
        let t = tags(b"pii");
        let scope = string::utf8(b"external");

        let result = governance::check_write_access(
            &policy, &record, 2, 2, &t, &scope, false,
        );
        assert!(!result, 35);

        governance::destroy_policy_for_testing(policy);
        governance::destroy_record_for_testing(record);
        test_scenario::end(scenario);
    }

    #[test]
    fun test_pii_to_internal_scope_allowed() {
        let mut scenario = test_scenario::begin(OPERATOR);
        let policy = make_policy(&mut scenario);
        let record = make_record(0, &mut scenario);
        let t = tags(b"pii");
        let scope = string::utf8(b"internal");

        let result = governance::check_write_access(
            &policy, &record, 2, 2, &t, &scope, false,
        );
        assert!(result, 36);

        governance::destroy_policy_for_testing(policy);
        governance::destroy_record_for_testing(record);
        test_scenario::end(scenario);
    }

    #[test]
    fun test_low_trust_without_supervisor_blocked() {
        let mut scenario = test_scenario::begin(OPERATOR);
        let policy = make_policy(&mut scenario); // min_trust=2 but we pass trust=1
        // We need policy min_trust=1 so trust check passes, but supervisor check fires.
        let policy1 = governance::create_policy_for_testing(
            b"ws_t1", 1, 8, test_scenario::ctx(&mut scenario),
        );
        let record = make_record(0, &mut scenario);
        let t = no_tags();
        let scope = string::utf8(b"internal");

        // trust=1 → requires_supervisor_countersign = true, no_countersign → blocked
        let result = governance::check_write_access(
            &policy1, &record, 1, 2, &t, &scope, false,
        );
        assert!(!result, 37);

        governance::destroy_policy_for_testing(policy);
        governance::destroy_policy_for_testing(policy1);
        governance::destroy_record_for_testing(record);
        test_scenario::end(scenario);
    }

    #[test]
    fun test_low_trust_with_supervisor_allowed() {
        let mut scenario = test_scenario::begin(OPERATOR);
        // min_trust=1 so trust check passes
        let policy = governance::create_policy_for_testing(
            b"ws_t1", 1, 8, test_scenario::ctx(&mut scenario),
        );
        let record = make_record(0, &mut scenario);
        let t = no_tags();
        let scope = string::utf8(b"internal");

        let result = governance::check_write_access(
            &policy, &record, 1, 2, &t, &scope, true, // has_supervisor=true
        );
        assert!(result, 38);

        governance::destroy_policy_for_testing(policy);
        governance::destroy_record_for_testing(record);
        test_scenario::end(scenario);
    }

    #[test]
    fun test_admin_role_passes_write_check() {
        let mut scenario = test_scenario::begin(OPERATOR);
        let policy = make_policy(&mut scenario);
        let record = make_record(0, &mut scenario);
        let t = no_tags();
        let scope = string::utf8(b"internal");

        let result = governance::check_write_access(
            &policy, &record, 3, 4, &t, &scope, false, // role=ADMIN
        );
        assert!(result, 39);

        governance::destroy_policy_for_testing(policy);
        governance::destroy_record_for_testing(record);
        test_scenario::end(scenario);
    }

    // ── record_deny + enforcement escalation ──────────────────────────────────

    #[test]
    fun test_record_deny_increments_count() {
        let mut scenario = test_scenario::begin(OPERATOR);
        let mut record = make_record(0, &mut scenario);

        governance::record_deny(&mut record, 10, test_scenario::ctx(&mut scenario));
        assert!(governance::deny_count(&record) == 1, 40);

        governance::record_deny(&mut record, 10, test_scenario::ctx(&mut scenario));
        assert!(governance::deny_count(&record) == 2, 41);

        governance::destroy_record_for_testing(record);
        test_scenario::end(scenario);
    }

    #[test]
    fun test_deny_does_not_escalate_before_threshold() {
        let mut scenario = test_scenario::begin(OPERATOR);
        let mut record = make_record(0, &mut scenario);

        // threshold=3: 2 denies should not escalate
        governance::record_deny(&mut record, 3, test_scenario::ctx(&mut scenario));
        governance::record_deny(&mut record, 3, test_scenario::ctx(&mut scenario));
        assert!(governance::enforcement_stage(&record) == 0, 42); // still NONE

        governance::destroy_record_for_testing(record);
        test_scenario::end(scenario);
    }

    #[test]
    fun test_deny_escalates_at_threshold() {
        let mut scenario = test_scenario::begin(OPERATOR);
        let mut record = make_record(0, &mut scenario);

        // threshold=3: 3rd deny should escalate NONE → DETECT
        governance::record_deny(&mut record, 3, test_scenario::ctx(&mut scenario));
        governance::record_deny(&mut record, 3, test_scenario::ctx(&mut scenario));
        governance::record_deny(&mut record, 3, test_scenario::ctx(&mut scenario));
        assert!(governance::enforcement_stage(&record) == 1, 43); // DETECT

        governance::destroy_record_for_testing(record);
        test_scenario::end(scenario);
    }

    #[test]
    fun test_deny_escalates_through_stages() {
        let mut scenario = test_scenario::begin(OPERATOR);
        let mut record = make_record(0, &mut scenario);
        let threshold: u64 = 1; // escalate on every deny

        governance::record_deny(&mut record, threshold, test_scenario::ctx(&mut scenario));
        assert!(governance::enforcement_stage(&record) == 1, 44); // DETECT

        governance::record_deny(&mut record, threshold, test_scenario::ctx(&mut scenario));
        assert!(governance::enforcement_stage(&record) == 2, 45); // COACH

        governance::record_deny(&mut record, threshold, test_scenario::ctx(&mut scenario));
        assert!(governance::enforcement_stage(&record) == 3, 46); // QUARANTINE

        governance::record_deny(&mut record, threshold, test_scenario::ctx(&mut scenario));
        assert!(governance::enforcement_stage(&record) == 4, 47); // EVICT

        // Should not exceed EVICT (4)
        governance::record_deny(&mut record, threshold, test_scenario::ctx(&mut scenario));
        assert!(governance::enforcement_stage(&record) == 4, 48);

        governance::destroy_record_for_testing(record);
        test_scenario::end(scenario);
    }

    #[test]
    fun test_reputation_degrades_on_deny() {
        let mut scenario = test_scenario::begin(OPERATOR);
        let mut record = make_record(0, &mut scenario);

        let initial = governance::reputation(&record);
        assert!(initial == 1_000_000, 49);

        governance::record_deny(&mut record, 10, test_scenario::ctx(&mut scenario));
        let after = governance::reputation(&record);
        // 5% of 1_000_000 = 50_000; new rep = 950_000
        assert!(after == 950_000, 50);

        governance::destroy_record_for_testing(record);
        test_scenario::end(scenario);
    }

    // ── set_enforcement_stage (operator control) ──────────────────────────────

    #[test]
    fun test_operator_can_set_stage() {
        let mut scenario = test_scenario::begin(OPERATOR);
        let policy = make_policy(&mut scenario);
        let mut record = make_record(0, &mut scenario);

        governance::set_enforcement_stage(
            &mut record, &policy, 3, test_scenario::ctx(&mut scenario),
        );
        assert!(governance::enforcement_stage(&record) == 3, 51); // QUARANTINE

        governance::destroy_policy_for_testing(policy);
        governance::destroy_record_for_testing(record);
        test_scenario::end(scenario);
    }

    #[test]
    fun test_operator_can_reverse_stage() {
        let mut scenario = test_scenario::begin(OPERATOR);
        let policy = make_policy(&mut scenario);
        let mut record = make_record(3, &mut scenario); // start at QUARANTINE

        governance::set_enforcement_stage(
            &mut record, &policy, 0, test_scenario::ctx(&mut scenario),
        );
        assert!(governance::enforcement_stage(&record) == 0, 52); // back to NONE

        governance::destroy_policy_for_testing(policy);
        governance::destroy_record_for_testing(record);
        test_scenario::end(scenario);
    }

    #[test]
    #[expected_failure(abort_code = recall_workspace_governance::governance::E_NOT_OPERATOR)]
    fun test_non_operator_cannot_set_stage() {
        let mut scenario = test_scenario::begin(OPERATOR);
        let policy = make_policy(&mut scenario);

        // Switch to AGENT context.
        test_scenario::next_tx(&mut scenario, AGENT);
        let mut record = make_record(0, &mut scenario);

        // AGENT tries to set stage using OPERATOR's policy — should abort.
        governance::set_enforcement_stage(
            &mut record, &policy, 3, test_scenario::ctx(&mut scenario),
        );

        governance::destroy_policy_for_testing(policy);
        governance::destroy_record_for_testing(record);
        test_scenario::end(scenario);
    }

    #[test]
    #[expected_failure(abort_code = recall_workspace_governance::governance::E_NOT_OPERATOR)]
    fun test_non_operator_cannot_add_supervisor_tag() {
        let mut scenario = test_scenario::begin(OPERATOR);
        let mut policy = make_policy(&mut scenario);

        // Switch to AGENT context and try to mutate the policy via entry function.
        test_scenario::next_tx(&mut scenario, AGENT);
        governance::add_supervisor_required_tag(
            &mut policy,
            b"high_value",
            test_scenario::ctx(&mut scenario),
        );

        governance::destroy_policy_for_testing(policy);
        test_scenario::end(scenario);
    }

    // ── add_supervisor_required_tag ───────────────────────────────────────────

    #[test]
    fun test_supervisor_tag_blocks_matching_write() {
        let mut scenario = test_scenario::begin(OPERATOR);
        let mut policy = make_policy(&mut scenario);
        governance::add_tag_for_testing(&mut policy, b"clinical");

        let record = make_record(0, &mut scenario);
        let t = tags(b"clinical");
        let scope = string::utf8(b"internal");

        // clinical tag is in supervisor_required_tags; trust=2, no countersign → blocked
        let result = governance::check_write_access(
            &policy, &record, 2, 2, &t, &scope, false,
        );
        assert!(!result, 53);

        governance::destroy_policy_for_testing(policy);
        governance::destroy_record_for_testing(record);
        test_scenario::end(scenario);
    }

    #[test]
    fun test_supervisor_tag_with_countersign_allowed() {
        let mut scenario = test_scenario::begin(OPERATOR);
        let mut policy = make_policy(&mut scenario);
        governance::add_tag_for_testing(&mut policy, b"clinical");

        let record = make_record(0, &mut scenario);
        let t = tags(b"clinical");
        let scope = string::utf8(b"internal");

        let result = governance::check_write_access(
            &policy, &record, 2, 2, &t, &scope, true, // has countersign
        );
        assert!(result, 54);

        governance::destroy_policy_for_testing(policy);
        governance::destroy_record_for_testing(record);
        test_scenario::end(scenario);
    }

    // ── entry function round-trips ────────────────────────────────────────────

    #[test]
    fun test_create_policy_entry_shares_object() {
        let mut scenario = test_scenario::begin(OPERATOR);

        test_scenario::next_tx(&mut scenario, OPERATOR);
        {
            governance::create_policy(
                b"ws_entry", 2, 8, test_scenario::ctx(&mut scenario),
            );
        };

        test_scenario::next_tx(&mut scenario, OPERATOR);
        {
            let policy = test_scenario::take_shared<WorkspacePolicy>(&scenario);
            assert!(*governance::policy_workspace_id(&policy) == string::utf8(b"ws_entry"), 55);
            assert!(governance::min_trust_level(&policy) == 2, 56);
            test_scenario::return_shared(policy);
        };

        test_scenario::end(scenario);
    }

    #[test]
    fun test_register_agent_entry_shares_object() {
        let mut scenario = test_scenario::begin(OPERATOR);

        test_scenario::next_tx(&mut scenario, OPERATOR);
        {
            governance::register_agent(
                b"pp_agent_001", b"ws_entry", test_scenario::ctx(&mut scenario),
            );
        };

        test_scenario::next_tx(&mut scenario, OPERATOR);
        {
            let record = test_scenario::take_shared<AgentEnforcementRecord>(&scenario);
            assert!(governance::enforcement_stage(&record) == 0, 57);
            assert!(governance::reputation(&record) == 1_000_000, 58);
            test_scenario::return_shared(record);
        };

        test_scenario::end(scenario);
    }
}
