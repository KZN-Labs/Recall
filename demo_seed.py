"""
RECALL demo seed script.

Populates the control plane with the acme-customer-ops scenario so the
dashboard has real data to show from the moment you open it.

Usage:
    python demo_seed.py [--endpoint http://localhost:8080]
"""
import asyncio
import sys
import argparse

import httpx

sys.path.insert(0, "sdks/python/src")

from recall.client import RecallClient


async def control_plane_walrus_enabled(endpoint: str) -> bool:
    """
    Ask the control plane directly whether it has a Walrus backend wired up.
    This is the truthful answer — checking *this* process's env vars only
    tells you what this script has, not whether the writes actually landed
    on Walrus (which is the control plane's responsibility).
    """
    try:
        async with httpx.AsyncClient(timeout=2.0) as client:
            resp = await client.get(f"{endpoint}/health")
            if resp.status_code == 200:
                return bool(resp.json().get("walrus_enabled", False))
    except Exception:
        pass
    return False

async def seed(endpoint: str) -> None:
    recall = RecallClient(http_endpoint=endpoint)

    print(f"Seeding RECALL demo data → {endpoint}\n")

    # ── Support agent ──────────────────────────────────────────────────────────
    support = await recall.connect(
        "acme-customer-ops",
        agent="support-agent",
        model="claude-sonnet-4-6",
    )
    print("support-agent connected")

    r = await support.write("sarah@email.com", "login_issue_resolved", True,
        metadata={"case_id": "case_8821", "channel": "chat"}, tags=["customer"])
    print(f"  ✓ login_issue_resolved   receipt={r.id[:16]}…")

    r = await support.write("sarah@email.com", "credit_offered", "10%",
        metadata={"reason": "login_issue_resolved", "case_id": "case_8821"},
        tags=["customer", "billing"])
    print(f"  ✓ credit_offered         receipt={r.id[:16]}…")

    r = await support.write("bob@corp.com", "account_created", True,
        metadata={"channel": "web"}, tags=["customer"])
    print(f"  ✓ account_created        receipt={r.id[:16]}…")

    r = await support.write("bob@corp.com", "onboarding_completed", True,
        metadata={"steps_completed": 5}, tags=["customer"])
    print(f"  ✓ onboarding_completed   receipt={r.id[:16]}…")

    # ── Fraud agent (will trigger conflict with credit_offered) ────────────────
    fraud = await recall.connect(
        "acme-customer-ops",
        agent="fraud-agent",
        model="claude-sonnet-4-6",
    )
    print("\nfraud-agent connected")

    r = await fraud.write("sarah@email.com", "flag_suspicious", "velocity_anomaly",
        metadata={"rule": "login_velocity", "score": 0.91}, tags=["security"])
    print(f"  ✓ flag_suspicious        receipt={r.id[:16]}…  ← conflict with credit_offered")

    r = await fraud.write("bob@corp.com", "risk_score_computed", 0.12,
        metadata={"model": "xgb-v3"}, tags=["security"])
    print(f"  ✓ risk_score_computed    receipt={r.id[:16]}…")

    # ── Billing agent ──────────────────────────────────────────────────────────
    billing = await recall.connect(
        "acme-customer-ops",
        agent="billing-agent",
        model="claude-sonnet-4-6",
    )
    print("\nbilling-agent connected")

    r = await billing.write("sarah@email.com", "credit_applied", "10%",
        metadata={"applied_by": "billing-agent", "case_id": "case_8821"},
        tags=["billing"])
    print(f"  ✓ credit_applied         receipt={r.id[:16]}…")

    # ── DeFi research workspace ────────────────────────────────────────────────
    research = await recall.connect(
        "defi-research",
        agent="research-agent",
        model="claude-opus-4-5",
    )
    print("\nresearch-agent connected (ws_defi-research)")

    r = await research.write("aave-v3", "protocol_analysis", {
        "tvl_usd": 12_400_000_000, "apy_range": [0.02, 0.18], "risk": "LOW"},
        tags=["defi", "lending"])
    print(f"  ✓ protocol_analysis      receipt={r.id[:16]}…")

    r = await research.write("uniswap-v4", "protocol_analysis", {
        "tvl_usd": 5_800_000_000, "hooks_enabled": True, "risk": "MEDIUM"},
        tags=["defi", "dex"])
    print(f"  ✓ protocol_analysis      receipt={r.id[:16]}…")

    print("\n✓ Seed complete. Open http://localhost:3000 and go to Memory Inspector.")
    print("  The workspace 'ws_acme-customer-ops' has entries for:")
    print("    sarah@email.com  — login, credit offer, fraud flag, credit applied")
    print("    bob@corp.com     — account creation, onboarding, risk score")
    print()
    print("  Expected conflict: credit_offered ↔ flag_suspicious for sarah@email.com")
    print("  Receipts issued for every write — visible in Inspector stats bar.")
    print()
    walrus_on = await control_plane_walrus_enabled(endpoint)
    if walrus_on:
        print("  Walrus: ENABLED on the control plane — every write is a permanent blob")
        print("  Blobs are verifiable at: https://aggregator.walrus-testnet.walrus.space/v1/blobs/<id>")
    else:
        print("  Walrus: DISABLED on the control plane")
        print("  Restart the server with: --walrus-testnet  (or set WALRUS_PUBLISHER_URL)")


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--endpoint", default="http://localhost:8080")
    args = parser.parse_args()
    asyncio.run(seed(args.endpoint))
