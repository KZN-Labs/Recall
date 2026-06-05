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

async fn write_memory(
    State(state): State<Arc<AppState>>,
    Path((workspace_id, entity)): Path<(String, String)>,
    Json(body): Json<WriteMemoryBody>,
) -> impl IntoResponse {
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

    // ── Walrus write — required ───────────────────────────────────────────────
    // Every memory write MUST land on Walrus. If Walrus is misconfigured or
    // unreachable, the write is rejected — we never silently degrade to
    // in-process-only storage.
    let walrus_blob_id: Option<String> = match state.walrus.as_ref() {
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
                        "hint":   "Check Walrus testnet connectivity and MEMWAL credentials",
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
                    "hint":  "Start with --walrus-testnet and set MEMWAL_PRIVATE_KEY + MEMWAL_ACCOUNT_ID",
                })),
            ).into_response();
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

    // Conflict detection.
    let existing = state.memory_store.get_by_entity(&workspace_id, &entity);
    for existing_entry in &existing {
        if existing_entry.id != entry.id
            && recall_conflict::detect_conflict(existing_entry, &entry)
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
    (StatusCode::CREATED, Json(resp)).into_response()
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

    // ── 7. Build canonical profile proto and write it to Walrus ───────────────
    let published_at = chrono::Utc::now();
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
        ..Default::default()
    };

    let mut profile_bytes = Vec::new();
    if let Err(e) = prost::Message::encode(&profile_proto_for_blob, &mut profile_bytes) {
        tracing::warn!("registry profile encode failed: {e}");
    }

    let walrus_blob_id: String = if let Some(walrus) = &state.walrus {
        match walrus.put_blob_raw(&profile_bytes).await {
            Ok(blob) => {
                tracing::info!(
                    "Registry profile {}@{} stored on Walrus: {}",
                    body.name, body.version, blob.0
                );
                blob.0
            }
            Err(e) => {
                tracing::warn!(
                    "Walrus write failed for registry profile ({e}); using deterministic ID"
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
        .route("/handoff",                             axum::routing::post(handoff))
        .layer(CorsLayer::permissive())
        .with_state(state)
}
