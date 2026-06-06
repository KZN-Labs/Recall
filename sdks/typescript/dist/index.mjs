// src/crypto.ts
import { sha256 } from "@noble/hashes/sha256";
import { bytesToHex, hexToBytes } from "@noble/hashes/utils";
import { ed25519 } from "@noble/curves/ed25519";
function generateKeypair() {
  const privateKey = ed25519.utils.randomPrivateKey();
  const publicKey = ed25519.getPublicKey(privateKey);
  return { publicKey, privateKey };
}
function sign(message, privateKey) {
  return ed25519.sign(message, privateKey);
}
function sha256Hex(data) {
  return bytesToHex(sha256(data));
}
function toHex(bytes) {
  return bytesToHex(bytes);
}
function signMessage(message, keypair) {
  return bytesToHex(sign(message, keypair.privateKey));
}

// src/client.ts
var DEFAULT_ENDPOINT = "http://localhost:8080";
var WorkspaceHandle = class {
  workspaceId;
  agentId;
  passportId;
  config;
  keypair;
  constructor(workspaceId, config) {
    this.workspaceId = workspaceId;
    this.config = config;
    this.keypair = generateKeypair();
    this.agentId = crypto.randomUUID();
    this.passportId = sha256Hex(new TextEncoder().encode(`${this.agentId}:${workspaceId}`));
  }
  /** Read all memory entries for an entity in this workspace. */
  async read(input) {
    const endpoint = this.config.endpoint ?? DEFAULT_ENDPOINT;
    try {
      const resp = await fetch(
        `${endpoint}/memory/${this.workspaceId}/${encodeURIComponent(input.entity)}`,
        { headers: { Authorization: `Bearer ${this.config.apiKey ?? ""}` } }
      );
      if (!resp.ok) return [];
      return await resp.json();
    } catch (err) {
      console.warn("[recall] read failed, returning empty array:", err);
      return [];
    }
  }
  /** Write a memory entry. Returns the emitted receipt. */
  async write(input) {
    const endpoint = this.config.endpoint ?? DEFAULT_ENDPOINT;
    const data = { value: input.value, ...input.metadata };
    const ts = (/* @__PURE__ */ new Date()).toISOString();
    const fallbackReceiptId = sha256Hex(
      new TextEncoder().encode(`${this.agentId}:${input.event}:${ts}`)
    );
    const body = {
      agent_id: this.agentId,
      passport_id: this.passportId,
      event: input.event,
      value: input.value,
      metadata: input.metadata,
      tags: input.tags ?? [],
      scope: input.scope ?? "internal",
      model_provider: "anthropic",
      model_name: this.config.model,
      trust_level: this.config.trustLevel ?? 2
    };
    let receiptId = fallbackReceiptId;
    let walrusBlobId;
    try {
      const resp = await fetch(
        `${endpoint}/memory/${this.workspaceId}/${encodeURIComponent(input.entity)}`,
        {
          method: "POST",
          headers: {
            "Content-Type": "application/json",
            Authorization: `Bearer ${this.config.apiKey ?? ""}`
          },
          body: JSON.stringify(body)
        }
      );
      if (resp.ok) {
        const result = await resp.json();
        receiptId = result.receipt_id ?? fallbackReceiptId;
        walrusBlobId = result.walrus_blob_id;
      }
    } catch (err) {
      console.warn("[recall] write failed, returning local receipt stub:", err);
    }
    void data;
    const receipt = {
      id: { hex: receiptId },
      actionKind: "memory.write",
      workspaceId: { value: this.workspaceId },
      actorPassportId: { hex: this.passportId },
      actorAgentId: { value: this.agentId },
      timestamp: ts,
      causalPredecessors: [],
      evidenceDigest: { hex: receiptId },
      sealStatus: "UNSEALED",
      unmetCaveats: [],
      reputationDelta: 0
    };
    if (walrusBlobId) {
      receipt.walrusBlob = { blobId: walrusBlobId };
    }
    return receipt;
  }
};
var RecallClient = class {
  endpoint;
  globalKeypair;
  globalPassportId;
  constructor(endpoint = DEFAULT_ENDPOINT) {
    this.endpoint = endpoint;
    this.globalKeypair = generateKeypair();
    this.globalPassportId = sha256Hex(
      new TextEncoder().encode(toHex(this.globalKeypair.publicKey))
    );
  }
  /** Connect to a workspace. Returns a handle for read/write. */
  async connect(workspaceName, config) {
    const workspaceId = `ws_${workspaceName}`;
    return new WorkspaceHandle(workspaceId, {
      ...config,
      endpoint: config.endpoint ?? this.endpoint
    });
  }
  /** Hand off entity memory from one agent to another. */
  async handoff(input) {
    const body = {
      from_agent_id: input.from,
      to_agent_id: input.to,
      entity: input.entity,
      workspace_id: input.workspaceId ?? "ws_default"
    };
    try {
      const resp = await fetch(`${this.endpoint}/handoff`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(body)
      });
      if (resp.ok) {
        const data = await resp.json();
        return {
          id: data.capsule_id ?? `capsule_${crypto.randomUUID()}`,
          fromAgentId: { value: input.from },
          toAgentId: { value: input.to },
          entity: input.entity,
          workspaceId: { value: input.workspaceId ?? "ws_default" },
          memorySnapshot: data.memory_snapshot ?? [],
          createdAt: data.created_at ?? (/* @__PURE__ */ new Date()).toISOString()
        };
      }
    } catch (err) {
      console.warn("[recall] handoff failed, using local stub:", err);
    }
    return {
      id: `capsule_${crypto.randomUUID()}`,
      fromAgentId: { value: input.from },
      toAgentId: { value: input.to },
      entity: input.entity,
      workspaceId: { value: input.workspaceId ?? "ws_default" },
      memorySnapshot: [],
      createdAt: (/* @__PURE__ */ new Date()).toISOString()
    };
  }
  /**
   * Publish a workspace memory profile to the RECALL Registry.
   *
   * Signs the canonical payload `<name>@<version>:<passport_id>` with the
   * client's Ed25519 keypair. The control plane verifies the signature
   * server-side — authorship cannot be forged.
   *
   * Throws on 409 (already published — profiles are immutable).
   * Throws on 401 (signature rejected).
   */
  async publish(input) {
    const message = new TextEncoder().encode(
      `${input.name}@${input.version}:${this.globalPassportId}`
    );
    const signatureHex = signMessage(message, this.globalKeypair);
    const publicKeyHex = toHex(this.globalKeypair.publicKey);
    const body = {
      name: input.name,
      version: input.version,
      description: input.description ?? "",
      workspace_id: input.workspaceId,
      category: "",
      passport_id: this.globalPassportId,
      signature: signatureHex,
      public_key: publicKeyHex
    };
    try {
      const resp = await fetch(`${this.endpoint}/registry`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(body)
      });
      if (resp.status === 409) {
        throw new Error(
          `${input.name}@${input.version} already published \u2014 profiles are immutable. publish a new version.`
        );
      }
      if (resp.status === 401) {
        const data = await resp.json().catch(() => ({}));
        throw new Error(`publish rejected: ${data.error ?? "unauthorized"}`);
      }
      if (resp.ok) {
        const data = await resp.json();
        return {
          name: data.name ?? input.name,
          version: data.version ?? input.version,
          author: data.author ?? "",
          category: data.category ?? "",
          description: data.description ?? input.description ?? "",
          memoryCount: data.memory_count ?? 0,
          artifactCount: data.artifact_count ?? 0,
          importCount: data.import_count ?? 0,
          recommendedSystemPrompt: data.recommended_system_prompt ?? "",
          publishedAt: data.published_at ?? (/* @__PURE__ */ new Date()).toISOString(),
          immutable: true,
          walrusBlobId: data.walrus_blob_id,
          publisherPassportId: data.publisher_passport_id
        };
      }
    } catch (err) {
      if (err instanceof Error && (err.message.includes("already published") || err.message.includes("publish rejected"))) {
        throw err;
      }
      console.warn("[recall] publish failed, using local stub:", err);
    }
    return {
      name: input.name,
      version: input.version,
      author: "",
      category: "",
      description: input.description ?? "",
      memoryCount: 0,
      artifactCount: 0,
      importCount: 0,
      recommendedSystemPrompt: "",
      publishedAt: (/* @__PURE__ */ new Date()).toISOString(),
      immutable: true
    };
  }
};
var recall = new RecallClient();
export {
  RecallClient,
  WorkspaceHandle,
  generateKeypair,
  recall,
  sha256Hex
};
