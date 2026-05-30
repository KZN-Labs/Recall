"""Microsoft Agent Framework (MAF) adapter for RECALL."""
from __future__ import annotations

from typing import Any, Optional

from ..client import Workspace


class RecallMafPlugin:
    """MAF plugin that exposes RECALL memory as agent-addressable state."""

    def __init__(self, workspace: Workspace):
        self._workspace = workspace

    async def on_agent_read(self, entity: str) -> list[dict[str, Any]]:
        result = await self._workspace.read(entity=entity)
        return [e.model_dump() for e in result]

    async def on_agent_write(
        self,
        entity: str,
        event: str,
        value: Any,
        metadata: Optional[dict] = None,
    ) -> dict[str, Any]:
        receipt = await self._workspace.write(
            entity=entity, event=event, value=value, metadata=metadata
        )
        return {"receipt_id": receipt.id}
