//! Integration tests for GET /api/fees endpoint

use anyhow::{Context, Result};
use axum::{body::Body, routing::get, Router};
use http::{Request, StatusCode};
use sqlx::PgPool;
use std::str::FromStr;
use std::sync::Arc;
use tower::util::ServiceExt;
use Bitmesh_backend::api::fees::{get_fees, FeesState};
use Bitmesh_backend::services::fee_calculation::FeeCalculationService;

async fn setup_test_db() -> Result<PgPool> {
    let database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgresql://postgres:postgres@localhost/aframp_test".to_string());

    PgPool::connect(&database_url)
        .await
        .context("failed to connect to test database")
}

async fn seed_fee_structures(pool: &PgPool) -> Result<()> {
    sqlx::query("DELETE FROM fee_structures WHERE transaction_type LIKE 'test_%' OR transaction_type IN ('onramp', 'offramp', 'bill_payment')")
        .execute(pool)
        .await
        .context("failed to delete existing fee structures")?;

    sqlx::query(
        r#"
        INSERT INTO fee_structures 
        (transaction_type, payment_provider, payment_method, min_amount, max_amount,
         provider_fee_percent, provider_fee_flat, provider_fee_cap, platform_fee_percent, is_active)
        VALUES ('onramp', 'flutterwave', 'card', 1000, 50000, 1.4, 100, 2000, 0.5, true)
        "#,
    )
    .execute(pool)
    .await
    .context("failed to insert first fee structure")?;

    sqlx::query(
        r#"
        INSERT INTO fee_structures 
        (transaction_type, payment_provider, payment_method, min_amount, max_amount,
         provider_fee_percent, provider_fee_flat, provider_fee_cap, platform_fee_percent, is_active)
        VALUES ('onramp', 'flutterwave', 'card', 50001, 500000, 1.4, 0, 2000, 0.3, true)
        "#,
    )
    .execute(pool)
    .await
    .context("failed to insert second fee structure")?;

    sqlx::query(
        r#"
        INSERT INTO fee_structures 
        (transaction_type, payment_provider, payment_method, min_amount, max_amount,
         provider_fee_percent, provider_fee_flat, provider_fee_cap, platform_fee_percent, is_active)
        VALUES ('onramp', 'paystack', 'card', 1000, 50000, 1.5, 0, 2000, 0.5, true)
        "#,
    )
    .execute(pool)
    .await
    .context("failed to insert third fee structure")?;

    sqlx::query(
        r#"
        INSERT INTO fee_structures 
        (transaction_type, payment_provider, payment_method, min_amount, max_amount,
         provider_fee_percent, provider_fee_flat, provider_fee_cap, platform_fee_percent, is_active)
        VALUES ('offramp', 'flutterwave', 'bank_transfer', 1000, NULL, 0.8, 50, 5000, 0.5, true)
        "#,
    )
    .execute(pool)
    .await
    .context("failed to insert fourth fee structure")?;

    Ok(())
}

fn build_fees_app(pool: PgPool) -> Router {
    let fee_service = Arc::new(FeeCalculationService::new(pool));
    let state = FeesState {
        fee_service,
        cache: None,
    };
    Router::new()
        .route("/api/fees", get(get_fees))
        .with_state(state)
}

/// Build a GET request for the given URI.
fn get(uri: &str) -> Result<Request<Body>> {
    Request::builder()
        .uri(uri)
        .body(Body::empty())
        .context("failed to build request")
}

/// Deserialize the response body as JSON.
async fn json_body(resp: axum::response::Response) -> Result<serde_json::Value> {
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .context("failed to read response body")?;
    serde_json::from_slice(&bytes).context("response body is not valid JSON")
}

#[tokio::test]
#[ignore] // Requires DATABASE_URL and test database
async fn test_fees_no_params_returns_full_structure() -> Result<()> {
    let pool = setup_test_db().await?;
    seed_fee_structures(&pool).await?;
    let app = build_fees_app(pool);

    let response = app
        .oneshot(get("/api/fees")?)
        .await
        .context("oneshot failed")?;

    assert_eq!(response.status(), StatusCode::OK);

    let json = json_body(response).await?;

    assert!(json.get("fee_structure").is_some());
    assert!(json.get("timestamp").is_some());
    let structure = json.get("fee_structure").unwrap();
    assert!(structure.get("onramp").is_some());
    assert!(structure.get("offramp").is_some());
    Ok(())
}

#[tokio::test]
#[ignore] // Requires DATABASE_URL and test database
async fn test_fees_amount_type_provider_returns_calculated() -> Result<()> {
    let pool = setup_test_db().await?;
    seed_fee_structures(&pool).await?;
    let app = build_fees_app(pool);

    let response = app
        .oneshot(get(
            "/api/fees?amount=10000&type=onramp&provider=flutterwave",
        )?)
        .await
        .context("oneshot failed")?;

    assert_eq!(response.status(), StatusCode::OK);

    let json = json_body(response).await?;

    assert_eq!(json.get("amount").and_then(|v| v.as_f64()), Some(10000.0));
    assert_eq!(json.get("type").and_then(|v| v.as_str()), Some("onramp"));
    assert_eq!(
        json.get("provider").and_then(|v| v.as_str()),
        Some("flutterwave")
    );
    let breakdown = json.get("breakdown").unwrap();
    assert!(breakdown.get("platform_fee_ngn").is_some());
    assert!(breakdown.get("provider_fee_ngn").is_some());
    assert!(breakdown.get("total_fee_ngn").is_some());
    assert!(breakdown.get("amount_after_fees_ngn").is_some());
    assert!(breakdown.get("platform_fee_pct").is_some());
    assert!(breakdown.get("provider_fee_pct").is_some());

    // Provider fee: 10,000 × 1.4% + 100 = 240, Platform: 50, Total: 290
    let total = breakdown.get("total_fee_ngn").unwrap().as_f64().unwrap();
    assert!(
        (total - 290.0).abs() < 1.0,
        "expected total ~290, got {}",
        total
    );
    Ok(())
}

