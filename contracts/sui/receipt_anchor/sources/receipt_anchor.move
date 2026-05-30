/// RECALL receipt anchoring contract.
/// Commits Merkle roots of receipt batches to the Sui chain for permanent verifiability.
/// Every anchor is an on-chain record that any party can verify against Walrus blobs.
#[allow(lint(public_entry))]
module recall_receipt_anchor::receipt_anchor {
    use sui::event;
    use std::string::{Self, String};
    use sui::clock::{Self, Clock};

    /// Immutable on-chain record of a receipt batch Merkle root.
    public struct ReceiptAnchor has key, store {
        id: UID,
        /// SHA-256 Merkle root of the receipt batch (hex).
        merkle_root: String,
        /// Walrus blob ID containing the full receipt batch protobuf.
        walrus_blob_id: String,
        /// Workspace this batch belongs to.
        workspace_id: String,
        /// Number of receipts in this batch.
        receipt_count: u64,
        /// Timestamp (epoch milliseconds) when this anchor was committed.
        committed_at_ms: u64,
        /// Sequence number within the workspace (for ordering anchors).
        sequence: u64,
    }

    /// Global registry of all anchors across workspaces.
    public struct AnchorRegistry has key {
        id: UID,
        /// Total number of anchors committed across all workspaces.
        total_anchors: u64,
    }

    /// Emitted on every anchor commit.
    public struct AnchorCommitted has copy, drop {
        merkle_root: String,
        workspace_id: String,
        receipt_count: u64,
        walrus_blob_id: String,
        sequence: u64,
        committed_at_ms: u64,
    }

    /// Initialize the global anchor registry (called once on package publish).
    fun init(ctx: &mut TxContext) {
        let registry = AnchorRegistry {
            id: object::new(ctx),
            total_anchors: 0,
        };
        transfer::share_object(registry);
    }

    /// Commit a receipt batch Merkle root to the chain.
    public entry fun commit_anchor(
        registry: &mut AnchorRegistry,
        merkle_root: vector<u8>,
        walrus_blob_id: vector<u8>,
        workspace_id: vector<u8>,
        receipt_count: u64,
        clock: &Clock,
        ctx: &mut TxContext,
    ) {
        let sequence = registry.total_anchors;
        registry.total_anchors = registry.total_anchors + 1;

        let committed_at_ms = clock::timestamp_ms(clock);

        let merkle_root_str  = string::utf8(merkle_root);
        let blob_id_str      = string::utf8(walrus_blob_id);
        let workspace_str    = string::utf8(workspace_id);

        event::emit(AnchorCommitted {
            merkle_root: merkle_root_str,
            workspace_id: workspace_str,
            receipt_count,
            walrus_blob_id: blob_id_str,
            sequence,
            committed_at_ms,
        });

        let anchor = ReceiptAnchor {
            id: object::new(ctx),
            merkle_root: merkle_root_str,
            walrus_blob_id: blob_id_str,
            workspace_id: workspace_str,
            receipt_count,
            committed_at_ms,
            sequence,
        };

        // Transfer to the transaction sender (the control-plane operator).
        transfer::transfer(anchor, tx_context::sender(ctx));
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Accessors
    // ─────────────────────────────────────────────────────────────────────────

    public fun merkle_root(anchor: &ReceiptAnchor): &String    { &anchor.merkle_root }
    public fun walrus_blob_id(anchor: &ReceiptAnchor): &String { &anchor.walrus_blob_id }
    public fun workspace_id(anchor: &ReceiptAnchor): &String   { &anchor.workspace_id }
    public fun receipt_count(anchor: &ReceiptAnchor): u64      { anchor.receipt_count }
    public fun committed_at_ms(anchor: &ReceiptAnchor): u64    { anchor.committed_at_ms }
    public fun sequence(anchor: &ReceiptAnchor): u64           { anchor.sequence }
    public fun total_anchors(registry: &AnchorRegistry): u64   { registry.total_anchors }

    // ─────────────────────────────────────────────────────────────────────────
    // Test-only helpers
    // ─────────────────────────────────────────────────────────────────────────

    #[test_only]
    public fun init_for_testing(ctx: &mut TxContext) {
        init(ctx);
    }
}
