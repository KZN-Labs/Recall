use recall_core::{error::RecallError, ids::ContentHash};
use recall_proto::receipt as receipt_proto;
use std::collections::{HashMap, HashSet};
use std::sync::RwLock;

/// Append-only in-process receipt store. Production swaps for Postgres.
pub struct ReceiptStore {
    receipts: RwLock<HashMap<String, receipt_proto::Receipt>>,
    /// IDs of receipts already committed to an anchor batch. We track this
    /// set so the anchor scheduler can find unanchored receipts in O(N) and
    /// avoid double-counting on the next tick. `anchor.commit` receipts
    /// themselves are auto-added so they never get nested into a future batch.
    anchored: RwLock<HashSet<String>>,
}

impl Default for ReceiptStore {
    fn default() -> Self {
        Self {
            receipts: RwLock::new(HashMap::new()),
            anchored: RwLock::new(HashSet::new()),
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

    /// Drain receipts that have not yet been committed to a Sui anchor batch.
    ///
    /// Returns the unanchored receipts sorted oldest-first (so the Merkle root
    /// is deterministic across runs given the same inputs) and immediately
    /// records them as anchored. `anchor.commit` receipts are skipped so we
    /// never anchor an anchor.
    pub fn take_unanchored(&self) -> Vec<receipt_proto::Receipt> {
        let mut out: Vec<receipt_proto::Receipt> = {
            let receipts = self.receipts.read().unwrap();
            let anchored = self.anchored.read().unwrap();
            receipts
                .values()
                .filter(|r| {
                    let id = r.id.as_ref().map(|h| h.hex.as_str()).unwrap_or("");
                    !id.is_empty()
                        && r.action_kind != "anchor.commit"
                        && !anchored.contains(id)
                })
                .cloned()
                .collect()
        };
        out.sort_by_key(|r| r.timestamp.as_ref().map(|t| t.seconds).unwrap_or(0));

        let mut anchored = self.anchored.write().unwrap();
        for r in &out {
            if let Some(h) = r.id.as_ref() {
                anchored.insert(h.hex.clone());
            }
        }
        out
    }

    /// Mark a single receipt as already anchored. Used at startup if any
    /// `anchor.commit` receipts are loaded from persistent storage.
    pub fn mark_anchored(&self, id: &str) {
        self.anchored.write().unwrap().insert(id.to_string());
    }

    pub fn count(&self) -> usize {
        self.receipts.read().unwrap().len()
    }
}
