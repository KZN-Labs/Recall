# RECALL

**Shared memory OS for AI agents — governed, auditable, permanent.**

---

## The problem

When multiple AI agents share memory today, there are no rules. A support agent offers a customer a 10% credit. A fraud agent flags the same customer for suspicious activity. A billing agent reads memory and applies the credit anyway — because it never saw the fraud flag. Nobody knows. There is no trail.

This is how every multi-agent system using Mem0, Zep, or LangMem behaves today. The most recent write wins silently. Conflicts are invisible. Nothing is verifiable. In production, this causes wrong decisions, compliance failures, and bugs that are impossible to debug after the fact.

RECALL fixes this.

---

## What RECALL does

Every memory write is **governed** by on-chain rules, produces an **immutable receipt**, and is checked for **conflicts** with other agents writing about the same entity. Receipts are anchored to Sui via Walrus blobs — any party can independently verify the full decision trail without trusting the control plane.

```
  agent A          agent B          agent C
     │                │                │
     └────────────────┴────────────────┘
                      │
              ┌───────▼────────┐
              │ control plane  │  ← governance rules (on-chain)
              │  gRPC :9090    │  ← conflict detection
              │  REST :8080    │  ← receipt emission
              └───────┬────────┘
                      │
           ┌──────────┴──────────┐
           │                     │
     ┌─────▼──────┐       ┌──────▼──────┐
     │   Walrus   │       │     Sui     │
     │ blob store │       │  Merkle root│
     │  (memory)  │       │  (receipts) │
     └────────────┘       └─────────────┘
```

**What makes RECALL different from existing memory layers:**

| | Mem0 / Zep / LangMem | RECALL |
|---|---|---|
| Multi-agent conflict detection | ✗ | ✓ |
| Immutable receipt per write | ✗ | ✓ |
| On-chain governance rules | ✗ | ✓ |
| Verifiable audit trail | ✗ | ✓ |
| Portable memory registry | ✗ | ✓ |
| Rollback to any point in time | ✗ | ✓ |

---

## Quickstart

**1. Start the control plane**

```bash
cargo run -p recall-control-plane
# gRPC on :9090  REST on :8080
```

**2. Add RECALL to your agent**

```python
pip install recall-sdk
```

```python
from recall import recall

# Connect any agent to a shared workspace
workspace = await recall.connect(
    "acme-ops",
    agent="support-agent",
    model="claude-sonnet-4-6",
)

# Read what other agents have written about this entity
memory = await workspace.read(entity="sarah@email.com")

# Write a memory entry — produces a signed receipt
receipt = await workspace.write(
    entity="sarah@email.com",
    event="credit_offered",
    value="10%",
    tags=["customer", "billing"],
)

# Hand off entity context to another agent
capsule = await recall.handoff(
    from_agent="support-agent",
    to_agent="billing-agent",
    entity="sarah@email.com",
)

# Publish your agent's accumulated knowledge to the Registry
profile = await recall.publish(
    name="support-agent-v1",
    version="1.0",
    description="6 months of customer support memory",
)
```

**3. Run the demo scenario**

```bash
python demo_seed.py
# Seeds 3 agents, 2 entities, 1 conflict into a live workspace
```

---

## CLI

The `recall` CLI is the primary interface for inspecting and managing multi-agent memory.

### Watch live agent activity

```bash
recall logs --follow
```
```
TIMESTAMP             AGENT                 EVENT                    ENTITY
────────────────────────────────────────────────────────────────────────────
2026-05-30 11:59:04   support-agent         login_issue_resolved     sarah@email.com
2026-05-30 11:59:04   support-agent         credit_offered           sarah@email.com
2026-05-30 11:59:04   fraud-agent           flag_suspicious          sarah@email.com   ← conflict
2026-05-30 11:59:04   billing-agent         credit_applied           sarah@email.com
```

### Find conflicts and denied writes

```bash
recall failures
```
```
CONFLICTS
────────────────────────────────────────────────────────────────────────────
—   ⚠ conflict_019e7   support-agent vs fraud-agent   sarah@email.com   ⚠ UNRESOLVED
```

### Trace the full decision trail for any entity

