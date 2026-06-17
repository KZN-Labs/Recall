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


def _sidecar_main() -> int:
    """
    Sidecar entry point.

    Reads a JSON object from stdin with shape:
        {"content": "<string to remember>"}
    Calls MemWal.remember_and_wait, prints JSON to stdout:
        {"ok": true,  "job_id": "...", "blob_id": "...?"}     on success
        {"ok": false, "error":  "..."}                        on failure

    This exists so the Rust control plane can route memory writes through
    the official MemWal SDK without needing a Rust SDK port. Invoked from
    write_memory when RECALL_USE_MEMWAL=1 is set.
    """
    import asyncio
    import json
    import sys

    try:
        payload = json.loads(sys.stdin.read() or "{}")
    except json.JSONDecodeError as exc:
        json.dump({"ok": False, "error": f"invalid stdin JSON: {exc}"}, sys.stdout)
        return 2

    content = payload.get("content")
    if not isinstance(content, str) or not content:
        json.dump({"ok": False, "error": "missing or empty 'content' field"}, sys.stdout)
        return 2

    async def run() -> dict:
        c = RecallMemWalClient()
        if not c._available:
            return {"ok": False, "error": "memwal SDK unavailable or credentials missing"}
        try:
            job = await c.client.remember_and_wait(content)  # type: ignore[union-attr]
            # The SDK returns a RememberResult with .id / .blob_id / .owner /
            # .namespace. Older builds may use .job_id. Pull both defensively.
            job_id = (
                getattr(job, "id", None)
                or getattr(job, "job_id", None)
            )
            blob_id = (
                getattr(job, "blob_id", None)
                or getattr(job, "walrus_blob_id", None)
                or getattr(job, "blobId", None)
            )
            return {"ok": True, "job_id": str(job_id) if job_id else None, "blob_id": blob_id}
        except Exception as exc:  # noqa: BLE001 — sidecar reports anything that fails
            return {"ok": False, "error": str(exc)}
        finally:
            await c.close()

    result = asyncio.run(run())
    json.dump(result, sys.stdout)
    return 0 if result.get("ok") else 1


if __name__ == "__main__":  # pragma: no cover — sidecar entry
    import sys
    sys.exit(_sidecar_main())
