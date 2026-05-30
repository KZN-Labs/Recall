use recall_core::{error::RecallError, ids::ContentHash};
use recall_proto::receipt as receipt_proto;
use std::collections::HashMap;
use std::sync::RwLock;

/// Append-only in-process receipt store. Production swaps for Postgres.
pub struct ReceiptStore {
    receipts: RwLock<HashMap<String, receipt_proto::Receipt>>,
}

impl Default for ReceiptStore {
    fn default() -> Self {
        Self {
            receipts: RwLock::new(HashMap::new()),
        }
    }
}

impl ReceiptStore {
    /// Append a receipt. Returns error if ID already exists (immutability guarantee).
    pub fn append(&self, receipt: receipt_proto::Receipt) -> Result<ContentHash, RecallError> {
        let id = receipt
            .id
            .as_ref()
            .ok_or_else(|| RecallError::Internal("receipt missing ID".into()))?
            .hex
            .clone();

        let mut store = self.receipts.write().unwrap();
        if store.contains_key(&id) {
            return Err(RecallError::Internal(format!(
                "receipt {} already exists (append-only)",
                id
            )));
        }
        store.insert(id.clone(), receipt);
        Ok(ContentHash(id))
    }

    pub fn get(&self, id: &ContentHash) -> Option<receipt_proto::Receipt> {
        self.receipts.read().unwrap().get(&id.0).cloned()
    }

    pub fn list_by_workspace(&self, workspace_id: &str) -> Vec<receipt_proto::Receipt> {
        self.receipts
            .read()
            .unwrap()
            .values()
            .filter(|r| {
                r.workspace_id
                    .as_ref()
                    .map(|w| w.value == workspace_id)
                    .unwrap_or(false)
            })
            .cloned()
            .collect()
    }
}
