// RECALL TypeScript SDK — wire types (mirroring the proto spec)

export interface AgentId { value: string }
export interface WorkspaceId { value: string }
export interface Hash { hex: string }
export interface WalrusBlobRef { blobId: string }

export interface CostAnnotation {
  modelProvider: string
  modelName: string
  tokensIn: number
  tokensOut: number
  usdCents: number
}

export interface CausalRef { receiptId: Hash }

export interface Signature {
  bytes: Uint8Array
  role: string
  signerPublicKey: Uint8Array
}

export interface MemoryEntry {
  // Identity
  id: string
  workspaceId: WorkspaceId
  entity: string

  // Payload
  event: string
  data: Record<string, unknown>
  tags: string[]
  scope: string

  // Actor
  agentId: AgentId
  passportId?: Hash
  trustLevel: number
  modelProvider: string
  modelName: string

  // Cryptographic links (set by the control plane on write)
  receiptId?: Hash
  walrusBlob?: WalrusBlobRef
  /** Plain string form of the Walrus blob ID — convenience mirror of walrusBlob.blobId */
  walrusBlobId?: string
  /** Ready-to-fetch aggregator URL for the blob */
  walrusUrl?: string
  /** IDs of any conflict records this entry participates in */
  conflictIds?: string[]
  sealStatus: 'UNSEALED' | 'SEALED'
  costAnnotation?: CostAnnotation
  causalPredecessors: CausalRef[]

  // Timestamps (both formats provided by the server)
  /** ISO 8601 string, e.g. "2026-06-05T23:11:48Z" */
  timestamp?: string
  /** Unix epoch seconds */
  timestampSecs?: number
}

export interface ConflictSignal {
  memoryId: string
  agentId: AgentId
  trustLevel: number
  event: string
  timestamp?: string
}

export interface ConflictRecord {
  id: string
  receiptId?: Hash
  workspaceId: WorkspaceId
  entity: string
  signalA: ConflictSignal
  signalB: ConflictSignal
  status: 'PENDING' | 'AUTO_RESOLVED' | 'MANUALLY_RESOLVED'
  autoResolution: string
  detectedAt?: string
  resolvedAt?: string
  resolution?: string
  walrusBlob?: WalrusBlobRef
}

export interface WorkspaceAgent {
  passportId: Hash
  agentId: AgentId
  role: string
  trustLevel: number
  modelProvider: string
  modelName: string
  reputation: number
  enforcementStage: 'NONE' | 'DETECT' | 'COACH' | 'QUARANTINE' | 'EVICT'
}

export interface Workspace {
  id: WorkspaceId
  name: string
  topologyMode: 'CLOSED' | 'OPEN'
  createdAt?: string
  activeConstitutionVersion: string
  agents: WorkspaceAgent[]
  snapshotBlob?: WalrusBlobRef
}

export interface HandoffCapsule {
  id: string
  fromAgentId: AgentId
  toAgentId: AgentId
  entity: string
  workspaceId: WorkspaceId
  memorySnapshot: MemoryEntry[]
  createdAt?: string
  walrusBlob?: WalrusBlobRef
}

export interface Receipt {
  id: Hash
  actionKind: string
  workspaceId: WorkspaceId
  actorPassportId: Hash
  actorAgentId: AgentId
  timestamp?: string
  causalPredecessors: CausalRef[]
  evidenceDigest: Hash
  sealStatus: 'UNSEALED' | 'SEALED'
  costAnnotation?: CostAnnotation
  denyReason?: string
  unmetCaveats: string[]
  reputationDelta: number
  walrusBlob?: WalrusBlobRef
}

export interface RegistryProfile {
  name: string
  version: string
  author: string
  category: string
  description: string
  memoryCount: number
  artifactCount: number
  importCount: number
  recommendedSystemPrompt: string
  workspaceConfig?: {
    suggestedTrustLevel: number
    suggestedRole: string
    constitutionVersion: string
  }
  walrusBlob?: WalrusBlobRef
  walrusBlobId?: string
  publisherPassportId?: string
  publishedAt?: string
  immutable: boolean
}
