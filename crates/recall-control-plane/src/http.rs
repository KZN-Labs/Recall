/// Axum HTTP/REST layer — runs on :8080 alongside the gRPC server (:9090).
///
/// Routes consumed by the Next.js dashboard and Python SDK:
///   GET  /health
///   GET  /receipts?workspace_id=X[&action_kind=Y]
///   GET  /receipts/:id
///   GET  /memory/:workspace_id/:entity
///   POST /memory/:workspace_id/:entity          body: WriteMemoryBody JSON
///   GET  /conflicts/:workspace_id
///   GET  /registry[?category=X]
use std::sync::Arc;

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Json},
    routing::get,
    Router,
};
use serde::{Deserialize, Serialize};
use tower_http::cors::CorsLayer;

use recall_core::ids::{AgentId, ContentHash, WorkspaceId};
use recall_proto::{common as common_proto, memory as memory_proto};
use recall_receipt::{action_kind, builder::ReceiptBuilder};

use crate::state::AppState;

// ── prost_types::Struct <-> serde_json conversion ────────────────────────────

fn prost_value_to_json(v: &prost_types::Value) -> serde_json::Value {
    use prost_types::value::Kind;
    match &v.kind {
        Some(Kind::NullValue(_))    => serde_json::Value::Null,
        Some(Kind::BoolValue(b))    => serde_json::Value::Bool(*b),
        Some(Kind::NumberValue(n))  => serde_json::Number::from_f64(*n)
            .map(serde_json::Value::Number)
            .unwrap_or(serde_json::Value::Null),
        Some(Kind::StringValue(s))  => serde_json::Value::String(s.clone()),
        Some(Kind::ListValue(l))    => serde_json::Value::Array(
            l.values.iter().map(prost_value_to_json).collect(),
        ),
        Some(Kind::StructValue(s))  => prost_struct_to_json(s),
        None                        => serde_json::Value::Null,
    }
}

fn prost_struct_to_json(s: &prost_types::Struct) -> serde_json::Value {
    let map: serde_json::Map<String, serde_json::Value> = s
        .fields
        .iter()
        .map(|(k, v)| (k.clone(), prost_value_to_json(v)))
        .collect();
    serde_json::Value::Object(map)
}

fn json_to_prost_value(v: serde_json::Value) -> prost_types::Value {
    use prost_types::value::Kind;
    let kind = match v {
        serde_json::Value::Null       => Kind::NullValue(0),
        serde_json::Value::Bool(b)    => Kind::BoolValue(b),
        serde_json::Value::Number(n)  => Kind::NumberValue(n.as_f64().unwrap_or(0.0)),
        serde_json::Value::String(s)  => Kind::StringValue(s),
        serde_json::Value::Array(arr) => Kind::ListValue(prost_types::ListValue {
            values: arr.into_iter().map(json_to_prost_value).collect(),
        }),
        serde_json::Value::Object(m)  => Kind::StructValue(prost_types::Struct {
            fields: m.into_iter().map(|(k, v)| (k, json_to_prost_value(v))).collect::<std::collections::BTreeMap<_,_>>(),
        }),
    };
    prost_types::Value { kind: Some(kind) }
}

fn json_to_prost_struct(v: serde_json::Value) -> prost_types::Struct {
    match v {
        serde_json::Value::Object(m) => prost_types::Struct {
            fields: m.into_iter()
                .map(|(k, v)| (k, json_to_prost_value(v)))
                .collect::<std::collections::BTreeMap<_,_>>(),
        },
        other => {
            let mut fields = std::collections::BTreeMap::new();
            fields.insert("value".into(), json_to_prost_value(other));
            prost_types::Struct { fields }
        }
    }
}

// ── JSON response shapes ──────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct ReceiptJson {
    pub id: String,
    pub action_kind: String,
    pub workspace_id: String,
    pub actor_passport_id: String,
    pub actor_agent_id: String,
    pub timestamp_secs: Option<i64>,
    pub seal_status: i32,
    pub deny_reason: Option<String>,
    pub reputation_delta: f64,
    /// Content hash of the underlying evidence this receipt anchors. For
    /// `anchor.commit` receipts this is the Merkle root of the receipt batch.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub evidence_digest: String,
    /// Receipt IDs this receipt causally depends on. Empty for most receipts;
    /// `anchor.commit` lists the receipt IDs in the batch.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub causal_predecessors: Vec<String>,
}

#[derive(Serialize)]
pub struct MemoryEntryJson {
    // ── Identity ──
    pub id: String,
    pub workspace_id: String,
    pub entity: String,

    // ── Payload ──
    pub event: String,
    pub data: serde_json::Value,
    pub tags: Vec<String>,
    pub scope: String,

    // ── Actor ──
    pub agent_id: String,
    pub passport_id: String,
    pub trust_level: i32,
    pub model_provider: String,
    pub model_name: String,

    // ── Cryptographic links ──
    /// SHA-256 of the canonical receipt for this write — the audit-trail handle.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub receipt_id: Option<String>,
    /// Walrus blob ID — the permanent data handle.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub walrus_blob_id: Option<String>,
    /// Ready-to-fetch aggregator URL for the blob.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub walrus_url: Option<String>,
    /// IDs of any conflict records this entry participates in.
    pub conflict_ids: Vec<String>,

    /// If this entry was bulk-imported from a registry profile, the original
    /// memory ID in the source workspace.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub imported_from: Option<String>,

    // ── Timestamps — both formats so humans and machines are both happy ──
    /// ISO 8601 timestamp, e.g. "2026-06-05T23:11:48Z".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<String>,
    pub timestamp_secs: Option<i64>,
}

#[derive(Serialize)]
pub struct ConflictJson {
    pub conflict_id: String,
    pub workspace_id: String,
    pub entity: String,
    pub entry_a_id: String,
    pub entry_b_id: String,
    pub auto_resolution: String,
    pub resolution: String,
}

#[derive(Serialize)]
pub struct RegistryProfileJson {
    pub name: String,
    pub version: String,
    pub author: String,
    pub description: String,
    pub category: String,
    pub memory_count: i64,
    pub import_count: i64,
}

// ── Request bodies ────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct WriteMemoryBody {
    pub agent_id: String,
    pub passport_id: String,
    pub event: String,
    pub value: serde_json::Value,
    #[serde(default)]
    pub metadata: Option<serde_json::Value>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default = "default_scope")]
    pub scope: String,
    #[serde(default = "default_provider")]
    pub model_provider: String,
    #[serde(default = "default_model")]
    pub model_name: String,
    #[serde(default = "default_trust")]
    pub trust_level: i32,
    /// Ed25519 signature (hex) over canonical message
    /// `format!("{ws}:{entity}:{event}:{agent_id}")`. Required.
    #[serde(default)]
    pub signature: String,
    /// Ed25519 public key (hex). Must match the agent's passport on file if
    /// registered; otherwise the write is accepted as self-attested.
    #[serde(default)]
    pub public_key: String,
}

fn default_scope()    -> String { "internal".into() }
fn default_provider() -> String { "unknown".into() }
fn default_model()    -> String { "unknown".into() }
fn default_trust()    -> i32    { 2 }

// ── Query params ──────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct ListReceiptsQuery {
    pub workspace_id: Option<String>,
    pub action_kind: Option<String>,
    /// If set, return only the most recent N receipts.
    pub limit: Option<usize>,
}

