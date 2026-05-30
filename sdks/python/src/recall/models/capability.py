from __future__ import annotations

from datetime import datetime
from typing import Optional

from pydantic import BaseModel, Field


class CapabilityScope(BaseModel):
    permitted_action_kinds: list[str] = Field(default_factory=list)
    permitted_entity_scopes: list[str] = Field(default_factory=list)
    forbidden_entity_scopes: list[str] = Field(default_factory=list)
    resource_tags: list[str] = Field(default_factory=list)
    usd_max: str = ""
    unrestricted_actions: bool = False


class Capability(BaseModel):
    id: str
    issuer_passport_id: str
    holder_passport_id: str
    workspace_id: str
    scope: CapabilityScope
    caveats: list[dict] = Field(default_factory=list)
    valid_from: Optional[datetime] = None
    valid_until: datetime
    parent_capability_id: Optional[str] = None
    attenuation_depth: int = 0
