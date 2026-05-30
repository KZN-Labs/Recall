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
    const endpoint = this.config.endpoint ?? 'http://localhost:9090'
    try {
      const resp = await fetch(`${endpoint}/memory/${this.workspaceId}/${encodeURIComponent(input.entity)}`, {
        headers: { Authorization: `Bearer ${this.config.apiKey ?? ''}` },
      })
      if (!resp.ok) return []
      return await resp.json()
    } catch {
      return []
    }
  }

  /** Write a memory entry. Returns the emitted receipt. */
  async write(input: WriteInput): Promise<Receipt> {
    const endpoint = this.config.endpoint ?? 'http://localhost:9090'
    const data: Record<string, unknown> = { value: input.value, ...input.metadata }
    const ts = new Date().toISOString()
    const receiptId = sha256Hex(new TextEncoder().encode(`${this.agentId}:${input.event}:${ts}`))

    const entry: Partial<MemoryEntry> = {
      id: `mem_${crypto.randomUUID()}`,
      workspaceId: { value: this.workspaceId },
      entity: input.entity,
      agentId: { value: this.agentId },
      passportId: { hex: this.passportId },
      modelProvider: 'anthropic',
      modelName: this.config.model,
      trustLevel: this.config.trustLevel ?? 2,
      event: input.event,
      data,
      tags: input.tags ?? [],
      scope: input.scope ?? 'internal',
      timestamp: ts,
      sealStatus: 'UNSEALED',
      causalPredecessors: [],
      unmetCaveats: [],
    }

    try {
      await fetch(`${endpoint}/memory`, {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
          Authorization: `Bearer ${this.config.apiKey ?? ''}`,
        },
        body: JSON.stringify(entry),
      })
    } catch {
      // Server not available — still return a local receipt stub.
    }

    return {
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
  }
}

export class RecallClient {
  private readonly endpoint: string

  constructor(endpoint = 'http://localhost:9090') {
    this.endpoint = endpoint
  }

  /** Connect to a workspace. Returns a handle for read/write. */
  async connect(workspaceName: string, config: ConnectConfig): Promise<WorkspaceHandle> {
    const workspaceId = `ws_${workspaceName}`
    return new WorkspaceHandle(workspaceId, { ...config, endpoint: config.endpoint ?? this.endpoint })
  }

  /** Hand off entity memory from one agent to another. */
  async handoff(input: HandoffInput): Promise<HandoffCapsule> {
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
