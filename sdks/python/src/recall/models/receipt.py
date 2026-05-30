from __future__ import annotations

from datetime import datetime
from typing import Any, Optional

from pydantic import BaseModel, Field


class CostAnnotation(BaseModel):
    model_provider: str
    model_name: str
    tokens_in: int = 0
    tokens_out: int = 0
    usd_cents: int = 0


class Receipt(BaseModel):
    id: str
    action_kind: str
    workspace_id: str
    actor_passport_id: str
    actor_agent_id: str
    timestamp: Optional[datetime] = None
    causal_predecessors: list[str] = Field(default_factory=list)
    evidence_digest: str = ""
    walrus_blob_id: Optional[str] = None
    seal_status: str = "UNSEALED"
    cost_annotation: Optional[CostAnnotation] = None
    deny_reason: str = ""
    unmet_caveats: list[str] = Field(default_factory=list)
    reputation_delta: float = 0.0
