# RECALL

Shared memory OS for AI agents.

## What it does

RECALL gives AI agents persistent shared memory with conflict detection and a verifiable audit trail. Every write is governed by on-chain rules, every action produces an immutable receipt, and every conflict between agents is automatically detected and resolved. Memory profiles are portable — publish once to the RECALL Registry on Walrus, import into any agent deployment.

## Install

```bash
pip install recall-sdk
npm install @recall/sdk
```

## Add to your agent

```python
from recall import recall

workspace = await recall.connect("acme-ops", agent="support-agent", model="claude-sonnet-4-6")

memory    = await workspace.read(entity="sarah@email.com")
receipt   = await workspace.write(entity="sarah@email.com", event="credit_offered", value="10%")
capsule   = await recall.handoff(from_agent="support-agent", to_agent="billing-agent", entity="sarah@email.com")
profile   = await recall.publish(name="support-v1", version="1.0", description="6 months of support memory")
```

## CLI

```
recall logs --follow
```
```
2026-05-18 10:30:01  support-agent   login_issue_resolved   sarah@email.com   #b1bc1d17b334
2026-05-18 10:32:04  support-agent   credit_offered         sarah@email.com   #312769fef518
2026-05-18 10:34:11  fraud-agent     flag_suspicious        sarah@email.com   #a1b99801695
2026-05-18 10:35:02  billing-agent   credit_applied         sarah@email.com   #d5c98496e279
```

```
recall failures
```
```
CONFLICTS
────────────────────────────────────────────────────────────────────────
2026-05-18 10:34   ⚠  conflict_001   support-agent vs fraud-agent   sarah@email.com   ⚠ UNRESOLVED
```

```
recall why --entity sarah@email.com
```
```
RECALL receipt trail for sarah@email.com
────────────────────────────────────────────────────────────────────────
10:30:01  support-agent → login_issue_resolved
          value: true   tags: customer   receipt: #b1bc1d17b334
          │
10:32:04  support-agent → credit_offered
          value: "10%"   tags: customer billing   receipt: #312769fef518
          ⚡ CONFLICT with: flag_suspicious (fraud-agent) → SIGNAL_B_PREFERRED
          │
10:34:11  fraud-agent → flag_suspicious
          value: "velocity_anomaly"   tags: security   trust:HIGH   receipt: #a1b99801695
          │
10:35:02  billing-agent → credit_applied
          value: "10%"   tags: billing   receipt: #d5c98496e279

4 entries  ·  1 conflict (0 resolved)  ·  receipt chain intact
```

```
recall inspect <id>          # receipt · conflict · memory entry
recall agents                # all agents with enforcement stages
recall workspace list        # all workspaces with activity stats
recall rollback --workspace acme-ops --to 2026-05-18T10:30:00Z
recall export --entity sarah@email.com   # PDF audit trail
```

## Registry

```bash
recall registry publish      # interactive — publishes memory profile to Walrus
recall registry import support-v1@1.0   # import into a new pre-loaded workspace
```

Publish once, import anywhere. Every profile is immutable and permanently stored on Walrus with a Merkle root anchored on Sui.

## Stack

Rust · Python · TypeScript · Walrus · MemWal · Seal · Sui · Move
