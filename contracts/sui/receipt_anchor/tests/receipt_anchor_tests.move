#[test_only]
module recall_receipt_anchor::receipt_anchor_tests {
    use recall_receipt_anchor::receipt_anchor::{Self, AnchorRegistry, ReceiptAnchor};
    use sui::test_scenario::{Self};
    use sui::clock;
    use std::string;

    const OPERATOR: address = @0xCA11;
    const OTHER:    address = @0xB0B;

    // ── helpers ───────────────────────────────────────────────────────────────

    /// Deploy the package (creates the shared AnchorRegistry).
    fun deploy(scenario: &mut sui::test_scenario::Scenario) {
        receipt_anchor::init_for_testing(test_scenario::ctx(scenario));
    }

    // ── tests ─────────────────────────────────────────────────────────────────

    #[test]
    fun test_init_creates_registry() {
        let mut scenario = test_scenario::begin(OPERATOR);
        deploy(&mut scenario);

        test_scenario::next_tx(&mut scenario, OPERATOR);
        {
            let registry = test_scenario::take_shared<AnchorRegistry>(&scenario);
            assert!(receipt_anchor::total_anchors(&registry) == 0, 0);
            test_scenario::return_shared(registry);
        };

        test_scenario::end(scenario);
    }

    #[test]
    fun test_commit_anchor_increments_total() {
        let mut scenario = test_scenario::begin(OPERATOR);
        deploy(&mut scenario);

        test_scenario::next_tx(&mut scenario, OPERATOR);
        {
            let mut registry = test_scenario::take_shared<AnchorRegistry>(&scenario);
            let clk = clock::create_for_testing(test_scenario::ctx(&mut scenario));

            receipt_anchor::commit_anchor(
                &mut registry,
                b"deadbeef01234567",
                b"blob_id_walrus_001",
                b"ws_acme",
                42,
                &clk,
                test_scenario::ctx(&mut scenario),
            );

            assert!(receipt_anchor::total_anchors(&registry) == 1, 1);

            clock::destroy_for_testing(clk);
            test_scenario::return_shared(registry);
        };

        test_scenario::end(scenario);
    }

    #[test]
    fun test_anchor_fields_are_correct() {
        let mut scenario = test_scenario::begin(OPERATOR);
        deploy(&mut scenario);

        test_scenario::next_tx(&mut scenario, OPERATOR);
        {
            let mut registry = test_scenario::take_shared<AnchorRegistry>(&scenario);
            let mut clk = clock::create_for_testing(test_scenario::ctx(&mut scenario));
            clock::set_for_testing(&mut clk, 1_716_028_320_000);

            receipt_anchor::commit_anchor(
                &mut registry,
                b"aabbcc",
                b"walrus_blob_42",
                b"ws_test",
                100,
                &clk,
                test_scenario::ctx(&mut scenario),
            );

            clock::destroy_for_testing(clk);
            test_scenario::return_shared(registry);
        };

        // The anchor is transferred to OPERATOR; retrieve it.
        test_scenario::next_tx(&mut scenario, OPERATOR);
        {
            let anchor = test_scenario::take_from_sender<ReceiptAnchor>(&scenario);

            assert!(*receipt_anchor::merkle_root(&anchor)   == string::utf8(b"aabbcc"),         2);
            assert!(*receipt_anchor::walrus_blob_id(&anchor) == string::utf8(b"walrus_blob_42"), 3);
            assert!(*receipt_anchor::workspace_id(&anchor)  == string::utf8(b"ws_test"),        4);
            assert!(receipt_anchor::receipt_count(&anchor)  == 100,                              5);
            assert!(receipt_anchor::committed_at_ms(&anchor) == 1_716_028_320_000,               6);
            assert!(receipt_anchor::sequence(&anchor)       == 0,                                7);

            test_scenario::return_to_sender(&scenario, anchor);
        };

        test_scenario::end(scenario);
    }

