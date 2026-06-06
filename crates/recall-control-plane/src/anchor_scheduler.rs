//! Anchor scheduler — periodically seals the set of unanchored receipts into
//! a Merkle batch and submits the root to the `receipt_anchor` Move package
//! on Sui. The on-chain submission is best-effort: if the Sui driver is not
//! configured (env vars missing) it falls back to a synthetic digest and the
//! control plane still produces a valid `anchor.commit` receipt that the
//! `recall anchors` CLI can browse.
//!
//! Spawned once at startup from `main.rs`. Holds an `Arc<AppState>` and the
//! `SuiAnchorDriver`. Sleeps for `interval` between ticks; on each tick:
//!
//!   1. Drains unanchored receipts from `state.receipt_store` (oldest first).
//!   2. Computes the Merkle root over their IDs.
//!   3. Builds a `ReceiptBatch` and calls `driver.anchor_batch()`.
//!   4. Emits an `anchor.commit` receipt with:
//!        - `evidence_digest = merkle_root`
//!        - `causal_predecessors = receipt IDs that were anchored`
//!      and appends it to the receipt store. The receipt is itself marked
//!      anchored so we never anchor the anchor.

use std::sync::Arc;
use std::time::Duration;

use prost_types::Timestamp;
use recall_core::ids::{AgentId, ContentHash, WorkspaceId};
use recall_proto::{common as common_proto, receipt as receipt_proto};
use recall_receipt::{action_kind, builder::ReceiptBuilder, merkle::merkle_root};
use sui_anchor::SuiAnchorDriver;
use tracing::{debug, info, warn};

use crate::state::AppState;

/// Control-plane agent IDs used when the control plane itself is the actor.
const CP_AGENT_ID:    &str = "00000000-0000-0000-0000-000000000001";
const CP_PASSPORT_ID: &str = "cp_passport";
/// All anchor.commit receipts go under the "global" workspace because a batch
/// can span workspaces — the receipt is about the system, not about any one
/// workspace's writes.
const ANCHOR_WORKSPACE_ID: &str = "global";

pub fn spawn(state: Arc<AppState>, interval: Duration) {
    if interval.is_zero() {
        info!("Anchor scheduler: DISABLED (interval=0)");
        return;
    }

    let driver = SuiAnchorDriver::testnet();
    info!(
        "Anchor scheduler: every {}s — Merkle roots committed via receipt_anchor::commit_anchor()",
        interval.as_secs()
    );

    tokio::spawn(async move {
        // Wait one full interval before the first tick so we don't anchor
        // immediately at startup when the store may still be empty.
        tokio::time::sleep(interval).await;
        loop {
            if let Err(e) = tick(&state, &driver).await {
                warn!("anchor tick failed: {e}");
            }
            tokio::time::sleep(interval).await;
        }
    });
}

async fn tick(state: &Arc<AppState>, driver: &SuiAnchorDriver) -> anyhow::Result<()> {
    let pending = state.receipt_store.take_unanchored();

    if pending.is_empty() {
        debug!("anchor tick: no unanchored receipts");
        return Ok(());
    }

    let receipt_ids: Vec<ContentHash> = pending
        .iter()
        .filter_map(|r| r.id.as_ref().map(|h| ContentHash(h.hex.clone())))
        .collect();

    let root = merkle_root(&receipt_ids);
    let count = receipt_ids.len();

    info!(
        "anchor tick: batching {} receipts under merkle root {}…",
        count,
        &root.0[..16.min(root.0.len())]
    );

    let batch = receipt_proto::ReceiptBatch {
        merkle_root: Some(common_proto::Hash { hex: root.0.clone() }),
        receipt_ids: receipt_ids
            .iter()
            .map(|h| common_proto::Hash { hex: h.0.clone() })
            .collect(),
        sui_tx_digest: String::new(),
        sealed_at: Some(Timestamp {
            seconds: chrono::Utc::now().timestamp(),
            nanos:   0,
        }),
        batch_signature: None,
    };

    let sui_tx = match driver.anchor_batch(&batch).await {
        Ok(d) => d,
        Err(e) => {
            warn!("Sui anchor submission errored: {e}; recording with synthetic digest");
            format!("sui_tx_{}", &root.0[..16.min(root.0.len())])
        }
    };

    // Emit the anchor.commit receipt.
    let ws       = WorkspaceId(ANCHOR_WORKSPACE_ID.to_string());
    let passport = ContentHash(CP_PASSPORT_ID.to_string());
    let agent    = AgentId(CP_AGENT_ID.to_string());

    let mut builder = ReceiptBuilder::new(action_kind::ANCHOR_COMMIT, &ws, &passport, &agent)
        .with_evidence_digest(&root);
    for id in &receipt_ids {
        builder = builder.with_causal_predecessor(id);
    }
    let mut receipt = builder.build(&state.cp_keypair);

    // Stash the Sui tx digest in the deny_reason field — it's the only string
    // slot on Receipt and we want this visible in the CLI. (The proto's
    // sui_tx_digest lives on ReceiptBatch, which our HTTP API doesn't expose
    // yet — this is a pragmatic compromise.)
    receipt.deny_reason = sui_tx.clone();

    let appended = state.receipt_store.append(receipt.clone());

    // Mark the anchor.commit receipt itself as anchored so future ticks won't
    // attempt to nest it inside another batch.
    if let Ok(id) = appended {
        state.receipt_store.mark_anchored(&id.0);
    }

    // Broadcast to any streaming subscribers on the anchor workspace.
    state.subscribe_hub.publish(ANCHOR_WORKSPACE_ID, receipt);

    info!(
        "✓ anchor sealed: {} receipts → merkle {} → sui_tx {}",
        count,
        &root.0[..12.min(root.0.len())],
        &sui_tx[..16.min(sui_tx.len())]
    );

    Ok(())
}
