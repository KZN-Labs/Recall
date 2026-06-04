/**
 * RECALL TypeScript SDK — five public methods.
 *
 *   const workspace = await recall.connect('acme-customer-ops', { agent, model, apiKey })
 *   const memory    = await workspace.read({ entity: 'sarah@email.com' })
 *   await workspace.write({ entity, event, value, metadata })
 *   const capsule   = await recall.handoff({ from, to, entity })
 *   await recall.publish({ name, version, description })
 */

import type {
  HandoffCapsule,
  MemoryEntry,
  Receipt,
  RegistryProfile,
} from './models'
import { generateKeypair, sha256Hex } from './crypto'

export interface ConnectConfig {
  agent: string
  model: string
  apiKey?: string
  endpoint?: string
  trustLevel?: number
}

export interface WriteInput {
  entity: string
  event: string
  value: unknown
  metadata?: Record<string, unknown>
  tags?: string[]
  scope?: string
}

export interface ReadInput {
  entity: string
}

export interface HandoffInput {
  from: string
  to: string
  entity: string
  workspaceId?: string
}

export interface PublishInput {
  name: string
  version: string
  description?: string
  workspaceId?: string
}

const DEFAULT_ENDPOINT = 'http://localhost:8080'

export class WorkspaceHandle {
  private readonly workspaceId: string
  private readonly agentId: string
  private readonly passportId: string
  private readonly config: ConnectConfig
  private readonly keypair: ReturnType<typeof generateKeypair>

  constructor(workspaceId: string, config: ConnectConfig) {
    this.workspaceId = workspaceId
    this.config = config
    this.keypair = generateKeypair()
    this.agentId = crypto.randomUUID()
    this.passportId = sha256Hex(new TextEncoder().encode(`${this.agentId}:${workspaceId}`))
  }

  /** Read all memory entries for an entity in this workspace. */
  async read(input: ReadInput): Promise<MemoryEntry[]> {
    const endpoint = this.config.endpoint ?? DEFAULT_ENDPOINT
    try {
      const resp = await fetch(
        `${endpoint}/memory/${this.workspaceId}/${encodeURIComponent(input.entity)}`,
        { headers: { Authorization: `Bearer ${this.config.apiKey ?? ''}` } }
      )
      if (!resp.ok) return []
      return await resp.json()
    } catch (err) {
      console.warn('[recall] read failed, returning empty array:', err)
      return []
    }
  }

  /** Write a memory entry. Returns the emitted receipt. */
  async write(input: WriteInput): Promise<Receipt> {
    const endpoint = this.config.endpoint ?? DEFAULT_ENDPOINT
    const data: Record<string, unknown> = { value: input.value, ...input.metadata }
    const ts = new Date().toISOString()
    const fallbackReceiptId = sha256Hex(
      new TextEncoder().encode(`${this.agentId}:${input.event}:${ts}`)
    )

    const body = {
      agent_id: this.agentId,
      passport_id: this.passportId,
      event: input.event,
      value: input.value,
      metadata: input.metadata,
      tags: input.tags ?? [],
      scope: input.scope ?? 'internal',
      model_provider: 'anthropic',
      model_name: this.config.model,
      trust_level: this.config.trustLevel ?? 2,
    }

    let receiptId = fallbackReceiptId
    let walrusBlobId: string | undefined

    try {
      const resp = await fetch(
        `${endpoint}/memory/${this.workspaceId}/${encodeURIComponent(input.entity)}`,
        {
          method: 'POST',
          headers: {
            'Content-Type': 'application/json',
            Authorization: `Bearer ${this.config.apiKey ?? ''}`,
          },
          body: JSON.stringify(body),
        }
      )
      if (resp.ok) {
        const result = await resp.json()
        receiptId = result.receipt_id ?? fallbackReceiptId
        walrusBlobId = result.walrus_blob_id
      }
    } catch (err) {
      console.warn('[recall] write failed, returning local receipt stub:', err)
    }

    // Silence the unused-var warning while keeping `data` available for future
    // local-only persistence implementations.
    void data

    const receipt: Receipt = {
      id: { hex: receiptId },
      actionKind: 'memory.write',
      workspaceId: { value: this.workspaceId },
      actorPassportId: { hex: this.passportId },
      actorAgentId: { value: this.agentId },
      timestamp: ts,
      causalPredecessors: [],
      evidenceDigest: { hex: receiptId },
      sealStatus: 'UNSEALED',
      unmetCaveats: [],
      reputationDelta: 0,
    }

    if (walrusBlobId) {
      receipt.walrusBlob = { blobId: walrusBlobId }
    }

    return receipt
  }
}

