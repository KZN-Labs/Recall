"""
Customer Operations example: three agents sharing memory through RECALL.

Support agent handles the initial interaction, writes credit offer to memory.
Billing agent reads memory to apply the credit.
Fraud agent monitors for suspicious signals — a conflict is detected when it
flags the same entity the support agent offered credit to.

Demonstrates:
  - recall.connect() for each agent
  - workspace.read() to hydrate agent context
  - workspace.write() for memory events with metadata
  - recall.handoff() to transfer entity context between agents
  - conflict detection (automatic, trust-ranked)
"""
import asyncio

from recall import recall


async def support_agent(entity: str) -> None:
    workspace = await recall.connect(
        "acme-customer-ops",
        agent="support-agent",
        model="claude-sonnet-4-6",
    )

    # Read existing memory to understand the customer's history.
    memory = await workspace.read(entity=entity)
    print(f"[support-agent] Loaded {len(memory)} memory entries for {entity}")

    # Resolve the customer's login issue, then offer a credit.
    await workspace.write(
        entity=entity,
        event="login_issue_resolved",
        value=True,
        metadata={"case_id": "case_8821", "channel": "chat"},
    )

    await workspace.write(
        entity=entity,
        event="credit_offered",
        value="10%",
        metadata={"reason": "login_issue_resolved", "case_id": "case_8821"},
        tags=["customer", "billing"],
    )

    print(f"[support-agent] Wrote credit offer for {entity}")

    # Hand off to billing agent to apply the credit.
    capsule = await recall.handoff(
        from_agent="support-agent",
        to_agent="billing-agent",
        entity=entity,
    )
    print(f"[support-agent] Handed off to billing-agent: capsule {capsule.id}")


async def billing_agent(entity: str) -> None:
    workspace = await recall.connect(
        "acme-customer-ops",
        agent="billing-agent",
        model="claude-sonnet-4-6",
    )

    memory = await workspace.read(entity=entity)
    print(f"[billing-agent] Loaded {len(memory)} memory entries for {entity}")

    await workspace.write(
        entity=entity,
        event="credit_applied",
        value="10%",
        metadata={"applied_by": "billing-agent", "case_id": "case_8821"},
        tags=["billing"],
    )

    print(f"[billing-agent] Applied credit for {entity}")


async def fraud_agent(entity: str) -> None:
    workspace = await recall.connect(
        "acme-customer-ops",
        agent="fraud-agent",
        model="claude-sonnet-4-6",
        trust_level=3,
    )

    memory = await workspace.read(entity=entity)
    print(f"[fraud-agent] Loaded {len(memory)} memory entries for {entity}")

    # Fraud agent flags the entity — this will conflict with the credit_offered entry.
    await workspace.write(
        entity=entity,
        event="flag_suspicious",
        value="velocity_anomaly",
        metadata={"rule": "login_velocity", "score": 0.91},
        tags=["security"],
    )

    print(f"[fraud-agent] Flagged {entity} as suspicious — conflict will be detected")


async def main() -> None:
    entity = "sarah@email.com"
    print(f"=== RECALL Customer Ops Demo — entity: {entity} ===\n")

    # Run agents concurrently (in a real deployment, these are separate processes).
    await asyncio.gather(
        support_agent(entity),
        fraud_agent(entity),
    )

    print("\n[billing-agent] Running after support handoff...")
    await billing_agent(entity)

    print("\n=== Demo complete. In production, check the RECALL Inspector dashboard ===")
    print("    for conflicts, receipts, and the full causal audit trail.")


if __name__ == "__main__":
    asyncio.run(main())
