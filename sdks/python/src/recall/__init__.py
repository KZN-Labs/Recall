"""
RECALL Python SDK.

Three equivalent ways to use it:

    # 1. Module-level (shortest)
    import recall
    workspace = await recall.connect("acme-ops", agent="support", model="claude-sonnet-4-6")

    # 2. Singleton instance
    from recall import recall
    workspace = await recall.connect("acme-ops", agent="support", model="claude-sonnet-4-6")

    # 3. Explicit client (when you want a custom endpoint)
    from recall import RecallClient
    client = RecallClient(http_endpoint="http://my-cp:8080")
    workspace = await client.connect("acme-ops", agent="support", model="claude-sonnet-4-6")
"""
from .client import RecallClient, Workspace

# Default singleton — talks to http://localhost:8080 unless RECALL_ENDPOINT is set.
recall = RecallClient()

# Module-level convenience functions — delegate to the singleton.
# Lets you write `import recall; await recall.connect(...)` directly.
connect = recall.connect
handoff = recall.handoff
publish = recall.publish

__all__ = ["RecallClient", "Workspace", "recall", "connect", "handoff", "publish"]
