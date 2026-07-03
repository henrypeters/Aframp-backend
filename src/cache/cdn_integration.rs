//! CDN Integration for Edge Caching
//!
//! Provides CDN integration for static assets and API responses:
//! - Cache-Control header management
//! - ETag generation and validation
//! - Edge function integration
//! - Geographic distribution optimization

use axum::http::{HeaderMap, HeaderValue};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;
use tracing::{debug, info, warn};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CDNConfig {
    pub enabled: bool,
    pub provider: CDNProvider,
    pub cache_control: CacheControlConfig,
    pub edge_functions: EdgeFunctionConfig,
    pub geographic: GeographicConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CDNProvider {
    Cloudflare,
    CloudFront,
    Fastly,
    Akamai,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheControlConfig {
    pub static_assets_ttl: u64,     // seconds
    pub api_responses_ttl: u64,     // seconds
    pub public_data_ttl: u64,       // seconds
    pub private_data_ttl: u64,      // seconds
    pub immutable_assets_ttl: u64,  // seconds
    pub max_age_shared_cache: u64,  // seconds
    pub max_age_private_cache: u64, // seconds
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EdgeFunctionConfig {
    pub rate_limiting_enabled: bool,
    pub geo_blocking_enabled: bool,
    pub request_normalization_enabled: bool,
    pub bot_protection_enabled: bool,
    pub ddos_protection_enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeographicConfig {
    pub default_region: String,
    pub region_mapping: HashMap<String, String>,
    pub latency_optimization: bool,
    pub content_localization: bool,
}

impl Default for CDNConfig {
    fn default() -> Self {
        Self {
            enabled: std::env::var("CDN_ENABLED")
                .unwrap_or_else(|_| "false".to_string())
                .parse()
                .unwrap_or(false),
            provider: match std::env::var("CDN_PROVIDER")
                .unwrap_or_else(|_| "cloudflare".to_string())
                .as_str()
            {
                "cloudfront" => CDNProvider::CloudFront,
                "fastly" => CDNProvider::Fastly,
                "akamai" => CDNProvider::Akamai,
                _ => CDNProvider::Cloudflare,
            },
            cache_control: CacheControlConfig {
                static_assets_ttl: std::env::var("CDN_STATIC_ASSETS_TTL")
                    .unwrap_or_else(|_| "31536000".to_string())
                    .parse()
                    .unwrap_or(31536000), // 1 year
                api_responses_ttl: std::env::var("CDN_API_RESPONSES_TTL")
                    .unwrap_or_else(|_| "300".to_string())
                    .parse()
                    .unwrap_or(300), // 5 minutes
                public_data_ttl: std::env::var("CDN_PUBLIC_DATA_TTL")
                    .unwrap_or_else(|_| "3600".to_string())
                    .parse()
                    .unwrap_or(3600), // 1 hour
                private_data_ttl: std::env::var("CDN_PRIVATE_DATA_TTL")
                    .unwrap_or_else(|_| "60".to_string())
                    .parse()
                    .unwrap_or(60), // 1 minute
                immutable_assets_ttl: std::env::var("CDN_IMMUTABLE_ASSETS_TTL")
                    .unwrap_or_else(|_| "31536000".to_string())
                    .parse()
                    .unwrap_or(31536000), // 1 year
                max_age_shared_cache: std::env::var("CDN_MAX_AGE_SHARED")
                    .unwrap_or_else(|_| "86400".to_string())
                    .parse()
                    .unwrap_or(86400), // 1 day
                max_age_private_cache: std::env::var("CDN_MAX_AGE_PRIVATE")
                    .unwrap_or_else(|_| "300".to_string())
                    .parse()
                    .unwrap_or(300), // 5 minutes
            },
            edge_functions: EdgeFunctionConfig {
                rate_limiting_enabled: std::env::var("CDN_RATE_LIMITING_ENABLED")
                    .unwrap_or_else(|_| "true".to_string())
                    .parse()
                    .unwrap_or(true),
                geo_blocking_enabled: std::env::var("CDN_GEO_BLOCKING_ENABLED")
                    .unwrap_or_else(|_| "false".to_string())
                    .parse()
                    .unwrap_or(false),
                request_normalization_enabled: std::env::var("CDN_REQUEST_NORMALIZATION_ENABLED")
                    .unwrap_or_else(|_| "true".to_string())
                    .parse()
                    .unwrap_or(true),
                bot_protection_enabled: std::env::var("CDN_BOT_PROTECTION_ENABLED")
                    .unwrap_or_else(|_| "true".to_string())
                    .parse()
                    .unwrap_or(true),
                ddos_protection_enabled: std::env::var("CDN_DDOS_PROTECTION_ENABLED")
                    .unwrap_or_else(|_| "true".to_string())
                    .parse()
                    .unwrap_or(true),
            },
            geographic: GeographicConfig {
                default_region: std::env::var("CDN_DEFAULT_REGION")
                    .unwrap_or_else(|_| "us-east-1".to_string()),
                region_mapping: HashMap::new(),
                latency_optimization: std::env::var("CDN_LATENCY_OPTIMIZATION")
                    .unwrap_or_else(|_| "true".to_string())
                    .parse()
                    .unwrap_or(true),
                content_localization: std::env::var("CDN_CONTENT_LOCALIZATION")
                    .unwrap_or_else(|_| "false".to_string())
                    .parse()
                    .unwrap_or(false),
            },
        }
    }
}

#[derive(Debug, Clone)]
pub struct CDNManager {
    config: CDNConfig,
    etag_cache: std::collections::HashMap<String, String>,
}

impl CDNManager {
    pub fn new(config: CDNConfig) -> Self {
        Self {
            config,
            etag_cache: std::collections::HashMap::new(),
        }
    }

    /// Add CDN-specific headers to HTTP responses
    pub fn add_cdn_headers(&self, headers: &mut HeaderMap, resource_type: ResourceType) {
        if !self.config.enabled {
            return;
        }

        // Add Cache-Control header based on resource type
        let cache_control = self.get_cache_control_header(resource_type);
        if let Ok(value) = HeaderValue::from_str(&cache_control) {
            headers.insert(axum::http::header::CACHE_CONTROL, value);
        }

        // Add ETag header
        let etag = self.generate_etag(resource_type);
        if let Ok(value) = HeaderValue::from_str(&etag) {
            headers.insert(axum::http::header::ETAG, value);
        }

        // Add CDN-specific headers
        self.add_provider_headers(headers, resource_type);

        // Add security headers
        self.add_security_headers(headers);

        // Add geographic headers if enabled
        if self.config.geographic.latency_optimization {
            self.add_geographic_headers(headers);
        }

        debug!("Added CDN headers for resource type: {:?}", resource_type);
    }

    fn get_cache_control_header(&self, resource_type: ResourceType) -> String {
        let ttl = match resource_type {
            ResourceType::StaticAsset => self.config.cache_control.static_assets_ttl,
            ResourceType::APIResponse => self.config.cache_control.api_responses_ttl,
            ResourceType::PublicData => self.config.cache_control.public_data_ttl,
            ResourceType::PrivateData => self.config.cache_control.private_data_ttl,
            ResourceType::ImmutableAsset => self.config.cache_control.immutable_assets_ttl,
        };

        let (max_age, s_maxage, private) = match resource_type {
            ResourceType::PrivateData => (
                self.config.cache_control.max_age_private_cache,
                0,
                "private",
            ),
            _ => (
                self.config.cache_control.max_age_shared_cache,
                ttl,
                "public",
            ),
        };

        let mut directives = vec![
            format!("max-age={}", max_age),
            format!("s-maxage={}", s_maxage),
            private.to_string(),
        ];

        // Add immutable directive for immutable assets
        if matches!(resource_type, ResourceType::ImmutableAsset) {
            directives.push("immutable".to_string());
        }

        // Add must-revalidate for dynamic content
        if matches!(
            resource_type,
            ResourceType::APIResponse | ResourceType::PrivateData
        ) {
            directives.push("must-revalidate".to_string());
        }

        directives.join(", ")
    }

    fn generate_etag(&self, resource_type: ResourceType) -> String {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        resource_type.hash(&mut hasher);
        Utc::now().timestamp_millis().hash(&mut hasher);

        let hash = hasher.finish();
        format!("\"{:x}\"", hash)
    }

    fn add_provider_headers(&self, headers: &mut HeaderMap, resource_type: ResourceType) {
        match self.config.provider {
            CDNProvider::Cloudflare => {
                headers.insert("CF-Cache-Status", HeaderValue::from_static("DYNAMIC"));
                headers.insert("CF-RAY", HeaderValue::from_static("dynamic"));

                if self.config.edge_functions.bot_protection_enabled {
                    headers.insert("CF-Bot-Protection", HeaderValue::from_static("active"));
                }
            }
            CDNProvider::CloudFront => {
                headers.insert("X-Amz-Cf-Id", HeaderValue::from_static("dynamic"));
                headers.insert("X-Amz-Cf-Pop", HeaderValue::from_static("dynamic"));
            }
            CDNProvider::Fastly => {
                headers.insert("Fastly-Debug", HeaderValue::from_static("1"));
                headers.insert("Fastly-SSL", HeaderValue::from_static("1"));
            }
            CDNProvider::Akamai => {
                headers.insert("Akamai-Origin-Hop", HeaderValue::from_static("2"));
            }
        }
    }

    fn add_security_headers(&self, headers: &mut HeaderMap) {
        // Content Security Policy
        let csp = "default-src 'self'; script-src 'self' 'unsafe-inline'; style-src 'self' 'unsafe-inline'; img-src 'self' data: https:; font-src 'self' data:; connect-src 'self' https:; frame-ancestors 'none';";
        if let Ok(value) = HeaderValue::from_str(csp) {
            headers.insert("Content-Security-Policy", value);
        }

        // Strict Transport Security
        let hsts = "max-age=31536000; includeSubDomains; preload";
        if let Ok(value) = HeaderValue::from_str(hsts) {
            headers.insert("Strict-Transport-Security", value);
        }

        // Other security headers
        headers.insert(
            "X-Content-Type-Options",
            HeaderValue::from_static("nosniff"),
        );
        headers.insert("X-Frame-Options", HeaderValue::from_static("DENY"));
        headers.insert(
            "X-XSS-Protection",
            HeaderValue::from_static("1; mode=block"),
        );
        headers.insert(
            "Referrer-Policy",
            HeaderValue::from_static("strict-origin-when-cross-origin"),
        );
        headers.insert(
            "Permissions-Policy",
            HeaderValue::from_static("geolocation=(), microphone=(), camera=()"),
        );
    }

    fn add_geographic_headers(&self, headers: &mut HeaderMap) {
        headers.insert("X-Geo-Country", HeaderValue::from_static("dynamic"));
        headers.insert("X-Geo-Region", HeaderValue::from_static("dynamic"));
        headers.insert("X-Edge-Location", HeaderValue::from_static("dynamic"));
    }

    /// Check if a request should be served from CDN cache
    pub fn should_cache_request(&self, path: &str, method: &str) -> bool {
        if !self.config.enabled {
            return false;
        }

        // Only cache GET requests by default
        if method != "GET" {
            return false;
        }

        // Don't cache authenticated endpoints
        if path.contains("/api/auth/") || path.contains("/api/admin/") {
            return false;
        }

        // Cache static assets
        if path.starts_with("/static/") || path.starts_with("/assets/") {
            return true;
        }

        // Cache public API endpoints
        if path.starts_with("/api/public/") {
            return true;
        }

        // Cache health checks
        if path == "/health" || path == "/status" {
            return true;
        }

        false
    }

    /// Get optimal region for a request based on geography
    pub fn get_optimal_region(&self, country: &str) -> String {
        if let Some(region) = self.config.geographic.region_mapping.get(country) {
            region.clone()
        } else {
            self.config.geographic.default_region.clone()
        }
    }

    /// Invalidate CDN cache for specific resources
    pub fn invalidate_cache(&self, paths: &[String]) -> Result<(), CDNError> {
        if !self.config.enabled {
            return Ok(());
        }

        info!("Invalidating CDN cache for {} paths", paths.len());

        // In a real implementation, this would call the CDN provider's API
        // For now, we'll just log the invalidation request
        for path in paths {
            debug!("Invalidating CDN cache for path: {}", path);
        }

        Ok(())
    }

    /// Warm up CDN cache with frequently accessed resources
    pub async fn warm_cache(&self, resources: Vec<CacheWarmupResource>) -> Result<(), CDNError> {
        if !self.config.enabled {
            return Ok(());
        }

        info!("Warming CDN cache with {} resources", resources.len());

        for resource in resources {
            debug!(
                "Warming cache resource: {} ({})",
                resource.path, resource.resource_type
            );

            // In a real implementation, this would make HTTP requests to warm the cache
            // For now, we'll just simulate the warming process
            tokio::time::sleep(Duration::from_millis(10)).await;
        }

        info!("CDN cache warming completed");
        Ok(())
    }

    /// Get CDN performance metrics
    pub fn get_metrics(&self) -> CDNMetrics {
        CDNMetrics {
            enabled: self.config.enabled,
            provider: format!("{:?}", self.config.provider),
            cache_hit_rate: 0.0, // Would be populated from CDN provider metrics
            total_requests: 0,
            cache_hits: 0,
            cache_misses: 0,
            bandwidth_saved: 0,
            average_response_time: Duration::from_millis(0),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheWarmupResource {
    pub path: String,
    pub resource_type: ResourceType,
    pub priority: WarmupPriority,
    pub headers: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ResourceType {
    StaticAsset,
    APIResponse,
    PublicData,
    PrivateData,
    ImmutableAsset,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WarmupPriority {
    High,
    Medium,
    Low,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CDNMetrics {
    pub enabled: bool,
    pub provider: String,
    pub cache_hit_rate: f64,
    pub total_requests: u64,
    pub cache_hits: u64,
    pub cache_misses: u64,
    pub bandwidth_saved: u64, // bytes
    pub average_response_time: Duration,
}

#[derive(Debug, thiserror::Error)]
pub enum CDNError {
    #[error("CDN provider error: {0}")]
    ProviderError(String),
    #[error("Configuration error: {0}")]
    ConfigurationError(String),
    #[error("Network error: {0}")]
    NetworkError(String),
    #[error("Authentication error: {0}")]
    AuthenticationError(String),
}

// Middleware for CDN integration
pub struct CDNMiddleware {
    cdn_manager: CDNManager,
}

impl CDNMiddleware {
    pub fn new(config: CDNConfig) -> Self {
        Self {
            cdn_manager: CDNManager::new(config),
        }
    }

    pub fn process_response(&self, path: &str, headers: &mut HeaderMap) {
        let resource_type = self.determine_resource_type(path);
        self.cdn_manager.add_cdn_headers(headers, resource_type);
    }

    fn determine_resource_type(&self, path: &str) -> ResourceType {
        if path.starts_with("/static/") || path.starts_with("/assets/") {
            if path.contains("/immutable/") {
                ResourceType::ImmutableAsset
            } else {
                ResourceType::StaticAsset
            }
        } else if path.starts_with("/api/public/") {
            ResourceType::PublicData
        } else if path.starts_with("/api/") {
            ResourceType::APIResponse
        } else {
            ResourceType::PrivateData
        }
    }

    pub fn should_cache(&self, path: &str, method: &str) -> bool {
        self.cdn_manager.should_cache_request(path, method)
    }
}

// ---------------------------------------------------------------------------
// Axum middleware (Issue #459) — body ETag + 304 + route-based Cache-Control
// ---------------------------------------------------------------------------

use axum::{
    body::Body,
    extract::Request,
    middleware::Next,
    response::Response,
};

/// Generate a deterministic ETag from response body bytes (SHA-256 first 16 hex chars).
pub fn body_etag(body: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let hash = Sha256::digest(body);
    // Format as lowercase hex, take first 16 bytes (32 chars) for compactness
    let hex: String = hash.iter().take(16).map(|b| format!("{:02x}", b)).collect();
    format!("\"{}\"", hex)
}

/// Return the correct `Cache-Control` header value for a given request path.
///
/// Route map (aligned with #459 acceptance criteria):
/// - `/api/rates`, `/api/fees`, `/api/liquidity` → `public, max-age=90, s-maxage=90`
/// - `/api/admin/`                               → `no-store, private`
/// - `/api/v1/user/`, `/api/users/`              → `private, max-age=0, must-revalidate`
/// - everything else                             → `no-cache`
pub fn route_cache_control(path: &str) -> &'static str {
    if path.starts_with("/api/rates")
        || path.starts_with("/api/fees")
        || path.starts_with("/api/liquidity")
    {
        "public, max-age=90, s-maxage=90"
    } else if path.starts_with("/api/admin") {
        "no-store, private"
    } else if path.starts_with("/api/v1/user") || path.starts_with("/api/users") {
        "private, max-age=0, must-revalidate"
    } else {
        "no-cache"
    }
}

/// Axum `from_fn` middleware that:
/// 1. Sets `Cache-Control` based on request path
/// 2. Generates a body-hash ETag on GET 200 responses
/// 3. Returns 304 if `If-None-Match` matches the ETag
///
/// Mount on routes that serve public data (rates, fees, liquidity).
pub async fn cdn_cache_middleware(request: Request, next: Next) -> Response {
    use axum::http::{header, StatusCode};

    let path = request.uri().path().to_string();
    let method = request.method().clone();
    let if_none_match = request
        .headers()
        .get(header::IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    let cache_control = route_cache_control(&path);

    let mut response = next.run(request).await;

    // Set Cache-Control on every response
    if let Ok(value) = header::HeaderValue::from_str(cache_control) {
        response.headers_mut().insert(header::CACHE_CONTROL, value);
    }

    // Only compute ETag for successful GET/HEAD responses
    if method != axum::http::Method::GET && method != axum::http::Method::HEAD {
        return response;
    }
    if response.status() != StatusCode::OK {
        return response;
    }

    // Collect body bytes to hash — only for public routes (no private data)
    if cache_control.contains("private") || cache_control.contains("no-store") {
        return response;
    }

    let (parts, body) = response.into_parts();
    let bytes = match axum::body::to_bytes(body, 4 * 1024 * 1024).await {
        Ok(b) => b,
        Err(_) => return Response::from_parts(parts, Body::empty()),
    };

    let etag = body_etag(&bytes);

    // 304 if client already has this version
    if if_none_match.as_deref() == Some(&etag) {
        crate::metrics::cache::cdn_cache_status_total()
            .with_label_values(&["304", path_prefix(&path)])
            .inc();
        let mut not_modified = Response::new(Body::empty());
        *not_modified.status_mut() = StatusCode::NOT_MODIFIED;
        if let Ok(v) = header::HeaderValue::from_str(&etag) {
            not_modified.headers_mut().insert(header::ETAG, v);
        }
        return not_modified;
    }

    crate::metrics::cache::cdn_cache_status_total()
        .with_label_values(&["200", path_prefix(&path)])
        .inc();

    let mut new_resp = Response::from_parts(parts, Body::from(bytes));
    if let Ok(v) = header::HeaderValue::from_str(&etag) {
        new_resp.headers_mut().insert(header::ETAG, v);
    }

    // CDN edge-cache control headers (Surrogate-Control for Varnish/Fastly,
    // Surrogate-Key / Cache-Tag for per-tag purge via Cloudflare/CloudFront)
    let tag = path_prefix(&path);
    if let Ok(v) = header::HeaderValue::from_str(&format!("max-age=90")) {
        new_resp.headers_mut().insert("Surrogate-Control", v);
    }
    if let Ok(v) = header::HeaderValue::from_str(tag) {
        new_resp.headers_mut().insert("Surrogate-Key", v);
        // CloudFront uses Cache-Tag
        new_resp.headers_mut().insert("Cache-Tag", header::HeaderValue::from_str(tag).unwrap());
    }

    new_resp
}

fn path_prefix(path: &str) -> &'static str {
    if path.starts_with("/api/rates") { "rates" }
    else if path.starts_with("/api/fees") { "fees" }
    else if path.starts_with("/api/liquidity") { "liquidity" }
    else { "other" }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cdn_config_default() {
        let config = CDNConfig::default();
        assert!(config.cache_control.static_assets_ttl > 0);
        assert!(config.cache_control.api_responses_ttl > 0);
    }

    #[test]
    fn test_cache_control_header_generation() {
        let config = CDNConfig::default();
        let manager = CDNManager::new(config);

        let header = manager.get_cache_control_header(ResourceType::StaticAsset);
        assert!(header.contains("max-age="));
        assert!(header.contains("public"));
    }

    #[test]
    fn test_etag_generation_is_deterministic() {
        // Body-hash ETags for the same content must be equal
        let body = b"exchange-rate-response-body";
        let e1 = body_etag(body);
        let e2 = body_etag(body);
        assert_eq!(e1, e2);
        assert!(e1.starts_with('"'));
        assert!(e1.ends_with('"'));
    }

    #[test]
    fn test_etag_differs_for_different_bodies() {
        let e1 = body_etag(b"body-one");
        let e2 = body_etag(b"body-two");
        assert_ne!(e1, e2);
    }

    #[test]
    fn test_route_cache_control_rates() {
        let cc = route_cache_control("/api/rates");
        assert!(cc.contains("public"));
        assert!(cc.contains("max-age=90"));
    }

    #[test]
    fn test_route_cache_control_admin() {
        let cc = route_cache_control("/api/admin/something");
        assert!(cc.contains("no-store"));
    }

    #[test]
    fn test_route_cache_control_user() {
        let cc = route_cache_control("/api/v1/user/profile");
        assert!(cc.contains("private"));
    }
}
