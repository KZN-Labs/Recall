use chrono::Utc;
use prost::Message;
use prost_types::Struct;
use recall_core::ids::{AgentId, ContentHash, WorkspaceId};
use recall_crypto::RecallKeypair;
use recall_proto::{common as common_proto, memory as mem_proto};
use uuid::Uuid;

/// Build a signed MemoryEntry proto from typed fields.
pub fn build_entry(
    workspace_id: &WorkspaceId,
    entity: &str,
    agent_id: &AgentId,
    passport_id: &ContentHash,
    model_provider: &str,
    model_name: &str,
    trust_level: i32,
    event: &str,
    data: serde_json::Value,
    tags: Vec<String>,
    scope: &str,
    causal_predecessors: Vec<ContentHash>,
    cost_annotation: Option<common_proto::CostAnnotation>,
    keypair: &RecallKeypair,
) -> mem_proto::MemoryEntry {
    let now = Utc::now();
    let entry_id = format!("mem_{}", Uuid::now_v7());

    // Convert serde_json::Value to prost Struct.
    let data_struct = json_to_prost_struct(data);

    let mut entry = mem_proto::MemoryEntry {
        id: entry_id,
        receipt_id: None, // filled in after receipt is created
        workspace_id: Some(common_proto::WorkspaceId { value: workspace_id.0.clone() }),
        entity: entity.to_string(),
        agent_id: Some(common_proto::AgentId { value: agent_id.0.clone() }),
        passport_id: Some(common_proto::Hash { hex: passport_id.0.clone() }),
        model_provider: model_provider.to_string(),
        model_name: model_name.to_string(),
        trust_level,
        event: event.to_string(),
        data: Some(data_struct),
        tags,
        scope: scope.to_string(),
        timestamp: Some(prost_types::Timestamp {
            seconds: now.timestamp(),
            nanos: 0,
        }),
        walrus_blob: None,
        signature: None,
        causal_predecessors: causal_predecessors
            .iter()
            .map(|p| common_proto::CausalRef {
                receipt_id: Some(common_proto::Hash { hex: p.0.clone() }),
            })
            .collect(),
        cost_annotation,
        seal_status: 1, // UNSEALED
        imported_from: None,
    };

    // Sign the canonical bytes with ACTOR key.
    let mut canonical_entry = entry.clone();
    canonical_entry.signature = None;
    let mut buf = Vec::new();
    canonical_entry.encode(&mut buf).expect("prost encode");
    let sig_bytes = keypair.sign(&buf).to_bytes();
    entry.signature = Some(common_proto::Signature {
        bytes: sig_bytes.to_vec(),
        role: "ACTOR".to_string(),
        signer_public_key: keypair.public_key().to_bytes().to_vec(),
    });

    entry
}

fn json_to_prost_struct(val: serde_json::Value) -> Struct {
    let _json_bytes = serde_json::to_vec(&val).expect("json serialize");
    // Round-trip through JSON text and prost Struct parsing.
    let fields: std::collections::BTreeMap<String, prost_types::Value> =
        if let serde_json::Value::Object(map) = val {
            map.into_iter()
                .map(|(k, v)| (k, json_value_to_prost(v)))
                .collect()
        } else {
            Default::default()
        };
    Struct { fields }
}

fn json_value_to_prost(val: serde_json::Value) -> prost_types::Value {
    use prost_types::value::Kind;
    let kind = match val {
        serde_json::Value::Null => Kind::NullValue(0),
        serde_json::Value::Bool(b) => Kind::BoolValue(b),
        serde_json::Value::Number(n) => {
            Kind::NumberValue(n.as_f64().unwrap_or(0.0))
        }
        serde_json::Value::String(s) => Kind::StringValue(s),
        serde_json::Value::Array(arr) => Kind::ListValue(prost_types::ListValue {
            values: arr.into_iter().map(json_value_to_prost).collect(),
        }),
        serde_json::Value::Object(map) => Kind::StructValue(Struct {
            fields: map.into_iter().map(|(k, v)| (k, json_value_to_prost(v))).collect(),
        }),
    };
    prost_types::Value { kind: Some(kind) }
}
