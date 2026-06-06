/// REST API client for the RECALL control plane (:8080).
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};

// ── Response shapes ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
pub struct WorkspaceInfo {
    pub workspace_id:   String,
    pub memory_count:   u64,
    pub receipt_count:  u64,
    pub conflict_count: u64,
    pub agent_count:    u64,
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct AgentInfo {
    pub agent_id:    String,
    pub role:        String,
    pub trust_level: i32,
    pub model:       String,
    pub stage:       String,
    pub reputation:  f64,
    pub write_count: i64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MemoryEntry {
    pub id:            String,
    pub workspace_id:  String,
    pub entity:        String,
    pub agent_id:      String,
    pub passport_id:   String,
    pub event:         String,
    pub data:          serde_json::Value,
    pub tags:          Vec<String>,
    pub scope:         String,
    pub trust_level:   i32,
    pub model_provider: String,
    pub model_name:    String,
    pub timestamp_secs: Option<i64>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Receipt {
    pub id:                  String,
    pub action_kind:         String,
    pub workspace_id:        String,
    pub actor_passport_id:   String,
    pub actor_agent_id:      String,
    pub timestamp_secs:      Option<i64>,
    pub seal_status:         i32,
    pub deny_reason:         Option<String>,
    pub reputation_delta:    f64,
    /// For `anchor.commit` receipts: hex Merkle root of the anchored batch.
    #[serde(default)]
    pub evidence_digest:     String,
    /// For `anchor.commit` receipts: receipt IDs included in the batch.
    #[serde(default)]
    pub causal_predecessors: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Conflict {
    pub conflict_id:     String,
    pub workspace_id:    String,
    pub entity:          String,
    pub entry_a_id:      String,
    pub entry_b_id:      String,
    pub auto_resolution: String,
    pub resolution:      String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RegistryProfile {
    pub name:         String,
    pub version:      String,
    pub author:       String,
    pub description:  String,
    pub category:     String,
    pub memory_count: i64,
    pub import_count: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct RollbackRequest {
    pub to_timestamp: i64,
}

#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
pub struct RollbackResult {
    pub workspace_id:        String,
    pub entries_removed:     usize,
    pub rolled_back_to:      i64,
    pub rollback_receipt_id: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct PublishRequest {
    pub name:         String,
    pub version:      String,
    pub category:     String,
    pub description:  String,
    pub workspace_id: Option<String>,
    pub passport_id:  String,
    pub signature:    String,
    pub public_key:   String,
}

// ── Client ────────────────────────────────────────────────────────────────────

pub struct ApiClient {
    base:   String,
    client: reqwest::Client,
}

impl ApiClient {
    pub fn new(endpoint: &str) -> Self {
        let base = endpoint.trim_end_matches('/').to_string();
        Self { base, client: reqwest::Client::builder().timeout(std::time::Duration::from_secs(10)).build().unwrap() }
    }

    pub async fn health(&self) -> bool {
        self.client.get(format!("{}/health", self.base)).send().await
            .map(|r| r.status().is_success()).unwrap_or(false)
    }

    pub async fn list_workspaces(&self) -> Result<Vec<WorkspaceInfo>> {
        self.get("/workspaces").await
    }

    #[allow(dead_code)]
    pub async fn workspace_stats(&self, ws: &str) -> Result<WorkspaceInfo> {
        // stats endpoint returns a slightly different shape — map it
        let v: serde_json::Value = self.get(&format!("/stats/{}", encode(ws))).await?;
        Ok(WorkspaceInfo {
            workspace_id:   v["workspace_id"].as_str().unwrap_or("").to_string(),
            memory_count:   v["memory_count"].as_u64().unwrap_or(0),
            receipt_count:  v["receipt_count"].as_u64().unwrap_or(0),
            conflict_count: v["conflict_count"].as_u64().unwrap_or(0),
            agent_count:    v["agent_count"].as_u64().unwrap_or(0),
        })
    }

    pub async fn workspace_agents(&self, ws: &str) -> Result<Vec<AgentInfo>> {
        self.get(&format!("/workspace/{}/agents", encode(ws))).await
    }

    pub async fn list_memory(&self, ws: &str) -> Result<Vec<MemoryEntry>> {
        self.get(&format!("/memory/{}", encode(ws))).await
    }

    pub async fn get_entity(&self, entity: &str) -> Result<Vec<MemoryEntry>> {
        self.get(&format!("/entity/{}", encode(entity))).await
    }

    pub async fn list_receipts(&self, ws: &str, action_kind: Option<&str>) -> Result<Vec<Receipt>> {
        let mut url = format!("{}/receipts?workspace_id={}", self.base, encode(ws));
        if let Some(ak) = action_kind { url.push_str(&format!("&action_kind={}", ak)); }
        let r = self.client.get(&url).send().await?;
        if !r.status().is_success() { return Ok(vec![]); }
        Ok(r.json().await?)
    }

    /// Fetch the most recent N receipts of a given action_kind across all workspaces.
    pub async fn list_recent_receipts(
        &self,
        action_kind: Option<&str>,
        limit: usize,
    ) -> Result<Vec<Receipt>> {
        let mut url = format!("{}/receipts?limit={}", self.base, limit);
        if let Some(ak) = action_kind {
            url.push_str(&format!("&action_kind={}", encode(ak)));
        }
        let r = self.client.get(&url).send().await?;
        if !r.status().is_success() { return Ok(vec![]); }
        Ok(r.json().await?)
    }

    pub async fn get_receipt(&self, id: &str) -> Result<Receipt> {
        self.get(&format!("/receipts/{}", id)).await
    }

    pub async fn list_conflicts(&self, ws: &str) -> Result<Vec<Conflict>> {
        self.get(&format!("/conflicts/{}", encode(ws))).await
    }

    pub async fn all_conflicts(&self) -> Result<Vec<Conflict>> {
        let workspaces = self.list_workspaces().await?;
        let mut all = vec![];
        for ws in workspaces {
            if let Ok(cs) = self.list_conflicts(&ws.workspace_id).await {
                all.extend(cs);
            }
        }
        Ok(all)
    }

    pub async fn list_registry(&self, category: Option<&str>) -> Result<Vec<RegistryProfile>> {
        let mut url = format!("{}/registry", self.base);
        if let Some(cat) = category { url.push_str(&format!("?category={}", cat)); }
        let r = self.client.get(&url).send().await?;
        Ok(r.json().await?)
    }

    pub async fn rollback(&self, ws: &str, to_ts: i64) -> Result<RollbackResult> {
        let r = self.client.post(format!("{}/workspace/{}/rollback", self.base, encode(ws)))
            .json(&RollbackRequest { to_timestamp: to_ts })
            .send().await?;
        if !r.status().is_success() {
            return Err(anyhow!("rollback failed: {}", r.status()));
        }
        Ok(r.json().await?)
    }

    pub async fn publish_registry(&self, req: &PublishRequest) -> Result<serde_json::Value> {
        let r = self.client.post(format!("{}/registry", self.base))
            .json(req).send().await?;
        let status = r.status();
        if status == reqwest::StatusCode::CONFLICT {
            return Err(anyhow!(
                "{}@{} already exists — profiles are immutable. publish a new version with --version <x.y>",
                req.name, req.version
            ));
        }
        if status == reqwest::StatusCode::UNAUTHORIZED {
            let body: serde_json::Value = r.json().await.unwrap_or_default();
            let msg = body.get("error").and_then(|v| v.as_str()).unwrap_or("unauthorized");
            return Err(anyhow!("publish rejected: {msg}"));
        }
        if !status.is_success() {
            return Err(anyhow!("publish failed: {}", r.text().await?));
        }
        Ok(r.json().await?)
    }

    async fn get<T: serde::de::DeserializeOwned>(&self, path: &str) -> Result<T> {
        let r = self.client.get(format!("{}{}", self.base, path)).send().await?;
        if !r.status().is_success() {
            return Err(anyhow!("HTTP {}: {}", r.status(), path));
        }
        Ok(r.json().await?)
    }
}

fn encode(s: &str) -> String {
    s.replace('/', "%2F").replace('@', "%40")
}
