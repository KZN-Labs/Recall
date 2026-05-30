"""LangGraph adapter for RECALL.

Provides a RecallMemoryNode that can be inserted into a LangGraph StateGraph
to give agents persistent, governed, shared memory.

Usage:
    from recall.langgraph import RecallMemoryNode
    from recall import recall

    async with recall.connect("my-workspace", agent="my-agent", model="claude-sonnet-4-6") as workspace:
        node = RecallMemoryNode(workspace)
        graph = StateGraph(AgentState)
        graph.add_node("memory", node)
"""
from __future__ import annotations

from typing import Any, Optional

from ..client import Workspace


class RecallMemoryNode:
    """LangGraph node that reads/writes RECALL memory for the current entity."""

    def __init__(self, workspace: Workspace, entity_key: str = "entity"):
        self._workspace = workspace
        self._entity_key = entity_key

    async def __call__(self, state: dict[str, Any]) -> dict[str, Any]:
        entity = state.get(self._entity_key, "")
        if not entity:
            return state

        memory = await self._workspace.read(entity=entity)
        state["recall_memory"] = [e.model_dump() for e in memory]
        return state

    async def write(self, entity: str, event: str, value: Any, metadata: Optional[dict] = None) -> None:
        await self._workspace.write(entity=entity, event=event, value=value, metadata=metadata)
