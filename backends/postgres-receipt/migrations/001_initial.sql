-- RECALL receipt store — append-only by design.
-- Rows are never deleted. Rollback = new snapshot receipt pointing to earlier state.

CREATE TABLE IF NOT EXISTS receipts (
    id              TEXT PRIMARY KEY,          -- SHA-256 hex content address
    action_kind     TEXT NOT NULL,
    workspace_id    TEXT NOT NULL,
    actor_passport_id TEXT NOT NULL,
    actor_agent_id  TEXT NOT NULL,
    timestamp       TIMESTAMPTZ NOT NULL,
    evidence_digest TEXT NOT NULL,
    seal_status     TEXT NOT NULL DEFAULT 'UNSEALED',
    walrus_blob_id  TEXT,
    deny_reason     TEXT,
    unmet_caveats   JSONB,
    reputation_delta DOUBLE PRECISION DEFAULT 0.0,
    cost_provider   TEXT,
    cost_model      TEXT,
    cost_tokens_in  BIGINT,
    cost_tokens_out BIGINT,
    cost_usd_cents  BIGINT,
    proto_bytes     BYTEA NOT NULL,            -- full serialized proto for verification
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS receipts_workspace_idx ON receipts (workspace_id, timestamp DESC);
CREATE INDEX IF NOT EXISTS receipts_agent_idx ON receipts (actor_agent_id, timestamp DESC);
CREATE INDEX IF NOT EXISTS receipts_action_kind_idx ON receipts (action_kind);

-- Causal predecessors: separate table for the DAG edges.
CREATE TABLE IF NOT EXISTS receipt_predecessors (
    receipt_id      TEXT NOT NULL REFERENCES receipts(id),
    predecessor_id  TEXT NOT NULL,
    PRIMARY KEY (receipt_id, predecessor_id)
);

-- Signature entries (one row per signature on a receipt).
CREATE TABLE IF NOT EXISTS receipt_signatures (
    receipt_id      TEXT NOT NULL REFERENCES receipts(id),
    role            TEXT NOT NULL,
    signer_pubkey   TEXT NOT NULL,
    signature_bytes TEXT NOT NULL,
    PRIMARY KEY (receipt_id, role)
);

-- Merkle batches anchored to Sui.
CREATE TABLE IF NOT EXISTS receipt_batches (
    merkle_root     TEXT PRIMARY KEY,
    sui_tx_digest   TEXT,
    sealed_at       TIMESTAMPTZ,
    batch_signature TEXT,
    receipt_count   BIGINT NOT NULL
);

-- Mapping receipt → batch (set when sealed).
CREATE TABLE IF NOT EXISTS receipt_batch_membership (
    receipt_id      TEXT NOT NULL REFERENCES receipts(id),
    merkle_root     TEXT NOT NULL REFERENCES receipt_batches(merkle_root),
    PRIMARY KEY (receipt_id, merkle_root)
);
