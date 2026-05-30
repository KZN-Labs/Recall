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
    pub id: String,
    pub workspace_id: String,
    pub entity: String,
    pub agent_id: String,
    pub passport_id: String,
    pub event: String,
    pub data: serde_json::Value,
    pub tags: Vec<String>,
    pub scope: String,
    pub trust_level: i32,
    pub model_provider: String,
    pub model_name: String,
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

fn entry_to_json(e: &memory_proto::MemoryEntry) -> MemoryEntryJson {
    let data = e.data.as_ref()
        .map(prost_struct_to_json)
        .unwrap_or(serde_json::Value::Object(Default::default()));
    MemoryEntryJson {
        id: e.id.clone(),
        workspace_id: e.workspace_id.as_ref().map(|w| w.value.clone()).unwrap_or_default(),
        entity: e.entity.clone(),
        agent_id: e.agent_id.as_ref().map(|a| a.value.clone()).unwrap_or_default(),
        passport_id: e.passport_id.as_ref().map(|h| h.hex.clone()).unwrap_or_default(),
        event: e.event.clone(),
        data,
        tags: e.tags.clone(),
        scope: e.scope.clone(),
        trust_level: e.trust_level,
        model_provider: e.model_provider.clone(),
        model_name: e.model_name.clone(),
        timestamp_secs: e.timestamp.as_ref().map(|t| t.seconds),
    }
}

// ── Route handlers ────────────────────────────────────────────────────────────

async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "status": "ok", "service": "recall-control-plane" }))
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
    Json(entries.iter().map(entry_to_json).collect())
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

    Json(entries.iter().map(entry_to_json).collect())
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

    state.memory_store.insert(entry.clone());

    // Conflict detection.
    let existing = state.memory_store.get_by_entity(&workspace_id, &entity);
    let conflict_anchor = ContentHash("conflict_placeholder".into());
    for existing_entry in &existing {
        if existing_entry.id != entry.id
            && recall_conflict::detect_conflict(existing_entry, &entry)
        {
            let conflict = recall_conflict::build_conflict_record(
                existing_entry, &entry, &conflict_anchor,
            );
            state.conflict_store.insert(conflict);
        }
    }

    // Emit memory.write receipt.
    let ws       = WorkspaceId(workspace_id.clone());
    let passport = ContentHash(body.passport_id.clone());
    let agent    = AgentId(body.agent_id.clone());
    let receipt  = ReceiptBuilder::new(action_kind::MEMORY_WRITE, &ws, &passport, &agent)
        .with_cost_annotation(&body.model_provider, &body.model_name, 0, 0, 0)
        .build(&state.cp_keypair);
    let receipt_id = receipt.id.as_ref().map(|h| h.hex.clone()).unwrap_or_default();
    let _ = state.receipt_store.append(receipt.clone());

    // Broadcast to streaming subscribers.
    state.subscribe_hub.publish(&workspace_id, receipt);

    (StatusCode::CREATED, Json(serde_json::json!({
        "memory_id":  entry_id,
        "receipt_id": receipt_id,
    })))
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
    let ws_ids = state.memory_store.list_workspaces();
    let out = ws_ids.iter().map(|ws| {
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
    Json(entries.iter().map(entry_to_json).collect())
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
    name:        String,
    version:     String,
    author:      String,
    category:    String,
    description: String,
}

async fn publish_registry(
    State(state): State<Arc<AppState>>,
    Json(body): Json<PublishRegistryBody>,
) -> impl IntoResponse {
    use recall_proto::registry as reg_proto;
    let profile = reg_proto::RegistryProfile {
        name:        body.name.clone(),
        version:     body.version.clone(),
        author:      body.author.clone(),
        category:    body.category.clone(),
        description: body.description.clone(),
        memory_count: state.memory_store.list_workspaces().len() as i64,
        immutable:   true,
        ..Default::default()
    };
    match state.registry_store.publish(profile.clone()) {
        Ok(()) => (StatusCode::CREATED, Json(serde_json::json!({
            "name":    body.name,
            "version": body.version,
            "ok":      true,
        }))).into_response(),
        Err(e) => (StatusCode::CONFLICT, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    }
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
        .layer(CorsLayer::permissive())
        .with_state(state)
}
