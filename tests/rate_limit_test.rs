//! Integration tests for Rate Limiting middleware
//!
//! Requires: REDIS_URL
//! Run with: cargo test rate_limit -- --ignored

use axum::{
    body::Body,
    extract::ConnectInfo,
    http::{header, Request, StatusCode},
    routing::get,
    Router,
};
use std::sync::Arc;
use tower::ServiceExt;

use std::collections::HashMap;
use Bitmesh_backend::cache::{init_cache_pool, CacheConfig, RedisCache};
use Bitmesh_backend::middleware::rate_limit::{
    rate_limit_middleware, EndpointLimits, LimitConfig, RateLimitConfig, RateLimitState,
};

async fn setup_router() -> Router {
    let redis_url =
        std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1:6379".to_string());
    let cache_config = CacheConfig {
        redis_url,
        ..Default::default()
    };
    // Redis init failure is unrecoverable in this test setup
    let cache_pool = init_cache_pool(cache_config)
        .await
        .expect("Redis init: ensure REDIS_URL is set and Redis is reachable");
    let redis_cache = RedisCache::new(cache_pool);

    let mut endpoints = HashMap::new();
    endpoints.insert(
        "/api/test_limit".to_string(),
        EndpointLimits {
            per_ip: Some(LimitConfig {
                limit: 2,
                window: 60,
            }),
            per_wallet: None,
        },
    );

    let config = Arc::new(RateLimitConfig {
        endpoints,
        default: EndpointLimits {
            per_ip: Some(LimitConfig {
                limit: 100,
                window: 60,
            }),
            per_wallet: None,
        },
    });

    let state = RateLimitState {
        cache: Arc::new(redis_cache),
        config,
    };

    Router::new()
        .route("/api/test_limit", get(|| async { "OK" }))
        .route("/health", get(|| async { "OK" }))
        .layer(axum::middleware::from_fn_with_state(
            state,
            rate_limit_middleware,
        ))
}

fn create_request(
    uri: &str,
    ip: &str,
    token: Option<&str>,
) -> Result<Request<Body>, Box<dyn std::error::Error>> {
    let mut builder = Request::builder().uri(uri).method("GET");

    if let Some(t) = token {
        builder = builder.header(header::AUTHORIZATION, format!("Bearer {}", t));
    }

    let mut req = builder.body(Body::empty())?;

    let ip_addr: std::net::IpAddr = ip.parse()?;
    req.extensions_mut()
        .insert(ConnectInfo(std::net::SocketAddr::new(ip_addr, 8080)));
    Ok(req)
}

#[tokio::test]
#[ignore]
async fn test_sliding_window_rate_limits() -> Result<(), Box<dyn std::error::Error>> {
    let app = setup_router().await;

    // First request should succeed
    let req1 = create_request("/api/test_limit", "192.168.1.100", None)?;
    let res1 = app.clone().oneshot(req1).await?;
    assert_eq!(res1.status(), StatusCode::OK);
    let remaining1 = res1
        .headers()
        .get("X-RateLimit-Remaining")
        .ok_or("missing X-RateLimit-Remaining header")?
        .to_str()?;
    assert_eq!(remaining1, "1");

    // Second request should succeed
    let req2 = create_request("/api/test_limit", "192.168.1.100", None)?;
    let res2 = app.clone().oneshot(req2).await?;
    assert_eq!(res2.status(), StatusCode::OK);
    let remaining2 = res2
        .headers()
        .get("X-RateLimit-Remaining")
        .ok_or("missing X-RateLimit-Remaining header")?
        .to_str()?;
    assert_eq!(remaining2, "0");

    // Third request should be blocked
    let req3 = create_request("/api/test_limit", "192.168.1.100", None)?;
    let res3 = app.clone().oneshot(req3).await?;
    assert_eq!(res3.status(), StatusCode::TOO_MANY_REQUESTS);

    let retry_after = res3
        .headers()
        .get("Retry-After")
        .ok_or("missing Retry-After header")?
        .to_str()?;
    assert_eq!(retry_after, "60");

    let body_bytes = axum::body::to_bytes(res3.into_body(), usize::MAX).await?;
    let err_json: serde_json::Value = serde_json::from_slice(&body_bytes)?;
    assert_eq!(err_json["error"]["code"], "RATE_LIMIT_EXCEEDED");

    // Test different IP is NOT blocked
    let req4 = create_request("/api/test_limit", "10.0.0.5", None)?;
    let res4 = app.clone().oneshot(req4).await?;
    assert_eq!(res4.status(), StatusCode::OK);

    // Test Admin Token is perfectly bypassed
    let req5 = create_request("/api/test_limit", "192.168.1.100", Some("admin-bypass-token"))?;
    let res5 = app.clone().oneshot(req5).await?;
    assert_eq!(res5.status(), StatusCode::OK);

    // Test Health endpoint bypasses limit configs natively
    let req6 = create_request("/health", "192.168.1.100", None)?;
    let res6 = app.clone().oneshot(req6).await?;
    assert_eq!(res6.status(), StatusCode::OK);

    Ok(())
}