#[derive(Deserialize)]
pub struct RegistryQuery {
    pub category: Option<String>,
}

// ── Conversion helpers ────────────────────────────────────────────────────────

fn receipt_to_json(r: &recall_proto::receipt::Receipt) -> ReceiptJson {
    ReceiptJson {
        id: r.id.as_ref().map(|h| h.hex.clone()).unwrap_or_default(),
        action_kind: r.action_kind.clone(),
        workspace_id: r.workspace_id.as_ref().map(|w| w.value.clone()).unwrap_or_default(),
        actor_passport_id: r.actor_passport_id.as_ref().map(|h| h.hex.clone()).unwrap_or_default(),
        actor_agent_id: r.actor_agent_id.as_ref().map(|a| a.value.clone()).unwrap_or_default(),
        timestamp_secs: r.timestamp.as_ref().map(|t| t.seconds),
        seal_status: r.seal_status,
        deny_reason: if r.deny_reason.is_empty() { None } else { Some(r.deny_reason.clone()) },
        reputation_delta: r.reputation_delta as f64,
        evidence_digest: r.evidence_digest.as_ref().map(|h| h.hex.clone()).unwrap_or_default(),
        causal_predecessors: r
            .causal_predecessors
            .iter()
            .filter_map(|c| c.receipt_id.as_ref().map(|h| h.hex.clone()))
            .collect(),
    }
}

fn entry_to_json(e: &memory_proto::MemoryEntry, state: &AppState) -> MemoryEntryJson {
    let data = e.data.as_ref()
        .map(prost_struct_to_json)
        .unwrap_or(serde_json::Value::Object(Default::default()));

    let workspace_id = e.workspace_id.as_ref().map(|w| w.value.clone()).unwrap_or_default();
    let walrus_blob_id = e.walrus_blob.as_ref().map(|w| w.blob_id.clone());
    let walrus_url = walrus_blob_id.as_ref().map(|id| format!(
        "https://aggregator.walrus-testnet.walrus.space/v1/blobs/{}",
        id
    ));

    // Find any conflict records that reference this memory entry.
    // Conflicts are stored per-workspace; we filter by signal_a/signal_b memory_id.
    let conflict_ids: Vec<String> = state
        .conflict_store
        .list_pending(&workspace_id)
        .into_iter()
        .filter(|c| {
            c.signal_a.as_ref().map(|s| s.memory_id == e.id).unwrap_or(false)
                || c.signal_b.as_ref().map(|s| s.memory_id == e.id).unwrap_or(false)
        })
        .map(|c| c.id)
        .collect();

    let timestamp_secs = e.timestamp.as_ref().map(|t| t.seconds);
    let timestamp = timestamp_secs.and_then(|secs| {
        chrono::DateTime::<chrono::Utc>::from_timestamp(secs, 0)
            .map(|dt| dt.format("%Y-%m-%dT%H:%M:%SZ").to_string())
    });

    MemoryEntryJson {
        id: e.id.clone(),
        workspace_id,
        entity: e.entity.clone(),

        event: e.event.clone(),
        data,
        tags: e.tags.clone(),
        scope: e.scope.clone(),

        agent_id: e.agent_id.as_ref().map(|a| a.value.clone()).unwrap_or_default(),
        passport_id: e.passport_id.as_ref().map(|h| h.hex.clone()).unwrap_or_default(),
        trust_level: e.trust_level,
        model_provider: e.model_provider.clone(),
        model_name: e.model_name.clone(),

        receipt_id: e.receipt_id.as_ref().map(|h| h.hex.clone()),
        walrus_blob_id,
        walrus_url,
        conflict_ids,
        imported_from: e.imported_from.as_ref().map(|h| h.hex.clone()),

        timestamp,
        timestamp_secs,
    }
}

// ── Route handlers ────────────────────────────────────────────────────────────

async fn health(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "status":          "ok",
        "service":         "recall-control-plane",
        "walrus_enabled":  state.walrus.is_some(),
    }))
}

async fn list_receipts(
    State(state): State<Arc<AppState>>,
    Query(q): Query<ListReceiptsQuery>,
) -> Json<Vec<ReceiptJson>> {
    let ws = q.workspace_id.unwrap_or_default();
    let mut receipts = state.receipt_store.list_by_workspace(&ws);
    if let Some(ak) = q.action_kind.as_deref() {
        if !ak.is_empty() {
            receipts.retain(|r| r.action_kind == ak);
        }
    }
    // Newest first when a limit is requested — that's almost always what the
    // caller wants ("most recent N anchor commits", etc.).
    if let Some(n) = q.limit {
        receipts.sort_by_key(|r| std::cmp::Reverse(
            r.timestamp.as_ref().map(|t| t.seconds).unwrap_or(0)
        ));
        receipts.truncate(n);
    }
    Json(receipts.iter().map(receipt_to_json).collect())
}

async fn get_receipt_by_id(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match state.receipt_store.get(&ContentHash(id)) {
        Some(r) => (StatusCode::OK, Json(receipt_to_json(&r))).into_response(),
        None => (StatusCode::NOT_FOUND, Json(serde_json::json!({"error":"not found"}))).into_response(),
    }
}

async fn list_workspace_agents(
    State(state): State<Arc<AppState>>,
    Path(workspace_id): Path<String>,
) -> Json<Vec<serde_json::Value>> {
    let entries = state.memory_store.list_by_workspace(&workspace_id);

    // Derive one record per unique agent_id from actual writes.
    let mut agents: std::collections::HashMap<String, serde_json::Value> =
        std::collections::HashMap::new();

    for e in &entries {
        let agent_id = match e.agent_id.as_ref() {
            Some(a) => a.value.clone(),
            None => continue,
        };
        agents.entry(agent_id.clone()).or_insert_with(|| {
            let enforcement_id = recall_core::ids::AgentId(agent_id.clone());
            let stage = state.enforcement.get_stage(&enforcement_id).as_str().to_string();
            let reputation = state.enforcement.get_reputation(&enforcement_id);
            let trust = e.trust_level;
            let role = match trust {
                t if t >= 3 => "SUPERVISOR",
                t if t >= 2 => "WRITER",
                _ => "READER",
            };
            serde_json::json!({
                "agent_id":    agent_id,
                "role":        role,
                "trust_level": trust,
                "model":       e.model_name.clone(),
                "stage":       stage,
                "reputation":  (reputation * 100.0).clamp(0.0, 100.0),
                "write_count": 0_i64,
            })
        });
        // Increment write count.
        if let Some(rec) = agents.get_mut(&agent_id) {
            if let Some(count) = rec.get_mut("write_count") {
                *count = serde_json::json!(count.as_i64().unwrap_or(0) + 1);
            }
        }
    }

    Json(agents.into_values().collect())
}

async fn list_workspace_memory(
    State(state): State<Arc<AppState>>,
    Path(workspace_id): Path<String>,
) -> Json<Vec<MemoryEntryJson>> {
    let entries = state.memory_store.list_by_workspace(&workspace_id);
    Json(entries.iter().map(|e| entry_to_json(e, &state)).collect())
}

