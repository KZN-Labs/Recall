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

if [ -z "${MEMWAL_PRIVATE_KEY:-}" ]; then
    warn "MEMWAL_PRIVATE_KEY not set — MemWal blobs disabled"
else
    pass "MEMWAL_PRIVATE_KEY set"
fi

echo ""

# ── 2. Build the control plane ────────────────────────────────────────────────
echo "Building control plane..."
cargo build -p recall-control-plane -p recall-ops --release 2>&1 | tail -3
pass "Build succeeded"
echo ""

# ── 3. Start control plane in background ─────────────────────────────────────
echo "Starting control plane..."
./target/release/recall-control-plane --walrus-testnet &
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
python demo_seed.py 2>&1
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

# ── 7. Check for real Walrus blob ID ──────────────────────────────────────────
echo "Checking Walrus blob IDs..."

LOGS=$(./target/release/recall logs 2>&1)
if echo "$LOGS" | grep -qE "[a-zA-Z0-9_-]{40,}"; then
    pass "Walrus blob IDs present in logs"
else
    warn "No Walrus blob IDs found — check WALRUS_PUBLISHER_URL or --walrus-testnet flag"
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
    pass "Sui anchoring: LIVE"
else
    warn "Sui anchoring: SYNTHETIC (set env vars for real anchoring)"
fi

if [ -n "${MEMWAL_PRIVATE_KEY:-}" ]; then
    pass "MemWal: ENABLED"
else
    warn "MemWal: DISABLED (set MEMWAL_PRIVATE_KEY + MEMWAL_ACCOUNT_ID)"
fi

echo ""
echo "Smoke test complete."
echo "If all checks passed with real env vars set — you are ready to submit."
echo ""
