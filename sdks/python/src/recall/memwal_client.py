"""
MemWal (Walrus Memory) client wrapper.

Uses the official MemWal SDK to store agent memory entries as permanent,
verifiable blobs on Walrus, and to perform semantic retrieval over stored
memories via MemWal's recall() API.

Requires:
    export MEMWAL_PRIVATE_KEY="your-ed25519-private-key"
    export MEMWAL_ACCOUNT_ID="your-memwal-account-id"

Without these, all operations are no-ops — the rest of RECALL continues
working normally using in-memory control-plane storage.
"""
from __future__ import annotations

import os


class RecallMemWalClient:
    """
    Thin wrapper around the official MemWal (Walrus Memory) SDK.
    Used by RECALL to store memory entries as permanent, verifiable blobs.
    """

    def __init__(self) -> None:
        self._available = bool(
            os.environ.get("MEMWAL_PRIVATE_KEY") and os.environ.get("MEMWAL_ACCOUNT_ID")
        )
        self.client = None

        if self._available:
            try:
                from memwal import MemWal, RecallParams  # type: ignore[import]
                self._RecallParams = RecallParams
                self.client = MemWal.create(
                    key=os.environ.get("MEMWAL_PRIVATE_KEY", ""),
                    account_id=os.environ.get("MEMWAL_ACCOUNT_ID", ""),
                    env="prod",
                    namespace="recall",
                )
            except ImportError:
                print(
                    "[recall] memwal package not installed — "
                    "run `pip install memwal` to enable Walrus Memory storage"
                )
                self._available = False
            except Exception as exc:
                print(f"[recall] MemWal init failed: {exc}")
                self._available = False

    async def store(self, content: str) -> str:
        """
        Store content via MemWal.
        Returns the job_id as the blob reference, or empty string on failure.
        """
        if not self._available or self.client is None:
            return ""
        try:
            job = await self.client.remember_and_wait(content)
            return job.job_id if hasattr(job, "job_id") else str(job)
        except Exception as exc:
            print(f"[recall] MemWal store failed: {exc}")
            return ""

    async def retrieve(self, query: str) -> list[str]:
        """
        Retrieve relevant memories via MemWal semantic search.
        Returns a list of text results, or empty list on failure.
        """
        if not self._available or self.client is None:
            return []
        try:
            result = await self.client.recall(self._RecallParams(query=query))
            return [m.text for m in result.results]
        except Exception as exc:
            print(f"[recall] MemWal retrieve failed: {exc}")
            return []

    async def close(self) -> None:
        if self._available and self.client is not None:
            try:
                await self.client.close()
            except Exception:
                pass