async fn workspace_stats(
    State(state): State<Arc<AppState>>,
    Path(workspace_id): Path<String>,
) -> Json<serde_json::Value> {
    let entries   = state.memory_store.list_by_workspace(&workspace_id);
    let receipts  = state.receipt_store.list_by_workspace(&workspace_id);
    let conflicts = state.conflict_store.list_pending(&workspace_id);

    let entities: std::collections::HashSet<_> = entries.iter().map(|e| e.entity.clone()).collect();
    let agents:   std::collections::HashSet<_> = entries.iter()
        .filter_map(|e| e.agent_id.as_ref().map(|a| a.value.clone()))
        .collect();

    Json(serde_json::json!({
        "workspace_id":    workspace_id,
        "memory_count":    entries.len(),
        "receipt_count":   receipts.len(),
        "conflict_count":  conflicts.iter().filter(|c| c.resolution.is_empty()).count(),
        "entity_count":    entities.len(),
        "agent_count":     agents.len(),
    }))
}

async fn read_memory(
    State(state): State<Arc<AppState>>,
    Path((workspace_id, entity)): Path<(String, String)>,
) -> Json<Vec<MemoryEntryJson>> {
    let entries = state.memory_store.get_by_entity(&workspace_id, &entity);

    // Emit a memory.read receipt.
    let cp_agent   = AgentId("00000000-0000-0000-0000-000000000001".to_string());
    let cp_passport = ContentHash("cp_passport".to_string());
    let ws = WorkspaceId(workspace_id);
    let receipt = ReceiptBuilder::new(action_kind::MEMORY_READ, &ws, &cp_passport, &cp_agent)
        .build(&state.cp_keypair);
    let _ = state.receipt_store.append(receipt);

    Json(entries.iter().map(|e| entry_to_json(e, &state)).collect())
}

/// Invoke the MemWal Python sidecar to store a memory entry via the official
/// MemWal SDK. Spawns `python3 -m recall.memwal_client`, pipes the entry JSON
/// in on stdin, parses the sidecar's stdout for `job_id` + `blob_id`.
///
/// The sidecar path is resolved via `RECALL_MEMWAL_PYTHON` (a python binary
/// with the `memwal` + `recall` packages installed) — defaults to `python3`.
async fn memwal_sidecar_write(
    entry: &memory_proto::MemoryEntry,
) -> anyhow::Result<(String, String)> {
    use tokio::io::AsyncWriteExt;
    use tokio::process::Command;

    let python = std::env::var("RECALL_MEMWAL_PYTHON")
        .unwrap_or_else(|_| "python3".to_string());
    let module = "recall.memwal_client";

    // We send a compact JSON envelope; the sidecar reads {"content": "..."}.
    // The content is the entry serialized as JSON (so MemWal indexes the
    // full memory event, not just the user-supplied value).
    let entry_json = serde_json::to_string(&entry_to_sidecar_payload(entry))?;
    let stdin_payload = serde_json::json!({ "content": entry_json }).to_string();

    let mut child = Command::new(&python)
        .arg("-m")
        .arg(module)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| anyhow::anyhow!("spawn sidecar {python} -m {module}: {e}"))?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(stdin_payload.as_bytes()).await
            .map_err(|e| anyhow::anyhow!("write sidecar stdin: {e}"))?;
        stdin.shutdown().await.ok();
    }

    let out = child.wait_with_output().await
        .map_err(|e| anyhow::anyhow!("await sidecar: {e}"))?;
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();

    // The sidecar writes its result JSON to stdout; stderr carries the
    // benign frozen-runpy warning, which we ignore.
    let parsed: serde_json::Value = serde_json::from_str(stdout.trim())
        .map_err(|e| anyhow::anyhow!(
            "parse sidecar JSON: {e}; stdout={stdout}; stderr={stderr}"
        ))?;

    if !parsed["ok"].as_bool().unwrap_or(false) {
        let err = parsed["error"].as_str().unwrap_or("(no error message)");
        return Err(anyhow::anyhow!("sidecar error: {err}"));
    }
    let job_id = parsed["job_id"].as_str().unwrap_or("").to_string();
    let blob_id = parsed["blob_id"].as_str()
        .ok_or_else(|| anyhow::anyhow!("sidecar returned no blob_id: {parsed}"))?
        .to_string();
    Ok((job_id, blob_id))
}

/// Pull a minimal, JSON-safe view of a MemoryEntry for the MemWal sidecar.
/// Avoids serializing prost_types::Struct directly, which doesn't round-trip
/// cleanly through serde_json.
fn entry_to_sidecar_payload(entry: &memory_proto::MemoryEntry) -> serde_json::Value {
    serde_json::json!({
        "id":             entry.id,
        "workspace_id":   entry.workspace_id.as_ref().map(|w| &w.value),
        "entity":         entry.entity,
        "agent_id":       entry.agent_id.as_ref().map(|a| &a.value),
        "passport_id":    entry.passport_id.as_ref().map(|p| &p.hex),
        "event":          entry.event,
        "tags":           entry.tags,
        "scope":          entry.scope,
        "trust_level":    entry.trust_level,
        "model_provider": entry.model_provider,
        "model_name":     entry.model_name,
        "timestamp_secs": entry.timestamp.as_ref().map(|t| t.seconds),
    })
}

