#!/usr/bin/env bash
set -euo pipefail

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

pass() { echo -e "${GREEN}✓${NC} $1"; }
fail() { echo -e "${RED}✗${NC} $1"; exit 1; }
warn() { echo -e "${YELLOW}⚠${NC} $1"; }

echo ""
echo "RECALL — pre-submission smoke test"
echo "────────────────────────────────────────────"
echo ""

# ── 1. Check env vars ─────────────────────────────────────────────────────────
echo "Checking environment..."

if [ -z "${RECALL_SUI_PRIVATE_KEY:-}" ]; then
    warn "RECALL_SUI_PRIVATE_KEY not set — Sui anchoring will use synthetic digest"
else
    pass "RECALL_SUI_PRIVATE_KEY set"
fi

if [ -z "${RECALL_SUI_SENDER_ADDRESS:-}" ]; then
    warn "RECALL_SUI_SENDER_ADDRESS not set — address derived locally (may not match funded wallet)"
else
    pass "RECALL_SUI_SENDER_ADDRESS set: $RECALL_SUI_SENDER_ADDRESS"
fi

if [ -z "${MEMWAL_PRIVATE_KEY:-}" ] || [ -z "${MEMWAL_ACCOUNT_ID:-}" ]; then
    fail "MEMWAL_PRIVATE_KEY or MEMWAL_ACCOUNT_ID not set — required for Walrus storage"
fi
pass "MEMWAL_PRIVATE_KEY + MEMWAL_ACCOUNT_ID set"

echo ""

# ── 2. Build the control plane ────────────────────────────────────────────────
echo "Building control plane..."
cargo build -p recall-control-plane -p recall-ops --release 2>&1 | tail -3
pass "Build succeeded"
echo ""

# ── 3. Start control plane in background ─────────────────────────────────────
echo "Starting control plane..."
./target/release/recall-control-plane --walrus-testnet 2>&1 | tee /tmp/smoke_cp.log &
CP_PID=$!
trap "kill $CP_PID 2>/dev/null; exit" INT TERM EXIT

sleep 2

# Check it's alive
if ! kill -0 $CP_PID 2>/dev/null; then
    fail "Control plane failed to start"
fi

HEALTH=$(curl -sf http://localhost:8080/health || echo "")
if [ -z "$HEALTH" ]; then
    fail "Control plane not responding on :8080"
fi
pass "Control plane running (pid $CP_PID)"
echo ""

# ── 4. Run demo seed ──────────────────────────────────────────────────────────
echo "Running demo seed..."
# Prefer the project's .venv if it exists, otherwise system python3.
if [ -x ".venv/bin/python" ]; then
    PY=".venv/bin/python"
elif command -v python3 >/dev/null; then
    PY="python3"
elif command -v python >/dev/null; then
    PY="python"
else
    fail "python3 not found — install python3 or activate .venv to run demo_seed.py"
fi
"$PY" demo_seed.py 2>&1
pass "Demo seed complete"
echo ""

# ── 5. Check recall failures shows a conflict ─────────────────────────────────
echo "Checking conflict detection..."
FAILURES=$(./target/release/recall failures 2>&1)
echo "$FAILURES"

if echo "$FAILURES" | grep -q "CONFLICT\|conflict\|⚠"; then
    pass "Conflict detected"
else
    fail "No conflict found — conflict detection may be broken"
fi
echo ""

# ── 6. Check recall why shows a receipt trail ─────────────────────────────────
echo "Checking receipt trail..."
WHY=$(./target/release/recall why --entity sarah@email.com 2>&1)
echo "$WHY"

if echo "$WHY" | grep -q "receipt\|mem_"; then
    pass "Receipt trail present"
else
    fail "No receipt trail found"
fi
echo ""

# ── 7. Check for real Walrus blob ID and verify it is fetchable ──────────────
echo "Verifying real Walrus blobs..."

# Disable strict-fail for the extraction pipelines — grep returning no-match
# would otherwise tear down the whole script under `set -euo pipefail`.
set +e

# Primary source: scan the control plane's own logs for either
#   "Walrus blob stored: <id>"  (raw-Walrus fallback path)
#   "MemWal blob stored: job_id=<...> blob_id=<id>"  (canonical MemWal path)
# Whichever fires first is the blob we just persisted.
BLOB_ID=""
if [ -f /tmp/smoke_cp.log ]; then
    # Match any of:
    #   "MemWal blob stored: job_id=<...> blob_id=<id>"  (canonical MemWal path)
    #   "Walrus blob stored: <id>"                       (raw Walrus, pre-fallback)
    #   "Walrus blob stored (walrus_raw[_fallback]): <id>"  (post-fallback, tagged)
    BLOB_ID=$(grep -oE '(MemWal|Walrus) blob stored( \([a-z_]+\))?:.*[A-Za-z0-9_-]{40,}' /tmp/smoke_cp.log \
        | head -1 | grep -oE '[A-Za-z0-9_-]{40,}' | tail -1)
fi

# Fallback: query the HTTP API and look for any 40+ char base64url-ish token
# (Walrus blob IDs match that shape).
if [ -z "$BLOB_ID" ]; then
    WS_ID=$(curl -s --max-time 5 http://localhost:8080/workspaces 2>/dev/null \
        | grep -oE '"workspace_id":"[^"]+"' | head -1 | cut -d'"' -f4)
    if [ -n "$WS_ID" ]; then
        BLOB_ID=$(curl -s --max-time 5 "http://localhost:8080/memory/$WS_ID" 2>/dev/null \
            | grep -oE '"blob_id":"[A-Za-z0-9_-]{40,}"' | head -1 \
            | sed 's/.*"blob_id":"\([^"]*\)".*/\1/')
    fi
fi

set -e

if [ -z "$BLOB_ID" ]; then
    fail "No Walrus blob ID found via control-plane logs or /memory endpoint"
fi
pass "Walrus blob ID: $BLOB_ID"

HTTP_STATUS=$(curl -s -o /dev/null -w "%{http_code}" --max-time 10 \
    "https://aggregator.walrus-testnet.walrus.space/v1/blobs/$BLOB_ID")

if [ "$HTTP_STATUS" = "200" ]; then
    pass "Walrus blob verified on testnet aggregator (HTTP 200)"
else
    fail "Walrus blob NOT found on testnet (HTTP $HTTP_STATUS for $BLOB_ID)"
fi
echo ""

# ── 8. Check registry publish ────────────────────────────────────────────────
echo "Checking registry..."
REGISTRY=$(./target/release/recall registry list 2>&1)
echo "$REGISTRY"
pass "Registry responding"
echo ""

# ── 9. Summary ───────────────────────────────────────────────────────────────
echo "────────────────────────────────────────────"
echo ""

if [ -n "${RECALL_SUI_PRIVATE_KEY:-}" ] && [ -n "${RECALL_SUI_SENDER_ADDRESS:-}" ]; then
    pass "Sui anchoring: LIVE (client-built PTB submission)"
else
    warn "Sui anchoring: UNANCHORED markers (set env vars for real anchoring)"
fi

if [ "${RECALL_USE_MEMWAL:-0}" = "1" ] || [ "${RECALL_USE_MEMWAL:-0}" = "true" ]; then
    pass "Storage: MemWal SDK sidecar (canonical Walrus track path)"
else
    pass "Storage: raw Walrus publisher (fallback — set RECALL_USE_MEMWAL=1 for MemWal SDK)"
fi

echo ""
echo "Smoke test complete."
echo "If all checks passed with real env vars set — you are ready to submit."
echo ""