#[tokio::test]
#[ignore] // Requires DATABASE_URL and test database
async fn test_fees_amount_type_no_provider_returns_comparison() -> Result<()> {
    let pool = setup_test_db().await?;
    seed_fee_structures(&pool).await?;
    let app = build_fees_app(pool);

    let response = app
        .oneshot(get("/api/fees?amount=10000&type=onramp")?)
        .await
        .context("oneshot failed")?;

    assert_eq!(response.status(), StatusCode::OK);

    let json = json_body(response).await?;

    assert_eq!(json.get("amount").and_then(|v| v.as_f64()), Some(10000.0));
    assert_eq!(json.get("type").and_then(|v| v.as_str()), Some("onramp"));
    assert!(json.get("comparison").is_some());
    assert!(json.get("cheapest_provider").is_some());
    Ok(())
}

#[tokio::test]
#[ignore] // Requires DATABASE_URL and test database
async fn test_fees_amount_without_type_returns_400_missing_type() -> Result<()> {
    let pool = setup_test_db().await?;
    seed_fee_structures(&pool).await?;
    let app = build_fees_app(pool);

    let response = app
        .oneshot(get("/api/fees?amount=10000")?)
        .await
        .context("oneshot failed")?;

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let json = json_body(response).await?;
    let error = json.get("error").unwrap();
    assert_eq!(
        error.get("code").and_then(|v| v.as_str()),
        Some("MISSING_TYPE")
    );
    Ok(())
}

#[tokio::test]
#[ignore] // Requires DATABASE_URL and test database
async fn test_fees_invalid_type_returns_400() -> Result<()> {
    let pool = setup_test_db().await?;
    seed_fee_structures(&pool).await?;
    let app = build_fees_app(pool);

    let response = app
        .oneshot(get("/api/fees?amount=10000&type=xyz&provider=flutterwave")?)
        .await
        .context("oneshot failed")?;

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let json = json_body(response).await?;
    let error = json.get("error").unwrap();
    assert_eq!(
        error.get("code").and_then(|v| v.as_str()),
        Some("INVALID_TYPE")
    );
    Ok(())
}

#[tokio::test]
#[ignore] // Requires DATABASE_URL and test database
async fn test_fees_invalid_provider_returns_400() -> Result<()> {
    let pool = setup_test_db().await?;
    seed_fee_structures(&pool).await?;
    let app = build_fees_app(pool);

    let response = app
        .oneshot(get("/api/fees?amount=10000&type=onramp&provider=xyz")?)
        .await
        .context("oneshot failed")?;

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let json = json_body(response).await?;
    let error = json.get("error").unwrap();
    assert_eq!(
        error.get("code").and_then(|v| v.as_str()),
        Some("INVALID_PROVIDER")
    );
    Ok(())
}

#[tokio::test]
#[ignore] // Requires DATABASE_URL and test database
async fn test_fees_zero_amount_returns_400() -> Result<()> {
    let pool = setup_test_db().await?;
    seed_fee_structures(&pool).await?;
    let app = build_fees_app(pool);

    let response = app
        .oneshot(get("/api/fees?amount=0&type=onramp&provider=flutterwave")?)
        .await
        .context("oneshot failed")?;

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    let json = json_body(response).await?;
    let error = json.get("error").unwrap();
    assert_eq!(
        error.get("code").and_then(|v| v.as_str()),
        Some("INVALID_AMOUNT")
    );
    Ok(())
}

#[tokio::test]
#[ignore] // Requires DATABASE_URL and test database
async fn test_fees_fee_values_match_fee_calculation_service() -> Result<()> {
    let pool = setup_test_db().await?;
    seed_fee_structures(&pool).await?;

    let service = FeeCalculationService::new(pool.clone());
    let amount = sqlx::types::BigDecimal::from_str("10000").context("failed to parse amount")?;
    let breakdown = service
        .calculate_fees("onramp", amount, Some("flutterwave"), Some("card"))
        .await
        .context("calculate_fees failed")?;

    let app = build_fees_app(pool);
    let response = app
        .oneshot(get(
            "/api/fees?amount=10000&type=onramp&provider=flutterwave",
        )?)
        .await
        .context("oneshot failed")?;

    assert_eq!(response.status(), StatusCode::OK);
    let json = json_body(response).await?;
    let b = json.get("breakdown").unwrap();

    let api_total: f64 = b.get("total_fee_ngn").unwrap().as_f64().unwrap();
    let api_net: f64 = b.get("amount_after_fees_ngn").unwrap().as_f64().unwrap();
    let svc_total: f64 = breakdown
        .total
        .to_string()
        .parse()
        .context("failed to parse svc_total")?;
    let svc_net: f64 = breakdown
        .net_amount
        .to_string()
        .parse()
        .context("failed to parse svc_net")?;

    assert!(
        (api_total - svc_total).abs() < 0.01,
        "total mismatch: api={} svc={}",
        api_total,
        svc_total
    );
    assert!(
        (api_net - svc_net).abs() < 0.01,
        "net mismatch: api={} svc={}",
        api_net,
        svc_net
    );
    Ok(())
}