export class RecallClient {
  private readonly endpoint: string

  constructor(endpoint = DEFAULT_ENDPOINT) {
    this.endpoint = endpoint
  }

  /** Connect to a workspace. Returns a handle for read/write. */
  async connect(workspaceName: string, config: ConnectConfig): Promise<WorkspaceHandle> {
    const workspaceId = `ws_${workspaceName}`
    return new WorkspaceHandle(workspaceId, {
      ...config,
      endpoint: config.endpoint ?? this.endpoint,
    })
  }

  /** Hand off entity memory from one agent to another. */
  async handoff(input: HandoffInput): Promise<HandoffCapsule> {
    const body = {
      from_agent_id: input.from,
      to_agent_id: input.to,
      entity: input.entity,
      workspace_id: input.workspaceId ?? 'ws_default',
    }

    try {
      const resp = await fetch(`${this.endpoint}/handoff`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(body),
      })

      if (resp.ok) {
        const data = await resp.json()
        return {
          id: data.capsule_id ?? `capsule_${crypto.randomUUID()}`,
          fromAgentId: { value: input.from },
          toAgentId: { value: input.to },
          entity: input.entity,
          workspaceId: { value: input.workspaceId ?? 'ws_default' },
          memorySnapshot: data.memory_snapshot ?? [],
          createdAt: data.created_at ?? new Date().toISOString(),
        }
      }
    } catch (err) {
      console.warn('[recall] handoff failed, using local stub:', err)
    }

    // Fallback — control plane not reachable
    return {
      id: `capsule_${crypto.randomUUID()}`,
      fromAgentId: { value: input.from },
      toAgentId: { value: input.to },
      entity: input.entity,
      workspaceId: { value: input.workspaceId ?? 'ws_default' },
      memorySnapshot: [],
      createdAt: new Date().toISOString(),
    }
  }

  /** Publish a workspace memory profile to the RECALL Registry. */
  async publish(input: PublishInput): Promise<RegistryProfile> {
    const body = {
      name: input.name,
      version: input.version,
      description: input.description ?? '',
      workspace_id: input.workspaceId,
    }

    try {
      const resp = await fetch(`${this.endpoint}/registry`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(body),
      })

      if (resp.ok) {
        const data = await resp.json()
        return {
          name: data.name ?? input.name,
          version: data.version ?? input.version,
          author: data.author ?? '',
          category: data.category ?? '',
          description: data.description ?? input.description ?? '',
          memoryCount: data.memory_count ?? 0,
          artifactCount: data.artifact_count ?? 0,
          importCount: data.import_count ?? 0,
          recommendedSystemPrompt: data.recommended_system_prompt ?? '',
          publishedAt: data.published_at ?? new Date().toISOString(),
          immutable: true,
          walrusBlobId: data.walrus_blob_id,
        }
      }
    } catch (err) {
      console.warn('[recall] publish failed, using local stub:', err)
    }

    // Fallback — control plane not reachable
    return {
      name: input.name,
      version: input.version,
      author: '',
      category: '',
      description: input.description ?? '',
      memoryCount: 0,
      artifactCount: 0,
      importCount: 0,
      recommendedSystemPrompt: '',
      publishedAt: new Date().toISOString(),
      immutable: true,
    }
  }
}

export const recall = new RecallClient()
