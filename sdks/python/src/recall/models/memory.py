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


class MemoryEntry(BaseModel):
    id: str
    receipt_id: Optional[str] = None
    workspace_id: str
    entity: str
    agent_id: str
    passport_id: Optional[str] = None
    model_provider: str = ""
    model_name: str = ""
    trust_level: int = 1
    event: str
    data: dict[str, Any] = Field(default_factory=dict)
    tags: list[str] = Field(default_factory=list)
    scope: str = "internal"
    timestamp: Optional[datetime] = None
    walrus_blob_id: Optional[str] = None
    seal_status: str = "UNSEALED"
    cost_annotation: Optional[CostAnnotation] = None


class ConflictSignal(BaseModel):
    memory_id: str
    agent_id: str
    trust_level: int
    event: str
    timestamp: Optional[datetime] = None


class ConflictRecord(BaseModel):
    id: str
    receipt_id: Optional[str] = None
    workspace_id: str
    entity: str
    signal_a: ConflictSignal
    signal_b: ConflictSignal
    status: str = "PENDING"
    auto_resolution: str = ""
    detected_at: Optional[datetime] = None
    resolved_at: Optional[datetime] = None
    resolution: Optional[str] = None
    walrus_blob_id: Optional[str] = None


class HandoffCapsule(BaseModel):
    id: str
    from_agent_id: str
    to_agent_id: str
    entity: str
    workspace_id: str
    memory_snapshot: list[MemoryEntry] = Field(default_factory=list)
    created_at: Optional[datetime] = None
    walrus_blob_id: Optional[str] = None


class WorkspaceAgent(BaseModel):
    passport_id: str
    agent_id: str
    role: str
    trust_level: int
    model_provider: str
    model_name: str
    reputation: float = 1.0
    enforcement_stage: str = "NONE"


class Workspace(BaseModel):
    id: str
    name: str
    topology_mode: str = "CLOSED"
    created_at: Optional[datetime] = None
    active_constitution_version: str = "1.0.0"
    agents: list[WorkspaceAgent] = Field(default_factory=list)
    snapshot_blob_id: Optional[str] = None
