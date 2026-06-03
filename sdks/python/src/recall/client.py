"""
RECALL SDK — five public methods.

  workspace = await recall.connect(workspace_name, config)
  memory    = await workspace.read(entity=...)
  receipt   = await workspace.write(entity=..., event=..., value=..., metadata=...)
  capsule   = await recall.handoff(from_agent=..., to_agent=..., entity=...)
  profile   = await recall.publish(name=..., version=..., description=...)

Calls the RECALL control-plane HTTP REST API on port 8080.
gRPC (port 9090) is used by the `recall` CLI.
"""
from __future__ import annotations

import json
import os
from datetime import datetime, timezone
from typing import Any, Optional

import httpx

from .crypto import RecallKeypair, sha256_hex
from .memwal_client import RecallMemWalClient
from .models.memory import MemoryEntry, HandoffCapsule
from .models.receipt import Receipt


class WorkspaceConfig:
    def __init__(
        self,
        agent: str,
        model: str,
        api_key: Optional[str] = None,
        trust_level: int = 2,
        http_endpoint: str = "http://localhost:8080",
    ):
        self.agent = agent
        self.model = model
        self.api_key = api_key or os.getenv("RECALL_API_KEY", "")
        self.trust_level = trust_level
        self.http_endpoint = http_endpoint


class ReadResult:
    def __init__(self, entries: list[MemoryEntry], memwal_context: list[str] | None = None):
        self.entries = entries
        # Semantic context retrieved from MemWal (Walrus Memory).
        # Non-empty when MEMWAL_PRIVATE_KEY / MEMWAL_ACCOUNT_ID are configured.
        self.memwal_context: list[str] = memwal_context or []

    def __iter__(self):
        return iter(self.entries)

    def __len__(self):
        return len(self.entries)

    def latest(self) -> Optional[MemoryEntry]:
        if not self.entries:
            return None
        return max(
            self.entries,
            key=lambda e: e.timestamp or datetime.min.replace(tzinfo=timezone.utc),
        )


class Workspace:
    """Connected workspace handle. Exposes read() and write()."""

    def __init__(
        self,
        workspace_id: str,
        workspace_name: str,
        config: WorkspaceConfig,
        keypair: RecallKeypair,
        memwal: RecallMemWalClient,
    ):
        self._workspace_id = workspace_id
        self._workspace_name = workspace_name
        self._config = config
        self._keypair = keypair
        self._agent_id = config.agent
        self._passport_id = sha256_hex(
            f"{config.agent}:{keypair.public_key_bytes().hex()}".encode()
        )
        self._memwal = memwal

    async def read(self, entity: str) -> ReadResult:
        """Read all memory entries for an entity in this workspace."""
        url = f"{self._config.http_endpoint}/memory/{self._workspace_id}/{entity}"
        try:
            async with httpx.AsyncClient(timeout=10.0) as client:
                resp = await client.get(url)
                resp.raise_for_status()
                raw: list[dict[str, Any]] = resp.json()
        except (httpx.HTTPError, httpx.ConnectError) as exc:
            print(f"[recall] read failed ({exc}) — returning empty result")
            return ReadResult(entries=[])

        entries: list[MemoryEntry] = []
        for item in raw:
            ts_secs = item.get("timestamp_secs")
            ts = datetime.fromtimestamp(ts_secs, tz=timezone.utc) if ts_secs else None
            entries.append(
                MemoryEntry(
                    id=item.get("id", ""),
                    workspace_id=item.get("workspace_id", ""),
                    entity=item.get("entity", ""),
                    agent_id=item.get("agent_id", ""),
                    passport_id=item.get("passport_id", ""),
                    model_provider=item.get("model_provider", ""),
                    model_name=item.get("model_name", ""),
                    trust_level=item.get("trust_level", 2),
                    event=item.get("event", ""),
                    data=item.get("data", {}),
                    tags=item.get("tags", []),
                    scope=item.get("scope", "internal"),
                    timestamp=ts,
                )
            )

        # ── MemWal semantic retrieval ─────────────────────────────────────────
        memwal_context = await self._memwal.retrieve(query=f"memories for {entity}")

        return ReadResult(entries=entries, memwal_context=memwal_context)

    async def write(
        self,
        entity: str,
        event: str,
        value: Any,
        metadata: Optional[dict[str, Any]] = None,
        tags: Optional[list[str]] = None,
        scope: str = "internal",
    ) -> Receipt:
        """Write a memory entry. POSTs to the control-plane REST API."""
        url = f"{self._config.http_endpoint}/memory/{self._workspace_id}/{entity}"
        body: dict[str, Any] = {
            "agent_id": self._agent_id,
            "passport_id": self._passport_id,
            "event": event,
            "value": value,
            "tags": tags or [],
            "scope": scope,
            "model_provider": "anthropic",
            "model_name": self._config.model,
            "trust_level": self._config.trust_level,
        }
        if metadata:
            body["metadata"] = metadata

        receipt_id = ""
        try:
            async with httpx.AsyncClient(timeout=10.0) as client:
                resp = await client.post(url, json=body)
                resp.raise_for_status()
                result = resp.json()
                receipt_id = result.get("receipt_id", "")
        except (httpx.HTTPError, httpx.ConnectError) as exc:
            print(f"[recall] write failed ({exc}) — generating local receipt")
            receipt_id = sha256_hex(
                f"{self._workspace_id}:{entity}:{event}".encode()
            )

        # ── MemWal (Walrus Memory) permanent blob storage ─────────────────────
        value_str = value if isinstance(value, str) else json.dumps(value, default=str)
        memwal_text = (
            f"{event}: {value_str} "
            f"(entity: {entity}, agent: {self._agent_id}, tags: {tags or []})"
        )
        memwal_blob_id = await self._memwal.store(memwal_text)

        return Receipt(
            id=receipt_id,
            action_kind="memory.write",
            workspace_id=self._workspace_id,
            actor_passport_id=self._passport_id,
            actor_agent_id=self._agent_id,
            timestamp=datetime.now(tz=timezone.utc),
            memwal_blob_id=memwal_blob_id or None,
        )


