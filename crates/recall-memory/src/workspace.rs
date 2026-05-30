use recall_core::ids::WorkspaceId;
use recall_proto::{common as common_proto, memory as mem_proto};

pub fn new_workspace(
    name: &str,
    topology_mode: i32,
    constitution_version: &str,
) -> mem_proto::Workspace {
    let id = WorkspaceId::new(name);
    mem_proto::Workspace {
        id: Some(common_proto::WorkspaceId { value: id.0 }),
        name: name.to_string(),
        topology_mode,
        created_at: Some(prost_types::Timestamp {
            seconds: chrono::Utc::now().timestamp(),
            nanos: 0,
        }),
        active_constitution_version: constitution_version.to_string(),
        agents: vec![],
        snapshot_blob: None,
    }
}