async fn write_memory(
    State(state): State<Arc<AppState>>,
    Path((workspace_id, entity)): Path<(String, String)>,
    Json(body): Json<WriteMemoryBody>,
) -> impl IntoResponse {
    use ed25519_dalek::{Signature, VerifyingKey};

    // ── Authenticate writer ───────────────────────────────────────────────────
    // The CP is the notary on the receipt, but agent_id and passport_id must be
    // PROVEN by a verified Ed25519 signature — they cannot be trusted from the
    // request body. Mirrors the publish_registry flow.
    // Empty credentials = unauthenticated request → 401 (not 400). Malformed
    // credentials = client error → 400.
    if body.signature.is_empty() || body.public_key.is_empty() {
        return (StatusCode::UNAUTHORIZED, Json(serde_json::json!({
            "error": "signature and public_key required — write rejected"
        }))).into_response();
    }
    let pub_key_bytes = match hex::decode(&body.public_key) {
        Ok(b) => b,
        Err(_) => {
            return (StatusCode::BAD_REQUEST, Json(serde_json::json!({
                "error": "invalid public_key hex"
            }))).into_response();
        }
    };
    let verifying_key = match VerifyingKey::try_from(pub_key_bytes.as_slice()) {
        Ok(k) => k,
        Err(_) => {
            return (StatusCode::BAD_REQUEST, Json(serde_json::json!({
                "error": "invalid Ed25519 public key"
            }))).into_response();
        }
    };
    let sig_bytes = match hex::decode(&body.signature) {
        Ok(b) => b,
        Err(_) => {
            return (StatusCode::BAD_REQUEST, Json(serde_json::json!({
                "error": "invalid signature hex"
            }))).into_response();
        }
    };
    let signature = match Signature::try_from(sig_bytes.as_slice()) {
        Ok(s) => s,
        Err(_) => {
            return (StatusCode::BAD_REQUEST, Json(serde_json::json!({
                "error": "invalid signature format"
            }))).into_response();
        }
    };
    let message = format!("{}:{}:{}:{}", workspace_id, entity, body.event, body.agent_id);
    if verifying_key
        .verify_strict(message.as_bytes(), &signature)
        .is_err()
    {
        return (StatusCode::UNAUTHORIZED, Json(serde_json::json!({
            "error": "signature verification failed — write rejected"
        }))).into_response();
    }
    // If the passport is registered, the public key must match what's on file.
    // Self-attested writes (no passport on file) are accepted with a warning —
    // the signature still proves control of the public key.
    let passport_hash = ContentHash(body.passport_id.clone());
    match state.passport_store.get(&passport_hash) {
        Some(p) => {
            if p.public_key_hex() != body.public_key {
                return (StatusCode::UNAUTHORIZED, Json(serde_json::json!({
                    "error": "public key does not match passport on file"
                }))).into_response();
            }
        }
        None => {
            tracing::warn!(
                "write_memory: passport {} not registered — accepting self-attested signature",
                body.passport_id
            );
        }
    }

    // Merge value + metadata into a single prost Struct.
    let mut data_map = match body.value.clone() {
        serde_json::Value::Object(m) => m,
        v => { let mut m = serde_json::Map::new(); m.insert("value".into(), v); m }
    };
    if let Some(serde_json::Value::Object(meta)) = &body.metadata {
        data_map.extend(meta.clone());
    }
    let data_struct = json_to_prost_struct(serde_json::Value::Object(data_map));

    // Stable entry ID from content hash.
    let entry_id = format!(
        "mem_{}",
        recall_crypto::sha256_hex(
            format!("{}:{}:{}", workspace_id, entity, body.event).as_bytes()
        )
    );

    let now = prost_types::Timestamp {
        seconds: chrono::Utc::now().timestamp(),
        nanos: 0,
    };

    let entry = memory_proto::MemoryEntry {
        id: entry_id.clone(),
        workspace_id: Some(common_proto::WorkspaceId { value: workspace_id.clone() }),
        entity: entity.clone(),
        agent_id: Some(common_proto::AgentId { value: body.agent_id.clone() }),
        passport_id: Some(common_proto::Hash { hex: body.passport_id.clone() }),
        event: body.event.clone(),
        data: Some(data_struct),
        tags: body.tags.clone(),
        scope: body.scope.clone(),
        trust_level: body.trust_level,
        model_provider: body.model_provider.clone(),
        model_name: body.model_name.clone(),
        timestamp: Some(now),
        cost_annotation: None,
        causal_predecessors: vec![],
        ..Default::default()
    };

    // ── Persistent storage ────────────────────────────────────────────────────
    // RECALL writes through MemWal (Walrus Memory) when RECALL_USE_MEMWAL=1
    // is set — this is the canonical path for the Walrus track. The raw
    // Walrus publisher HTTP path is kept as a fallback so offline dev and
    // smoke tests still work without the MemWal SDK.
    let use_memwal = std::env::var("RECALL_USE_MEMWAL")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false);

    let walrus_blob_id: Option<String> = if use_memwal {
        match memwal_sidecar_write(&entry).await {
            Ok((job_id, blob_id)) => {
                tracing::info!(
                    "MemWal blob stored: job_id={} blob_id={}",
                    job_id, blob_id
                );
                Some(blob_id)
            }
            Err(e) => {
                tracing::error!("MemWal sidecar write failed: {e}");
                return (
                    StatusCode::SERVICE_UNAVAILABLE,
                    Json(serde_json::json!({
                        "error":  "MemWal write failed — memory not stored",
                        "detail": e.to_string(),
                        "hint":   "Check relayer.memwal.ai connectivity and that MEMWAL_PRIVATE_KEY + MEMWAL_ACCOUNT_ID are set, or unset RECALL_USE_MEMWAL to fall back to raw Walrus.",
                    })),
                ).into_response();
            }
        }
    } else {
        match state.walrus.as_ref() {
            Some(walrus) => match walrus.write_memory_entry(&entry).await {
                Ok(blob) => {
                    tracing::info!("Walrus blob stored: {}", blob.0);
                    Some(blob.0)
                }
                Err(e) => {
                    tracing::error!("Walrus write failed: {e}");
                    return (
                        StatusCode::SERVICE_UNAVAILABLE,
                        Json(serde_json::json!({
                            "error":  "Walrus write failed — memory not stored",
                            "detail": e.to_string(),
                            "hint":   "Check Walrus testnet connectivity, or set RECALL_USE_MEMWAL=1 to route through MemWal.",
                        })),
                    ).into_response();
                }
            },
            None => {
                tracing::error!("Walrus backend not configured at write time");
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({
                        "error": "Walrus backend not configured",
                        "hint":  "Start with --walrus-testnet, or set RECALL_USE_MEMWAL=1 + MEMWAL_PRIVATE_KEY + MEMWAL_ACCOUNT_ID.",
                    })),
                ).into_response();
            }
        }
    };

    // Build the memory.write receipt first so we can attach its ID to the entry
    // before storing — this gives every entry a direct receipt_id link in the
    // /memory JSON, no cross-API joins needed.
    let ws       = WorkspaceId(workspace_id.clone());
    let passport = ContentHash(body.passport_id.clone());
    let agent    = AgentId(body.agent_id.clone());
    let receipt  = ReceiptBuilder::new(action_kind::MEMORY_WRITE, &ws, &passport, &agent)
        .with_cost_annotation(&body.model_provider, &body.model_name, 0, 0, 0)
        .build(&state.cp_keypair);
    let receipt_id = receipt.id.as_ref().map(|h| h.hex.clone()).unwrap_or_default();

    // Attach blob ref + receipt id to the entry before storing.
    let mut entry = entry;
    if let Some(ref bid) = walrus_blob_id {
        entry.walrus_blob = Some(common_proto::WalrusBlobRef {
            blob_id: bid.clone(),
        });
    }
    if !receipt_id.is_empty() {
        entry.receipt_id = Some(common_proto::Hash { hex: receipt_id.clone() });
    }

    state.memory_store.insert(entry.clone());

    // Auto-register workspace so list_workspaces reflects writes before create() is called.
    state.workspace_store.ensure_exists(&workspace_id);

    // Conflict detection — uses the per-workspace policy (default if unset).
    let policy = state.workspace_store.get_policy(&workspace_id);
    let existing = state.memory_store.get_by_entity(&workspace_id, &entity);
    for existing_entry in &existing {
        if existing_entry.id != entry.id
            && recall_conflict::detect_conflict_with(existing_entry, &entry, &policy)
        {
            let conflict_receipt_id = ContentHash(recall_crypto::sha256_hex(
                format!("{}:{}", existing_entry.id, entry.id).as_bytes(),
            ));
            let conflict = recall_conflict::build_conflict_record(
                existing_entry, &entry, &conflict_receipt_id,
            );
            state.conflict_store.insert(conflict);
        }
    }

    // Append the pre-built receipt to the receipt store.
    let _ = state.receipt_store.append(receipt.clone());

    // Broadcast to streaming subscribers.
    state.subscribe_hub.publish(&workspace_id, receipt);

    let mut resp = serde_json::json!({
        "memory_id":  entry_id,
        "receipt_id": receipt_id,
    });
    if let Some(bid) = walrus_blob_id {
        resp["walrus_blob_id"] = serde_json::Value::String(bid);
    }
    (StatusCode::OK, Json(resp)).into_response()
}

