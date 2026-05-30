#[test_only]
module recall_workspace_registry::workspace_registry_tests {
    use recall_workspace_registry::workspace_registry::{
        Self, GlobalRegistry, WorkspaceRecord, CapabilityToken,
    };
    use sui::test_scenario::{Self};
    use sui::clock;
    use std::string;

    const OPERATOR: address = @0xABC1;
    const AGENT:    address = @0xABC2;

    // ── helpers ───────────────────────────────────────────────────────────────

    fun deploy(scenario: &mut sui::test_scenario::Scenario) {
        workspace_registry::init_for_testing(test_scenario::ctx(scenario));
    }

    // ── registry init ─────────────────────────────────────────────────────────

    #[test]
    fun test_init_creates_shared_registry() {
        let mut scenario = test_scenario::begin(OPERATOR);
        deploy(&mut scenario);

        test_scenario::next_tx(&mut scenario, OPERATOR);
        {
            let registry = test_scenario::take_shared<GlobalRegistry>(&scenario);
            assert!(workspace_registry::total_workspaces(&registry) == 0, 0);
            test_scenario::return_shared(registry);
        };

        test_scenario::end(scenario);
    }

    // ── create_workspace ──────────────────────────────────────────────────────

    #[test]
    fun test_create_workspace_increments_counter() {
        let mut scenario = test_scenario::begin(OPERATOR);
        deploy(&mut scenario);

        test_scenario::next_tx(&mut scenario, OPERATOR);
        {
            let mut registry = test_scenario::take_shared<GlobalRegistry>(&scenario);
            let clk = clock::create_for_testing(test_scenario::ctx(&mut scenario));

            workspace_registry::create_workspace(
                &mut registry,
                b"ws_acme",
                b"Acme Corp Workspace",
                b"CLOSED",
                b"1.0.0",
                &clk,
                test_scenario::ctx(&mut scenario),
            );

            assert!(workspace_registry::total_workspaces(&registry) == 1, 1);
            assert!(workspace_registry::has_workspace(&registry, string::utf8(b"ws_acme")), 2);

            clock::destroy_for_testing(clk);
            test_scenario::return_shared(registry);
        };

        test_scenario::end(scenario);
    }

    #[test]
    fun test_workspace_record_fields() {
        let mut scenario = test_scenario::begin(OPERATOR);
        deploy(&mut scenario);

        test_scenario::next_tx(&mut scenario, OPERATOR);
        {
            let mut registry = test_scenario::take_shared<GlobalRegistry>(&scenario);
            let mut clk = clock::create_for_testing(test_scenario::ctx(&mut scenario));
            clock::set_for_testing(&mut clk, 9_000_000);

            workspace_registry::create_workspace(
                &mut registry,
                b"ws_fields",
                b"Field Test WS",
                b"OPEN",
                b"2.0.0",
                &clk,
                test_scenario::ctx(&mut scenario),
            );

            clock::destroy_for_testing(clk);
            test_scenario::return_shared(registry);
        };

        test_scenario::next_tx(&mut scenario, OPERATOR);
        {
            let record = test_scenario::take_from_sender<WorkspaceRecord>(&scenario);
            assert!(*workspace_registry::workspace_id(&record)  == string::utf8(b"ws_fields"),    3);
            assert!(*workspace_registry::name(&record)          == string::utf8(b"Field Test WS"), 4);
            assert!(*workspace_registry::topology_mode(&record) == string::utf8(b"OPEN"),          5);
            assert!(workspace_registry::created_at_ms(&record)  == 9_000_000,                      6);
            // snapshot_blob_id starts empty
            assert!(*workspace_registry::snapshot_blob_id(&record) == string::utf8(b""),           7);
            test_scenario::return_to_sender(&scenario, record);
        };

        test_scenario::end(scenario);
    }

    #[test]
    fun test_two_workspaces_both_registered() {
        let mut scenario = test_scenario::begin(OPERATOR);
        deploy(&mut scenario);

        test_scenario::next_tx(&mut scenario, OPERATOR);
        {
            let mut registry = test_scenario::take_shared<GlobalRegistry>(&scenario);
            let clk = clock::create_for_testing(test_scenario::ctx(&mut scenario));
            workspace_registry::create_workspace(
                &mut registry, b"ws_one", b"One", b"CLOSED", b"1.0.0", &clk,
                test_scenario::ctx(&mut scenario),
            );
            clock::destroy_for_testing(clk);
            test_scenario::return_shared(registry);
        };

        test_scenario::next_tx(&mut scenario, OPERATOR);
        {
            let mut registry = test_scenario::take_shared<GlobalRegistry>(&scenario);
            let clk = clock::create_for_testing(test_scenario::ctx(&mut scenario));
            workspace_registry::create_workspace(
                &mut registry, b"ws_two", b"Two", b"OPEN", b"1.0.0", &clk,
                test_scenario::ctx(&mut scenario),
            );
            clock::destroy_for_testing(clk);
            assert!(workspace_registry::total_workspaces(&registry) == 2, 8);
            assert!(workspace_registry::has_workspace(&registry, string::utf8(b"ws_one")), 9);
            assert!(workspace_registry::has_workspace(&registry, string::utf8(b"ws_two")), 10);
            test_scenario::return_shared(registry);
        };

        test_scenario::end(scenario);
    }

