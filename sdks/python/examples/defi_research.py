"""
DeFi Research example: multiple research agents sharing findings via RECALL.

Multiple agents research different DeFi protocols and write findings to shared memory.
The registry captures the research workspace for other teams to import.
"""
import asyncio

from recall import recall


async def protocol_researcher(protocol: str, workspace_name: str) -> None:
    workspace = await recall.connect(
        workspace_name,
        agent=f"researcher-{protocol.lower()}",
        model="claude-sonnet-4-6",
    )

    existing = await workspace.read(entity=protocol)
    print(f"[researcher-{protocol}] Found {len(existing)} existing entries")

    await workspace.write(
        entity=protocol,
        event="tvl_snapshot",
        value="$2.4B",
        metadata={"source": "defillama", "timestamp": "2026-05-18T10:00:00Z"},
        tags=["defi", "metrics"],
    )

    await workspace.write(
        entity=protocol,
        event="risk_assessment",
        value="medium",
        metadata={"audit_count": 3, "bug_bounty": "$500k"},
        tags=["defi", "risk"],
    )

    print(f"[researcher-{protocol}] Wrote research findings")


async def main() -> None:
    workspace_name = "defi-research-2026-q2"
    protocols = ["Uniswap", "Aave", "Compound"]

    print("=== RECALL DeFi Research Demo ===\n")

    await asyncio.gather(*[
        protocol_researcher(proto, workspace_name) for proto in protocols
    ])

    # Publish the workspace for other teams to import.
    profile = await recall.publish(
        name="defi-research-agents",
        version="1.0",
        description="DeFi research agent with Q2 2026 protocol coverage",
    )
    print(f"\n[registry] Published profile: {profile['name']}@{profile['version']}")


if __name__ == "__main__":
    asyncio.run(main())