```bash
recall why --entity sarah@email.com
```
```
RECALL receipt trail for sarah@email.com
────────────────────────────────────────────────────────────────────────────
11:59:04  support-agent  →  login_issue_resolved
          value: true   tags: customer   receipt: #mem_6d21b53f
          │
11:59:04  support-agent  →  credit_offered
          value: "10%"   tags: customer billing   receipt: #mem_f97c062d
          ⚡ CONFLICT with: flag_suspicious (fraud-agent) → UNRESOLVED
          │
11:59:04  fraud-agent  →  flag_suspicious
          value: "velocity_anomaly"   tags: security   trust:HIGH   receipt: #mem_da21527a
          │
11:59:04  billing-agent  →  credit_applied
          value: "10%"   tags: billing   receipt: #mem_edc523ad

4 entries  ·  1 conflict (0 resolved)  ·  receipt chain intact
```

### Incident response flow

```bash
recall failures                                           # find the problem
recall why --entity <id>                                  # understand what happened
recall inspect <receipt-id or conflict-id>                # deep dive
recall rollback --workspace acme-ops --to <timestamp>     # fix it
recall export --entity <id>                               # PDF audit trail
```

### All commands

```
recall logs        [--workspace] [--entity] [--follow]
recall failures    [--workspace] [--unresolved]
recall why         --entity <id> [--workspace]
recall inspect     <id>
recall agents      [--workspace]
recall rollback    --workspace <name> --to <timestamp>
recall export      --entity <id> [--output <path.pdf>]
recall workspace   list | create <name> | add-agent --workspace <name>
recall registry    list | inspect <name@version> | import <name@version> | publish
recall keygen
```

---

## Registry

The Registry is npm for agent knowledge.

When an agent accumulates months of domain-specific memory — customer patterns, clinical decisions, financial rules — that knowledge is portable. Publish it once, import it anywhere. Every profile is immutable and permanently stored on Walrus with a Merkle root anchored on Sui. Any party can verify the profile hasn't been tampered with.

```bash
# Publish your agent's memory to the registry
recall registry publish
# → name: support-agent-v1  version: 1.0
# → walrus blob: 0xf1a2b3...  anchored on Sui

# Any team can import it — new agent starts with full context
recall registry import support-agent-v1@1.0

# Browse available profiles
recall registry list
```

**Use cases:**
- A new agent deployment instantly inherits 6 months of institutional knowledge
- Regulatory compliance profiles published by trusted sources, cryptographically verifiable
- Open-source community profiles for common domains (customer support, fraud detection, clinical triage)
- Teams sell or share specialized agent knowledge the same way software packages are distributed

---

## On-chain governance

Write access is enforced by Move contracts deployed on Sui. The control plane cannot override governance rules — they are verified on-chain.

Rules evaluated on every write (in priority order):
1. Quarantined or evicted agents are always blocked
2. Agent role must be WRITER, SUPERVISOR, or ADMIN
3. Agent trust level must meet the workspace minimum
4. PII-tagged entries cannot be written to external scope
5. Low-trust agents or entries matching supervisor-required tags need countersign

Enforcement escalates automatically on repeated violations: `NONE → DETECT → COACH → QUARANTINE → EVICT`

---

## Architecture

```
sdks/python/          Python SDK (httpx → REST :8080)
sdks/typescript/      TypeScript SDK

crates/
  recall-control-plane/   gRPC server + Axum REST API
  recall-ops/             recall CLI (all commands)
  recall-receipt/         receipt builder + Merkle tree
  recall-memory/          in-process memory store
  recall-conflict/        conflict detection engine
  recall-capability/      capability tokens + UCAN caveats
  recall-passport/        agent identity (Ed25519)
  recall-crypto/          signing, hashing, canonical serialisation

backends/
  walrus-memory/          Walrus blob storage for memory
  walrus-receipt/         receipt batch sealing + Merkle anchoring
  sui-anchor/             Sui transaction driver for receipt anchoring
  seal-encryption/        Seal threshold encryption for PII entries

contracts/sui/
  workspace_registry/     workspace ownership + capability tokens
  workspace_governance/   write-access policy enforcement
  receipt_anchor/         Merkle root commitment to chain
```

---

## Stack

Rust · Python · TypeScript · Walrus · MemWal · Seal · Sui · Move
