use recall_proto::receipt as receipt_proto;
use std::collections::HashMap;
use std::sync::RwLock;
use tokio::sync::broadcast;

const CHANNEL_CAPACITY: usize = 256;

/// Per-workspace broadcast channel for receipt streaming.
pub struct SubscribeHub {
    channels: RwLock<HashMap<String, broadcast::Sender<receipt_proto::Receipt>>>,
}

impl Default for SubscribeHub {
    fn default() -> Self {
        Self {
            channels: RwLock::new(HashMap::new()),
        }
    }
}

impl SubscribeHub {
    pub fn subscribe(&self, workspace_id: &str) -> broadcast::Receiver<receipt_proto::Receipt> {
        let mut channels = self.channels.write().unwrap();
        let sender = channels
            .entry(workspace_id.to_string())
            .or_insert_with(|| broadcast::channel(CHANNEL_CAPACITY).0);
        sender.subscribe()
    }

    pub fn publish(&self, workspace_id: &str, receipt: receipt_proto::Receipt) {
        let channels = self.channels.read().unwrap();
        if let Some(sender) = channels.get(workspace_id) {
            // Ignore lagged receiver errors.
            let _ = sender.send(receipt);
        }
    }
}
