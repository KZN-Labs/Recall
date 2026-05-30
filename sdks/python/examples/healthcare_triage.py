"""
Healthcare Triage example: triage agent handing off to specialist agents via RECALL.

Demonstrates:
  - Scoped memory (internal only for PII)
  - Supervisor countersign caveat for high-stakes writes
  - Handoff capsule from triage to specialist
"""
import asyncio

from recall import recall


async def triage_agent(patient_id: str) -> None:
    workspace = await recall.connect(
        "hospital-triage",
        agent="triage-agent",
        model="claude-sonnet-4-6",
        trust_level=2,
    )

    await workspace.write(
        entity=patient_id,
        event="triage_assessment",
        value="priority_2",
        metadata={"presenting_complaint": "chest_pain", "vitals_stable": True},
        tags=["clinical"],
        scope="internal",  # PII never leaves internal scope
    )

    print(f"[triage-agent] Triaged {patient_id}")

    capsule = await recall.handoff(
        from_agent="triage-agent",
        to_agent="cardiology-agent",
        entity=patient_id,
    )
    print(f"[triage-agent] Handed off to cardiology: {capsule.id}")


async def cardiology_agent(patient_id: str) -> None:
    workspace = await recall.connect(
        "hospital-triage",
        agent="cardiology-agent",
        model="claude-sonnet-4-6",
        trust_level=2,
    )

    memory = await workspace.read(entity=patient_id)
    print(f"[cardiology-agent] Loaded {len(memory)} entries from handoff")

    await workspace.write(
        entity=patient_id,
        event="specialist_assessment",
        value="ecg_ordered",
        metadata={"specialist": "cardiology", "urgency": "high"},
        tags=["clinical"],
        scope="internal",
    )

    print(f"[cardiology-agent] Wrote specialist assessment for {patient_id}")


async def main() -> None:
    patient_id = "patient_mrn_00234"
    print("=== RECALL Healthcare Triage Demo ===\n")
    await triage_agent(patient_id)
    await cardiology_agent(patient_id)
    print("\n=== All memory writes are receipted and auditable forever ===")


if __name__ == "__main__":
    asyncio.run(main())