class RecallClient:
    """Top-level RECALL client. Thread-safe, reusable across agents."""

    def __init__(self, http_endpoint: str = "http://localhost:8080"):
        self._http_endpoint = http_endpoint
        self._memwal = RecallMemWalClient()

    async def connect(
        self,
        workspace_name: str,
        config: Optional[WorkspaceConfig] = None,
        *,
        agent: Optional[str] = None,
        model: str = "claude-sonnet-4-6",
        api_key: Optional[str] = None,
    ) -> Workspace:
        """Connect to a workspace. Returns a handle for read/write operations."""
        if config is None:
            suffix = sha256_hex(workspace_name.encode())[:8]
            config = WorkspaceConfig(
                agent=agent or f"agent-{suffix}",
                model=model,
                api_key=api_key,
                http_endpoint=self._http_endpoint,
            )

        keypair = RecallKeypair.generate()
        workspace_id = f"ws_{workspace_name}"
        return Workspace(
            workspace_id=workspace_id,
            workspace_name=workspace_name,
            config=config,
            keypair=keypair,
            memwal=self._memwal,
        )

    async def handoff(
        self,
        from_agent: str,
        to_agent: str,
        entity: str,
        workspace_id: Optional[str] = None,
    ) -> HandoffCapsule:
        """Hand off entity memory from one agent to another."""
        url = f"{self._http_endpoint}/handoff"
        body = {
            "from_agent_id": from_agent,
            "to_agent_id": to_agent,
            "entity": entity,
            "workspace_id": workspace_id or "ws_default",
        }
        try:
            async with httpx.AsyncClient(timeout=10.0) as client:
                resp = await client.post(url, json=body)
                resp.raise_for_status()
                result = resp.json()
                return HandoffCapsule(
                    id=result.get(
                        "capsule_id",
                        f"capsule_{sha256_hex(f'{from_agent}:{to_agent}:{entity}'.encode())[:16]}",
                    ),
                    from_agent_id=from_agent,
                    to_agent_id=to_agent,
                    entity=entity,
                    workspace_id=workspace_id or "ws_default",
                    memory_snapshot=[],
                    created_at=datetime.now(tz=timezone.utc),
                )
        except Exception as exc:
            print(f"[recall] handoff failed ({exc}) — returning local capsule")
            capsule_id = sha256_hex(f"{from_agent}:{to_agent}:{entity}".encode())
            return HandoffCapsule(
                id=f"capsule_{capsule_id[:16]}",
                from_agent_id=from_agent,
                to_agent_id=to_agent,
                entity=entity,
                workspace_id=workspace_id or "ws_default",
                memory_snapshot=[],
                created_at=datetime.now(tz=timezone.utc),
            )

    async def publish(
        self,
        name: str,
        version: str,
        description: str = "",
        workspace_id: Optional[str] = None,
    ) -> dict[str, Any]:
        """Publish a workspace memory profile to the RECALL Registry."""
        url = f"{self._http_endpoint}/registry"
        body = {
            "name": name,
            "version": version,
            "description": description,
        }
        try:
            async with httpx.AsyncClient(timeout=10.0) as client:
                resp = await client.post(url, json=body)
                resp.raise_for_status()
                return resp.json()  # type: ignore[no-any-return]
        except Exception as exc:
            print(f"[recall] publish failed ({exc}) — returning local profile")
            return {
                "name": name,
                "version": version,
                "description": description,
                "published_at": datetime.now(tz=timezone.utc).isoformat(),
                "immutable": True,
            }