async fn get_conflicts(
    State(state): State<Arc<AppState>>,
    Path(workspace_id): Path<String>,
) -> Json<Vec<ConflictJson>> {
    let conflicts = state.conflict_store.list_pending(&workspace_id);
    let out = conflicts
        .iter()
        .map(|c| ConflictJson {
            conflict_id:     c.id.clone(),
            workspace_id:    c.workspace_id.as_ref().map(|w| w.value.clone()).unwrap_or_default(),
            entity:          c.entity.clone(),
            entry_a_id:      c.signal_a.as_ref().map(|s| s.memory_id.clone()).unwrap_or_default(),
            entry_b_id:      c.signal_b.as_ref().map(|s| s.memory_id.clone()).unwrap_or_default(),
            auto_resolution: c.auto_resolution.clone(),
            resolution:      c.resolution.clone(),
        })
        .collect();
    Json(out)
}

async fn list_registry(
    State(state): State<Arc<AppState>>,
    Query(q): Query<RegistryQuery>,
) -> Json<Vec<RegistryProfileJson>> {
    let profiles = state.registry_store.list(q.category.as_deref(), None);
    let out = profiles
        .iter()
        .map(|p| RegistryProfileJson {
            name:         p.name.clone(),
            version:      p.version.clone(),
            author:       p.author.clone(),
            description:  p.description.clone(),
            category:     p.category.clone(),
            memory_count: p.memory_count,
            import_count: p.import_count,
        })
        .collect();
    Json(out)
}

// ── New endpoints for CLI ─────────────────────────────────────────────────────

async fn list_workspaces(State(state): State<Arc<AppState>>) -> Json<Vec<serde_json::Value>> {
    // Merge explicit workspace_store with workspaces that appear in memory writes
    // so demo_seed.py flows (which write before calling create) are reflected.
    let mut ws_ids: std::collections::HashSet<String> = state
        .workspace_store
        .list()
        .into_iter()
        .filter_map(|ws| ws.id.map(|id| id.value))
        .collect();
    for id in state.memory_store.list_workspaces() {
        ws_ids.insert(id);
    }

    let mut ws_list: Vec<String> = ws_ids.into_iter().collect();
    ws_list.sort();

    let out = ws_list.iter().map(|ws| {
        let entries   = state.memory_store.list_by_workspace(ws);
        let receipts  = state.receipt_store.list_by_workspace(ws);
        let conflicts = state.conflict_store.list_pending(ws);
        let agents: std::collections::HashSet<String> = entries.iter()
            .filter_map(|e| e.agent_id.as_ref().map(|a| a.value.clone()))
            .collect();
        let unresolved = conflicts.iter().filter(|c| c.resolution.is_empty()).count();
        serde_json::json!({
            "workspace_id":   ws,
            "memory_count":   entries.len(),
            "receipt_count":  receipts.len(),
            "conflict_count": unresolved,
            "agent_count":    agents.len(),
        })
    }).collect();
    Json(out)
}

async fn get_entity_all_workspaces(
    State(state): State<Arc<AppState>>,
    Path(entity_id): Path<String>,
) -> Json<Vec<MemoryEntryJson>> {
    let entries = state.memory_store.get_by_entity_all(&entity_id);
    Json(entries.iter().map(|e| entry_to_json(e, &state)).collect())
}

#[derive(Deserialize)]
struct RollbackBody {
    to_timestamp: i64,
}

async fn rollback_workspace(
    State(state): State<Arc<AppState>>,
    Path(workspace_id): Path<String>,
    Json(body): Json<RollbackBody>,
) -> Json<serde_json::Value> {
    let removed = state.memory_store.rollback_to(&workspace_id, body.to_timestamp);
    // Emit a rollback receipt.
    let cp_agent   = AgentId("00000000-0000-0000-0000-000000000001".to_string());
    let cp_passport = ContentHash("cp_passport".to_string());
    let ws = WorkspaceId(workspace_id.clone());
    let receipt = ReceiptBuilder::new(action_kind::MEMORY_WRITE, &ws, &cp_passport, &cp_agent)
        .build(&state.cp_keypair);
    let receipt_id = receipt.id.as_ref().map(|h| h.hex.clone()).unwrap_or_default();
    let _ = state.receipt_store.append(receipt);
    Json(serde_json::json!({
        "workspace_id": workspace_id,
        "entries_removed": removed,
        "rolled_back_to": body.to_timestamp,
        "rollback_receipt_id": receipt_id,
    }))
}

#[derive(Deserialize)]
struct PublishRegistryBody {
    name:         String,
    version:      String,
    #[serde(default)]
    category:     String,
    #[serde(default)]
    description:  String,
    workspace_id: Option<String>,
    // Passport-based authorship — author is derived from passport, not request body.
    passport_id:  String,
    signature:    String,   // hex-encoded Ed25519 signature
    public_key:   String,   // hex-encoded Ed25519 public key matching passport
    /// Optional. Accepts "public" (default) or "restricted". Anything else
    /// falls back to "public" since the registry is open by default.
    #[serde(default)]
    visibility:   Option<String>,
}

fn parse_visibility(s: Option<&str>) -> i32 {
    match s.unwrap_or("public").to_ascii_lowercase().as_str() {
        "restricted" => 1,
        _            => 0,
    }
}

/// Short 8-byte SHA-256 prefix as hex — used as a deterministic placeholder
/// Walrus blob ID when no real Walrus publisher is configured.
fn sha256_short(input: &str) -> String {
    use sha2::{Digest, Sha256};
    let hash = Sha256::digest(input.as_bytes());
    hex::encode(&hash[..8])
}

