use recall_proto::{common as common_proto, memory as mem_proto};
use std::collections::HashMap;
use std::sync::RwLock;

pub struct WorkspaceStore {
    inner: RwLock<HashMap<String, mem_proto::Workspace>>,
}

impl Default for WorkspaceStore {
    fn default() -> Self {
        Self {
            inner: RwLock::new(HashMap::new()),
        }
    }
}

impl WorkspaceStore {
    pub fn insert(&self, ws: mem_proto::Workspace) {
        if let Some(id) = &ws.id {
            self.inner.write().unwrap().insert(id.value.clone(), ws);
        }
    }

    pub fn get(&self, id: &str) -> Option<mem_proto::Workspace> {
        self.inner.read().unwrap().get(id).cloned()
    }

    pub fn list(&self) -> Vec<mem_proto::Workspace> {
        let inner = self.inner.read().unwrap();
        let mut workspaces: Vec<_> = inner.values().cloned().collect();
        workspaces.sort_by(|a, b| a.name.cmp(&b.name));
        workspaces
    }

    /// Auto-register a workspace by ID if it has not been explicitly created.
    /// Used to back-fill workspaces that appear in memory writes before create() is called.
    pub fn ensure_exists(&self, workspace_id: &str) {
        let exists = self.inner.read().unwrap().contains_key(workspace_id);
        if !exists {
            let name = workspace_id
                .strip_prefix("ws_")
                .unwrap_or(workspace_id)
                .to_string();
            let ws = mem_proto::Workspace {
                id: Some(common_proto::WorkspaceId {
                    value: workspace_id.to_string(),
                }),
                name,
                topology_mode: 0,
                created_at: Some(prost_types::Timestamp {
                    seconds: chrono::Utc::now().timestamp(),
                    nanos: 0,
                }),
                active_constitution_version: "v1".to_string(),
                agents: vec![],
                snapshot_blob: None,
            };
            self.inner.write().unwrap().insert(workspace_id.to_string(), ws);
        }
    }
}