    // ── update_snapshot ───────────────────────────────────────────────────────

    #[test]
    fun test_update_snapshot_changes_blob_id() {
        let mut scenario = test_scenario::begin(OPERATOR);
        deploy(&mut scenario);

        test_scenario::next_tx(&mut scenario, OPERATOR);
        {
            let mut registry = test_scenario::take_shared<GlobalRegistry>(&scenario);
            let clk = clock::create_for_testing(test_scenario::ctx(&mut scenario));
            workspace_registry::create_workspace(
                &mut registry, b"ws_snap", b"Snap WS", b"CLOSED", b"1.0.0", &clk,
                test_scenario::ctx(&mut scenario),
            );
            clock::destroy_for_testing(clk);
            test_scenario::return_shared(registry);
        };

        test_scenario::next_tx(&mut scenario, OPERATOR);
        {
            let mut record = test_scenario::take_from_sender<WorkspaceRecord>(&scenario);
            workspace_registry::update_snapshot(
                &mut record,
                b"0xwalrus_blob_xyz",
                test_scenario::ctx(&mut scenario),
            );
            assert!(*workspace_registry::snapshot_blob_id(&record) ==
                string::utf8(b"0xwalrus_blob_xyz"), 11);
            test_scenario::return_to_sender(&scenario, record);
        };

        test_scenario::end(scenario);
    }

    // ── issue_capability ──────────────────────────────────────────────────────

    #[test]
    fun test_issue_capability_fields() {
        let mut scenario = test_scenario::begin(OPERATOR);
        deploy(&mut scenario);

        test_scenario::next_tx(&mut scenario, OPERATOR);
        {
            let mut clk = clock::create_for_testing(test_scenario::ctx(&mut scenario));
            clock::set_for_testing(&mut clk, 5_000);

            workspace_registry::issue_capability(
                b"ws_cap",
                b"cap_001",
                AGENT,
                b"pp_agent_001",
                b"memory.write,memory.read",
                99_999_999,
                &clk,
                test_scenario::ctx(&mut scenario),
            );

            clock::destroy_for_testing(clk);
        };

        // AGENT receives the CapabilityToken.
        test_scenario::next_tx(&mut scenario, AGENT);
        {
            let token = test_scenario::take_from_sender<CapabilityToken>(&scenario);

            assert!(*workspace_registry::capability_id(&token)  == string::utf8(b"cap_001"), 12);
            assert!(*workspace_registry::cap_workspace_id(&token) == string::utf8(b"ws_cap"), 13);
            assert!(workspace_registry::valid_until_ms(&token)  == 99_999_999,                14);
            assert!(workspace_registry::attenuation_depth(&token) == 0,                       15);

            test_scenario::return_to_sender(&scenario, token);
        };

        test_scenario::end(scenario);
    }

    #[test]
    fun test_capability_goes_to_correct_holder() {
        let mut scenario = test_scenario::begin(OPERATOR);
        deploy(&mut scenario);

        test_scenario::next_tx(&mut scenario, OPERATOR);
        {
            let clk = clock::create_for_testing(test_scenario::ctx(&mut scenario));
            // Issue cap to AGENT, not OPERATOR
            workspace_registry::issue_capability(
                b"ws_x", b"cap_x", AGENT, b"pp_agent", b"memory.write",
                1_000_000_000, &clk, test_scenario::ctx(&mut scenario),
            );
            clock::destroy_for_testing(clk);
        };

        // OPERATOR should NOT have a CapabilityToken.
        test_scenario::next_tx(&mut scenario, OPERATOR);
        {
            assert!(!test_scenario::has_most_recent_for_sender<CapabilityToken>(&scenario), 16);
        };

        // AGENT should have one.
        test_scenario::next_tx(&mut scenario, AGENT);
        {
            assert!(test_scenario::has_most_recent_for_sender<CapabilityToken>(&scenario), 17);
            let token = test_scenario::take_from_sender<CapabilityToken>(&scenario);
            test_scenario::return_to_sender(&scenario, token);
        };

        test_scenario::end(scenario);
    }
}