    #[test]
    fun test_sequence_increments_per_commit() {
        let mut scenario = test_scenario::begin(OPERATOR);
        deploy(&mut scenario);

        // First commit.
        test_scenario::next_tx(&mut scenario, OPERATOR);
        {
            let mut registry = test_scenario::take_shared<AnchorRegistry>(&scenario);
            let clk = clock::create_for_testing(test_scenario::ctx(&mut scenario));
            receipt_anchor::commit_anchor(
                &mut registry, b"root1", b"blob1", b"ws_a", 10, &clk,
                test_scenario::ctx(&mut scenario),
            );
            clock::destroy_for_testing(clk);
            test_scenario::return_shared(registry);
        };

        // Second commit.
        test_scenario::next_tx(&mut scenario, OPERATOR);
        {
            let mut registry = test_scenario::take_shared<AnchorRegistry>(&scenario);
            let clk = clock::create_for_testing(test_scenario::ctx(&mut scenario));
            receipt_anchor::commit_anchor(
                &mut registry, b"root2", b"blob2", b"ws_a", 20, &clk,
                test_scenario::ctx(&mut scenario),
            );
            clock::destroy_for_testing(clk);
            test_scenario::return_shared(registry);
        };

        // Third commit.
        test_scenario::next_tx(&mut scenario, OPERATOR);
        {
            let mut registry = test_scenario::take_shared<AnchorRegistry>(&scenario);
            let clk = clock::create_for_testing(test_scenario::ctx(&mut scenario));
            receipt_anchor::commit_anchor(
                &mut registry, b"root3", b"blob3", b"ws_b", 5, &clk,
                test_scenario::ctx(&mut scenario),
            );
            clock::destroy_for_testing(clk);
            assert!(receipt_anchor::total_anchors(&registry) == 3, 8);
            test_scenario::return_shared(registry);
        };

        // Collect the three anchors and check sequences.
        test_scenario::next_tx(&mut scenario, OPERATOR);
        {
            // Anchors come out in reverse order from the sender's inventory.
            let a2 = test_scenario::take_from_sender<ReceiptAnchor>(&scenario);
            assert!(receipt_anchor::sequence(&a2) == 2, 9);
            let a1 = test_scenario::take_from_sender<ReceiptAnchor>(&scenario);
            assert!(receipt_anchor::sequence(&a1) == 1, 10);
            let a0 = test_scenario::take_from_sender<ReceiptAnchor>(&scenario);
            assert!(receipt_anchor::sequence(&a0) == 0, 11);
            test_scenario::return_to_sender(&scenario, a0);
            test_scenario::return_to_sender(&scenario, a1);
            test_scenario::return_to_sender(&scenario, a2);
        };

        test_scenario::end(scenario);
    }

    #[test]
    fun test_different_senders_each_receive_their_anchor() {
        let mut scenario = test_scenario::begin(OPERATOR);
        deploy(&mut scenario);

        // OPERATOR commits an anchor.
        test_scenario::next_tx(&mut scenario, OPERATOR);
        {
            let mut registry = test_scenario::take_shared<AnchorRegistry>(&scenario);
            let clk = clock::create_for_testing(test_scenario::ctx(&mut scenario));
            receipt_anchor::commit_anchor(
                &mut registry, b"op_root", b"op_blob", b"ws_op", 1, &clk,
                test_scenario::ctx(&mut scenario),
            );
            clock::destroy_for_testing(clk);
            test_scenario::return_shared(registry);
        };

        // OTHER address commits an anchor.
        test_scenario::next_tx(&mut scenario, OTHER);
        {
            let mut registry = test_scenario::take_shared<AnchorRegistry>(&scenario);
            let clk = clock::create_for_testing(test_scenario::ctx(&mut scenario));
            receipt_anchor::commit_anchor(
                &mut registry, b"other_root", b"other_blob", b"ws_other", 2, &clk,
                test_scenario::ctx(&mut scenario),
            );
            clock::destroy_for_testing(clk);
            test_scenario::return_shared(registry);
        };

        // Each sender owns their own anchor.
        test_scenario::next_tx(&mut scenario, OPERATOR);
        {
            let a = test_scenario::take_from_sender<ReceiptAnchor>(&scenario);
            assert!(*receipt_anchor::workspace_id(&a) == string::utf8(b"ws_op"), 12);
            test_scenario::return_to_sender(&scenario, a);
        };
        test_scenario::next_tx(&mut scenario, OTHER);
        {
            let a = test_scenario::take_from_sender<ReceiptAnchor>(&scenario);
            assert!(*receipt_anchor::workspace_id(&a) == string::utf8(b"ws_other"), 13);
            test_scenario::return_to_sender(&scenario, a);
        };

        test_scenario::end(scenario);
    }

    #[test]
    fun test_zero_receipt_count_allowed() {
        let mut scenario = test_scenario::begin(OPERATOR);
        deploy(&mut scenario);

        test_scenario::next_tx(&mut scenario, OPERATOR);
        {
            let mut registry = test_scenario::take_shared<AnchorRegistry>(&scenario);
            let clk = clock::create_for_testing(test_scenario::ctx(&mut scenario));
            receipt_anchor::commit_anchor(
                &mut registry, b"root", b"blob", b"ws_empty", 0, &clk,
                test_scenario::ctx(&mut scenario),
            );
            clock::destroy_for_testing(clk);
            test_scenario::return_shared(registry);
        };

        test_scenario::next_tx(&mut scenario, OPERATOR);
        {
            let a = test_scenario::take_from_sender<ReceiptAnchor>(&scenario);
            assert!(receipt_anchor::receipt_count(&a) == 0, 14);
            test_scenario::return_to_sender(&scenario, a);
        };

        test_scenario::end(scenario);
    }
}
