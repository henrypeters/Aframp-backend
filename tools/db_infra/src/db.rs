use anyhow::Result;
use sqlx::{PgPool, postgres::PgPoolOptions, Row};
use std::collections::HashMap;
use tokio::sync::RwLock;
use std::sync::Arc;
use crate::shard_locator::ShardLocator;
use metrics::increment_counter;

#[derive(Clone)]
pub struct DbRouter {
    pub shards_write: Arc<RwLock<HashMap<String, PgPool>>>,
    pub shards_read: Arc<RwLock<HashMap<String, Vec<(PgPool, u32)>>>>, // pool + weight
    pub locator: ShardLocator,
}

impl DbRouter {
    pub async fn new(write_urls: Vec<String>, read_urls: HashMap<String, Vec<(String, u32)>>) -> Result<Self> {
        let mut shards_write = HashMap::new();
        for url in write_urls.iter() {
            let pool = PgPoolOptions::new().max_connections(16).connect(url).await?;
            // derive shard id from url (simple)
            let id = url.clone();
            shards_write.insert(id, pool);
        }
        let mut shards_read = HashMap::new();
        for (shard, vec) in read_urls.iter() {
            let mut pools = Vec::new();
            for (u, _w) in vec.iter() {
                let pool = PgPoolOptions::new().max_connections(32).connect(u).await?;
                pools.push((pool, 1u32));
            }
            shards_read.insert(shard.clone(), pools);
        }
        let nodes: Vec<String> = shards_write.keys().cloned().collect();
        let locator = ShardLocator::new(nodes, 128);
        Ok(DbRouter {
            shards_write: Arc::new(RwLock::new(shards_write)),
            shards_read: Arc::new(RwLock::new(shards_read)),
            locator,
        })
    }

    pub async fn route_write(&self, shard_key: &str, sql: &str) -> Result<u64> {
        // locate shard
        let shard = self.locator.locate(shard_key).ok_or_else(|| anyhow::anyhow!("no shard"))?;
        let map = self.shards_write.read().await;
        let pool = map.get(shard).ok_or_else(|| anyhow::anyhow!("no pool for shard"))?;
        let res = sqlx::query(sql).execute(pool).await?;
        increment_counter!("db_queries_routed_total", "type" => "write");
        Ok(res.rows_affected())
    }

    pub async fn route_read(&self, shard_key: &str, sql: &str, consistent: bool) -> Result<sqlx::postgres::PgRow> {
        // if consistent -> read from master
        let shard = self.locator.locate(shard_key).ok_or_else(|| anyhow::anyhow!("no shard"))?;
        if consistent {
            let map = self.shards_write.read().await;
            let pool = map.get(shard).ok_or_else(|| anyhow::anyhow!("no pool for shard"))?;
            let row = sqlx::query(sql).fetch_one(pool).await?;
            increment_counter!("db_queries_routed_total", "type" => "read_master");
            return Ok(row);
        }
        let reads = self.shards_read.read().await;
        if let Some(vec) = reads.get(shard) {
            // pick first available (round robin or weight-based)
            if let Some((pool, _w)) = vec.get(0) {
                let row = sqlx::query(sql).fetch_one(pool).await?;
                increment_counter!("db_queries_routed_total", "type" => "read_replica");
                return Ok(row);
            }
        }
        // fallback to master
        let map = self.shards_write.read().await;
        let pool = map.get(shard).ok_or_else(|| anyhow::anyhow!("no pool for shard"))?;
        let row = sqlx::query(sql).fetch_one(pool).await?;
        increment_counter!("db_queries_routed_total", "type" => "read_fallback");
        Ok(row)
    }

    // metrics helpers
    pub async fn status(&self) -> Result<serde_json::Value> {
        let mut arr = Vec::new();
        let writes = self.shards_write.read().await;
        for (k, pool) in writes.iter() {
            let stats = json!({
                "shard": k,
                "connections": pool.size(),
            });
            arr.push(stats);
        }
        Ok(json!({"write_shards": arr}))
    }
}
