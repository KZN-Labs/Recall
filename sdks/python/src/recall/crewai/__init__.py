"""CrewAI adapter for RECALL.

Provides a RecallMemoryTool that CrewAI agents can use to read/write shared memory.

Usage:
    from recall.crewai import RecallMemoryTool
    from recall import recall

    workspace = await recall.connect("my-workspace", agent="crew-agent", model="claude-sonnet-4-6")
    tool = RecallMemoryTool(workspace)

    agent = Agent(tools=[tool], ...)
"""
from __future__ import annotations

from typing import Any, Optional

from ..client import Workspace


class RecallMemoryTool:
    """CrewAI tool adapter for RECALL memory operations."""

    name: str = "recall_memory"
    description: str = (
        "Read and write persistent shared memory for an entity via the RECALL system. "
        "Use this to share knowledge between agents, track customer interactions, "
        "and access memory written by other agents in the same workspace."
    )

    def __init__(self, workspace: Workspace):
        self._workspace = workspace

    async def read(self, entity: str) -> list[dict[str, Any]]:
        result = await self._workspace.read(entity=entity)
        return [e.model_dump() for e in result]

    async def write(
        self,
        entity: str,
        event: str,
        value: Any,
        metadata: Optional[dict] = None,
    ) -> dict[str, Any]:
        receipt = await self._workspace.write(
            entity=entity, event=event, value=value, metadata=metadata
        )
        return {"receipt_id": receipt.id, "action_kind": receipt.action_kind}