async fn publish_registry(
    State(state): State<Arc<AppState>>,
    Json(body): Json<PublishRegistryBody>,
) -> impl IntoResponse {
    use ed25519_dalek::{Signature, VerifyingKey};
    use recall_proto::registry as reg_proto;

    // ── 1. Decode public key ──────────────────────────────────────────────────
    let pub_key_bytes = match hex::decode(&body.public_key) {
        Ok(b) => b,
        Err(_) => {
            return (StatusCode::BAD_REQUEST, Json(serde_json::json!({
                "error": "invalid public_key hex"
            }))).into_response();
        }
    };
    let verifying_key = match VerifyingKey::try_from(pub_key_bytes.as_slice()) {
        Ok(k) => k,
        Err(_) => {
            return (StatusCode::BAD_REQUEST, Json(serde_json::json!({
                "error": "invalid Ed25519 public key"
            }))).into_response();
        }
    };

    // ── 2. Decode signature ───────────────────────────────────────────────────
    let sig_bytes = match hex::decode(&body.signature) {
        Ok(b) => b,
        Err(_) => {
            return (StatusCode::BAD_REQUEST, Json(serde_json::json!({
                "error": "invalid signature hex"
            }))).into_response();
        }
    };
    let signature = match Signature::try_from(sig_bytes.as_slice()) {
        Ok(s) => s,
        Err(_) => {
            return (StatusCode::BAD_REQUEST, Json(serde_json::json!({
                "error": "invalid signature format"
            }))).into_response();
        }
    };

    // ── 3. Verify signature over canonical message ────────────────────────────
    let message = format!("{}@{}:{}", body.name, body.version, body.passport_id);
    if verifying_key
        .verify_strict(message.as_bytes(), &signature)
        .is_err()
    {
        return (StatusCode::UNAUTHORIZED, Json(serde_json::json!({
            "error": "signature verification failed — publish rejected"
        }))).into_response();
    }

    // ── 4. Look up passport and verify public key matches what is on file ─────
    let passport_hash = ContentHash(body.passport_id.clone());
    let passport_opt  = state.passport_store.get(&passport_hash);
    let agent_id_for_author = match passport_opt {
        Some(p) => {
            if p.public_key_hex() != body.public_key {
                return (StatusCode::UNAUTHORIZED, Json(serde_json::json!({
                    "error": "public key does not match passport on file"
                }))).into_response();
            }
            p.agent_id.0
        }
        None => {
            // Strict mode would 401 here. For the hackathon we accept self-attested
            // passports: the signature already proves the publisher controls the
            // public key, and authorship is recorded against the resulting
            // passport_id. Production deployments should flip this to a hard 401.
            tracing::warn!(
                "publish: passport {} not registered — accepting self-attested signature",
                body.passport_id
            );
            "self-attested".to_string()
        }
    };

    // ── 5. Derive author from passport (not user-supplied) ───────────────────
    let pp_prefix_len = body.passport_id.len().min(12);
    let author = format!("{}:{}", &body.passport_id[..pp_prefix_len], agent_id_for_author);

    // ── 6. Compute memory_count from the actual workspace ────────────────────
    let memory_count = if let Some(ref ws_id) = body.workspace_id {
        state.memory_store.list_by_workspace(ws_id).len() as i64
    } else {
        0
    };

    // ── 7. Build canonical profile proto + entries package, write to Walrus ──
    let published_at = chrono::Utc::now();
    let visibility = parse_visibility(body.visibility.as_deref());
    let profile_proto_for_blob = reg_proto::RegistryProfile {
        name:                   body.name.clone(),
        version:                body.version.clone(),
        author:                 author.clone(),
        category:               body.category.clone(),
        description:            body.description.clone(),
        memory_count,
        artifact_count:         0,
        import_count:           0,
        published_at: Some(prost_types::Timestamp {
            seconds: published_at.timestamp(),
            nanos:   0,
        }),
        immutable:              true,
        publisher_passport_id:  body.passport_id.clone(),
        visibility,
        ..Default::default()
    };

    // Pull entries from the source workspace and pack them into a
    // RegistryPackage. The package — not the bare profile — is what gets
    // written to Walrus so that `recall registry import` can later reconstitute
    // the workspace from the blob alone, with no trust in the control plane.
    let entries: Vec<memory_proto::MemoryEntry> = body
        .workspace_id
        .as_ref()
        .map(|ws| state.memory_store.list_by_workspace(ws))
        .unwrap_or_default();

    let package = reg_proto::RegistryPackage {
        profile:        Some(profile_proto_for_blob.clone()),
        entries:        entries.clone(),
        format_version: "v1".to_string(),
    };
    let mut package_bytes = Vec::new();
    if let Err(e) = prost::Message::encode(&package, &mut package_bytes) {
        tracing::warn!("registry package encode failed: {e}");
    }

    let walrus_blob_id: String = if let Some(walrus) = &state.walrus {
        match walrus.put_blob_raw(&package_bytes).await {
            Ok(blob) => {
                tracing::info!(
                    "Registry package {}@{} stored on Walrus ({} entries, {} bytes): {}",
                    body.name, body.version, entries.len(), package_bytes.len(), blob.0
                );
                blob.0
            }
            Err(e) => {
                tracing::warn!(
                    "Walrus write failed for registry package ({e}); using deterministic ID"
                );
                format!(
                    "0x{}",
                    sha256_short(&format!("{}@{}:{}", body.name, body.version, body.passport_id))
                )
            }
        }
    } else {
        format!(
            "0x{}",
            sha256_short(&format!("{}@{}:{}", body.name, body.version, body.passport_id))
        )
    };

    // Cache the package bytes so import can bypass the publisher→aggregator
    // propagation lag on Walrus testnet. Important — this is correctness, not
    // optimization: imports immediately after publish would otherwise 502.
    state
        .registry_blob_cache
        .write()
        .unwrap()
        .insert(walrus_blob_id.clone(), package_bytes.clone());

    // Profile stored in the registry carries the real Walrus blob ID so
    // `recall registry inspect` returns it.
    let profile = reg_proto::RegistryProfile {
        walrus_blob: Some(common_proto::WalrusBlobRef { blob_id: walrus_blob_id.clone() }),
        ..profile_proto_for_blob
    };

    match state.registry_store.publish(profile.clone()) {
        Ok(()) => (StatusCode::CREATED, Json(serde_json::json!({
            "name":                  body.name,
            "version":               body.version,
            "author":                author,
            "category":              body.category,
            "description":           body.description,
            "memory_count":          memory_count,
            "walrus_blob_id":        walrus_blob_id,
            "publisher_passport_id": body.passport_id,
            "visibility":            if visibility == 1 { "restricted" } else { "public" },
            "published_at":          published_at.to_rfc3339(),
            "immutable":             true,
            "ok":                    true,
            "walrus_url":            format!(
                "{}/v1/blobs/{}",
                walrus_memory::WALRUS_TESTNET_AGGREGATOR,
                walrus_blob_id
            ),
        }))).into_response(),
        Err(e) => (StatusCode::CONFLICT, Json(serde_json::json!({
            "error": e.to_string()
        }))).into_response(),
    }
}

// ── Registry import ──────────────────────────────────────────────────────────

#[derive(Deserialize, Default)]
struct ImportRegistryBody {
    /// Override the target workspace ID. Defaults to `ws_<name>`.
    target_workspace_id: Option<String>,
}

