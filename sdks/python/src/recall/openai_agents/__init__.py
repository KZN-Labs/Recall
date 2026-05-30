"""OpenAI Agents SDK adapter for RECALL."""
from __future__ import annotations

from typing import Any, Optional

from ..client import Workspace


class RecallMemoryContext:
    """Provides RECALL memory access inside an OpenAI Agents run context."""

    def __init__(self, workspace: Workspace):
        self._workspace = workspace

    async def read(self, entity: str) -> list[dict[str, Any]]:
        result = await self._workspace.read(entity=entity)
        return [e.model_dump() for e in result]

    async def write(self, entity: str, event: str, value: Any, metadata: Optional[dict] = None) -> str:
        receipt = await self._workspace.write(
            entity=entity, event=event, value=value, metadata=metadata
        )
        return receipt.id
