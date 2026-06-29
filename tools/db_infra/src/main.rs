mod shard_locator;
mod db;
mod admin;
mod metrics;

use actix_web::{App, HttpServer, web};
use tracing_subscriber;
use std::collections::HashMap;
use crate::db::DbRouter;

#[actix_web::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    let prometheus = metrics::init_metrics();

    // For demo: read env vars for DB urls; in production inject via config
    let write_urls = vec![
        std::env::var("DB_WRITE_SHARD_1").unwrap_or_else(|_| "postgres://postgres:postgres@localhost:5432/shard1".into()),
    ];
    let mut read_urls = HashMap::new();
    read_urls.insert("postgres://postgres:postgres@localhost:5432/shard1".to_string(), vec![(std::env::var("DB_READ_1").unwrap_or_else(|_| "postgres://postgres:postgres@localhost:5433/shard1_replica".into()), 1u32)]);

    let router = DbRouter::new(write_urls, read_urls).await?;
    let data = web::Data::new(router);

    HttpServer::new(move || {
        App::new()
            .app_data(data.clone())
            .service(admin::status)
            .service(admin::rebalance)
    })
    .bind(("0.0.0.0", 8088))?
    .run()
    .await?;

    Ok(())
}