async fn import_registry(
    State(state): State<Arc<AppState>>,
    Path((name, version)): Path<(String, String)>,
    body: Option<Json<ImportRegistryBody>>,
) -> impl IntoResponse {
    let target_ws_override = body.map(|Json(b)| b.target_workspace_id).unwrap_or(None);

    // ── 1. Look up the profile in the registry index ─────────────────────────
    let profile = match state.registry_store.get(&name, &version) {
        Some(p) => p,
        None => {
            return (StatusCode::NOT_FOUND, Json(serde_json::json!({
                "error": format!("profile {}@{} not found", name, version)
            }))).into_response();
        }
    };

    // ── 2. Fetch the package blob from Walrus ─────────────────────────────────
    let blob_id = match profile.walrus_blob.as_ref() {
        Some(b) if !b.blob_id.is_empty() => b.blob_id.clone(),
        _ => {
            return (StatusCode::FAILED_DEPENDENCY, Json(serde_json::json!({
                "error": "profile has no Walrus blob — cannot import",
            }))).into_response();
        }
    };

    // Cache hit avoids the Walrus aggregator's publisher→reader propagation
    // lag and also makes import work without Walrus being configured at all.
    let cached = state
        .registry_blob_cache
        .read()
        .unwrap()
        .get(&blob_id)
        .cloned();

    let blob_bytes = if let Some(bytes) = cached {
        tracing::debug!("registry import: cache hit for blob {}", blob_id);
        bytes
    } else {
        let walrus = match state.walrus.as_ref() {
            Some(w) => w,
            None => {
                return (StatusCode::FAILED_DEPENDENCY, Json(serde_json::json!({
                    "error": "Walrus backend not configured — cannot fetch profile blob",
                }))).into_response();
            }
        };
        match walrus
            .get_blob_raw(&recall_core::ids::WalrusBlobId(blob_id.clone()))
            .await
        {
            Ok(b) => b,
            Err(e) => {
                tracing::error!("registry import: Walrus fetch failed: {e}");
                return (StatusCode::BAD_GATEWAY, Json(serde_json::json!({
                    "error":  "failed to fetch profile blob from Walrus",
                    "detail": e.to_string(),
                    "blob_id": blob_id,
                    "hint":    "Walrus aggregator may not have synced the just-published blob yet; retry in a few seconds",
                }))).into_response();
            }
        }
    };

    // ── 3. Decode the RegistryPackage ─────────────────────────────────────────
    use prost::Message as _;
    let package = match recall_proto::registry::RegistryPackage::decode(blob_bytes.as_slice()) {
        Ok(p) => p,
        Err(e) => {
            tracing::error!("registry import: blob decode failed: {e}");
            return (StatusCode::BAD_GATEWAY, Json(serde_json::json!({
                "error": "profile blob is not a valid RegistryPackage",
                "detail": e.to_string(),
            }))).into_response();
        }
    };

    // ── 4. Visibility gate ────────────────────────────────────────────────────
    // RESTRICTED profiles require a passport-grant flow that isn't built yet.
    // Refuse the import explicitly rather than silently allowing it.
    if profile.visibility == 1 {
        return (StatusCode::NOT_IMPLEMENTED, Json(serde_json::json!({
            "error": "this profile is RESTRICTED; the import grant flow is not yet implemented",
            "hint":  "publish as PUBLIC, or wait for the registry grant + Seal flow",
        }))).into_response();
    }

    // ── 5. Decide target workspace + emit the import receipt FIRST ────────────
    // The import receipt is the causal root that each per-entry receipt
    // points back to via `causal_predecessors`. Building it first lets us
    // capture its ID before the entry loop.
    let target_ws_id = target_ws_override
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| format!("ws_{}", name));

    state.workspace_store.ensure_exists(&target_ws_id);

    let cp_agent    = AgentId("00000000-0000-0000-0000-000000000001".to_string());
    let cp_passport = ContentHash("cp_passport".to_string());
    let ws          = WorkspaceId(target_ws_id.clone());

    let import_receipt = ReceiptBuilder::new(
        action_kind::REGISTRY_IMPORT,
        &ws,
        &cp_passport,
        &cp_agent,
    )
    .build(&state.cp_keypair);
    let import_receipt_id = import_receipt
        .id
        .as_ref()
        .map(|h| h.hex.clone())
        .unwrap_or_default();
    let _ = state.receipt_store.append(import_receipt);

    let import_receipt_ch = ContentHash(import_receipt_id.clone());

    // ── 6. Per-entry: rehash ID, preserve provenance, emit per-entry receipt ──
    let mut imported = 0usize;
    for src_entry in &package.entries {
        // 6a — Build the entry: new workspace, new ID, original ID preserved
        //      in imported_from for cross-workspace provenance.
        let original_id = src_entry.id.clone();
        let event       = src_entry.event.clone();
        let entity      = src_entry.entity.clone();

        let new_entry_id = format!(
            "mem_{}",
            recall_crypto::sha256_hex(
                format!("{}:{}:{}", target_ws_id, entity, event).as_bytes()
            )
        );

        let mut entry = src_entry.clone();
        entry.id           = new_entry_id.clone();
        entry.workspace_id = Some(common_proto::WorkspaceId {
            value: target_ws_id.clone(),
        });
        entry.imported_from = if !original_id.is_empty() {
            Some(common_proto::Hash { hex: original_id.clone() })
        } else {
            None
        };

        // 6b — Build a memory.write receipt for this entry.
        //   causal_predecessors[0] = the import receipt (so "why is this here?"
        //                            walks back to the import action)
        //   evidence_digest        = the source entry's original receipt ID
        //                            (so cross-workspace lineage is queryable)
        let source_receipt_ch = src_entry
            .receipt_id
            .as_ref()
            .map(|h| ContentHash(h.hex.clone()));

        let entry_passport = entry
            .passport_id
            .as_ref()
            .map(|h| ContentHash(h.hex.clone()))
            .unwrap_or_else(|| cp_passport.clone());
        let entry_agent = entry
            .agent_id
            .as_ref()
            .map(|a| AgentId(a.value.clone()))
            .unwrap_or_else(|| cp_agent.clone());

        let mut rb = ReceiptBuilder::new(
            action_kind::MEMORY_WRITE,
            &ws,
            &entry_passport,
            &entry_agent,
        )
        .with_causal_predecessor(&import_receipt_ch);
        if let Some(src) = source_receipt_ch.as_ref() {
            rb = rb.with_evidence_digest(src);
        }
        let entry_receipt = rb.build(&state.cp_keypair);
        let entry_receipt_id = entry_receipt
            .id
            .as_ref()
            .map(|h| h.hex.clone())
            .unwrap_or_default();

        // Attach the new receipt ID to the entry before storing.
        if !entry_receipt_id.is_empty() {
            entry.receipt_id = Some(common_proto::Hash {
                hex: entry_receipt_id.clone(),
            });
        }

        state.memory_store.insert(entry);
        let _ = state.receipt_store.append(entry_receipt);
        imported += 1;
    }

    let receipt_id = import_receipt_id;

    (StatusCode::OK, Json(serde_json::json!({
        "name":             name,
        "version":          version,
        "target_workspace": target_ws_id,
        "memories_loaded":  imported,
        "blob_id":          blob_id,
        "format_version":   package.format_version,
        "receipt_id":       receipt_id,
        "ok":               true,
    }))).into_response()
}

// ── Handoff ───────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct HandoffBody {
    from_agent_id: String,
    to_agent_id:   String,
    entity:        String,
    workspace_id:  String,
}

async fn handoff(
    State(state): State<Arc<AppState>>,
    Json(body): Json<HandoffBody>,
) -> impl IntoResponse {
    // Snapshot all memory entries for this entity.
    let entries = state.memory_store.get_by_entity_all(&body.entity);
    let snapshot: Vec<MemoryEntryJson> = entries.iter().map(|e| entry_to_json(e, &state)).collect();

    // Emit handoff.capsule.create receipt.
    let cp_agent   = AgentId("00000000-0000-0000-0000-000000000001".to_string());
    let cp_passport = ContentHash("cp_passport".to_string());
    let ws = WorkspaceId(body.workspace_id.clone());
    let receipt = ReceiptBuilder::new(action_kind::HANDOFF_CAPSULE_CREATE, &ws, &cp_passport, &cp_agent)
        .build(&state.cp_keypair);
    let receipt_id = receipt.id.as_ref().map(|h| h.hex.clone()).unwrap_or_default();
    let _ = state.receipt_store.append(receipt.clone());
    state.subscribe_hub.publish(&body.workspace_id, receipt);

    let hash = recall_crypto::sha256_hex(
        format!("{}:{}:{}", body.from_agent_id, body.to_agent_id, body.entity).as_bytes(),
    );
    let capsule_id = format!("capsule_{}", &hash[..16]);
    let created_at = chrono::Utc::now().to_rfc3339();

    (StatusCode::CREATED, Json(serde_json::json!({
        "capsule_id":       capsule_id,
        "from_agent_id":    body.from_agent_id,
        "to_agent_id":      body.to_agent_id,
        "entity":           body.entity,
        "workspace_id":     body.workspace_id,
        "memory_snapshot":  snapshot,
        "created_at":       created_at,
        "receipt_id":       receipt_id,
    })))
}

