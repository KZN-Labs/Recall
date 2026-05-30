from __future__ import annotations

from datetime import datetime
from typing import Optional

from pydantic import BaseModel


class Passport(BaseModel):
    passport_id: str
    agent_id: str
    workspace_id: str
    trust_level: int = 2
    role: str = "WRITER"
    model_provider: str
    model_name: str
    expires_at: Optional[datetime] = None
    public_key_hex: str
    state: str = "ACTIVE"
