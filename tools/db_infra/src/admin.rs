use actix_web::{get, post, web, HttpResponse, Responder};
use serde::Deserialize;
use crate::db::DbRouter;

#[get("/api/v1/admin/infra/db/status")]
pub async fn status(router: web::Data<DbRouter>) -> impl Responder {
    match router.status().await {
        Ok(j) => HttpResponse::Ok().json(j),
        Err(e) => HttpResponse::InternalServerError().body(format!("error: {}", e)),
    }
}

#[derive(Deserialize)]
pub struct RebalanceReq {
    pub shard: String,
    pub replica_weights: Vec<(String, u32)>,
}

#[post("/api/v1/admin/infra/db/rebalance")]
pub async fn rebalance(router: web::Data<DbRouter>, body: web::Json<RebalanceReq>) -> impl Responder {
    // For simplicity: rebuild read pool mapping for shard
    // TODO: implement dynamic weighting; here we just acknowledge
    HttpResponse::Ok().json(serde_json::json!({"status":"ok","shard": body.shard }))
}
