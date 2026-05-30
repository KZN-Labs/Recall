use recall_proto::memory as mem_proto;
use std::collections::{HashMap, HashSet};
use std::sync::RwLock;

pub struct MemoryStore {
    entries: RwLock<HashMap<String, mem_proto::MemoryEntry>>,
    by_entity: RwLock<HashMap<String, Vec<String>>>,
}

impl Default for MemoryStore {
    fn default() -> Self {
        Self {
            entries: RwLock::new(HashMap::new()),
            by_entity: RwLock::new(HashMap::new()),
        }
    }
}

impl MemoryStore {
    pub fn insert(&self, entry: mem_proto::MemoryEntry) {
        let id = entry.id.clone();
        let entity = entry.entity.clone();
        self.entries.write().unwrap().insert(id.clone(), entry);
        self.by_entity.write().unwrap().entry(entity).or_default().push(id);
    }

    pub fn get(&self, id: &str) -> Option<mem_proto::MemoryEntry> {
        self.entries.read().unwrap().get(id).cloned()
    }

    pub fn list_workspaces(&self) -> Vec<String> {
        let mut ws: HashSet<String> = HashSet::new();
        for e in self.entries.read().unwrap().values() {
            if let Some(w) = &e.workspace_id {
                ws.insert(w.value.clone());
            }
        }
        let mut v: Vec<_> = ws.into_iter().collect();
        v.sort();
        v
    }

    pub fn list_by_workspace(&self, workspace_id: &str) -> Vec<mem_proto::MemoryEntry> {
        let mut entries: Vec<_> = self
            .entries.read().unwrap().values()
            .filter(|e| e.workspace_id.as_ref().map(|w| w.value == workspace_id).unwrap_or(false))
            .cloned().collect();
        entries.sort_by_key(|e| e.timestamp.as_ref().map(|t| t.seconds).unwrap_or(0));
        entries
    }

    pub fn get_by_entity_all(&self, entity: &str) -> Vec<mem_proto::MemoryEntry> {
        let mut entries: Vec<_> = self
            .entries.read().unwrap().values()
            .filter(|e| e.entity == entity)
            .cloned().collect();
        entries.sort_by_key(|e| e.timestamp.as_ref().map(|t| t.seconds).unwrap_or(0));
        entries
    }

    pub fn get_by_entity(&self, workspace_id: &str, entity: &str) -> Vec<mem_proto::MemoryEntry> {
        let ids = self.by_entity.read().unwrap().get(entity).cloned().unwrap_or_default();
        let entries = self.entries.read().unwrap();
        ids.iter()
            .filter_map(|id| entries.get(id))
            .filter(|e| e.workspace_id.as_ref().map(|w| w.value == workspace_id).unwrap_or(false))
            .cloned().collect()
    }

    /// Remove entries written AFTER `before_ts` (unix seconds). Returns count removed.
    pub fn rollback_to(&self, workspace_id: &str, before_ts: i64) -> usize {
        let to_remove: Vec<String> = self.entries.read().unwrap().values()
            .filter(|e| {
                e.workspace_id.as_ref().map(|w| w.value == workspace_id).unwrap_or(false)
                && e.timestamp.as_ref().map(|t| t.seconds > before_ts).unwrap_or(false)
            })
            .map(|e| e.id.clone())
            .collect();

        let count = to_remove.len();
        let mut entries = self.entries.write().unwrap();
        for id in &to_remove { entries.remove(id); }

        let mut by_entity = self.by_entity.write().unwrap();
        for list in by_entity.values_mut() {
            list.retain(|id| !to_remove.contains(id));
        }
        count
    }
}
