use anyhow::Result;
use prost::Message;
use recall_core::ids::ContentHash;
use recall_proto::receipt as receipt_proto;
use sqlx::PgPool;

pub struct PostgresReceiptStore {
    pool: PgPool,
}

impl PostgresReceiptStore {
    pub async fn new(database_url: &str) -> Result<Self> {
        let pool = PgPool::connect(database_url).await?;
        Ok(Self { pool })
    }

    pub async fn run_migrations(&self) -> Result<()> {
        sqlx::migrate!("./migrations").run(&self.pool).await?;
        Ok(())
    }

    pub async fn append(&self, receipt: &receipt_proto::Receipt) -> Result<ContentHash> {
        let id = receipt
            .id
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("receipt missing id"))?
            .hex
            .clone();

        let mut proto_bytes = Vec::new();
        receipt.encode(&mut proto_bytes)?;

        let workspace_id = receipt
            .workspace_id
            .as_ref()
            .map(|w| w.value.clone())
            .unwrap_or_default();

        let actor_passport_id = receipt
            .actor_passport_id
            .as_ref()
            .map(|h| h.hex.clone())
            .unwrap_or_default();

        let actor_agent_id = receipt
            .actor_agent_id
            .as_ref()
            .map(|a| a.value.clone())
            .unwrap_or_default();

        let timestamp: Option<chrono::DateTime<chrono::Utc>> =
            receipt.timestamp.as_ref().and_then(|ts| {
                chrono::DateTime::from_timestamp(ts.seconds, ts.nanos as u32)
            });

        let evidence_digest = receipt
            .evidence_digest
            .as_ref()
            .map(|h| h.hex.clone())
            .unwrap_or_default();

        let walrus_blob_id = receipt.walrus_blob.as_ref().map(|b| b.blob_id.clone());
        let deny_reason: Option<&str> = if receipt.deny_reason.is_empty() {
            None
        } else {
            Some(&receipt.deny_reason)
        };

        let unmet_caveats = serde_json::to_value(&receipt.unmet_caveats).ok();

        let cost = receipt.cost_annotation.as_ref();
        let cost_provider = cost.map(|c| c.model_provider.clone());
        let cost_model = cost.map(|c| c.model_name.clone());
        let cost_tokens_in: Option<i64> = cost.map(|c| c.tokens_in);
        let cost_tokens_out: Option<i64> = cost.map(|c| c.tokens_out);
        let cost_usd_cents: Option<i64> = cost.map(|c| c.usd_cents);

        sqlx::query(
            r#"
            INSERT INTO receipts (
                id, action_kind, workspace_id, actor_passport_id, actor_agent_id,
                timestamp, evidence_digest, seal_status, walrus_blob_id,
                deny_reason, unmet_caveats, reputation_delta,
                cost_provider, cost_model, cost_tokens_in, cost_tokens_out, cost_usd_cents,
                proto_bytes
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18)
            ON CONFLICT (id) DO NOTHING
            "#,
        )
        .bind(&id)
        .bind(&receipt.action_kind)
        .bind(&workspace_id)
        .bind(&actor_passport_id)
        .bind(&actor_agent_id)
        .bind(timestamp)
        .bind(&evidence_digest)
        .bind("UNSEALED")
        .bind(walrus_blob_id)
        .bind(deny_reason)
        .bind(unmet_caveats)
        .bind(receipt.reputation_delta)
        .bind(cost_provider)
        .bind(cost_model)
        .bind(cost_tokens_in)
        .bind(cost_tokens_out)
        .bind(cost_usd_cents)
        .bind(proto_bytes)
        .execute(&self.pool)
        .await?;

        Ok(ContentHash(id))
    }

    pub async fn get(&self, id: &ContentHash) -> Result<Option<receipt_proto::Receipt>> {
        let row: Option<(Vec<u8>,)> =
            sqlx::query_as("SELECT proto_bytes FROM receipts WHERE id = $1")
                .bind(&id.0)
                .fetch_optional(&self.pool)
                .await?;

        if let Some((proto_bytes,)) = row {
            let receipt = receipt_proto::Receipt::decode(proto_bytes.as_slice())?;
            Ok(Some(receipt))
        } else {
            Ok(None)
        }
    }

    pub async fn list_by_workspace(
        &self,
        workspace_id: &str,
        page_size: i64,
    ) -> Result<Vec<receipt_proto::Receipt>> {
        let rows: Vec<(Vec<u8>,)> = sqlx::query_as(
            "SELECT proto_bytes FROM receipts WHERE workspace_id = $1 ORDER BY timestamp DESC LIMIT $2",
        )
        .bind(workspace_id)
        .bind(page_size)
        .fetch_all(&self.pool)
        .await?;

        let receipts: Vec<_> = rows
            .into_iter()
            .filter_map(|(proto_bytes,)| receipt_proto::Receipt::decode(proto_bytes.as_slice()).ok())
            .collect();

        Ok(receipts)
    }
}