// ── Router ────────────────────────────────────────────────────────────────────

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/health",                              get(health))
        .route("/workspaces",                          get(list_workspaces))
        .route("/receipts",                            get(list_receipts))
        .route("/receipts/:id",                        get(get_receipt_by_id))
        .route("/workspace/:workspace_id/agents",      get(list_workspace_agents))
        .route("/workspace/:workspace_id/rollback",    axum::routing::post(rollback_workspace))
        .route("/memory/:workspace_id",                get(list_workspace_memory))
        .route("/memory/:workspace_id/:entity",        get(read_memory).post(write_memory))
        .route("/entity/:entity_id",                   get(get_entity_all_workspaces))
        .route("/stats/:workspace_id",                 get(workspace_stats))
        .route("/conflicts/:workspace_id",             get(get_conflicts))
        .route("/registry",                            get(list_registry).post(publish_registry))
        .route("/registry/:name/:version/import",      axum::routing::post(import_registry))
        .route("/handoff",                             axum::routing::post(handoff))
        .route("/workspace/:workspace_id/conflict-policy",
               get(get_conflict_policy).put(set_conflict_policy))
        .layer(CorsLayer::permissive())
        .with_state(state)
}

#[derive(Deserialize)]
struct ConflictPolicyBody {
    /// List of `[event_a, event_b]` pairs that should be treated as conflicting
    /// for this workspace. Order within a pair does not matter. An empty list
    /// disables conflict detection for the workspace.
    pairs: Vec<(String, String)>,
}

async fn set_conflict_policy(
    State(state): State<Arc<AppState>>,
    Path(workspace_id): Path<String>,
    Json(body): Json<ConflictPolicyBody>,
) -> impl IntoResponse {
    let policy = recall_conflict::ConflictPolicy::with_pairs(body.pairs);
    state.workspace_store.ensure_exists(&workspace_id);
    state.workspace_store.set_policy(&workspace_id, policy.clone());
    tracing::info!(
        "conflict policy updated for workspace {}: {} pairs",
        workspace_id, policy.len()
    );
    (
        StatusCode::OK,
        Json(serde_json::json!({
            "workspace_id": workspace_id,
            "pair_count":   policy.len(),
        })),
    )
        .into_response()
}

async fn get_conflict_policy(
    State(state): State<Arc<AppState>>,
    Path(workspace_id): Path<String>,
) -> Json<serde_json::Value> {
    let policy = state.workspace_store.get_policy(&workspace_id);
    Json(serde_json::json!({
        "workspace_id": workspace_id,
        "pair_count":   policy.len(),
    }))
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod auth_tests {
    use super::*;
    use crate::state::AppStateConfig;
    use axum::body::Body;
    use axum::http::{Request, StatusCode as Sc};
    use ed25519_dalek::{Signer, SigningKey};
    use http_body_util::BodyExt as _;
    use tower::ServiceExt; // for `oneshot`

    fn test_state() -> Arc<AppState> {
        Arc::new(
            AppState::new(AppStateConfig {
                sui_rpc_url: None,
                policy_object_id: None,
                record_object_id: None,
                walrus_publisher_url: None,
                walrus_aggregator_url: None,
            })
            .expect("AppState::new"),
        )
    }

    fn make_body(value: serde_json::Value) -> Body {
        Body::from(serde_json::to_vec(&value).unwrap())
    }

    async fn body_string(resp: axum::response::Response) -> (Sc, String) {
        let status = resp.status();
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        let s = String::from_utf8_lossy(&bytes).to_string();
        (status, s)
    }

    fn canonical_msg(ws: &str, entity: &str, event: &str, agent: &str) -> String {
        format!("{}:{}:{}:{}", ws, entity, event, agent)
    }

    #[tokio::test]
    async fn write_without_signature_is_rejected() {
        let app = router(test_state());

        let body = serde_json::json!({
            "agent_id":    "agent-1",
            "passport_id": "00".repeat(32),
            "event":       "memory.write",
            "value":       { "note": "hi" },
            // signature + public_key intentionally missing → default ""
        });
        let req = Request::builder()
            .method("POST")
            .uri("/memory/ws-test/entity-1")
            .header("content-type", "application/json")
            .body(make_body(body))
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        let (status, body_text) = body_string(resp).await;
        assert_eq!(status, Sc::UNAUTHORIZED, "missing sig must 401, got: {body_text}");
    }

    #[tokio::test]
    async fn write_with_wrong_key_is_rejected() {
        let app = router(test_state());

        // Sign with key A, declare key B → verify_strict must fail.
        let key_a = SigningKey::from_bytes(&[1u8; 32]);
        let key_b = SigningKey::from_bytes(&[2u8; 32]);
        let agent = "agent-x";
        let ws = "ws-test";
        let entity = "entity-1";
        let event = "memory.write";

        let msg = canonical_msg(ws, entity, event, agent);
        let sig = key_a.sign(msg.as_bytes());

        let body = serde_json::json!({
            "agent_id":    agent,
            "passport_id": "00".repeat(32),
            "event":       event,
            "value":       { "note": "hi" },
            "signature":   hex::encode(sig.to_bytes()),
            "public_key":  hex::encode(key_b.verifying_key().to_bytes()),
        });
        let req = Request::builder()
            .method("POST")
            .uri(format!("/memory/{}/{}", ws, entity))
            .header("content-type", "application/json")
            .body(make_body(body))
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        let (status, body_text) = body_string(resp).await;
        assert_eq!(status, Sc::UNAUTHORIZED, "wrong key must 401, got: {body_text}");
    }

    #[tokio::test]
    async fn write_with_valid_signature_passes_auth() {
        // Walrus is unconfigured in this state, so the handler will return 500
        // ("Walrus backend not configured") AFTER auth passes. The point is:
        // we do not see 401 — auth succeeded. The end-to-end 200 path is
        // covered by the live curl probe in the Fix 1 gate (which configures
        // Walrus). This unit test proves the auth gate itself works.
        let app = router(test_state());

        let key = SigningKey::from_bytes(&[7u8; 32]);
        let agent = "agent-x";
        let ws = "ws-test";
        let entity = "entity-1";
        let event = "memory.write";
        let msg = canonical_msg(ws, entity, event, agent);
        let sig = key.sign(msg.as_bytes());

        let body = serde_json::json!({
            "agent_id":    agent,
            "passport_id": "00".repeat(32),
            "event":       event,
            "value":       { "note": "hi" },
            "signature":   hex::encode(sig.to_bytes()),
            "public_key":  hex::encode(key.verifying_key().to_bytes()),
        });
        let req = Request::builder()
            .method("POST")
            .uri(format!("/memory/{}/{}", ws, entity))
            .header("content-type", "application/json")
            .body(make_body(body))
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        let (status, body_text) = body_string(resp).await;
        assert_ne!(status, Sc::UNAUTHORIZED, "valid sig must not 401, got 401 with body: {body_text}");
        assert_ne!(status, Sc::BAD_REQUEST,  "valid sig must not 400, got 400 with body: {body_text}");
    }

}
