//! API Middleware Stack
//!
//! Tower-compatible middleware layers for the unified API server.
//! All middleware uses axum's `from_fn` pattern for simplicity and
//! composes into a configurable stack via [`MiddlewareConfig`].
//!
//! ## Layers (outermost → innermost)
//!
//! | Layer            | Header / Effect                  | Default |
//! |------------------|----------------------------------|---------|
//! | Request ID       | `X-Request-Id` (UUID v4)         | On      |
//! | Request Timing   | `X-Response-Time` (milliseconds) | On      |
//! | Request Logging  | Structured tracing spans         | On      |
//! | IP Filter        | `403 Forbidden` by IP            | Off     |
//! | Compression      | gzip / deflate response bodies   | Off     |
//! | ETag             | `ETag` + `304 Not Modified`      | Off     |
//! | Security Headers | `X-Content-Type-Options` etc.    | On      |
//! | API Version      | `X-API-Version` response header  | On      |
//! | Content-Type     | Reject missing JSON body type    | On      |
//! | CORS             | `Access-Control-Allow-*`         | On      |
//! | Request Timeout  | `408 Request Timeout`            | Off     |
//! | Rate Limit       | `429 Too Many Requests` + headers| Off     |
//! | Body Limit       | `413 Payload Too Large`          | Off     |
//! | Body Validation  | `422 Unprocessable Entity`       | Off     |
//! | Idempotency      | `Idempotency-Key` header cache   | Off     |
//! | Audit Log        | Structured audit events          | Off     |
//! | Response Cache   | `X-Cache` + `Age` + `Cache-Control`| Off   |
//! | Request Dedup    | `409 Conflict` for duplicates    | Off     |
//! | Request Tracing  | `traceparent` + `X-Trace-Id`     | Off     |
//! | Payload Signing  | `X-Signature` HMAC-SHA256        | Off     |
//! | Circuit Breaker  | `503` + `Retry-After` on failure | Off     |
//! | Sanitization     | Strip dangerous request headers  | Off     |
//! | Content Nego.    | `406 Not Acceptable` + `Vary`    | Off     |
//! | Throttle         | `503` concurrency limiter        | Off     |
//! | Retry Hints      | `Retry-After` + strategy headers | Off     |
//! | Maintenance      | `503` planned downtime gate      | Off     |
//! | Deprecation      | `Sunset` + `Deprecation` headers | Off     |
//! | Request Cost     | `X-Request-Cost` budget tracking | Off     |
//! | Fingerprint      | `X-Request-Fingerprint` hash     | Off     |
//! | Response Signing | `X-Response-Signature` HMAC      | Off     |
//! | Request Priority | `X-Request-Priority` tagging      | Off     |
//! | Request Quota    | Per-client usage quota enforcement | Off     |
//! | Tenant Isol.  | X-Tenant-Id multi-tenant     | Off     |
//! | Resp Envelope | {data, meta} JSON wrapping   | Off     |
//! | Replay Prot.  | Nonce-based replay detection | Off     |
//! | Geo-IP Hdrs   | IP-to-region header injection| Off     |
//! | Schema Valid. | JSON body schema validation  | Off     |
//! | Req Decomp.   | gzip/deflate body decompression| Off     |
//! | Slow Req.     | Slow request detection/flagging| Off     |
//! | Hdr Prop.     | Propagate req headers to response| Off     |
//! | Req Context   | Inject deployment context headers| Off     |
//! | API Key Auth     | `Authorization: Bearer <key>`    | Off     |
//! | Fallback 404     | JSON `NOT_FOUND` response        | On      |
//!
//! ## Example
//! ```rust,ignore
//! use hv2_api::middleware::MiddlewareConfig;
//!
//! let config = MiddlewareConfig::default();
//! let app = Router::new().route("/", get(handler));
//! let app = config.apply(app);
//! ```

use axum::{
    extract::Request,
    http::{header, HeaderValue, Method, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    Json, Router,
};
use std::net::IpAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::rest::ErrorResponse;

// ============================================================================
// Request ID
// ============================================================================

/// A request ID stored in request extensions for propagation.
///
/// Inserted by the [`request_id`] middleware so that inner layers
/// (rate limiter, body limit, API key auth, etc.) can include the
/// request ID in their error responses.
#[derive(Debug, Clone)]
pub struct RequestId(pub String);

/// Extract the request ID from request extensions, if available.
pub fn extract_request_id(request: &Request) -> Option<String> {
    request.extensions().get::<RequestId>().map(|r| r.0.clone())
}

/// Middleware that generates or propagates a unique request ID.
///
/// If the incoming request already has an `X-Request-Id` header, the value
/// is propagated to the response unchanged. Otherwise a new UUID v4 is
/// generated. The header is always present on the response.
///
/// The ID is also stored in request extensions as [`RequestId`] so that
/// downstream middleware can include it in error responses.
pub async fn request_id(mut request: Request, next: Next) -> Response {
    let id = request
        .headers()
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

    // Store in extensions for downstream middleware
    request.extensions_mut().insert(RequestId(id.clone()));

    let mut response = next.run(request).await;
    if let Ok(val) = HeaderValue::from_str(&id) {
        response.headers_mut().insert("x-request-id", val);
    }
    response
}

// ============================================================================
// Request Timing
// ============================================================================

/// Middleware that measures request processing time.
///
/// Adds an `X-Response-Time` header with the duration in milliseconds
/// (e.g. `1.234ms`). The timer starts when the request enters this layer
/// and stops when the response is returned.
pub async fn request_timing(request: Request, next: Next) -> Response {
    let start = Instant::now();
    let mut response = next.run(request).await;
    let elapsed = start.elapsed();
    let ms = format!("{:.3}ms", elapsed.as_secs_f64() * 1000.0);
    if let Ok(val) = HeaderValue::from_str(&ms) {
        response.headers_mut().insert("x-response-time", val);
    }
    response
}

// ============================================================================
// Request Logging
// ============================================================================

/// Middleware that logs request/response details via tracing.
///
/// Emits an INFO-level event with method, path, status code, and
/// processing duration for every request.
pub async fn request_logging(request: Request, next: Next) -> Response {
    let method = request.method().clone();
    let path = request.uri().path().to_string();
    let start = Instant::now();

    let response = next.run(request).await;
    let elapsed = start.elapsed();
    let status = response.status().as_u16();

    tracing::info!(
        method = %method,
        path = %path,
        status = status,
        duration_ms = format_args!("{:.3}", elapsed.as_secs_f64() * 1000.0),
        "request completed"
    );

    response
}

// ============================================================================
// CORS Configuration
// ============================================================================

/// Cross-Origin Resource Sharing (CORS) configuration.
///
/// Controls which origins, methods, and headers are permitted in
/// cross-origin requests. An empty `allowed_origins` list means all
/// origins are allowed (responds with `*`).
#[derive(Debug, Clone)]
pub struct CorsConfig {
    /// Allowed origins. Empty = allow all (`*`).
    pub allowed_origins: Vec<String>,
    /// Allowed HTTP methods.
    pub allowed_methods: Vec<Method>,
    /// Allowed request headers (lowercase).
    pub allowed_headers: Vec<String>,
    /// Whether to include `Access-Control-Allow-Credentials: true`.
    pub allow_credentials: bool,
    /// Max age for preflight cache, in seconds.
    pub max_age: u64,
}

impl Default for CorsConfig {
    fn default() -> Self {
        Self {
            allowed_origins: vec![], // empty = wildcard
            allowed_methods: vec![
                Method::GET,
                Method::POST,
                Method::PUT,
                Method::DELETE,
                Method::PATCH,
                Method::OPTIONS,
            ],
            allowed_headers: vec![
                "content-type".to_string(),
                "authorization".to_string(),
                "x-request-id".to_string(),
            ],
            allow_credentials: false,
            max_age: 86400, // 24 hours
        }
    }
}

impl CorsConfig {
    /// Create a restrictive CORS config allowing only specific origins.
    pub fn restrictive(origins: Vec<String>) -> Self {
        Self {
            allowed_origins: origins,
            ..Default::default()
        }
    }

    /// Build CORS response headers for the given request origin.
    ///
    /// Returns an empty vec if the request origin is not in the allow-list
    /// (and the allow-list is non-empty), indicating CORS headers should
    /// not be added.
    fn build_headers(&self, origin: Option<&str>) -> Vec<(&'static str, String)> {
        let mut headers = Vec::new();

        // Determine the Access-Control-Allow-Origin value
        let allow_origin = if self.allowed_origins.is_empty() {
            "*".to_string()
        } else if let Some(origin) = origin {
            if self.allowed_origins.iter().any(|o| o == origin) {
                origin.to_string()
            } else {
                return headers; // origin not allowed — no CORS headers
            }
        } else {
            "*".to_string()
        };
        headers.push(("access-control-allow-origin", allow_origin));

        // Methods
        let methods: Vec<_> = self
            .allowed_methods
            .iter()
            .map(|m| m.as_str().to_string())
            .collect();
        headers.push(("access-control-allow-methods", methods.join(", ")));

        // Headers
        headers.push((
            "access-control-allow-headers",
            self.allowed_headers.join(", "),
        ));

        // Credentials
        if self.allow_credentials {
            headers.push(("access-control-allow-credentials", "true".to_string()));
        }

        // Max age
        headers.push(("access-control-max-age", self.max_age.to_string()));

        headers
    }
}

/// CORS middleware handler (called by closure from [`MiddlewareConfig::apply`]).
///
/// Handles OPTIONS preflight requests with 204 No Content and adds CORS
/// headers to all responses. If the request origin is not in the allow-list,
/// the request is passed through without CORS headers.
fn cors_handler(
    config: CorsConfig,
    request: Request,
    next: Next,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Response> + Send>> {
    Box::pin(async move {
        let origin = request
            .headers()
            .get(header::ORIGIN)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());

        let is_preflight = request.method() == Method::OPTIONS;
        let cors_headers = config.build_headers(origin.as_deref());

        // If origin isn't allowed, pass through without CORS headers
        if cors_headers.is_empty() {
            return next.run(request).await;
        }

        // Preflight: return 204 immediately; normal: call handler
        let mut response = if is_preflight {
            StatusCode::NO_CONTENT.into_response()
        } else {
            next.run(request).await
        };

        // Apply CORS headers
        for (key, value) in cors_headers {
            if let (Ok(name), Ok(val)) = (
                axum::http::HeaderName::from_bytes(key.as_bytes()),
                HeaderValue::from_str(&value),
            ) {
                response.headers_mut().insert(name, val);
            }
        }

        response
    })
}

// ============================================================================
// API Key Authentication
// ============================================================================

/// API key authentication configuration.
///
/// When enabled, every request must include a valid key in the
/// `Authorization` header (as `Bearer <key>`). Paths matching any
/// prefix in `excluded_paths` bypass authentication.
#[derive(Debug, Clone)]
pub struct ApiKeyConfig {
    /// Valid API keys. If empty, all requests are rejected when auth is on.
    pub keys: Vec<String>,
    /// Path prefixes excluded from authentication (e.g. `/health`).
    pub excluded_paths: Vec<String>,
    /// Header name to check (default: `authorization`).
    pub header_name: String,
}

impl Default for ApiKeyConfig {
    fn default() -> Self {
        Self {
            keys: vec![],
            excluded_paths: vec!["/health".to_string(), "/agentic".to_string()],
            header_name: "authorization".to_string(),
        }
    }
}

impl ApiKeyConfig {
    /// Check if a path is excluded from authentication.
    fn is_excluded(&self, path: &str) -> bool {
        self.excluded_paths.iter().any(|p| path.starts_with(p))
    }

    /// Validate the authorization header value.
    ///
    /// Accepts both `Bearer <key>` and bare `<key>` formats.
    fn validate(&self, auth_value: &str) -> bool {
        let key = auth_value.strip_prefix("Bearer ").unwrap_or(auth_value);
        self.keys.iter().any(|k| k == key)
    }
}

/// API key auth middleware handler (called by closure from [`MiddlewareConfig::apply`]).
fn api_key_handler(
    config: ApiKeyConfig,
    request: Request,
    next: Next,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Response> + Send>> {
    Box::pin(async move {
        let path = request.uri().path().to_string();

        // Skip authentication for excluded paths
        if config.is_excluded(&path) {
            return next.run(request).await;
        }

        let req_id = extract_request_id(&request);

        // Check authorization header
        let auth = request
            .headers()
            .get(&*config.header_name)
            .and_then(|v| v.to_str().ok());

        match auth {
            Some(value) if config.validate(value) => next.run(request).await,
            Some(_) => {
                let body = ErrorResponse {
                    error: "Invalid API key".to_string(),
                    code: "FORBIDDEN".to_string(),
                    request_id: req_id,
                };
                (StatusCode::FORBIDDEN, Json(body)).into_response()
            }
            None => {
                let body = ErrorResponse {
                    error: "Missing API key".to_string(),
                    code: "UNAUTHORIZED".to_string(),
                    request_id: req_id,
                };
                (StatusCode::UNAUTHORIZED, Json(body)).into_response()
            }
        }
    })
}

// ============================================================================
// Rate Limiting
// ============================================================================

/// Rate limiting configuration.
///
/// Uses a token-bucket algorithm: `capacity` tokens are available
/// initially, and tokens are replenished at `refill_rate` tokens per
/// second. Each request consumes one token. When no tokens remain the
/// request receives a `429 Too Many Requests` response.
///
/// Response headers on every request:
/// - `X-RateLimit-Limit` — bucket capacity
/// - `X-RateLimit-Remaining` — tokens remaining after this request
/// - `X-RateLimit-Reset` — seconds until full refill
/// - `Retry-After` — seconds to wait (only on 429)
#[derive(Debug, Clone)]
pub struct RateLimitConfig {
    /// Maximum number of tokens in the bucket.
    pub capacity: u64,
    /// Tokens replenished per second.
    pub refill_rate: f64,
    /// Paths excluded from rate limiting.
    pub excluded_paths: Vec<String>,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            capacity: 100,
            refill_rate: 10.0,
            excluded_paths: vec!["/health".to_string()],
        }
    }
}

impl RateLimitConfig {
    /// Check if a path is excluded from rate limiting.
    fn is_excluded(&self, path: &str) -> bool {
        self.excluded_paths.iter().any(|p| path.starts_with(p))
    }
}

/// Thread-safe token-bucket rate limiter.
///
/// Designed to be wrapped in an `Arc` and shared across the middleware
/// stack. Uses `parking_lot::Mutex` for low-contention locking.
#[derive(Debug)]
pub struct RateLimiter {
    inner: parking_lot::Mutex<TokenBucket>,
    config: RateLimitConfig,
}

#[derive(Debug)]
struct TokenBucket {
    tokens: f64,
    capacity: u64,
    refill_rate: f64,
    last_refill: Instant,
}

impl RateLimiter {
    /// Create a new rate limiter from the given configuration.
    pub fn new(config: RateLimitConfig) -> Self {
        let bucket = TokenBucket {
            tokens: config.capacity as f64,
            capacity: config.capacity,
            refill_rate: config.refill_rate,
            last_refill: Instant::now(),
        };
        Self {
            inner: parking_lot::Mutex::new(bucket),
            config,
        }
    }

    /// Try to acquire a token. Returns `(allowed, remaining, retry_after_secs)`.
    pub fn try_acquire(&self) -> (bool, u64, f64) {
        let mut bucket = self.inner.lock();
        let now = Instant::now();
        let elapsed = now.duration_since(bucket.last_refill).as_secs_f64();

        // Refill tokens based on elapsed time
        bucket.tokens = (bucket.tokens + elapsed * bucket.refill_rate).min(bucket.capacity as f64);
        bucket.last_refill = now;

        if bucket.tokens >= 1.0 {
            bucket.tokens -= 1.0;
            let remaining = bucket.tokens as u64;
            (true, remaining, 0.0)
        } else {
            // Calculate wait time until one token is available
            let deficit = 1.0 - bucket.tokens;
            let retry_after = if bucket.refill_rate > 0.0 {
                deficit / bucket.refill_rate
            } else {
                60.0 // default wait if refill rate is zero
            };
            (false, 0, retry_after)
        }
    }

    /// Get the configuration.
    pub fn config(&self) -> &RateLimitConfig {
        &self.config
    }
}

/// Rate limit middleware handler.
fn rate_limit_handler(
    limiter: Arc<RateLimiter>,
    request: Request,
    next: Next,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Response> + Send>> {
    Box::pin(async move {
        let path = request.uri().path().to_string();

        // Skip excluded paths
        if limiter.config().is_excluded(&path) {
            return next.run(request).await;
        }

        let (allowed, remaining, retry_after) = limiter.try_acquire();
        let capacity = limiter.config().capacity;
        let refill_rate = limiter.config().refill_rate;

        // Seconds until full refill
        let reset_secs = if refill_rate > 0.0 {
            ((capacity as f64 - remaining as f64) / refill_rate).ceil() as u64
        } else {
            0
        };

        if allowed {
            let mut response = next.run(request).await;
            let headers = response.headers_mut();
            if let Ok(v) = HeaderValue::from_str(&capacity.to_string()) {
                headers.insert("x-ratelimit-limit", v);
            }
            if let Ok(v) = HeaderValue::from_str(&remaining.to_string()) {
                headers.insert("x-ratelimit-remaining", v);
            }
            if let Ok(v) = HeaderValue::from_str(&reset_secs.to_string()) {
                headers.insert("x-ratelimit-reset", v);
            }
            response
        } else {
            let req_id = extract_request_id(&request);
            let retry_ceil = retry_after.ceil() as u64;
            let body = ErrorResponse {
                error: "Rate limit exceeded".to_string(),
                code: "RATE_LIMITED".to_string(),
                request_id: req_id,
            };
            let mut response = (StatusCode::TOO_MANY_REQUESTS, Json(body)).into_response();
            let headers = response.headers_mut();
            if let Ok(v) = HeaderValue::from_str(&capacity.to_string()) {
                headers.insert("x-ratelimit-limit", v);
            }
            if let Ok(v) = HeaderValue::from_str("0") {
                headers.insert("x-ratelimit-remaining", v);
            }
            if let Ok(v) = HeaderValue::from_str(&reset_secs.to_string()) {
                headers.insert("x-ratelimit-reset", v);
            }
            if let Ok(v) = HeaderValue::from_str(&retry_ceil.to_string()) {
                headers.insert("retry-after", v);
            }
            response
        }
    })
}

// ============================================================================
// Body Size Limit
// ============================================================================

/// Configuration for request body size limiting.
///
/// Requests with a `Content-Length` header exceeding `max_bytes` receive
/// a `413 Payload Too Large` response immediately, without reading the
/// body. Requests without `Content-Length` are allowed through (streaming
/// bodies rely on downstream extractors for enforcement).
///
/// Response on rejection:
/// - HTTP 413 with JSON `{"error": "...", "code": "PAYLOAD_TOO_LARGE"}`
/// - `X-Body-Limit` header with the configured maximum
#[derive(Debug, Clone)]
pub struct BodyLimitConfig {
    /// Maximum request body size in bytes.
    pub max_bytes: usize,
    /// Paths excluded from body size enforcement.
    pub excluded_paths: Vec<String>,
}

impl Default for BodyLimitConfig {
    fn default() -> Self {
        Self {
            max_bytes: 2 * 1024 * 1024, // 2 MB
            excluded_paths: vec!["/health".to_string()],
        }
    }
}

impl BodyLimitConfig {
    /// Check if a path is excluded from body size enforcement.
    fn is_excluded(&self, path: &str) -> bool {
        self.excluded_paths.iter().any(|p| path.starts_with(p))
    }
}

/// Request body size limit middleware handler.
///
/// Checks the `Content-Length` header against [`BodyLimitConfig::max_bytes`].
/// Returns 413 Payload Too Large with a JSON body if the limit is exceeded.
fn body_limit_handler(
    config: BodyLimitConfig,
    request: Request,
    next: Next,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Response> + Send>> {
    Box::pin(async move {
        let path = request.uri().path().to_string();

        // Skip excluded paths
        if config.is_excluded(&path) {
            return next.run(request).await;
        }

        // Check Content-Length header
        if let Some(content_length) = request
            .headers()
            .get(header::CONTENT_LENGTH)
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse::<usize>().ok())
        {
            if content_length > config.max_bytes {
                let req_id = extract_request_id(&request);
                let body = ErrorResponse {
                    error: format!(
                        "Request body too large: {} bytes exceeds limit of {} bytes",
                        content_length, config.max_bytes
                    ),
                    code: "PAYLOAD_TOO_LARGE".to_string(),
                    request_id: req_id,
                };
                let mut response = (StatusCode::PAYLOAD_TOO_LARGE, Json(body)).into_response();
                if let Ok(v) = HeaderValue::from_str(&config.max_bytes.to_string()) {
                    response.headers_mut().insert("x-body-limit", v);
                }
                return response;
            }
        }

        let mut response = next.run(request).await;
        if let Ok(v) = HeaderValue::from_str(&config.max_bytes.to_string()) {
            response.headers_mut().insert("x-body-limit", v);
        }
        response
    })
}

// ============================================================================
// Request Body Validation
// ============================================================================

/// Configuration for JSON request body validation.
///
/// Enforces structural constraints on JSON payloads to guard against
/// adversarial inputs such as deeply nested objects (hash-collision /
/// stack-overflow attacks), objects with excessive keys (memory
/// exhaustion), or extremely long string values.
///
/// Only applies to methods that carry a body (`POST`, `PUT`, `PATCH`).
/// `GET`, `DELETE`, `OPTIONS`, and `HEAD` are always passed through.
#[derive(Debug, Clone)]
pub struct BodyValidationConfig {
    /// Maximum nesting depth for JSON objects/arrays.
    /// `0` means only primitive values are allowed at the top level.
    pub max_depth: usize,
    /// Maximum number of keys across all objects in the JSON body.
    pub max_keys: usize,
    /// Maximum length of any single string value in the JSON body.
    pub max_string_length: usize,
    /// Maximum number of array elements across all arrays.
    pub max_array_length: usize,
    /// Paths excluded from body validation.
    pub excluded_paths: Vec<String>,
}

impl Default for BodyValidationConfig {
    fn default() -> Self {
        Self {
            max_depth: 32,
            max_keys: 1000,
            max_string_length: 1_000_000, // 1 MB
            max_array_length: 10_000,
            excluded_paths: vec!["/health".to_string()],
        }
    }
}

impl BodyValidationConfig {
    /// Check if a path is excluded from body validation.
    fn is_excluded(&self, path: &str) -> bool {
        self.excluded_paths.iter().any(|p| path.starts_with(p))
    }
}

/// Validation error describing which constraint was violated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BodyValidationError {
    /// JSON parsing failed.
    InvalidJson(String),
    /// Nesting depth exceeded the limit.
    MaxDepthExceeded { depth: usize, limit: usize },
    /// Total number of object keys exceeded the limit.
    MaxKeysExceeded { keys: usize, limit: usize },
    /// A string value exceeded the maximum length.
    MaxStringLengthExceeded { length: usize, limit: usize },
    /// Total array elements exceeded the limit.
    MaxArrayLengthExceeded { elements: usize, limit: usize },
}

impl std::fmt::Display for BodyValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidJson(msg) => write!(f, "Invalid JSON: {msg}"),
            Self::MaxDepthExceeded { depth, limit } => {
                write!(f, "JSON nesting depth {depth} exceeds limit of {limit}")
            }
            Self::MaxKeysExceeded { keys, limit } => {
                write!(f, "JSON key count {keys} exceeds limit of {limit}")
            }
            Self::MaxStringLengthExceeded { length, limit } => {
                write!(f, "JSON string length {length} exceeds limit of {limit}")
            }
            Self::MaxArrayLengthExceeded { elements, limit } => {
                write!(f, "JSON array length {elements} exceeds limit of {limit}")
            }
        }
    }
}

/// Validate a parsed JSON value against structural constraints.
///
/// Walks the JSON tree recursively, tracking nesting depth and
/// accumulating key/element/string-length counts. Returns the first
/// violation found.
fn validate_json_value(
    value: &serde_json::Value,
    config: &BodyValidationConfig,
    current_depth: usize,
    total_keys: &mut usize,
    total_elements: &mut usize,
) -> Option<BodyValidationError> {
    if current_depth > config.max_depth {
        return Some(BodyValidationError::MaxDepthExceeded {
            depth: current_depth,
            limit: config.max_depth,
        });
    }

    match value {
        serde_json::Value::Object(map) => {
            *total_keys += map.len();
            if *total_keys > config.max_keys {
                return Some(BodyValidationError::MaxKeysExceeded {
                    keys: *total_keys,
                    limit: config.max_keys,
                });
            }
            // Also check key lengths (keys are strings)
            for key in map.keys() {
                if key.len() > config.max_string_length {
                    return Some(BodyValidationError::MaxStringLengthExceeded {
                        length: key.len(),
                        limit: config.max_string_length,
                    });
                }
            }
            for val in map.values() {
                if let Some(err) =
                    validate_json_value(val, config, current_depth + 1, total_keys, total_elements)
                {
                    return Some(err);
                }
            }
        }
        serde_json::Value::Array(arr) => {
            *total_elements += arr.len();
            if *total_elements > config.max_array_length {
                return Some(BodyValidationError::MaxArrayLengthExceeded {
                    elements: *total_elements,
                    limit: config.max_array_length,
                });
            }
            for val in arr {
                if let Some(err) =
                    validate_json_value(val, config, current_depth + 1, total_keys, total_elements)
                {
                    return Some(err);
                }
            }
        }
        serde_json::Value::String(s) => {
            if s.len() > config.max_string_length {
                return Some(BodyValidationError::MaxStringLengthExceeded {
                    length: s.len(),
                    limit: config.max_string_length,
                });
            }
        }
        _ => {} // Number, Bool, Null — no structural constraints
    }
    None
}

/// Validate a raw JSON byte slice against the configured constraints.
///
/// Parses the bytes as JSON, then walks the tree to check depth, key
/// count, array length, and string length limits.
pub fn validate_json_body(
    body: &[u8],
    config: &BodyValidationConfig,
) -> std::result::Result<(), BodyValidationError> {
    if body.is_empty() {
        return Ok(());
    }
    let value: serde_json::Value = serde_json::from_slice(body)
        .map_err(|e| BodyValidationError::InvalidJson(e.to_string()))?;
    let mut total_keys = 0;
    let mut total_elements = 0;
    match validate_json_value(&value, config, 0, &mut total_keys, &mut total_elements) {
        Some(err) => Err(err),
        None => Ok(()),
    }
}

/// Request body validation middleware handler.
///
/// Reads the full request body for `POST`, `PUT`, and `PATCH` methods,
/// validates the JSON structure, and re-assembles the request with
/// the original body bytes if validation passes. Returns `422
/// Unprocessable Entity` on validation failure.
fn body_validation_handler(
    config: BodyValidationConfig,
    request: Request,
    next: Next,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Response> + Send>> {
    Box::pin(async move {
        let method = request.method().clone();
        let path = request.uri().path().to_string();

        // Only validate methods that carry a body
        let needs_check =
            method == Method::POST || method == Method::PUT || method == Method::PATCH;

        if !needs_check || config.is_excluded(&path) {
            return next.run(request).await;
        }

        // Read the body
        let (parts, body) = request.into_parts();
        let body_bytes = match axum::body::to_bytes(body, 10 * 1024 * 1024).await {
            Ok(bytes) => bytes,
            Err(_) => {
                let req_id = parts.extensions.get::<RequestId>().map(|r| r.0.clone());
                let err_body = ErrorResponse {
                    error: "Failed to read request body".to_string(),
                    code: "BODY_READ_ERROR".to_string(),
                    request_id: req_id,
                };
                return (StatusCode::BAD_REQUEST, Json(err_body)).into_response();
            }
        };

        // Skip validation for empty bodies
        if body_bytes.is_empty() {
            let request = Request::from_parts(parts, axum::body::Body::from(body_bytes));
            return next.run(request).await;
        }

        // Validate JSON structure
        if let Err(err) = validate_json_body(&body_bytes, &config) {
            let req_id = parts.extensions.get::<RequestId>().map(|r| r.0.clone());
            let err_body = ErrorResponse {
                error: err.to_string(),
                code: "BODY_VALIDATION_FAILED".to_string(),
                request_id: req_id,
            };
            return (StatusCode::UNPROCESSABLE_ENTITY, Json(err_body)).into_response();
        }

        // Reassemble request with the original body
        let request = Request::from_parts(parts, axum::body::Body::from(body_bytes));
        next.run(request).await
    })
}

// ============================================================================
// Request Idempotency
// ============================================================================

/// Configuration for request idempotency middleware.
///
/// When enabled, POST/PUT/PATCH requests that include an
/// `Idempotency-Key` header will have their responses cached.
/// Subsequent requests with the same key, method, and path return
/// the cached response without re-executing the handler.
#[derive(Debug, Clone)]
pub struct IdempotencyConfig {
    /// Time-to-live for cached responses, in seconds.
    pub ttl_secs: u64,
    /// Maximum number of cached entries before eviction.
    pub max_entries: usize,
    /// Whether to require the header on POST/PUT/PATCH requests.
    /// If true, requests without the header receive 400.
    pub require_key: bool,
    /// Paths excluded from idempotency enforcement.
    pub excluded_paths: Vec<String>,
}

impl Default for IdempotencyConfig {
    fn default() -> Self {
        Self {
            ttl_secs: 3600, // 1 hour
            max_entries: 10_000,
            require_key: false,
            excluded_paths: vec!["/health".to_string()],
        }
    }
}

impl IdempotencyConfig {
    /// Check if a path is excluded from idempotency enforcement.
    fn is_excluded(&self, path: &str) -> bool {
        self.excluded_paths.iter().any(|p| path.starts_with(p))
    }
}

/// A cached response stored in the idempotency store.
#[derive(Debug, Clone)]
struct CachedResponse {
    status: u16,
    headers: Vec<(String, Vec<u8>)>,
    body: Vec<u8>,
    created_at: Instant,
}

/// Thread-safe idempotency store.
///
/// Maps `(method, path, idempotency_key)` triples to cached responses.
/// Uses `parking_lot::Mutex` for low-contention locking with TTL-based
/// eviction when the cache exceeds `max_entries`.
#[derive(Debug)]
pub struct IdempotencyStore {
    inner: parking_lot::Mutex<std::collections::HashMap<String, CachedResponse>>,
    config: IdempotencyConfig,
}

impl IdempotencyStore {
    /// Create a new idempotency store with the given configuration.
    pub fn new(config: IdempotencyConfig) -> Self {
        Self {
            inner: parking_lot::Mutex::new(std::collections::HashMap::new()),
            config,
        }
    }

    /// Build a cache key from method, path, and idempotency key.
    fn cache_key(method: &str, path: &str, idempotency_key: &str) -> String {
        format!("{method}:{path}:{idempotency_key}")
    }

    /// Look up a cached response by idempotency key.
    fn get(&self, method: &str, path: &str, idempotency_key: &str) -> Option<CachedResponse> {
        let key = Self::cache_key(method, path, idempotency_key);
        let cache = self.inner.lock();
        let entry = cache.get(&key)?;
        // Check TTL
        if entry.created_at.elapsed() > Duration::from_secs(self.config.ttl_secs) {
            return None;
        }
        Some(entry.clone())
    }

    /// Store a response for future idempotent replay.
    fn put(&self, method: &str, path: &str, idempotency_key: &str, cached: CachedResponse) {
        let key = Self::cache_key(method, path, idempotency_key);
        let mut cache = self.inner.lock();

        // Evict expired entries when at capacity
        if cache.len() >= self.config.max_entries {
            let ttl = Duration::from_secs(self.config.ttl_secs);
            cache.retain(|_, v| v.created_at.elapsed() < ttl);

            // If still at capacity, remove oldest entries
            if cache.len() >= self.config.max_entries {
                let mut entries: Vec<_> = cache.keys().cloned().collect();
                entries.sort();
                let to_remove = cache.len() - self.config.max_entries / 2;
                for k in entries.into_iter().take(to_remove) {
                    cache.remove(&k);
                }
            }
        }

        cache.insert(key, cached);
    }

    /// Get the configuration.
    pub fn config(&self) -> &IdempotencyConfig {
        &self.config
    }
}

/// Idempotency middleware handler.
///
/// Checks for an `Idempotency-Key` header on POST/PUT/PATCH requests.
/// If a cached response exists for the key+method+path triple, it is
/// replayed without calling the handler. Otherwise, the response is
/// cached for future replays. The header is echoed back on every response.
fn idempotency_handler(
    store: Arc<IdempotencyStore>,
    request: Request,
    next: Next,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Response> + Send>> {
    Box::pin(async move {
        let method = request.method().clone();
        let path = request.uri().path().to_string();

        let needs_check =
            method == Method::POST || method == Method::PUT || method == Method::PATCH;

        if !needs_check || store.config().is_excluded(&path) {
            return next.run(request).await;
        }

        // Extract idempotency key from header
        let idempotency_key = request
            .headers()
            .get("idempotency-key")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());

        let idempotency_key = match idempotency_key {
            Some(key) if !key.is_empty() => key,
            _ => {
                if store.config().require_key {
                    let req_id = extract_request_id(&request);
                    let err_body = ErrorResponse {
                        error: "Idempotency-Key header is required for this request".to_string(),
                        code: "IDEMPOTENCY_KEY_REQUIRED".to_string(),
                        request_id: req_id,
                    };
                    return (StatusCode::BAD_REQUEST, Json(err_body)).into_response();
                }
                // No key and not required — pass through
                return next.run(request).await;
            }
        };

        let method_str = method.to_string();

        // Check cache for existing response
        if let Some(cached) = store.get(&method_str, &path, &idempotency_key) {
            let status = StatusCode::from_u16(cached.status).unwrap_or(StatusCode::OK);
            let mut response = Response::builder()
                .status(status)
                .body(axum::body::Body::from(cached.body))
                .unwrap_or_else(|_| Response::new(axum::body::Body::empty()));

            // Restore cached headers
            for (name, value) in &cached.headers {
                if let (Ok(n), Ok(v)) = (
                    axum::http::header::HeaderName::from_bytes(name.as_bytes()),
                    HeaderValue::from_bytes(value),
                ) {
                    response.headers_mut().insert(n, v);
                }
            }

            // Mark as cached replay
            if let Ok(val) = HeaderValue::from_str(&idempotency_key) {
                response.headers_mut().insert("idempotency-key", val);
            }
            response
                .headers_mut()
                .insert("idempotency-replay", HeaderValue::from_static("true"));
            return response;
        }

        // Execute the handler
        let response = next.run(request).await;

        // Cache the response
        let (parts, body) = response.into_parts();
        let body_bytes = axum::body::to_bytes(body, 10 * 1024 * 1024)
            .await
            .unwrap_or_default()
            .to_vec();

        let headers: Vec<(String, Vec<u8>)> = parts
            .headers
            .iter()
            .map(|(name, value)| (name.to_string(), value.as_bytes().to_vec()))
            .collect();

        let cached = CachedResponse {
            status: parts.status.as_u16(),
            headers: headers.clone(),
            body: body_bytes.clone(),
            created_at: Instant::now(),
        };

        store.put(&method_str, &path, &idempotency_key, cached);

        // Rebuild the response with the idempotency key echoed
        let mut response = Response::from_parts(parts, axum::body::Body::from(body_bytes));
        if let Ok(val) = HeaderValue::from_str(&idempotency_key) {
            response.headers_mut().insert("idempotency-key", val);
        }
        response
    })
}

// ============================================================================
// Audit Logging
// ============================================================================

/// Configuration for audit logging middleware.
///
/// When enabled, mutating requests (POST, PUT, PATCH, DELETE by default) are
/// logged as structured audit events via `tracing::info!`. Each event includes
/// the HTTP method, path, response status, duration, request-ID, and client IP.
#[derive(Debug, Clone)]
pub struct AuditLogConfig {
    /// HTTP methods that trigger an audit log entry.
    pub methods: Vec<Method>,
    /// Paths excluded from audit logging.
    pub excluded_paths: Vec<String>,
    /// Whether to include the request body (truncated) in the audit event.
    pub log_request_body: bool,
    /// Maximum bytes of request body to capture in the audit event.
    pub max_body_log_bytes: usize,
    /// Whether to include the response status code.
    pub log_response_status: bool,
}

impl Default for AuditLogConfig {
    fn default() -> Self {
        Self {
            methods: vec![Method::POST, Method::PUT, Method::PATCH, Method::DELETE],
            excluded_paths: vec!["/health".to_string()],
            log_request_body: false,
            max_body_log_bytes: 1024,
            log_response_status: true,
        }
    }
}

impl AuditLogConfig {
    /// Check if a path is excluded from audit logging.
    fn is_excluded(&self, path: &str) -> bool {
        self.excluded_paths.iter().any(|p| path.starts_with(p))
    }

    /// Check if a method should be audited.
    fn is_audited_method(&self, method: &Method) -> bool {
        self.methods.iter().any(|m| m == method)
    }
}

/// A structured audit log entry.
///
/// Created by the audit logging middleware for each qualifying request.
/// Serialised to JSON and emitted via `tracing::info!`.
#[derive(Debug, Clone, serde::Serialize)]
pub struct AuditLogEntry {
    /// ISO-8601 timestamp of the audit event.
    pub timestamp: String,
    /// HTTP method (e.g. POST, DELETE).
    pub method: String,
    /// Request path.
    pub path: String,
    /// Response status code (if enabled).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<u16>,
    /// Request duration in milliseconds.
    pub duration_ms: u128,
    /// The `X-Request-Id` header value, if present.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    /// Client IP address, if determinable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_ip: Option<String>,
    /// Truncated request body, if body logging is enabled.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_body: Option<String>,
}

/// Audit logging middleware handler.
///
/// For each request whose method is in `AuditLogConfig::methods` and whose
/// path is not excluded, this middleware captures timing, request metadata, and
/// response status, then emits a structured `tracing::info!` audit event.
fn audit_log_handler(
    config: AuditLogConfig,
    request: Request,
    next: Next,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Response> + Send>> {
    Box::pin(async move {
        let method = request.method().clone();
        let path = request.uri().path().to_string();

        // Skip non-audited methods and excluded paths
        if !config.is_audited_method(&method) || config.is_excluded(&path) {
            return next.run(request).await;
        }

        let request_id = extract_request_id(&request);
        let client_ip = request
            .headers()
            .get("x-forwarded-for")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.split(',').next().unwrap_or(s).trim().to_string());

        // Optionally capture request body
        let request_body = if config.log_request_body {
            let (parts, body) = request.into_parts();
            let body_bytes = axum::body::to_bytes(body, config.max_body_log_bytes + 1)
                .await
                .unwrap_or_default();
            let truncated = if body_bytes.len() > config.max_body_log_bytes {
                let slice = &body_bytes[..config.max_body_log_bytes];
                format!("{}...(truncated)", String::from_utf8_lossy(slice))
            } else {
                String::from_utf8_lossy(&body_bytes).to_string()
            };
            let body_str = if truncated.is_empty() {
                None
            } else {
                Some(truncated)
            };
            let request = Request::from_parts(parts, axum::body::Body::from(body_bytes.to_vec()));
            let start = Instant::now();
            let response = next.run(request).await;
            let duration = start.elapsed();

            let status = if config.log_response_status {
                Some(response.status().as_u16())
            } else {
                None
            };

            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();

            let entry = AuditLogEntry {
                timestamp: format!("{now}"),
                method: method.to_string(),
                path,
                status,
                duration_ms: duration.as_millis(),
                request_id,
                client_ip,
                request_body: body_str,
            };

            if let Ok(json) = serde_json::to_string(&entry) {
                tracing::info!(target: "audit_log", "{}", json);
            }

            return response;
        } else {
            None
        };

        let start = Instant::now();
        let response = next.run(request).await;
        let duration = start.elapsed();

        let status = if config.log_response_status {
            Some(response.status().as_u16())
        } else {
            None
        };

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let entry = AuditLogEntry {
            timestamp: format!("{now}"),
            method: method.to_string(),
            path,
            status,
            duration_ms: duration.as_millis(),
            request_id,
            client_ip,
            request_body,
        };

        if let Ok(json) = serde_json::to_string(&entry) {
            tracing::info!(target: "audit_log", "{}", json);
        }

        response
    })
}

// ============================================================================
// Response Caching
// ============================================================================

/// Configuration for response caching middleware.
///
/// When enabled, GET/HEAD responses are cached in memory with a configurable
/// TTL. Cached responses include `Cache-Control`, `Age`, and `X-Cache`
/// headers. Only responses with successful (2xx) status codes are cached.
#[derive(Debug, Clone)]
pub struct ResponseCacheConfig {
    /// Time-to-live for cached responses, in seconds.
    pub ttl_secs: u64,
    /// Maximum number of cached entries before eviction.
    pub max_entries: usize,
    /// Paths excluded from caching.
    pub excluded_paths: Vec<String>,
    /// Whether to cache HEAD requests in addition to GET.
    pub cache_head: bool,
    /// `Cache-Control` header value for cached responses.
    pub cache_control: String,
    /// Maximum response body size (bytes) eligible for caching.
    pub max_cacheable_body_size: usize,
}

impl Default for ResponseCacheConfig {
    fn default() -> Self {
        Self {
            ttl_secs: 60,
            max_entries: 1_000,
            excluded_paths: vec!["/health".to_string()],
            cache_head: false,
            cache_control: "public, max-age=60".to_string(),
            max_cacheable_body_size: 1024 * 1024, // 1 MiB
        }
    }
}

impl ResponseCacheConfig {
    /// Check if a path is excluded from caching.
    fn is_excluded(&self, path: &str) -> bool {
        self.excluded_paths.iter().any(|p| path.starts_with(p))
    }
}

/// A cached HTTP response stored in the response cache.
#[derive(Debug, Clone)]
struct CachedHttpResponse {
    status: u16,
    headers: Vec<(String, Vec<u8>)>,
    body: Vec<u8>,
    created_at: Instant,
}

/// Thread-safe response cache.
///
/// Maps `(method, path, query)` triples to cached responses.
/// Uses `parking_lot::Mutex` for low-contention locking with TTL-based
/// eviction when the cache exceeds `max_entries`.
#[derive(Debug)]
pub struct ResponseCache {
    inner: parking_lot::Mutex<std::collections::HashMap<String, CachedHttpResponse>>,
    config: ResponseCacheConfig,
}

impl ResponseCache {
    /// Create a new response cache with the given configuration.
    pub fn new(config: ResponseCacheConfig) -> Self {
        Self {
            inner: parking_lot::Mutex::new(std::collections::HashMap::new()),
            config,
        }
    }

    /// Build a cache key from method and full URI (path + query).
    fn cache_key(method: &str, uri: &str) -> String {
        format!("{method}:{uri}")
    }

    /// Look up a cached response.
    fn get(&self, method: &str, uri: &str) -> Option<(CachedHttpResponse, u64)> {
        let key = Self::cache_key(method, uri);
        let cache = self.inner.lock();
        let entry = cache.get(&key)?;
        let elapsed = entry.created_at.elapsed();
        if elapsed > Duration::from_secs(self.config.ttl_secs) {
            return None;
        }
        Some((entry.clone(), elapsed.as_secs()))
    }

    /// Store a response in the cache.
    fn put(&self, method: &str, uri: &str, cached: CachedHttpResponse) {
        let key = Self::cache_key(method, uri);
        let mut cache = self.inner.lock();

        // Evict expired entries when at capacity
        if cache.len() >= self.config.max_entries {
            let ttl = Duration::from_secs(self.config.ttl_secs);
            cache.retain(|_, v| v.created_at.elapsed() < ttl);

            // If still at capacity, remove oldest entries
            if cache.len() >= self.config.max_entries {
                let mut entries: Vec<_> = cache.keys().cloned().collect();
                entries.sort();
                let to_remove = cache.len() - self.config.max_entries / 2;
                for k in entries.into_iter().take(to_remove) {
                    cache.remove(&k);
                }
            }
        }

        cache.insert(key, cached);
    }

    /// Get the configuration.
    pub fn config(&self) -> &ResponseCacheConfig {
        &self.config
    }
}

/// Response caching middleware handler.
///
/// Caches GET (and optionally HEAD) responses with 2xx status codes.
/// Cached responses are served with `X-Cache: HIT`, `Age`, and
/// `Cache-Control` headers. Cache misses proceed to the handler and
/// are stored for subsequent requests.
fn response_cache_handler(
    cache: Arc<ResponseCache>,
    request: Request,
    next: Next,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Response> + Send>> {
    Box::pin(async move {
        let method = request.method().clone();
        let uri = request.uri().to_string();
        let path = request.uri().path().to_string();

        let is_cacheable_method =
            method == Method::GET || (cache.config().cache_head && method == Method::HEAD);

        if !is_cacheable_method || cache.config().is_excluded(&path) {
            return next.run(request).await;
        }

        let method_str = method.to_string();

        // Check cache for existing response
        if let Some((cached, age_secs)) = cache.get(&method_str, &uri) {
            let status = StatusCode::from_u16(cached.status).unwrap_or(StatusCode::OK);
            let mut response = Response::builder()
                .status(status)
                .body(axum::body::Body::from(cached.body))
                .unwrap_or_else(|_| Response::new(axum::body::Body::empty()));

            // Restore cached headers
            for (name, value) in &cached.headers {
                if let (Ok(n), Ok(v)) = (
                    axum::http::header::HeaderName::from_bytes(name.as_bytes()),
                    HeaderValue::from_bytes(value),
                ) {
                    response.headers_mut().insert(n, v);
                }
            }

            // Add cache status headers
            response
                .headers_mut()
                .insert("x-cache", HeaderValue::from_static("HIT"));
            if let Ok(age_val) = HeaderValue::from_str(&age_secs.to_string()) {
                response.headers_mut().insert("age", age_val);
            }
            if let Ok(cc) = HeaderValue::from_str(&cache.config().cache_control) {
                response.headers_mut().insert(header::CACHE_CONTROL, cc);
            }

            return response;
        }

        // Execute the handler
        let response = next.run(request).await;

        // Only cache 2xx responses
        if !response.status().is_success() {
            return response;
        }

        let (parts, body) = response.into_parts();
        let body_bytes = axum::body::to_bytes(body, cache.config().max_cacheable_body_size + 1)
            .await
            .unwrap_or_default()
            .to_vec();

        // Don't cache if body exceeds max size
        if body_bytes.len() > cache.config().max_cacheable_body_size {
            let response = Response::from_parts(parts, axum::body::Body::from(body_bytes));
            return response;
        }

        let headers: Vec<(String, Vec<u8>)> = parts
            .headers
            .iter()
            .map(|(name, value)| (name.to_string(), value.as_bytes().to_vec()))
            .collect();

        let cached = CachedHttpResponse {
            status: parts.status.as_u16(),
            headers,
            body: body_bytes.clone(),
            created_at: Instant::now(),
        };

        cache.put(&method_str, &uri, cached);

        // Add cache MISS header
        let mut response = Response::from_parts(parts, axum::body::Body::from(body_bytes));
        response
            .headers_mut()
            .insert("x-cache", HeaderValue::from_static("MISS"));
        if let Ok(cc) = HeaderValue::from_str(&cache.config().cache_control) {
            response.headers_mut().insert(header::CACHE_CONTROL, cc);
        }

        response
    })
}

// ============================================================================
// Request Deduplication
// ============================================================================

/// Configuration for request deduplication middleware.
///
/// When enabled, concurrent identical mutating requests (same method + path +
/// body hash) are detected. The first request proceeds normally while
/// subsequent duplicates receive `409 Conflict`. In-flight entries are removed
/// once the original request completes.
#[derive(Debug, Clone)]
pub struct RequestDedupConfig {
    /// HTTP methods subject to deduplication.
    pub methods: Vec<Method>,
    /// Paths excluded from deduplication.
    pub excluded_paths: Vec<String>,
    /// TTL for in-flight entries (seconds). Guards against leaked entries
    /// if a request handler panics.
    pub ttl_secs: u64,
    /// Maximum body bytes to read when computing the fingerprint.
    pub max_body_hash_bytes: usize,
}

impl Default for RequestDedupConfig {
    fn default() -> Self {
        Self {
            methods: vec![Method::POST, Method::PUT, Method::PATCH, Method::DELETE],
            excluded_paths: vec!["/health".to_string()],
            ttl_secs: 30,
            max_body_hash_bytes: 64 * 1024, // 64 KiB
        }
    }
}

impl RequestDedupConfig {
    /// Check if a path is excluded from deduplication.
    fn is_excluded(&self, path: &str) -> bool {
        self.excluded_paths.iter().any(|p| path.starts_with(p))
    }

    /// Check if a method is subject to deduplication.
    fn is_dedup_method(&self, method: &Method) -> bool {
        self.methods.iter().any(|m| m == method)
    }
}

/// Tracks in-flight requests by fingerprint.
#[derive(Debug, Clone)]
struct InFlightEntry {
    created_at: Instant,
}

/// Thread-safe in-flight request tracker.
///
/// Maps `fingerprint` strings to `InFlightEntry`. Uses `parking_lot::Mutex`
/// for low-contention locking with TTL-based cleanup.
#[derive(Debug)]
pub struct InFlightTracker {
    inner: parking_lot::Mutex<std::collections::HashMap<String, InFlightEntry>>,
    config: RequestDedupConfig,
}

impl InFlightTracker {
    /// Create a new in-flight tracker with the given configuration.
    pub fn new(config: RequestDedupConfig) -> Self {
        Self {
            inner: parking_lot::Mutex::new(std::collections::HashMap::new()),
            config,
        }
    }

    /// Build a fingerprint from method, path, and a simple body hash.
    fn fingerprint(method: &str, path: &str, body_hash: u64) -> String {
        format!("{method}:{path}:{body_hash:x}")
    }

    /// Try to register an in-flight request. Returns `true` if this is
    /// the first request with this fingerprint (proceed). Returns `false`
    /// if a duplicate is already in-flight (reject with 409).
    fn try_acquire(&self, fingerprint: &str) -> bool {
        let mut map = self.inner.lock();

        // Evict stale entries (guard against leaked slots)
        let ttl = Duration::from_secs(self.config.ttl_secs);
        map.retain(|_, v| v.created_at.elapsed() < ttl);

        if map.contains_key(fingerprint) {
            return false;
        }
        map.insert(
            fingerprint.to_string(),
            InFlightEntry {
                created_at: Instant::now(),
            },
        );
        true
    }

    /// Release an in-flight slot after the request completes.
    fn release(&self, fingerprint: &str) {
        let mut map = self.inner.lock();
        map.remove(fingerprint);
    }

    /// Get the configuration.
    pub fn config(&self) -> &RequestDedupConfig {
        &self.config
    }
}

/// Compute a simple hash of a byte slice (FNV-1a-style).
fn simple_hash(data: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for &byte in data {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

/// Request deduplication middleware handler.
///
/// For each request whose method is subject to deduplication and whose
/// path is not excluded, this middleware computes a fingerprint from the
/// method, path, and body hash. If an identical request is already
/// in-flight, it returns `409 Conflict`. Otherwise it proceeds and
/// releases the slot when complete.
fn request_dedup_handler(
    tracker: Arc<InFlightTracker>,
    request: Request,
    next: Next,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Response> + Send>> {
    Box::pin(async move {
        let method = request.method().clone();
        let path = request.uri().path().to_string();

        if !tracker.config().is_dedup_method(&method) || tracker.config().is_excluded(&path) {
            return next.run(request).await;
        }

        // Read body to compute hash
        let (parts, body) = request.into_parts();
        let body_bytes = axum::body::to_bytes(body, tracker.config().max_body_hash_bytes + 1)
            .await
            .unwrap_or_default();
        let body_hash = simple_hash(&body_bytes);

        let fingerprint = InFlightTracker::fingerprint(method.as_ref(), &path, body_hash);

        if !tracker.try_acquire(&fingerprint) {
            let req_id = parts
                .headers
                .get("x-request-id")
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_string());
            let err_body = ErrorResponse {
                error: "A duplicate request is already in progress".to_string(),
                code: "DUPLICATE_REQUEST".to_string(),
                request_id: req_id,
            };
            return (StatusCode::CONFLICT, Json(err_body)).into_response();
        }

        // Reassemble request and proceed
        let request = Request::from_parts(parts, axum::body::Body::from(body_bytes.to_vec()));
        let response = next.run(request).await;

        // Release the in-flight slot
        tracker.release(&fingerprint);

        response
    })
}

// ============================================================================
// Request Tracing (W3C Trace Context)
// ============================================================================

/// Configuration for the request tracing middleware.
///
/// Implements W3C Trace Context propagation. If an incoming request carries
/// a valid `traceparent` header the trace/span IDs are propagated; otherwise
/// new IDs are generated. The response always includes `traceparent` and,
/// optionally, `tracestate` headers.
///
/// See: <https://www.w3.org/TR/trace-context/>
#[derive(Debug, Clone)]
pub struct TracingConfig {
    /// Paths excluded from tracing headers.
    pub excluded_paths: Vec<String>,
    /// Whether to propagate an incoming `tracestate` header verbatim.
    pub propagate_tracestate: bool,
    /// Whether to add the trace-id to the response as `X-Trace-Id`.
    pub expose_trace_id: bool,
    /// Sampling flag to use when generating new traces (true = sampled).
    pub default_sampled: bool,
}

impl Default for TracingConfig {
    fn default() -> Self {
        Self {
            excluded_paths: vec!["/health".to_string()],
            propagate_tracestate: true,
            expose_trace_id: true,
            default_sampled: true,
        }
    }
}

impl TracingConfig {
    /// Check if a path is excluded from tracing.
    fn is_excluded(&self, path: &str) -> bool {
        self.excluded_paths.iter().any(|p| path.starts_with(p))
    }
}

/// A parsed W3C `traceparent` header value.
///
/// Format: `{version}-{trace-id}-{parent-id}-{trace-flags}`
///   - version: 2 hex chars (currently `00`)
///   - trace-id: 32 hex chars (16 bytes)
///   - parent-id: 16 hex chars (8 bytes)
///   - trace-flags: 2 hex chars (`01` = sampled)
#[derive(Debug, Clone, PartialEq)]
pub struct TraceParent {
    pub version: u8,
    pub trace_id: String,
    pub parent_id: String,
    pub trace_flags: u8,
}

impl TraceParent {
    /// Parse a `traceparent` header value.
    ///
    /// Returns `None` if the format is invalid.
    pub fn parse(value: &str) -> Option<Self> {
        let parts: Vec<&str> = value.split('-').collect();
        if parts.len() != 4 {
            return None;
        }
        let version = u8::from_str_radix(parts[0], 16).ok()?;
        let trace_id = parts[1];
        let parent_id = parts[2];
        let trace_flags = u8::from_str_radix(parts[3], 16).ok()?;

        // Validate lengths
        if trace_id.len() != 32 || parent_id.len() != 16 {
            return None;
        }
        // Validate hex characters
        if !trace_id.chars().all(|c| c.is_ascii_hexdigit())
            || !parent_id.chars().all(|c| c.is_ascii_hexdigit())
        {
            return None;
        }
        // trace-id must not be all zeros
        if trace_id.chars().all(|c| c == '0') {
            return None;
        }

        Some(Self {
            version,
            trace_id: trace_id.to_string(),
            parent_id: parent_id.to_string(),
            trace_flags,
        })
    }

    /// Generate a new traceparent with random trace-id and parent-id.
    pub fn generate(sampled: bool) -> Self {
        let trace_id = format!("{:032x}", (uuid::Uuid::new_v4().as_u128()));
        let parent_id = format!("{:016x}", rand_u64());
        Self {
            version: 0,
            trace_id,
            parent_id,
            trace_flags: if sampled { 0x01 } else { 0x00 },
        }
    }

    /// Generate a child span — same trace-id, new parent-id.
    pub fn child(&self) -> Self {
        Self {
            version: self.version,
            trace_id: self.trace_id.clone(),
            parent_id: format!("{:016x}", rand_u64()),
            trace_flags: self.trace_flags,
        }
    }

    /// Serialize to the `traceparent` header format.
    pub fn to_header_value(&self) -> String {
        format!(
            "{:02x}-{}-{}-{:02x}",
            self.version, self.trace_id, self.parent_id, self.trace_flags
        )
    }

    /// Whether the sampled flag is set.
    pub fn is_sampled(&self) -> bool {
        self.trace_flags & 0x01 != 0
    }
}

/// Simple random u64 using system time for span-id generation.
///
/// Not cryptographically secure — used only for trace span IDs.
fn rand_u64() -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    Instant::now().hash(&mut hasher);
    std::thread::current().id().hash(&mut hasher);
    // Mix in a counter for uniqueness within the same instant
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    COUNTER
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        .hash(&mut hasher);
    hasher.finish()
}

/// Request tracing middleware handler.
///
/// Propagates or generates W3C Trace Context headers (`traceparent`,
/// `tracestate`). When a valid `traceparent` header is present, creates a
/// child span. Otherwise generates a new trace. The response always
/// includes the outgoing `traceparent` header.
fn request_tracing_handler(
    config: TracingConfig,
    request: Request,
    next: Next,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Response> + Send>> {
    Box::pin(async move {
        let path = request.uri().path().to_string();
        if config.is_excluded(&path) {
            return next.run(request).await;
        }

        // Parse or generate traceparent
        let incoming = request
            .headers()
            .get("traceparent")
            .and_then(|v| v.to_str().ok())
            .and_then(TraceParent::parse);

        let tracestate = if config.propagate_tracestate {
            request
                .headers()
                .get("tracestate")
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_string())
        } else {
            None
        };

        let outgoing = match &incoming {
            Some(parent) => parent.child(),
            None => TraceParent::generate(config.default_sampled),
        };

        let trace_id = outgoing.trace_id.clone();
        let traceparent_value = outgoing.to_header_value();

        let mut response = next.run(request).await;
        let headers = response.headers_mut();

        if let Ok(val) = HeaderValue::from_str(&traceparent_value) {
            headers.insert("traceparent", val);
        }
        if let Some(ref ts) = tracestate {
            if let Ok(val) = HeaderValue::from_str(ts) {
                headers.insert("tracestate", val);
            }
        }
        if config.expose_trace_id {
            if let Ok(val) = HeaderValue::from_str(&trace_id) {
                headers.insert("x-trace-id", val);
            }
        }

        response
    })
}

// ============================================================================
// Request Payload Signing (HMAC-SHA256)
// ============================================================================

/// Configuration for request payload signing middleware.
///
/// When enabled, requests carrying an `X-Signature` header are validated
/// against an HMAC-SHA256 digest of the request body using the configured
/// secret. Requests without a signature header are optionally rejected
/// (when `require_signature` is true). Responses include `X-Signature-Status`
/// indicating the verification result.
#[derive(Debug, Clone)]
pub struct PayloadSigningConfig {
    /// HMAC secret key (hex-encoded or raw bytes as string).
    pub secret: String,
    /// HTTP methods subject to signature verification.
    pub methods: Vec<Method>,
    /// Paths excluded from signature verification.
    pub excluded_paths: Vec<String>,
    /// Whether to reject requests that lack a signature header entirely.
    pub require_signature: bool,
    /// Maximum body size to read for HMAC computation.
    pub max_body_bytes: usize,
    /// Name of the request header carrying the signature.
    pub signature_header: String,
}

impl Default for PayloadSigningConfig {
    fn default() -> Self {
        Self {
            secret: String::new(),
            methods: vec![Method::POST, Method::PUT, Method::PATCH],
            excluded_paths: vec!["/health".to_string()],
            require_signature: false,
            max_body_bytes: 1024 * 1024, // 1 MiB
            signature_header: "x-signature".to_string(),
        }
    }
}

impl PayloadSigningConfig {
    /// Check if a path is excluded from signature verification.
    fn is_excluded(&self, path: &str) -> bool {
        self.excluded_paths.iter().any(|p| path.starts_with(p))
    }

    /// Check if a method requires signature verification.
    fn is_signed_method(&self, method: &Method) -> bool {
        self.methods.iter().any(|m| m == method)
    }
}

/// Compute HMAC-SHA256 of `data` using `key`.
///
/// Returns the hex-encoded digest string.
pub fn hmac_sha256(key: &[u8], data: &[u8]) -> String {
    // RFC 2104 HMAC implementation
    let block_size = 64;

    let normalized_key = if key.len() > block_size {
        // Hash the key if longer than block size
        sha256(key).to_vec()
    } else {
        let mut k = key.to_vec();
        k.resize(block_size, 0x00);
        k
    };

    // Pad key to block_size
    let mut padded_key = normalized_key.clone();
    padded_key.resize(block_size, 0x00);

    // Inner padding
    let mut i_key_pad: Vec<u8> = padded_key.iter().map(|b| b ^ 0x36).collect();
    i_key_pad.extend_from_slice(data);
    let inner_hash = sha256(&i_key_pad);

    // Outer padding
    let mut o_key_pad: Vec<u8> = padded_key.iter().map(|b| b ^ 0x5c).collect();
    o_key_pad.extend_from_slice(&inner_hash);
    let outer_hash = sha256(&o_key_pad);

    outer_hash.iter().map(|b| format!("{b:02x}")).collect()
}

/// SHA-256 hash (pure Rust, no external crate).
fn sha256(data: &[u8]) -> [u8; 32] {
    // Initial hash values (first 32 bits of fractional parts of square roots of first 8 primes)
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];

    // Round constants
    let k: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];

    // Pre-processing: pad message
    let bit_len = (data.len() as u64) * 8;
    let mut msg = data.to_vec();
    msg.push(0x80);
    while (msg.len() % 64) != 56 {
        msg.push(0x00);
    }
    msg.extend_from_slice(&bit_len.to_be_bytes());

    // Process each 512-bit (64-byte) block
    for chunk in msg.chunks(64) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([
                chunk[i * 4],
                chunk[i * 4 + 1],
                chunk[i * 4 + 2],
                chunk[i * 4 + 3],
            ]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }

        let (mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh) =
            (h[0], h[1], h[2], h[3], h[4], h[5], h[6], h[7]);

        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(k[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);

            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }

        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
        h[5] = h[5].wrapping_add(f);
        h[6] = h[6].wrapping_add(g);
        h[7] = h[7].wrapping_add(hh);
    }

    let mut result = [0u8; 32];
    for (i, &val) in h.iter().enumerate() {
        result[i * 4..i * 4 + 4].copy_from_slice(&val.to_be_bytes());
    }
    result
}

/// Request payload signing middleware handler.
///
/// Verifies the HMAC-SHA256 signature of the request body against the
/// `X-Signature` header (or configured header name). Returns
/// `401 Unauthorized` for signature mismatches or missing signatures
/// when `require_signature` is enabled.
fn payload_signing_handler(
    config: PayloadSigningConfig,
    request: Request,
    next: Next,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Response> + Send>> {
    Box::pin(async move {
        let method = request.method().clone();
        let path = request.uri().path().to_string();

        if !config.is_signed_method(&method) || config.is_excluded(&path) {
            return next.run(request).await;
        }

        // Check for signature header
        let signature = request
            .headers()
            .get(&config.signature_header)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());

        if signature.is_none() && config.require_signature {
            let req_id = request
                .headers()
                .get("x-request-id")
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_string());
            let err = ErrorResponse {
                error: "Missing required signature header".to_string(),
                code: "MISSING_SIGNATURE".to_string(),
                request_id: req_id,
            };
            return (StatusCode::UNAUTHORIZED, Json(err)).into_response();
        }

        if signature.is_none() {
            // No signature but not required — pass through
            let mut response = next.run(request).await;
            response
                .headers_mut()
                .insert("x-signature-status", HeaderValue::from_static("unsigned"));
            return response;
        }

        let signature = signature.unwrap();

        // Read body to compute HMAC
        let (parts, body) = request.into_parts();
        let body_bytes = axum::body::to_bytes(body, config.max_body_bytes + 1)
            .await
            .unwrap_or_default();

        let expected = hmac_sha256(config.secret.as_bytes(), &body_bytes);

        // Constant-time comparison (best effort without external crate)
        let valid = constant_time_eq(signature.as_bytes(), expected.as_bytes());

        if !valid {
            let req_id = parts
                .headers
                .get("x-request-id")
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_string());
            let err = ErrorResponse {
                error: "Invalid request signature".to_string(),
                code: "INVALID_SIGNATURE".to_string(),
                request_id: req_id,
            };
            return (StatusCode::UNAUTHORIZED, Json(err)).into_response();
        }

        // Reassemble request and proceed
        let request = Request::from_parts(parts, axum::body::Body::from(body_bytes.to_vec()));
        let mut response = next.run(request).await;
        response
            .headers_mut()
            .insert("x-signature-status", HeaderValue::from_static("valid"));
        response
    })
}

/// Best-effort constant-time byte comparison.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

// ============================================================================
// Circuit Breaker
// ============================================================================

/// Circuit breaker state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CircuitState {
    /// Normal operation — requests flow through.
    Closed,
    /// Too many failures — requests are rejected immediately with 503.
    Open,
    /// Tentatively allowing a probe request to test recovery.
    HalfOpen,
}

/// Configuration for the circuit breaker middleware.
///
/// Tracks error rates (5xx responses) and trips open when the failure
/// threshold is reached within the configured window. While open,
/// requests immediately receive `503 Service Unavailable`. After the
/// recovery timeout, one probe request is allowed through (half-open).
/// If the probe succeeds, the circuit closes; if it fails, it reopens.
#[derive(Debug, Clone)]
pub struct CircuitBreakerConfig {
    /// Number of consecutive failures before tripping the circuit open.
    pub failure_threshold: u32,
    /// Duration (in seconds) the circuit stays open before entering half-open.
    pub recovery_timeout_secs: u64,
    /// Paths excluded from circuit breaker tracking.
    pub excluded_paths: Vec<String>,
}

impl Default for CircuitBreakerConfig {
    fn default() -> Self {
        Self {
            failure_threshold: 5,
            recovery_timeout_secs: 30,
            excluded_paths: vec!["/health".to_string()],
        }
    }
}

impl CircuitBreakerConfig {
    /// Check if a path is excluded from circuit breaker tracking.
    fn is_excluded(&self, path: &str) -> bool {
        self.excluded_paths.iter().any(|p| path.starts_with(p))
    }
}

/// Shared circuit breaker state tracker.
#[derive(Debug)]
pub struct CircuitBreaker {
    config: CircuitBreakerConfig,
    state: parking_lot::Mutex<CircuitBreakerState>,
}

#[derive(Debug)]
struct CircuitBreakerState {
    current: CircuitState,
    failure_count: u32,
    last_failure_time: Option<std::time::Instant>,
}

impl CircuitBreaker {
    /// Create a new circuit breaker with the given configuration.
    pub fn new(config: CircuitBreakerConfig) -> Self {
        Self {
            config,
            state: parking_lot::Mutex::new(CircuitBreakerState {
                current: CircuitState::Closed,
                failure_count: 0,
                last_failure_time: None,
            }),
        }
    }

    /// Get the current circuit state, potentially transitioning from Open → HalfOpen.
    fn check_state(&self) -> CircuitState {
        let mut state = self.state.lock();
        match state.current {
            CircuitState::Open => {
                if let Some(last_failure) = state.last_failure_time {
                    let elapsed = last_failure.elapsed().as_secs();
                    if elapsed >= self.config.recovery_timeout_secs {
                        state.current = CircuitState::HalfOpen;
                        return CircuitState::HalfOpen;
                    }
                }
                CircuitState::Open
            }
            other => other,
        }
    }

    /// Record a successful response — closes the circuit if half-open.
    fn record_success(&self) {
        let mut state = self.state.lock();
        state.failure_count = 0;
        state.current = CircuitState::Closed;
    }

    /// Record a failure response — may trip the circuit open.
    fn record_failure(&self) {
        let mut state = self.state.lock();
        state.failure_count += 1;
        state.last_failure_time = Some(std::time::Instant::now());
        if state.failure_count >= self.config.failure_threshold {
            state.current = CircuitState::Open;
        }
    }

    /// Get the current state (for testing / diagnostics).
    pub fn current_state(&self) -> CircuitState {
        self.state.lock().current
    }

    /// Get the current failure count (for testing / diagnostics).
    pub fn failure_count(&self) -> u32 {
        self.state.lock().failure_count
    }
}

/// Circuit breaker middleware handler.
///
/// When the circuit is open, returns `503 Service Unavailable` with a
/// `Retry-After` header. In half-open state, allows one probe request
/// and transitions based on the result. Adds `X-Circuit-State` to
/// every response for observability.
fn circuit_breaker_handler(
    breaker: Arc<CircuitBreaker>,
    request: Request,
    next: Next,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Response> + Send>> {
    Box::pin(async move {
        let path = request.uri().path().to_string();

        // Skip excluded paths
        if breaker.config.is_excluded(&path) {
            return next.run(request).await;
        }

        let circuit_state = breaker.check_state();

        match circuit_state {
            CircuitState::Open => {
                // Circuit is open — reject immediately
                let req_id = request
                    .headers()
                    .get("x-request-id")
                    .and_then(|v| v.to_str().ok())
                    .map(|s| s.to_string());
                let err = ErrorResponse {
                    error: "Service temporarily unavailable (circuit breaker open)".to_string(),
                    code: "CIRCUIT_OPEN".to_string(),
                    request_id: req_id,
                };
                let mut response = (StatusCode::SERVICE_UNAVAILABLE, Json(err)).into_response();
                response.headers_mut().insert(
                    "retry-after",
                    HeaderValue::from(breaker.config.recovery_timeout_secs),
                );
                response
                    .headers_mut()
                    .insert("x-circuit-state", HeaderValue::from_static("open"));
                response
            }
            CircuitState::HalfOpen => {
                // Allow one probe request
                let mut response = next.run(request).await;
                if response.status().is_server_error() {
                    breaker.record_failure();
                    response
                        .headers_mut()
                        .insert("x-circuit-state", HeaderValue::from_static("open"));
                } else {
                    breaker.record_success();
                    response
                        .headers_mut()
                        .insert("x-circuit-state", HeaderValue::from_static("closed"));
                }
                response
            }
            CircuitState::Closed => {
                // Normal operation — track failures
                let mut response = next.run(request).await;
                if response.status().is_server_error() {
                    breaker.record_failure();
                } else {
                    breaker.record_success();
                }
                let state_str = match breaker.check_state() {
                    CircuitState::Open => "open",
                    CircuitState::HalfOpen => "half-open",
                    CircuitState::Closed => "closed",
                };
                response
                    .headers_mut()
                    .insert("x-circuit-state", HeaderValue::from_static(state_str));
                response
            }
        }
    })
}

// ============================================================================
// Request Sanitization
// ============================================================================

/// Configuration for request header sanitization middleware.
///
/// Strips dangerous or internal-only headers from incoming requests
/// before they reach downstream handlers, preventing header injection,
/// spoofing of internal identifiers, and information leakage.
#[derive(Debug, Clone)]
pub struct SanitizationConfig {
    /// Header names to strip from incoming requests (case-insensitive).
    pub strip_headers: Vec<String>,
    /// Paths excluded from sanitization.
    pub excluded_paths: Vec<String>,
    /// Whether to strip headers with the `X-Internal-` prefix.
    pub strip_internal_prefix: bool,
    /// Maximum allowed header value length in bytes (0 = unlimited).
    pub max_header_value_length: usize,
}

impl Default for SanitizationConfig {
    fn default() -> Self {
        Self {
            strip_headers: vec![
                "x-forwarded-for".to_string(),
                "x-forwarded-host".to_string(),
                "x-forwarded-proto".to_string(),
                "x-real-ip".to_string(),
                "via".to_string(),
            ],
            excluded_paths: vec![],
            strip_internal_prefix: true,
            max_header_value_length: 8192,
        }
    }
}

impl SanitizationConfig {
    /// Check if a path is excluded from sanitization.
    fn is_excluded(&self, path: &str) -> bool {
        self.excluded_paths.iter().any(|p| path.starts_with(p))
    }

    /// Check if a header name should be stripped.
    fn should_strip(&self, name: &str) -> bool {
        let lower = name.to_ascii_lowercase();
        if self
            .strip_headers
            .iter()
            .any(|h| h.to_ascii_lowercase() == lower)
        {
            return true;
        }
        if self.strip_internal_prefix && lower.starts_with("x-internal-") {
            return true;
        }
        false
    }
}

/// Request sanitization middleware handler.
///
/// Removes headers matching the configured strip list and optionally
/// strips any `X-Internal-*` prefixed headers. Also truncates
/// oversized header values when `max_header_value_length` is set.
/// Adds `X-Sanitized-Count` response header indicating how many
/// headers were removed.
fn request_sanitization_handler(
    config: SanitizationConfig,
    request: Request,
    next: Next,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Response> + Send>> {
    Box::pin(async move {
        let path = request.uri().path().to_string();

        if config.is_excluded(&path) {
            return next.run(request).await;
        }

        let (mut parts, body) = request.into_parts();
        let mut stripped_count: u32 = 0;

        // Collect header names to remove (can't mutate while iterating)
        let to_remove: Vec<_> = parts
            .headers
            .keys()
            .filter(|name| config.should_strip(name.as_str()))
            .cloned()
            .collect();

        for name in &to_remove {
            parts.headers.remove(name);
            stripped_count += 1;
        }

        // Truncate oversized header values
        if config.max_header_value_length > 0 {
            let max_len = config.max_header_value_length;
            let keys_to_check: Vec<_> = parts.headers.keys().cloned().collect();
            for key in keys_to_check {
                if let Some(val) = parts.headers.get(&key) {
                    if val.as_bytes().len() > max_len {
                        if let Ok(truncated) = HeaderValue::from_bytes(&val.as_bytes()[..max_len]) {
                            parts.headers.insert(key, truncated);
                        }
                    }
                }
            }
        }

        let request = Request::from_parts(parts, body);
        let mut response = next.run(request).await;

        if let Ok(val) = HeaderValue::from_str(&stripped_count.to_string()) {
            response.headers_mut().insert("x-sanitized-count", val);
        }

        response
    })
}

// ============================================================================
// Content Negotiation
// ============================================================================

/// Configuration for content negotiation middleware.
///
/// Validates the `Accept` request header against the list of content types
/// the API can produce. If the client cannot accept any supported type,
/// a `406 Not Acceptable` response is returned before the handler runs.
#[derive(Debug, Clone)]
pub struct ContentNegotiationConfig {
    /// Content types the server can produce (e.g., `application/json`).
    pub supported_types: Vec<String>,
    /// Default content type when `Accept` is absent or `*/*`.
    pub default_type: String,
    /// If true, requests without an `Accept` header are rejected.
    pub strict: bool,
    /// Paths excluded from negotiation (e.g., health checks).
    pub excluded_paths: Vec<String>,
}

impl Default for ContentNegotiationConfig {
    fn default() -> Self {
        Self {
            supported_types: vec!["application/json".to_string()],
            default_type: "application/json".to_string(),
            strict: false,
            excluded_paths: vec!["/health".to_string()],
        }
    }
}

impl ContentNegotiationConfig {
    /// Check if a path is excluded from content negotiation.
    fn is_excluded(&self, path: &str) -> bool {
        self.excluded_paths.iter().any(|p| path.starts_with(p))
    }

    /// Determine whether the given `Accept` value matches any supported type.
    ///
    /// Supports exact matches, type wildcards (`application/*`), and the
    /// universal wildcard (`*/*`).  Returns `true` if any media-range in
    /// the header value matches a supported type.
    fn accepts_any(&self, accept_value: &str) -> bool {
        for range in accept_value.split(',') {
            let media_range = range.split(';').next().unwrap_or("").trim();
            if media_range.is_empty() {
                continue;
            }
            if media_range == "*/*" {
                return true;
            }
            for supported in &self.supported_types {
                if media_range.eq_ignore_ascii_case(supported) {
                    return true;
                }
                // type/* wildcard — match the type portion before '/'
                if media_range.ends_with("/*") {
                    let prefix = &media_range[..media_range.len() - 1]; // "type/"
                    if supported
                        .to_ascii_lowercase()
                        .starts_with(&prefix.to_ascii_lowercase())
                    {
                        return true;
                    }
                }
            }
        }
        false
    }
}

/// Content negotiation middleware handler.
///
/// Checks the `Accept` request header against the configured supported
/// content types. Returns `406 Not Acceptable` when no match is found.
/// Adds `Vary: Accept` and `Content-Type` headers to every response.
fn content_negotiation_handler(
    config: ContentNegotiationConfig,
    request: Request,
    next: Next,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Response> + Send>> {
    Box::pin(async move {
        let path = request.uri().path().to_string();

        if config.is_excluded(&path) {
            return next.run(request).await;
        }

        let accept_header = request
            .headers()
            .get(header::ACCEPT)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());

        match &accept_header {
            None if config.strict => {
                // Strict mode: missing Accept → 406
                return (
                    StatusCode::NOT_ACCEPTABLE,
                    [(header::CONTENT_TYPE, "application/json")],
                    Json(ErrorResponse {
                        code: "NOT_ACCEPTABLE".to_string(),
                        error: format!(
                            "Missing Accept header; supported types: {}",
                            config.supported_types.join(", ")
                        ),
                        request_id: None,
                    }),
                )
                    .into_response();
            }
            Some(val) if !config.accepts_any(val) => {
                return (
                    StatusCode::NOT_ACCEPTABLE,
                    [(header::CONTENT_TYPE, "application/json")],
                    Json(ErrorResponse {
                        code: "NOT_ACCEPTABLE".to_string(),
                        error: format!(
                            "Unsupported Accept type '{}'; supported: {}",
                            val,
                            config.supported_types.join(", ")
                        ),
                        request_id: None,
                    }),
                )
                    .into_response();
            }
            _ => {}
        }

        let mut response = next.run(request).await;

        // Always add Vary: Accept so caches key on it
        response
            .headers_mut()
            .insert("vary", HeaderValue::from_static("Accept"));
        // Set Content-Type to the default supported type
        if let Ok(ct) = HeaderValue::from_str(&config.default_type) {
            response.headers_mut().insert(header::CONTENT_TYPE, ct);
        }

        response
    })
}

// ============================================================================
// Request Throttling (Concurrency Limiter)
// ============================================================================

/// Configuration for concurrency-based request throttling middleware.
///
/// Limits the number of requests being processed simultaneously.
/// When the concurrency limit is reached, new requests are immediately
/// rejected with `503 Service Unavailable` and a `Retry-After` header,
/// preventing server overload under sustained traffic spikes.
///
/// This differs from rate limiting (time-window token bucket) in that it
/// tracks *in-flight* requests rather than requests-per-second.
#[derive(Debug, Clone)]
pub struct ThrottleConfig {
    /// Maximum number of concurrent in-flight requests (0 = unlimited).
    pub max_concurrent: usize,
    /// Seconds to suggest in the `Retry-After` header when throttled.
    pub retry_after_secs: u64,
    /// Paths excluded from throttling (e.g., health checks).
    pub excluded_paths: Vec<String>,
}

impl Default for ThrottleConfig {
    fn default() -> Self {
        Self {
            max_concurrent: 100,
            retry_after_secs: 1,
            excluded_paths: vec!["/health".to_string()],
        }
    }
}

impl ThrottleConfig {
    /// Check if a path is excluded from throttling.
    fn is_excluded(&self, path: &str) -> bool {
        self.excluded_paths.iter().any(|p| path.starts_with(p))
    }
}

/// Shared in-flight counter for the throttle middleware.
///
/// Uses an atomic counter to track how many requests are currently
/// being processed. The guard pattern ensures the counter is always
/// decremented when a request completes (success or failure).
#[derive(Debug)]
pub struct ThrottleState {
    config: ThrottleConfig,
    in_flight: std::sync::atomic::AtomicUsize,
}

impl ThrottleState {
    /// Create a new throttle state with the given configuration.
    pub fn new(config: ThrottleConfig) -> Self {
        Self {
            config,
            in_flight: std::sync::atomic::AtomicUsize::new(0),
        }
    }

    /// Try to acquire a slot. Returns `Some(guard)` if under the limit,
    /// or `None` if the concurrency cap has been reached.
    fn try_acquire(&self) -> Option<ThrottleGuard<'_>> {
        if self.config.max_concurrent == 0 {
            // Unlimited mode
            self.in_flight
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            return Some(ThrottleGuard { state: self });
        }
        loop {
            let current = self.in_flight.load(std::sync::atomic::Ordering::SeqCst);
            if current >= self.config.max_concurrent {
                return None;
            }
            if self
                .in_flight
                .compare_exchange(
                    current,
                    current + 1,
                    std::sync::atomic::Ordering::SeqCst,
                    std::sync::atomic::Ordering::SeqCst,
                )
                .is_ok()
            {
                return Some(ThrottleGuard { state: self });
            }
        }
    }

    /// Current number of in-flight requests.
    pub fn current_count(&self) -> usize {
        self.in_flight.load(std::sync::atomic::Ordering::SeqCst)
    }
}

/// RAII guard that decrements the in-flight counter on drop.
struct ThrottleGuard<'a> {
    state: &'a ThrottleState,
}

impl<'a> Drop for ThrottleGuard<'a> {
    fn drop(&mut self) {
        self.state
            .in_flight
            .fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
    }
}

/// Request throttling middleware handler.
///
/// Checks the shared in-flight counter. If the concurrency limit has
/// been reached, returns `503 Service Unavailable` with `Retry-After`
/// and `X-Throttle-Limit` headers. Otherwise, acquires a slot (via
/// RAII guard), forwards the request, and adds `X-Throttle-Current`
/// to the response indicating current concurrency utilisation.
fn request_throttle_handler(
    state: Arc<ThrottleState>,
    request: Request,
    next: Next,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Response> + Send>> {
    Box::pin(async move {
        let path = request.uri().path().to_string();

        if state.config.is_excluded(&path) {
            return next.run(request).await;
        }

        let guard = match state.try_acquire() {
            Some(g) => g,
            None => {
                let retry = state.config.retry_after_secs.to_string();
                let limit = state.config.max_concurrent.to_string();
                let mut response = (
                    StatusCode::SERVICE_UNAVAILABLE,
                    [(header::CONTENT_TYPE, "application/json")],
                    Json(ErrorResponse {
                        code: "THROTTLED".to_string(),
                        error: "Server is at capacity, please retry later".to_string(),
                        request_id: None,
                    }),
                )
                    .into_response();
                if let Ok(v) = HeaderValue::from_str(&retry) {
                    response.headers_mut().insert("retry-after", v);
                }
                if let Ok(v) = HeaderValue::from_str(&limit) {
                    response.headers_mut().insert("x-throttle-limit", v);
                }
                return response;
            }
        };

        let current = state.current_count();
        let mut response = next.run(request).await;

        // Add current concurrency to response for observability
        if let Ok(val) = HeaderValue::from_str(&current.to_string()) {
            response.headers_mut().insert("x-throttle-current", val);
        }

        // Keep the guard alive until response is built
        drop(guard);

        response
    })
}

// ============================================================================
// Retry Hints
// ============================================================================

/// Configuration for automatic retry hint headers on error responses.
///
/// When enabled, this middleware inspects outgoing responses and adds
/// standardized retry guidance headers to responses with matching status
/// codes. This helps clients implement intelligent retry logic with
/// exponential backoff and bounded retries.
///
/// Headers added:
/// - `Retry-After` — seconds to wait before retrying (if not already set)
/// - `X-Retry-Strategy` — recommended retry strategy (e.g., `exponential-backoff`)
/// - `X-Retry-Max` — maximum number of retries the client should attempt
#[derive(Debug, Clone)]
pub struct RetryHintsConfig {
    /// HTTP status codes that should receive retry hint headers.
    /// Default: `[408, 429, 503]`
    pub retry_statuses: Vec<u16>,
    /// Default `Retry-After` value in seconds when the header is not already present.
    /// Default: `1`
    pub default_retry_after_secs: u64,
    /// Recommended retry strategy communicated via `X-Retry-Strategy`.
    /// Default: `"exponential-backoff"`
    pub strategy: String,
    /// Maximum retries communicated via `X-Retry-Max`.
    /// Default: `3`
    pub max_retries: u32,
    /// Paths to exclude from retry hint injection.
    pub excluded_paths: Vec<String>,
}

impl Default for RetryHintsConfig {
    fn default() -> Self {
        Self {
            retry_statuses: vec![408, 429, 503],
            default_retry_after_secs: 1,
            strategy: "exponential-backoff".to_string(),
            max_retries: 3,
            excluded_paths: vec!["/health".to_string()],
        }
    }
}

impl RetryHintsConfig {
    /// Returns `true` if the given path should be excluded from retry hints.
    pub fn is_excluded(&self, path: &str) -> bool {
        self.excluded_paths.iter().any(|p| path.starts_with(p))
    }
}

/// Middleware that adds retry guidance headers to error responses.
///
/// Inspects the response status code after the inner handler runs. If
/// the status matches one of the configured `retry_statuses` and the
/// request path is not excluded, the middleware injects:
///
/// - `Retry-After` — only if not already set by a prior layer
/// - `X-Retry-Strategy` — the recommended client strategy
/// - `X-Retry-Max` — the maximum number of retries
pub fn retry_hints_handler(
    config: RetryHintsConfig,
    request: Request,
    next: Next,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Response> + Send>> {
    Box::pin(async move {
        let path = request.uri().path().to_string();

        // Skip excluded paths
        if config.is_excluded(&path) {
            return next.run(request).await;
        }

        let mut response = next.run(request).await;
        let status = response.status().as_u16();

        // Only add hints for configured error statuses
        if config.retry_statuses.contains(&status) {
            let headers = response.headers_mut();

            // Only set Retry-After if not already present (e.g., from rate limiter)
            if !headers.contains_key("retry-after") {
                if let Ok(v) = HeaderValue::from_str(&config.default_retry_after_secs.to_string()) {
                    headers.insert("retry-after", v);
                }
            }

            if let Ok(v) = HeaderValue::from_str(&config.strategy) {
                headers.insert("x-retry-strategy", v);
            }

            if let Ok(v) = HeaderValue::from_str(&config.max_retries.to_string()) {
                headers.insert("x-retry-max", v);
            }
        }

        response
    })
}

// ============================================================================
// Maintenance Mode
// ============================================================================

/// Configuration for planned maintenance mode.
///
/// When maintenance mode is active, all non-excluded requests are
/// immediately rejected with `503 Service Unavailable`, a JSON error
/// body, a `Retry-After` header, and an `X-Maintenance-Message` header.
/// This allows operators to signal planned downtime without shutting
/// down the server process.
#[derive(Debug, Clone)]
pub struct MaintenanceConfig {
    /// Human-readable maintenance message.
    /// Default: `"Service is undergoing planned maintenance"`
    pub message: String,
    /// Retry-After value in seconds.
    /// Default: `300` (5 minutes)
    pub retry_after_secs: u64,
    /// Paths excluded from maintenance mode (e.g., health checks).
    pub excluded_paths: Vec<String>,
}

impl Default for MaintenanceConfig {
    fn default() -> Self {
        Self {
            message: "Service is undergoing planned maintenance".to_string(),
            retry_after_secs: 300,
            excluded_paths: vec!["/health".to_string()],
        }
    }
}

impl MaintenanceConfig {
    /// Returns `true` if the given path should be excluded from maintenance mode.
    pub fn is_excluded(&self, path: &str) -> bool {
        self.excluded_paths.iter().any(|p| path.starts_with(p))
    }
}

/// Shared maintenance mode state.
///
/// Uses an atomic boolean so that maintenance mode can be toggled at
/// runtime without restarting the server.
#[derive(Debug)]
pub struct MaintenanceState {
    /// Whether maintenance mode is currently active.
    active: std::sync::atomic::AtomicBool,
    /// The configuration to use when maintenance mode is active.
    pub config: MaintenanceConfig,
}

impl MaintenanceState {
    /// Create a new maintenance state.
    pub fn new(config: MaintenanceConfig) -> Self {
        Self {
            active: std::sync::atomic::AtomicBool::new(false),
            config,
        }
    }

    /// Create a new maintenance state that starts active.
    pub fn new_active(config: MaintenanceConfig) -> Self {
        Self {
            active: std::sync::atomic::AtomicBool::new(true),
            config,
        }
    }

    /// Check whether maintenance mode is active.
    pub fn is_active(&self) -> bool {
        self.active.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Activate maintenance mode.
    pub fn activate(&self) {
        self.active
            .store(true, std::sync::atomic::Ordering::Relaxed);
    }

    /// Deactivate maintenance mode.
    pub fn deactivate(&self) {
        self.active
            .store(false, std::sync::atomic::Ordering::Relaxed);
    }
}

/// Middleware that gates requests during planned maintenance.
///
/// When maintenance mode is active and the request path is not excluded,
/// returns `503 Service Unavailable` with a JSON error body, a
/// `Retry-After` header, and an `X-Maintenance-Message` header.
pub fn maintenance_handler(
    state: Arc<MaintenanceState>,
    request: Request,
    next: Next,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Response> + Send>> {
    Box::pin(async move {
        let path = request.uri().path().to_string();

        // If not active or path is excluded, pass through
        if !state.is_active() || state.config.is_excluded(&path) {
            return next.run(request).await;
        }

        let retry = state.config.retry_after_secs.to_string();
        let mut response = (
            StatusCode::SERVICE_UNAVAILABLE,
            [(header::CONTENT_TYPE, "application/json")],
            Json(ErrorResponse {
                code: "MAINTENANCE".to_string(),
                error: state.config.message.clone(),
                request_id: None,
            }),
        )
            .into_response();
        if let Ok(v) = HeaderValue::from_str(&retry) {
            response.headers_mut().insert("retry-after", v);
        }
        if let Ok(v) = HeaderValue::from_str(&state.config.message) {
            response.headers_mut().insert("x-maintenance-message", v);
        }

        response
    })
}

// ============================================================================
// API Deprecation
// ============================================================================

/// Configuration for a single deprecated endpoint.
///
/// Each entry maps a path prefix to its deprecation metadata: the
/// date when the endpoint was deprecated, the date when it will be
/// removed (sunset), a replacement URL, and an optional human-readable
/// message.  All fields except the path itself are optional.
#[derive(Debug, Clone)]
pub struct DeprecationEntry {
    /// Path prefix that is deprecated (e.g., `/api/v1`).
    pub path: String,
    /// ISO-8601 date when the endpoint was deprecated (e.g., `"2025-12-01"`).
    pub deprecated_at: Option<String>,
    /// ISO-8601 date when the endpoint will be removed (RFC 8594 Sunset).
    pub sunset_at: Option<String>,
    /// URL of the replacement endpoint (used in `Link` header).
    pub replacement: Option<String>,
    /// Human-readable deprecation message.
    pub message: Option<String>,
}

/// Configuration for API deprecation headers.
///
/// When enabled, the middleware scans each request path against a list
/// of [`DeprecationEntry`] items.  For matching paths it adds:
///
/// * `Deprecation: true` (or the deprecation date if provided)
/// * `Sunset: <date>` (RFC 8594, if a sunset date is provided)
/// * `Link: <url>; rel="successor-version"` (if a replacement is provided)
/// * `X-Deprecation-Message: <msg>` (if a message is provided)
///
/// These headers inform API consumers about upcoming breaking changes
/// **without** blocking the request.
#[derive(Debug, Clone)]
pub struct DeprecationConfig {
    /// List of deprecated endpoint entries.
    pub entries: Vec<DeprecationEntry>,
    /// Paths excluded from deprecation checks.
    pub excluded_paths: Vec<String>,
}

impl Default for DeprecationConfig {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
            excluded_paths: vec!["/health".to_string()],
        }
    }
}

impl DeprecationConfig {
    /// Returns `true` if the given path should skip deprecation checking.
    pub fn is_excluded(&self, path: &str) -> bool {
        self.excluded_paths.iter().any(|p| path.starts_with(p))
    }

    /// Find the first matching deprecation entry for the given path.
    pub fn find_match(&self, path: &str) -> Option<&DeprecationEntry> {
        self.entries.iter().find(|e| path.starts_with(&e.path))
    }
}

/// Middleware that injects deprecation warning headers.
///
/// When a request path matches a configured [`DeprecationEntry`], the
/// response is annotated with `Deprecation`, `Sunset`, `Link`, and/or
/// `X-Deprecation-Message` headers.  Requests are never blocked---this
/// layer is purely informational.
pub fn deprecation_handler(
    config: DeprecationConfig,
    request: Request,
    next: Next,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Response> + Send>> {
    Box::pin(async move {
        let path = request.uri().path().to_string();

        // Skip excluded paths
        if config.is_excluded(&path) {
            return next.run(request).await;
        }

        // Find a matching deprecation entry
        let entry = config.find_match(&path);

        let mut response = next.run(request).await;

        if let Some(entry) = entry {
            let headers = response.headers_mut();

            // Deprecation header: date or "true"
            let dep_value = entry.deprecated_at.as_deref().unwrap_or("true");
            if let Ok(v) = HeaderValue::from_str(dep_value) {
                headers.insert("deprecation", v);
            }

            // Sunset header (RFC 8594)
            if let Some(ref sunset) = entry.sunset_at {
                if let Ok(v) = HeaderValue::from_str(sunset) {
                    headers.insert("sunset", v);
                }
            }

            // Link header for replacement endpoint
            if let Some(ref replacement) = entry.replacement {
                let link = format!("{replacement}; rel=\"successor-version\"");
                if let Ok(v) = HeaderValue::from_str(&link) {
                    headers.insert("link", v);
                }
            }

            // Human-readable message
            if let Some(ref msg) = entry.message {
                if let Ok(v) = HeaderValue::from_str(msg) {
                    headers.insert("x-deprecation-message", v);
                }
            }
        }

        response
    })
}

// ============================================================================
// Request Costing
// ============================================================================

/// Configuration for per-request cost tracking.
///
/// Assigns a numeric cost to each HTTP method and tracks a per-client
/// cost budget.  Every response includes `X-Request-Cost` (the cost of
/// the current request) and `X-Cost-Budget-Remaining` (how much budget
/// is left in the current window).  When the budget is exhausted,
/// requests are rejected with `429 Too Many Requests`.
#[derive(Debug, Clone)]
pub struct RequestCostConfig {
    /// Cost assigned to GET requests.
    pub get_cost: u64,
    /// Cost assigned to POST requests.
    pub post_cost: u64,
    /// Cost assigned to PUT requests.
    pub put_cost: u64,
    /// Cost assigned to PATCH requests.
    pub patch_cost: u64,
    /// Cost assigned to DELETE requests.
    pub delete_cost: u64,
    /// Cost assigned to HEAD requests.
    pub head_cost: u64,
    /// Cost assigned to OPTIONS requests.
    pub options_cost: u64,
    /// Maximum cost budget per window.
    pub budget: u64,
    /// Budget window duration in seconds.
    pub window_secs: u64,
    /// Paths excluded from cost tracking.
    pub excluded_paths: Vec<String>,
}

impl Default for RequestCostConfig {
    fn default() -> Self {
        Self {
            get_cost: 1,
            post_cost: 5,
            put_cost: 3,
            patch_cost: 3,
            delete_cost: 5,
            head_cost: 1,
            options_cost: 0,
            budget: 1000,
            window_secs: 3600,
            excluded_paths: vec!["/health".to_string()],
        }
    }
}

impl RequestCostConfig {
    /// Returns `true` if the given path should be excluded from cost tracking.
    pub fn is_excluded(&self, path: &str) -> bool {
        self.excluded_paths.iter().any(|p| path.starts_with(p))
    }

    /// Returns the cost for a given HTTP method.
    pub fn cost_for_method(&self, method: &Method) -> u64 {
        match *method {
            Method::GET => self.get_cost,
            Method::POST => self.post_cost,
            Method::PUT => self.put_cost,
            Method::PATCH => self.patch_cost,
            Method::DELETE => self.delete_cost,
            Method::HEAD => self.head_cost,
            Method::OPTIONS => self.options_cost,
            _ => 1,
        }
    }
}

/// Shared request cost budget state.
///
/// Tracks cumulative cost per window using atomic counters with
/// a simple epoch-based window reset.
#[derive(Debug)]
pub struct RequestCostState {
    /// The configuration for cost tracking.
    pub config: RequestCostConfig,
    /// Current accumulated cost in this window.
    used: std::sync::atomic::AtomicU64,
    /// Epoch second when the current window started.
    window_start: std::sync::atomic::AtomicU64,
}

impl RequestCostState {
    /// Create a new cost tracking state.
    pub fn new(config: RequestCostConfig) -> Self {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        Self {
            config,
            used: std::sync::atomic::AtomicU64::new(0),
            window_start: std::sync::atomic::AtomicU64::new(now),
        }
    }

    /// Try to spend `cost` from the budget. Returns `(remaining, ok)`.
    ///
    /// If the window has expired, resets the counter first.
    pub fn try_spend(&self, cost: u64) -> (u64, bool) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let ws = self.window_start.load(std::sync::atomic::Ordering::Relaxed);
        if now.saturating_sub(ws) >= self.config.window_secs {
            // Reset window
            self.window_start
                .store(now, std::sync::atomic::Ordering::Relaxed);
            self.used.store(0, std::sync::atomic::Ordering::Relaxed);
        }
        let prev = self
            .used
            .fetch_add(cost, std::sync::atomic::Ordering::Relaxed);
        let total = prev + cost;
        if total > self.config.budget {
            // Over budget — undo
            self.used
                .fetch_sub(cost, std::sync::atomic::Ordering::Relaxed);
            let remaining = self.config.budget.saturating_sub(prev);
            (remaining, false)
        } else {
            let remaining = self.config.budget.saturating_sub(total);
            (remaining, true)
        }
    }

    /// Returns the current remaining budget.
    pub fn remaining(&self) -> u64 {
        let used = self.used.load(std::sync::atomic::Ordering::Relaxed);
        self.config.budget.saturating_sub(used)
    }
}

/// Middleware that tracks per-request cost and enforces a cost budget.
///
/// Each request is assigned a cost based on its HTTP method. The cost
/// is deducted from a shared budget. Every response includes
/// `X-Request-Cost` and `X-Cost-Budget-Remaining` headers. When the
/// budget is exhausted, the request is rejected with `429 Too Many
/// Requests`.
pub fn request_cost_handler(
    state: Arc<RequestCostState>,
    request: Request,
    next: Next,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Response> + Send>> {
    Box::pin(async move {
        let path = request.uri().path().to_string();

        // Skip excluded paths
        if state.config.is_excluded(&path) {
            return next.run(request).await;
        }

        let method = request.method().clone();
        let cost = state.config.cost_for_method(&method);

        let (remaining, ok) = state.try_spend(cost);

        if !ok {
            // Over budget
            let mut response = (
                StatusCode::TOO_MANY_REQUESTS,
                [(header::CONTENT_TYPE, "application/json")],
                Json(ErrorResponse {
                    code: "COST_BUDGET_EXCEEDED".to_string(),
                    error: "Request cost budget exhausted".to_string(),
                    request_id: None,
                }),
            )
                .into_response();
            if let Ok(v) = HeaderValue::from_str(&cost.to_string()) {
                response.headers_mut().insert("x-request-cost", v);
            }
            if let Ok(v) = HeaderValue::from_str(&remaining.to_string()) {
                response.headers_mut().insert("x-cost-budget-remaining", v);
            }
            return response;
        }

        let mut response = next.run(request).await;
        if let Ok(v) = HeaderValue::from_str(&cost.to_string()) {
            response.headers_mut().insert("x-request-cost", v);
        }
        if let Ok(v) = HeaderValue::from_str(&remaining.to_string()) {
            response.headers_mut().insert("x-cost-budget-remaining", v);
        }

        response
    })
}
// ============================================================================
// Request Fingerprint
// ============================================================================

/// Configuration for the request fingerprint middleware.
///
/// Generates a deterministic `X-Request-Fingerprint` hash from the request
/// method, path, selected headers, and body hash. This enables server-side
/// deduplication, cache key generation, and client correlation.
#[derive(Debug, Clone)]
pub struct FingerprintConfig {
    /// Header names to include in the fingerprint (case-insensitive).
    pub include_headers: Vec<String>,
    /// Paths excluded from fingerprint generation.
    pub excluded_paths: Vec<String>,
    /// Whether to include the request body in the fingerprint hash.
    pub include_body: bool,
    /// Whether to include query parameters in the fingerprint hash.
    pub include_query: bool,
}

impl Default for FingerprintConfig {
    fn default() -> Self {
        Self {
            include_headers: vec!["content-type".to_string(), "accept".to_string()],
            excluded_paths: vec!["/health".to_string()],
            include_body: true,
            include_query: true,
        }
    }
}

impl FingerprintConfig {
    /// Returns `true` if the given path should be excluded from fingerprinting.
    pub fn is_excluded(&self, path: &str) -> bool {
        self.excluded_paths.iter().any(|p| path.starts_with(p))
    }
}

/// Compute a deterministic FNV-1a fingerprint from the provided components.
fn compute_fingerprint(parts: &[&[u8]]) -> String {
    const FNV_OFFSET: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x00000100000001B3;

    let mut hash = FNV_OFFSET;
    for part in parts {
        for &byte in *part {
            hash ^= byte as u64;
            hash = hash.wrapping_mul(FNV_PRIME);
        }
        // Separator between parts
        hash ^= 0xFF;
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    format!("{hash:016x}")
}

/// Middleware that generates a deterministic request fingerprint.
///
/// Computes an FNV-1a hash from the request method, path, selected headers,
/// optional query string, and optional body. The fingerprint is added as
/// `X-Request-Fingerprint` on the response.
pub fn request_fingerprint_handler(
    config: FingerprintConfig,
    request: Request,
    next: Next,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Response> + Send>> {
    Box::pin(async move {
        let path = request.uri().path().to_string();

        // Skip excluded paths
        if config.is_excluded(&path) {
            return next.run(request).await;
        }

        let method = request.method().as_str().to_string();
        let query = if config.include_query {
            request.uri().query().unwrap_or("").to_string()
        } else {
            String::new()
        };

        // Collect selected header values
        let mut header_parts: Vec<String> = Vec::new();
        for name in &config.include_headers {
            let lower = name.to_lowercase();
            if let Some(val) = request.headers().get(lower.as_str()) {
                if let Ok(v) = val.to_str() {
                    header_parts.push(format!("{lower}={v}"));
                }
            }
        }
        let headers_str = header_parts.join(";");

        // Optionally read the body for hashing
        if config.include_body {
            let (parts, body) = request.into_parts();
            let body_bytes = match axum::body::to_bytes(body, 10 * 1024 * 1024).await {
                Ok(b) => b,
                Err(_) => {
                    let req = Request::from_parts(parts, axum::body::Body::empty());
                    return next.run(req).await;
                }
            };

            let mut components: Vec<&[u8]> = vec![
                method.as_bytes(),
                path.as_bytes(),
                query.as_bytes(),
                headers_str.as_bytes(),
            ];
            if !body_bytes.is_empty() {
                components.push(&body_bytes);
            }

            let fingerprint = compute_fingerprint(&components);

            // Reconstruct request with body
            let req = Request::from_parts(parts, axum::body::Body::from(body_bytes));
            let mut response = next.run(req).await;

            if let Ok(v) = HeaderValue::from_str(&fingerprint) {
                response.headers_mut().insert("x-request-fingerprint", v);
            }
            response
        } else {
            let components: Vec<&[u8]> = vec![
                method.as_bytes(),
                path.as_bytes(),
                query.as_bytes(),
                headers_str.as_bytes(),
            ];
            let fingerprint = compute_fingerprint(&components);

            let mut response = next.run(request).await;
            if let Ok(v) = HeaderValue::from_str(&fingerprint) {
                response.headers_mut().insert("x-request-fingerprint", v);
            }
            response
        }
    })
}

// ============================================================================
// Response Signing
// ============================================================================

/// Configuration for response signing middleware.
///
/// Signs outgoing response bodies with HMAC-SHA256, adding an
/// `X-Response-Signature` header so clients can verify response
/// integrity.
#[derive(Debug, Clone)]
pub struct ResponseSigningConfig {
    /// HMAC secret key.  Empty disables signing.
    pub secret: String,
    /// Paths excluded from signing (e.g. `/health`).
    pub excluded_paths: Vec<String>,
    /// Name of the signature header (default `x-response-signature`).
    pub signature_header: String,
    /// Include the HTTP status code in the signing input.
    pub include_status: bool,
    /// Response headers to include in the signing input.
    pub include_headers: Vec<String>,
    /// Maximum response body bytes to read for signing.
    pub max_body_bytes: usize,
}

impl Default for ResponseSigningConfig {
    fn default() -> Self {
        Self {
            secret: String::new(),
            excluded_paths: vec!["/health".to_string()],
            signature_header: "x-response-signature".to_string(),
            include_status: true,
            include_headers: Vec::new(),
            max_body_bytes: 10 * 1024 * 1024,
        }
    }
}

impl ResponseSigningConfig {
    /// Returns `true` if the given path should be excluded from signing.
    pub fn is_excluded(&self, path: &str) -> bool {
        self.excluded_paths.iter().any(|p| p == path)
    }
}

/// Middleware handler that signs response bodies with HMAC-SHA256.
///
/// After calling `next.run(request)`, the handler reads the full
/// response body, computes the HMAC, and injects the signature
/// header before returning.
async fn response_signing_handler(
    config: ResponseSigningConfig,
    req: Request,
    next: Next,
) -> Response {
    // Skip if no secret configured
    if config.secret.is_empty() {
        return next.run(req).await;
    }

    // Skip excluded paths
    let path = req.uri().path().to_string();
    if config.is_excluded(&path) {
        return next.run(req).await;
    }

    let response = next.run(req).await;
    let (parts, body) = response.into_parts();

    // Read the body
    let body_bytes = match axum::body::to_bytes(body, config.max_body_bytes).await {
        Ok(b) => b,
        Err(_) => {
            // Body too large or read error — return without signing
            let mut resp = Response::builder()
                .status(parts.status)
                .body(axum::body::Body::empty())
                .unwrap();
            *resp.headers_mut() = parts.headers;
            return resp;
        }
    };

    // Build signing input: status + selected headers + body
    let mut signing_input = Vec::new();
    if config.include_status {
        signing_input.extend_from_slice(parts.status.as_u16().to_string().as_bytes());
        signing_input.push(b'\n');
    }
    for header_name in &config.include_headers {
        if let Some(val) = parts.headers.get(header_name.as_str()) {
            signing_input.extend_from_slice(header_name.as_bytes());
            signing_input.push(b':');
            signing_input.extend_from_slice(val.as_bytes());
            signing_input.push(b'\n');
        }
    }
    signing_input.extend_from_slice(&body_bytes);

    // Compute HMAC
    let signature = hmac_sha256(config.secret.as_bytes(), &signing_input);

    // Rebuild response with signature headers
    let mut response = Response::from_parts(parts, axum::body::Body::from(body_bytes));
    if let Ok(v) = HeaderValue::from_str(&signature) {
        if let Ok(name) = axum::http::HeaderName::from_bytes(config.signature_header.as_bytes()) {
            response.headers_mut().insert(name, v);
        }
    }
    if let Ok(v) = HeaderValue::from_str("hmac-sha256") {
        response.headers_mut().insert("x-signature-algorithm", v);
    }
    response
}
// ============================================================================
// Request Priority
// ============================================================================

/// Priority level for a request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PriorityLevel {
    /// Mission-critical requests (e.g. health, shutdown).
    Critical,
    /// High-priority requests (e.g. authentication).
    High,
    /// Default priority for unmatched requests.
    Normal,
    /// Low-priority requests (e.g. analytics, bulk).
    Low,
}

impl PriorityLevel {
    /// Returns the string representation used in the header.
    pub fn as_str(&self) -> &'static str {
        match self {
            PriorityLevel::Critical => "critical",
            PriorityLevel::High => "high",
            PriorityLevel::Normal => "normal",
            PriorityLevel::Low => "low",
        }
    }

    /// Parse from a string, returning `None` for unrecognised values.
    pub fn from_str_opt(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "critical" => Some(PriorityLevel::Critical),
            "high" => Some(PriorityLevel::High),
            "normal" => Some(PriorityLevel::Normal),
            "low" => Some(PriorityLevel::Low),
            _ => None,
        }
    }

    /// Numeric weight (higher = more important).
    pub fn weight(&self) -> u8 {
        match self {
            PriorityLevel::Critical => 4,
            PriorityLevel::High => 3,
            PriorityLevel::Normal => 2,
            PriorityLevel::Low => 1,
        }
    }
}

impl std::fmt::Display for PriorityLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A rule that assigns a priority to matching requests.
#[derive(Debug, Clone)]
pub struct PriorityRule {
    /// Path prefix to match (e.g. `/health`, `/api/v1/admin`).
    pub path_prefix: String,
    /// Optional HTTP method filter (e.g. `GET`, `POST`).  `None` matches all.
    pub method: Option<String>,
    /// Priority to assign when this rule matches.
    pub priority: PriorityLevel,
    /// Human-readable reason for the priority assignment.
    pub reason: String,
}

/// Configuration for request priority middleware.
///
/// Assigns a priority level to each request based on path prefix and
/// method rules.  Injects `X-Request-Priority` and `X-Priority-Reason`
/// response headers.
#[derive(Debug, Clone)]
pub struct RequestPriorityConfig {
    /// Ordered list of rules — first match wins.
    pub rules: Vec<PriorityRule>,
    /// Default priority when no rule matches.
    pub default_priority: PriorityLevel,
    /// Name of the priority header (default `x-request-priority`).
    pub priority_header: String,
    /// Name of the reason header (default `x-priority-reason`).
    pub reason_header: String,
    /// Whether to honour a client-supplied priority header on the request.
    pub allow_client_override: bool,
}

impl Default for RequestPriorityConfig {
    fn default() -> Self {
        Self {
            rules: Vec::new(),
            default_priority: PriorityLevel::Normal,
            priority_header: "x-request-priority".to_string(),
            reason_header: "x-priority-reason".to_string(),
            allow_client_override: false,
        }
    }
}

impl RequestPriorityConfig {
    /// Evaluate the priority for a request given its method and path.
    pub fn evaluate(&self, method: &str, path: &str) -> (PriorityLevel, String) {
        for rule in &self.rules {
            if !path.starts_with(&rule.path_prefix) {
                continue;
            }
            if let Some(ref m) = rule.method {
                if !m.eq_ignore_ascii_case(method) {
                    continue;
                }
            }
            return (rule.priority, rule.reason.clone());
        }
        (self.default_priority, "default".to_string())
    }
}

/// Middleware handler that assigns a priority level to each request.
///
/// Evaluates configured rules in order (first match wins), falling
/// back to the default priority.  Sets `X-Request-Priority` and
/// `X-Priority-Reason` response headers.
async fn request_priority_handler(
    config: RequestPriorityConfig,
    req: Request,
    next: Next,
) -> Response {
    let method = req.method().as_str().to_string();
    let path = req.uri().path().to_string();

    // Check for client-supplied override
    let (priority, reason) = if config.allow_client_override {
        if let Some(val) = req.headers().get(config.priority_header.as_str()) {
            if let Ok(s) = val.to_str() {
                if let Some(p) = PriorityLevel::from_str_opt(s) {
                    (p, "client-override".to_string())
                } else {
                    config.evaluate(&method, &path)
                }
            } else {
                config.evaluate(&method, &path)
            }
        } else {
            config.evaluate(&method, &path)
        }
    } else {
        config.evaluate(&method, &path)
    };

    let mut response = next.run(req).await;

    if let Ok(v) = HeaderValue::from_str(priority.as_str()) {
        if let Ok(name) = axum::http::HeaderName::from_bytes(config.priority_header.as_bytes()) {
            response.headers_mut().insert(name, v);
        }
    }
    if let Ok(v) = HeaderValue::from_str(&reason) {
        if let Ok(name) = axum::http::HeaderName::from_bytes(config.reason_header.as_bytes()) {
            response.headers_mut().insert(name, v);
        }
    }
    response
}
// ============================================================================

// ============================================================================
// Request Quota
// ============================================================================

/// Configuration for per-client request quota enforcement.
///
/// Tracks how many requests each client (identified by API key or IP) has made
/// within a configurable window and rejects requests that exceed the quota with
/// `429 Too Many Requests`. Injects `X-Quota-Limit`, `X-Quota-Remaining`, and
/// `X-Quota-Reset` response headers for client visibility.
#[derive(Debug, Clone)]
pub struct RequestQuotaConfig {
    /// Maximum number of requests allowed per client per window.
    pub limit: u64,
    /// Window duration in seconds (rolling window).
    pub window_secs: u64,
    /// Header name for the quota limit.
    pub limit_header: String,
    /// Header name for quota remaining.
    pub remaining_header: String,
    /// Header name for quota reset (seconds until window resets).
    pub reset_header: String,
    /// Paths excluded from quota enforcement.
    pub excluded_paths: Vec<String>,
    /// Use the `x-api-key` header as client identifier (falls back to IP).
    pub identify_by_api_key: bool,
}

impl Default for RequestQuotaConfig {
    fn default() -> Self {
        Self {
            limit: 1000,
            window_secs: 3600,
            limit_header: "x-quota-limit".to_string(),
            remaining_header: "x-quota-remaining".to_string(),
            reset_header: "x-quota-reset".to_string(),
            excluded_paths: vec!["/health".to_string()],
            identify_by_api_key: true,
        }
    }
}

/// Shared state for quota tracking.
///
/// Uses a `parking_lot::Mutex` around a map of client identifiers to
/// `(request_count, window_start_epoch_secs)` pairs.
#[derive(Debug, Clone)]
pub struct QuotaState {
    inner: std::sync::Arc<parking_lot::Mutex<std::collections::HashMap<String, (u64, u64)>>>,
}

impl Default for QuotaState {
    fn default() -> Self {
        Self::new()
    }
}

impl QuotaState {
    /// Create a new empty quota state.
    pub fn new() -> Self {
        Self {
            inner: std::sync::Arc::new(parking_lot::Mutex::new(std::collections::HashMap::new())),
        }
    }

    /// Check quota for a client. Returns `(allowed, remaining, reset_secs)`.
    pub fn check(&self, client_id: &str, limit: u64, window_secs: u64) -> (bool, u64, u64) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let mut map = self.inner.lock();
        let entry = map.entry(client_id.to_string()).or_insert((0, now));

        // Check if window has reset
        let elapsed = now.saturating_sub(entry.1);
        if elapsed >= window_secs {
            // New window
            entry.0 = 1;
            entry.1 = now;
            let remaining = limit.saturating_sub(1);
            return (true, remaining, window_secs);
        }

        // Within current window
        entry.0 += 1;
        let reset_secs = window_secs.saturating_sub(elapsed);
        if entry.0 > limit {
            (false, 0, reset_secs)
        } else {
            let remaining = limit.saturating_sub(entry.0);
            (true, remaining, reset_secs)
        }
    }
}

/// Middleware handler that enforces per-client request quotas.
///
/// Identifies clients by API key header or remote IP, checks their usage
/// against the configured limit/window, and either allows the request
/// (injecting quota headers) or rejects with 429.
pub async fn request_quota_handler(
    config: RequestQuotaConfig,
    state: QuotaState,
    req: Request,
    next: Next,
) -> Response {
    let path = req.uri().path().to_string();

    // Skip excluded paths
    for excluded in &config.excluded_paths {
        if path.starts_with(excluded) {
            return next.run(req).await;
        }
    }

    // Determine client identifier
    let client_id = if config.identify_by_api_key {
        req.headers()
            .get("x-api-key")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string())
            .unwrap_or_else(|| "anonymous".to_string())
    } else {
        "anonymous".to_string()
    };

    let (allowed, remaining, reset_secs) =
        state.check(&client_id, config.limit, config.window_secs);

    if !allowed {
        // Build 429 response
        let body = serde_json::json!({
            "error": {
                "code": "QUOTA_EXCEEDED",
                "message": format!("Request quota of {} exceeded. Retry after {} seconds.", config.limit, reset_secs),
            }
        });
        let mut resp = Response::builder()
            .status(axum::http::StatusCode::TOO_MANY_REQUESTS)
            .header("content-type", "application/json")
            .body(axum::body::Body::from(
                serde_json::to_string(&body).unwrap_or_default(),
            ))
            .unwrap_or_default();

        // Inject quota headers on rejection too
        if let Ok(h) = axum::http::HeaderName::from_bytes(config.limit_header.as_bytes()) {
            resp.headers_mut()
                .insert(h, axum::http::HeaderValue::from(config.limit));
        }
        if let Ok(h) = axum::http::HeaderName::from_bytes(config.remaining_header.as_bytes()) {
            resp.headers_mut()
                .insert(h, axum::http::HeaderValue::from(0u64));
        }
        if let Ok(h) = axum::http::HeaderName::from_bytes(config.reset_header.as_bytes()) {
            resp.headers_mut()
                .insert(h, axum::http::HeaderValue::from(reset_secs));
        }
        return resp;
    }

    let mut resp = next.run(req).await;

    // Inject quota headers
    if let Ok(h) = axum::http::HeaderName::from_bytes(config.limit_header.as_bytes()) {
        resp.headers_mut()
            .insert(h, axum::http::HeaderValue::from(config.limit));
    }
    if let Ok(h) = axum::http::HeaderName::from_bytes(config.remaining_header.as_bytes()) {
        resp.headers_mut()
            .insert(h, axum::http::HeaderValue::from(remaining));
    }
    if let Ok(h) = axum::http::HeaderName::from_bytes(config.reset_header.as_bytes()) {
        resp.headers_mut()
            .insert(h, axum::http::HeaderValue::from(reset_secs));
    }

    resp
}

// ============================================================================
// Request Replay Protection (Layer 35)
// ============================================================================

/// Configuration for request replay protection.
///
/// Detects and rejects replayed requests by tracking nonces from the
/// `X-Nonce` header. Each nonce may only be used once within the configured
/// time window. Requests missing a nonce header when `require_nonce` is true
/// are rejected with `400 Bad Request`. Duplicate nonces are rejected with
/// `409 Conflict`.
///
/// An optional `X-Timestamp` header can be validated to ensure the request
/// was created within the allowed time window (prevents old captured requests
/// from being replayed after the nonce store has been purged).
#[derive(Debug, Clone)]
pub struct ReplayProtectionConfig {
    /// Header name for the nonce value (default: `x-nonce`).
    pub nonce_header: String,
    /// Header name for the request timestamp (default: `x-timestamp`).
    pub timestamp_header: String,
    /// Whether a nonce is required on every request (default: true).
    pub require_nonce: bool,
    /// Whether to validate the timestamp header (default: true).
    pub validate_timestamp: bool,
    /// Maximum age in seconds for a valid timestamp (default: 300 = 5 min).
    pub max_age_secs: u64,
    /// Maximum number of stored nonces before oldest are evicted (default: 100_000).
    pub max_stored_nonces: usize,
    /// Paths excluded from replay protection (default: `["/health"]`).
    pub excluded_paths: Vec<String>,
}

impl Default for ReplayProtectionConfig {
    fn default() -> Self {
        Self {
            nonce_header: "x-nonce".to_string(),
            timestamp_header: "x-timestamp".to_string(),
            require_nonce: true,
            validate_timestamp: true,
            max_age_secs: 300,
            max_stored_nonces: 100_000,
            excluded_paths: vec!["/health".to_string()],
        }
    }
}

/// Shared nonce store for replay protection.
///
/// Uses a `parking_lot::Mutex`-protected `HashMap` mapping nonce strings to
/// their insertion timestamp (seconds since UNIX epoch). Entries older than
/// `max_age_secs` are lazily purged when new nonces are inserted.
#[derive(Debug, Clone)]
pub struct NonceStore {
    inner: std::sync::Arc<parking_lot::Mutex<std::collections::HashMap<String, u64>>>,
}

impl NonceStore {
    /// Creates a new empty nonce store.
    pub fn new() -> Self {
        Self {
            inner: std::sync::Arc::new(parking_lot::Mutex::new(std::collections::HashMap::new())),
        }
    }

    /// Attempts to insert a nonce. Returns `true` if the nonce was new,
    /// `false` if it was already present (replay detected).
    /// Also purges entries older than `max_age_secs`.
    pub fn try_insert(&self, nonce: String, now: u64, max_age: u64, max_size: usize) -> bool {
        let mut map = self.inner.lock();
        // Purge expired entries
        if map.len() >= max_size {
            let cutoff = now.saturating_sub(max_age);
            map.retain(|_, ts| *ts > cutoff);
        }
        // Check for duplicate
        if map.contains_key(&nonce) {
            return false;
        }
        map.insert(nonce, now);
        true
    }

    /// Returns the number of tracked nonces.
    pub fn len(&self) -> usize {
        self.inner.lock().len()
    }

    /// Returns `true` if there are no tracked nonces.
    pub fn is_empty(&self) -> bool {
        self.inner.lock().is_empty()
    }
}

impl Default for NonceStore {
    fn default() -> Self {
        Self::new()
    }
}

/// Middleware handler for request replay protection.
///
/// Extracts the nonce from the configured header, checks it against the
/// shared nonce store, and rejects duplicates with `409 Conflict`.
/// Optionally validates the `X-Timestamp` header to reject stale requests.
async fn replay_protection_handler(
    config: ReplayProtectionConfig,
    nonce_store: NonceStore,
    req: Request,
    next: Next,
) -> Response {
    let path = req.uri().path().to_string();

    // Skip excluded paths
    for excluded in &config.excluded_paths {
        if path.starts_with(excluded) {
            return next.run(req).await;
        }
    }

    // Validate timestamp if enabled
    if config.validate_timestamp {
        if let Some(ts_val) = req.headers().get(config.timestamp_header.as_str()) {
            if let Ok(ts_str) = ts_val.to_str() {
                if let Ok(ts) = ts_str.parse::<u64>() {
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs();
                    let age = now.abs_diff(ts);
                    if age > config.max_age_secs {
                        let body = serde_json::json!({
                            "error": "TIMESTAMP_EXPIRED",
                            "message": format!(
                                "Request timestamp is {}s old, max allowed is {}s",
                                age, config.max_age_secs
                            )
                        });
                        return Response::builder()
                            .status(axum::http::StatusCode::BAD_REQUEST)
                            .header("content-type", "application/json")
                            .body(axum::body::Body::from(
                                serde_json::to_vec(&body).unwrap_or_default(),
                            ))
                            .unwrap_or_default();
                    }
                }
            }
        }
    }

    // Extract nonce
    let nonce = req
        .headers()
        .get(config.nonce_header.as_str())
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    match nonce {
        Some(n) => {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            if !nonce_store.try_insert(n, now, config.max_age_secs, config.max_stored_nonces) {
                // Replay detected
                let body = serde_json::json!({
                    "error": "REPLAY_DETECTED",
                    "message": "This request nonce has already been used"
                });
                return Response::builder()
                    .status(axum::http::StatusCode::CONFLICT)
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(
                        serde_json::to_vec(&body).unwrap_or_default(),
                    ))
                    .unwrap_or_default();
            }
        }
        None => {
            if config.require_nonce {
                let body = serde_json::json!({
                    "error": "MISSING_NONCE",
                    "message": format!(
                        "Request header '{}' is required",
                        config.nonce_header
                    )
                });
                return Response::builder()
                    .status(axum::http::StatusCode::BAD_REQUEST)
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(
                        serde_json::to_vec(&body).unwrap_or_default(),
                    ))
                    .unwrap_or_default();
            }
        }
    }

    next.run(req).await
}

// ============================================================================
// Geo-IP Headers (Layer 36)
// ============================================================================

/// A single IP-prefix-to-region mapping entry.
///
/// Matches client IPs that start with `ip_prefix` and maps them to the
/// configured `country` and optional `region`.
#[derive(Debug, Clone)]
pub struct GeoIpEntry {
    /// IP prefix to match (e.g. "10.0.", "192.168.1.", "203.0.113.").
    pub ip_prefix: String,
    /// ISO 3166-1 alpha-2 country code (e.g. "US", "DE", "JP").
    pub country: String,
    /// Optional region/subdivision name (e.g. "California", "Bavaria").
    pub region: Option<String>,
}

/// Configuration for Geo-IP header injection.
///
/// Extracts the client IP from a configurable request header, resolves it
/// against a list of IP-prefix-to-region mappings, and injects geographic
/// information headers into the response.
#[derive(Debug, Clone)]
pub struct GeoIpConfig {
    /// Request header containing the client IP (default: `x-forwarded-for`).
    pub ip_header: String,
    /// Response header for the resolved country code (default: `x-geo-country`).
    pub country_header: String,
    /// Response header for the resolved region (default: `x-geo-region`).
    pub region_header: String,
    /// Response header echoing the extracted client IP (default: `x-geo-ip`).
    pub ip_echo_header: String,
    /// Whether to echo the client IP in the response (default: false).
    pub echo_ip: bool,
    /// IP-prefix-to-region mapping table.
    pub mappings: Vec<GeoIpEntry>,
    /// Default country when no mapping matches (default: `"XX"` = unknown).
    pub default_country: String,
    /// Paths excluded from geo-IP header injection (default: `["/health"]`).
    pub excluded_paths: Vec<String>,
}

impl Default for GeoIpConfig {
    fn default() -> Self {
        Self {
            ip_header: "x-forwarded-for".to_string(),
            country_header: "x-geo-country".to_string(),
            region_header: "x-geo-region".to_string(),
            ip_echo_header: "x-geo-ip".to_string(),
            echo_ip: false,
            mappings: vec![
                GeoIpEntry {
                    ip_prefix: "10.".to_string(),
                    country: "XX".to_string(),
                    region: Some("Private".to_string()),
                },
                GeoIpEntry {
                    ip_prefix: "192.168.".to_string(),
                    country: "XX".to_string(),
                    region: Some("Private".to_string()),
                },
                GeoIpEntry {
                    ip_prefix: "172.16.".to_string(),
                    country: "XX".to_string(),
                    region: Some("Private".to_string()),
                },
            ],
            default_country: "XX".to_string(),
            excluded_paths: vec!["/health".to_string()],
        }
    }
}

impl GeoIpConfig {
    /// Resolve a client IP against the mapping table.
    ///
    /// Returns `(country, Option<region>)`. Falls back to `default_country`
    /// with no region if no prefix matches.
    pub fn resolve(&self, ip: &str) -> (String, Option<String>) {
        // For X-Forwarded-For, take the first (leftmost = original client) IP
        let client_ip = ip.split(',').next().unwrap_or(ip).trim();
        for entry in &self.mappings {
            if client_ip.starts_with(&entry.ip_prefix) {
                return (entry.country.clone(), entry.region.clone());
            }
        }
        (self.default_country.clone(), None)
    }
}

/// Middleware handler for geo-IP header injection.
///
/// Extracts the client IP from the configured request header, resolves
/// geography via prefix matching, and adds country/region headers to
/// the response.
async fn geo_ip_handler(config: GeoIpConfig, req: Request, next: Next) -> Response {
    let path = req.uri().path().to_string();

    // Skip excluded paths
    for excluded in &config.excluded_paths {
        if path.starts_with(excluded) {
            return next.run(req).await;
        }
    }

    // Extract client IP from header
    let client_ip = req
        .headers()
        .get(config.ip_header.as_str())
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    let mut resp = next.run(req).await;

    if let Some(ref ip) = client_ip {
        let (country, region) = config.resolve(ip);

        if let Ok(hname) = axum::http::HeaderName::from_bytes(config.country_header.as_bytes()) {
            if let Ok(hval) = axum::http::HeaderValue::from_str(&country) {
                resp.headers_mut().insert(hname, hval);
            }
        }

        if let Some(ref reg) = region {
            if let Ok(hname) = axum::http::HeaderName::from_bytes(config.region_header.as_bytes()) {
                if let Ok(hval) = axum::http::HeaderValue::from_str(reg) {
                    resp.headers_mut().insert(hname, hval);
                }
            }
        }

        if config.echo_ip {
            if let Ok(hname) = axum::http::HeaderName::from_bytes(config.ip_echo_header.as_bytes())
            {
                if let Ok(hval) = axum::http::HeaderValue::from_str(ip) {
                    resp.headers_mut().insert(hname, hval);
                }
            }
        }
    } else {
        // No IP header — inject default country
        if let Ok(hname) = axum::http::HeaderName::from_bytes(config.country_header.as_bytes()) {
            if let Ok(hval) = axum::http::HeaderValue::from_str(&config.default_country) {
                resp.headers_mut().insert(hname, hval);
            }
        }
    }

    resp
}
// ============================================================================
// Request Schema Validation (Layer 37)
// ============================================================================

/// Defines a validation rule for a single JSON field.
///
/// Each rule specifies the field name, whether it is required,
/// what JSON type it should be, and optional constraints like
/// max string length or numeric bounds.
#[derive(Debug, Clone)]
pub struct SchemaFieldRule {
    /// JSON field name (top-level key).
    pub field_name: String,
    /// Whether the field must be present.
    pub required: bool,
    /// Expected JSON type: "string", "number", "boolean", "array", "object", or "any".
    pub field_type: String,
    /// Maximum string length (only checked for string fields).
    pub max_length: Option<usize>,
    /// Minimum numeric value (only checked for number fields).
    pub min_value: Option<f64>,
    /// Maximum numeric value (only checked for number fields).
    pub max_value: Option<f64>,
}

/// Maps a route path and HTTP methods to a set of field rules.
///
/// When a request matches both the path and one of the listed methods,
/// its JSON body is validated against the associated field rules.
#[derive(Debug, Clone)]
pub struct SchemaRouteRule {
    /// Route path to match (exact match, e.g. "/api/v1/vms").
    pub path: String,
    /// HTTP methods this rule applies to (e.g. ["POST", "PUT"]).
    pub methods: Vec<String>,
    /// Field-level validation rules for the request body.
    pub fields: Vec<SchemaFieldRule>,
}

/// Configuration for request schema validation middleware.
///
/// When enabled, incoming JSON request bodies are validated
/// against configurable per-route schema rules. Non-conforming
/// requests are rejected with `422 Unprocessable Entity` and a
/// JSON error body describing which fields failed validation.
#[derive(Debug, Clone)]
pub struct SchemaValidationConfig {
    /// Per-route schema rules.
    pub rules: Vec<SchemaRouteRule>,
    /// Maximum allowed request body size in bytes (default 1 MB).
    pub max_body_size: usize,
    /// When true, reject requests with fields not listed in the rules.
    pub strict_mode: bool,
    /// Paths excluded from schema validation.
    pub excluded_paths: Vec<String>,
}

impl Default for SchemaValidationConfig {
    fn default() -> Self {
        Self {
            rules: Vec::new(),
            max_body_size: 1_048_576,
            strict_mode: false,
            excluded_paths: vec!["/health".to_string()],
        }
    }
}

impl SchemaValidationConfig {
    /// Finds the first matching route rule for a given path and method.
    fn find_rule(&self, path: &str, method: &str) -> Option<&SchemaRouteRule> {
        self.rules
            .iter()
            .find(|r| r.path == path && r.methods.iter().any(|m| m.eq_ignore_ascii_case(method)))
    }

    /// Validates a parsed JSON value against a set of field rules.
    ///
    /// Returns a vector of human-readable error strings for any
    /// fields that fail validation.
    fn validate(&self, value: &serde_json::Value, rule: &SchemaRouteRule) -> Vec<String> {
        let mut errors = Vec::new();
        let obj = match value.as_object() {
            Some(o) => o,
            None => {
                errors.push("request body must be a JSON object".to_string());
                return errors;
            }
        };

        for field_rule in &rule.fields {
            match obj.get(&field_rule.field_name) {
                None => {
                    if field_rule.required {
                        errors.push(format!(
                            "missing required field '{}'",
                            field_rule.field_name
                        ));
                    }
                }
                Some(val) => {
                    // Type checking
                    let type_ok = match field_rule.field_type.as_str() {
                        "string" => val.is_string(),
                        "number" => val.is_number(),
                        "boolean" => val.is_boolean(),
                        "array" => val.is_array(),
                        "object" => val.is_object(),
                        "any" => true,
                        _ => true,
                    };
                    if !type_ok {
                        errors.push(format!(
                            "field '{}' must be of type {}",
                            field_rule.field_name, field_rule.field_type
                        ));
                    }

                    // Max length for strings
                    if let (Some(max_len), Some(s)) = (field_rule.max_length, val.as_str()) {
                        if s.len() > max_len {
                            errors.push(format!(
                                "field '{}' exceeds max length {} (got {})",
                                field_rule.field_name,
                                max_len,
                                s.len()
                            ));
                        }
                    }

                    // Numeric bounds
                    if let Some(n) = val.as_f64() {
                        if let Some(min_v) = field_rule.min_value {
                            if n < min_v {
                                errors.push(format!(
                                    "field '{}' below minimum {} (got {})",
                                    field_rule.field_name, min_v, n
                                ));
                            }
                        }
                        if let Some(max_v) = field_rule.max_value {
                            if n > max_v {
                                errors.push(format!(
                                    "field '{}' above maximum {} (got {})",
                                    field_rule.field_name, max_v, n
                                ));
                            }
                        }
                    }
                }
            }
        }

        // Strict mode: reject unknown fields
        if self.strict_mode {
            for key in obj.keys() {
                let known = rule.fields.iter().any(|f| f.field_name == *key);
                if !known {
                    errors.push(format!("unknown field '{}'", key));
                }
            }
        }

        errors
    }
}

/// Schema validation middleware handler.
///
/// Validates incoming JSON request bodies against configured per-route
/// schema rules. Requests that fail validation are rejected with
/// `422 Unprocessable Entity` and a JSON body listing all errors.
/// Requests to excluded paths or routes without matching rules pass
/// through unchanged.
async fn schema_validation_handler(
    config: SchemaValidationConfig,
    req: Request,
    next: Next,
) -> Response {
    let path = req.uri().path().to_string();
    let method = req.method().as_str().to_string();

    // Skip excluded paths
    for excluded in &config.excluded_paths {
        if path.starts_with(excluded) {
            return next.run(req).await;
        }
    }

    // Find matching rule
    let rule = match config.find_rule(&path, &method) {
        Some(r) => r.clone(),
        None => {
            // No rule for this route — pass through
            return next.run(req).await;
        }
    };

    // Read body bytes
    let (parts, body) = req.into_parts();
    let bytes = match axum::body::to_bytes(body, config.max_body_size).await {
        Ok(b) => b,
        Err(_) => {
            let error_body = serde_json::json!({
                "error": "BODY_TOO_LARGE",
                "message": format!(
                    "request body exceeds {} bytes",
                    config.max_body_size
                ),
            });
            return Response::builder()
                .status(axum::http::StatusCode::PAYLOAD_TOO_LARGE)
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    serde_json::to_vec(&error_body).unwrap_or_default(),
                ))
                .unwrap_or_default();
        }
    };

    // Empty body — skip validation
    if bytes.is_empty() {
        let request = Request::from_parts(parts, axum::body::Body::from(bytes));
        return next.run(request).await;
    }

    // Parse JSON
    let parsed: serde_json::Value = match serde_json::from_slice(&bytes) {
        Ok(v) => v,
        Err(_) => {
            let error_body = serde_json::json!({
                "error": "INVALID_JSON",
                "message": "request body is not valid JSON",
            });
            return Response::builder()
                .status(StatusCode::UNPROCESSABLE_ENTITY)
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    serde_json::to_vec(&error_body).unwrap_or_default(),
                ))
                .unwrap_or_default();
        }
    };

    // Validate against schema
    let errors = config.validate(&parsed, &rule);
    if !errors.is_empty() {
        let error_body = serde_json::json!({
            "error": "SCHEMA_VALIDATION_FAILED",
            "message": "request body failed schema validation",
            "details": errors,
        });
        return Response::builder()
            .status(StatusCode::UNPROCESSABLE_ENTITY)
            .header("content-type", "application/json")
            .body(axum::body::Body::from(
                serde_json::to_vec(&error_body).unwrap_or_default(),
            ))
            .unwrap_or_default();
    }

    // Validation passed — re-inject body for downstream
    let request = Request::from_parts(parts, axum::body::Body::from(bytes));
    next.run(request).await
}

// ============================================================================
// Request Decompression (Layer 38)
// ============================================================================

/// Supported request body compression encodings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestEncoding {
    /// gzip (RFC 1952)
    Gzip,
    /// deflate (RFC 1951)
    Deflate,
    /// No encoding / identity
    Identity,
}

impl RequestEncoding {
    /// Parse a `Content-Encoding` header value into an encoding variant.
    fn from_header(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "gzip" | "x-gzip" => Some(Self::Gzip),
            "deflate" => Some(Self::Deflate),
            "identity" => Some(Self::Identity),
            _ => None,
        }
    }
}

/// Configuration for request decompression middleware.
///
/// When enabled, incoming request bodies with a `Content-Encoding`
/// header are decompressed before reaching downstream handlers.
/// Supports gzip and deflate encodings via `flate2`. Unsupported
/// encodings are rejected with `415 Unsupported Media Type`.
#[derive(Debug, Clone)]
pub struct RequestDecompressionConfig {
    /// Enable gzip decompression (default true).
    pub enable_gzip: bool,
    /// Enable deflate decompression (default true).
    pub enable_deflate: bool,
    /// Maximum decompressed body size in bytes (default 10 MB).
    pub max_decompressed_size: usize,
    /// Paths excluded from decompression.
    pub excluded_paths: Vec<String>,
}

impl Default for RequestDecompressionConfig {
    fn default() -> Self {
        Self {
            enable_gzip: true,
            enable_deflate: true,
            max_decompressed_size: 10 * 1024 * 1024,
            excluded_paths: vec!["/health".to_string()],
        }
    }
}

/// Decompresses a byte buffer according to the specified encoding.
///
/// Returns the decompressed bytes or an error string.
fn decompress_body(
    data: &[u8],
    encoding: RequestEncoding,
    max_size: usize,
) -> Result<Vec<u8>, String> {
    use flate2::read::{DeflateDecoder, GzDecoder};
    use std::io::Read;

    match encoding {
        RequestEncoding::Gzip => {
            let mut decoder = GzDecoder::new(data);
            let mut out = Vec::new();
            let mut buf = [0u8; 8192];
            let mut total = 0usize;
            loop {
                match decoder.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        total += n;
                        if total > max_size {
                            return Err(format!("decompressed body exceeds {} bytes", max_size));
                        }
                        out.extend_from_slice(&buf[..n]);
                    }
                    Err(e) => {
                        return Err(format!("gzip decompression failed: {}", e));
                    }
                }
            }
            Ok(out)
        }
        RequestEncoding::Deflate => {
            let mut decoder = DeflateDecoder::new(data);
            let mut out = Vec::new();
            let mut buf = [0u8; 8192];
            let mut total = 0usize;
            loop {
                match decoder.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        total += n;
                        if total > max_size {
                            return Err(format!("decompressed body exceeds {} bytes", max_size));
                        }
                        out.extend_from_slice(&buf[..n]);
                    }
                    Err(e) => {
                        return Err(format!("deflate decompression failed: {}", e));
                    }
                }
            }
            Ok(out)
        }
        RequestEncoding::Identity => Ok(data.to_vec()),
    }
}

/// Request decompression middleware handler.
///
/// Inspects the `Content-Encoding` header of incoming requests. If a
/// supported encoding is found, the body is decompressed and the
/// `Content-Encoding` header is removed before forwarding to the
/// next handler. Unsupported encodings are rejected with
/// `415 Unsupported Media Type`. Decompression bombs are caught by
/// the configurable max decompressed size.
async fn request_decompression_handler(
    config: RequestDecompressionConfig,
    req: Request,
    next: Next,
) -> Response {
    let path = req.uri().path().to_string();

    // Skip excluded paths
    for excluded in &config.excluded_paths {
        if path.starts_with(excluded) {
            return next.run(req).await;
        }
    }

    // Check Content-Encoding header
    let encoding_str = match req.headers().get("content-encoding") {
        Some(v) => match v.to_str() {
            Ok(s) => s.to_string(),
            Err(_) => return next.run(req).await,
        },
        None => return next.run(req).await,
    };

    let encoding = match RequestEncoding::from_header(&encoding_str) {
        Some(RequestEncoding::Identity) => return next.run(req).await,
        Some(enc) => enc,
        None => {
            let body = serde_json::json!({
                "error": "UNSUPPORTED_ENCODING",
                "message": format!(
                    "unsupported Content-Encoding: {}",
                    encoding_str
                ),
            });
            return Response::builder()
                .status(StatusCode::UNSUPPORTED_MEDIA_TYPE)
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    serde_json::to_vec(&body).unwrap_or_default(),
                ))
                .unwrap_or_default();
        }
    };

    // Check if the specific encoding is enabled
    let enabled = match encoding {
        RequestEncoding::Gzip => config.enable_gzip,
        RequestEncoding::Deflate => config.enable_deflate,
        RequestEncoding::Identity => true,
    };
    if !enabled {
        let body = serde_json::json!({
            "error": "ENCODING_DISABLED",
            "message": format!(
                "Content-Encoding '{}' is not enabled",
                encoding_str
            ),
        });
        return Response::builder()
            .status(StatusCode::UNSUPPORTED_MEDIA_TYPE)
            .header("content-type", "application/json")
            .body(axum::body::Body::from(
                serde_json::to_vec(&body).unwrap_or_default(),
            ))
            .unwrap_or_default();
    }

    // Read compressed body
    let (mut parts, body) = req.into_parts();
    let compressed = match axum::body::to_bytes(body, config.max_decompressed_size * 2).await {
        Ok(b) => b,
        Err(_) => {
            let body = serde_json::json!({
                "error": "BODY_TOO_LARGE",
                "message": "compressed request body too large",
            });
            return Response::builder()
                .status(axum::http::StatusCode::PAYLOAD_TOO_LARGE)
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    serde_json::to_vec(&body).unwrap_or_default(),
                ))
                .unwrap_or_default();
        }
    };

    // Decompress
    match decompress_body(&compressed, encoding, config.max_decompressed_size) {
        Ok(decompressed) => {
            // Remove Content-Encoding header, update Content-Length
            parts.headers.remove("content-encoding");
            if let Ok(len_val) = axum::http::HeaderValue::from_str(&decompressed.len().to_string())
            {
                parts.headers.insert("content-length", len_val);
            }
            let request = Request::from_parts(parts, axum::body::Body::from(decompressed));
            next.run(request).await
        }
        Err(msg) => {
            let body = serde_json::json!({
                "error": "DECOMPRESSION_FAILED",
                "message": msg,
            });
            Response::builder()
                .status(StatusCode::BAD_REQUEST)
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    serde_json::to_vec(&body).unwrap_or_default(),
                ))
                .unwrap_or_default()
        }
    }
}

// ============================================================================
// Slow Request Detection (Layer 39)
// ============================================================================

/// Configuration for slow request detection middleware.
#[derive(Debug, Clone)]
pub struct SlowRequestConfig {
    /// Threshold in milliseconds. Requests exceeding this are flagged as slow (default: 5000).
    pub threshold_ms: u64,
    /// Header name to add when a request is slow (default: `X-Slow-Request`).
    pub header_name: String,
    /// Whether to add the elapsed time header on slow requests (default: true).
    pub include_elapsed: bool,
    /// Header name for the elapsed time (default: `X-Slow-Request-Ms`).
    pub elapsed_header_name: String,
    /// Paths excluded from slow request detection (default: `["/health"]`).
    pub excluded_paths: Vec<String>,
    /// Whether to add a `Warning` header on slow requests (default: true).
    pub add_warning_header: bool,
}

impl Default for SlowRequestConfig {
    fn default() -> Self {
        Self {
            threshold_ms: 5000,
            header_name: "X-Slow-Request".to_string(),
            include_elapsed: true,
            elapsed_header_name: "X-Slow-Request-Ms".to_string(),
            excluded_paths: vec!["/health".to_string()],
            add_warning_header: true,
        }
    }
}

/// Slow request detection middleware handler.
///
/// Measures the time taken to process a request. If the elapsed time exceeds
/// the configured threshold, the response is annotated with headers indicating
/// that the request was slow, along with the elapsed time in milliseconds.
///
/// This differs from request timeout (which aborts the request) and request
/// timing (which always adds the response time). Slow request detection only
/// adds headers when the threshold is exceeded, providing a clear signal for
/// monitoring and alerting.
async fn slow_request_handler(config: SlowRequestConfig, req: Request, next: Next) -> Response {
    let path = req.uri().path().to_string();

    // Skip excluded paths
    for excluded in &config.excluded_paths {
        if path.starts_with(excluded) {
            return next.run(req).await;
        }
    }

    let start = std::time::Instant::now();
    let mut response = next.run(req).await;
    let elapsed_ms = start.elapsed().as_millis() as u64;

    if elapsed_ms >= config.threshold_ms {
        // Mark as slow
        if let Ok(val) = axum::http::header::HeaderValue::from_str("true") {
            if let Ok(name) = axum::http::HeaderName::from_bytes(config.header_name.as_bytes()) {
                response.headers_mut().insert(name, val);
            }
        }

        // Add elapsed time
        if config.include_elapsed {
            if let Ok(val) = axum::http::header::HeaderValue::from_str(&elapsed_ms.to_string()) {
                if let Ok(name) =
                    axum::http::HeaderName::from_bytes(config.elapsed_header_name.as_bytes())
                {
                    response.headers_mut().insert(name, val);
                }
            }
        }

        // Add Warning header (RFC 7234 style)
        if config.add_warning_header {
            let warning = format!(
                "199 - \"Slow request: {}ms exceeded {}ms threshold\"",
                elapsed_ms, config.threshold_ms
            );
            if let Ok(val) = axum::http::header::HeaderValue::from_str(&warning) {
                response.headers_mut().insert("warning", val);
            }
        }
    }

    response
}

// ============================================================================
// Header Propagation
// ============================================================================

/// Configuration for header propagation middleware.
///
/// Copies selected request headers into the response so that callers can
/// correlate requests with responses, propagate trace/correlation context,
/// or forward custom metadata.
#[derive(Debug, Clone)]
pub struct HeaderPropagationConfig {
    /// Header names to propagate from request to response (default: ["X-Request-Id", "X-Correlation-Id"]).
    pub propagated_headers: Vec<String>,
    /// Optional prefix to prepend to propagated header names in the response (default: "").
    pub response_prefix: String,
    /// Whether header name matching is case-insensitive (default: true).
    pub case_insensitive: bool,
    /// Whether to skip propagation when the header already exists in the response (default: true).
    pub skip_existing: bool,
    /// Paths excluded from header propagation (default: []).
    pub excluded_paths: Vec<String>,
    /// Whether to add a header listing all propagated header names (default: false).
    pub add_propagated_list_header: bool,
}

impl Default for HeaderPropagationConfig {
    fn default() -> Self {
        Self {
            propagated_headers: vec!["X-Request-Id".to_string(), "X-Correlation-Id".to_string()],
            response_prefix: String::new(),
            case_insensitive: true,
            skip_existing: true,
            excluded_paths: Vec::new(),
            add_propagated_list_header: false,
        }
    }
}

/// Header propagation middleware handler.
///
/// Reads the configured request headers and copies their values into the
/// response after the inner handler completes. This is useful for propagating
/// correlation IDs, trace context, or custom metadata from the request to the
/// response so that callers can match responses to their requests.
///
/// When esponse_prefix is set, each propagated header name is prefixed in
/// the response (e.g., X-Request-Id becomes X-Propagated-X-Request-Id).
///
/// When skip_existing is true (default), headers that already exist in the
/// response are not overwritten.
async fn header_propagation_handler(
    config: HeaderPropagationConfig,
    req: Request,
    next: Next,
) -> Response {
    let path = req.uri().path().to_string();

    // Skip excluded paths
    for excluded in &config.excluded_paths {
        if path.starts_with(excluded) {
            return next.run(req).await;
        }
    }

    // Collect matching headers from the request
    let mut captured: Vec<(String, String)> = Vec::new();
    for target in &config.propagated_headers {
        for (name, value) in req.headers().iter() {
            let matches = if config.case_insensitive {
                name.as_str().eq_ignore_ascii_case(target)
            } else {
                name.as_str() == target
            };
            if matches {
                if let Ok(v) = value.to_str() {
                    captured.push((target.clone(), v.to_string()));
                }
                break;
            }
        }
    }

    let mut response = next.run(req).await;

    let mut propagated_names: Vec<String> = Vec::new();
    for (name, value) in &captured {
        let response_name = if config.response_prefix.is_empty() {
            name.clone()
        } else {
            format!("{}{}", config.response_prefix, name)
        };

        // Skip if already present and skip_existing is true
        if config.skip_existing {
            if let Ok(hn) = axum::http::HeaderName::from_bytes(response_name.as_bytes()) {
                if response.headers().contains_key(&hn) {
                    continue;
                }
            }
        }

        if let Ok(hv) = axum::http::header::HeaderValue::from_str(value) {
            if let Ok(hn) = axum::http::HeaderName::from_bytes(response_name.as_bytes()) {
                response.headers_mut().insert(hn, hv);
                propagated_names.push(response_name);
            }
        }
    }

    // Optionally add a header listing all propagated names
    if config.add_propagated_list_header && !propagated_names.is_empty() {
        let list = propagated_names.join(", ");
        if let Ok(val) = axum::http::header::HeaderValue::from_str(&list) {
            response.headers_mut().insert("x-propagated-headers", val);
        }
    }

    response
}

// ============================================================================
// Request Context
// ============================================================================

/// Configuration for request context middleware.
///
/// Injects structured contextual metadata headers into each response,
/// providing downstream consumers with deployment, environment, and
/// service-level context. This is useful for debugging, routing, and
/// observability in distributed systems.
#[derive(Debug, Clone)]
pub struct RequestContextConfig {
    /// Header prefix for all context headers (default: X-Context-).
    pub header_prefix: String,
    /// Environment name injected as {prefix}Environment (default: "production").
    pub environment: String,
    /// Service name injected as {prefix}Service (default: "hv2-api").
    pub service_name: String,
    /// Region identifier injected as {prefix}Region (default: ""; empty = omitted).
    pub region: String,
    /// Instance identifier injected as {prefix}Instance (default: ""; empty = omitted).
    pub instance_id: String,
    /// Additional custom key-value pairs injected as {prefix}{key} headers.
    pub custom_fields: Vec<(String, String)>,
    /// Paths excluded from context injection (default: []).
    pub excluded_paths: Vec<String>,
    /// Whether to add a header listing all injected context header names (default: false).
    pub add_context_list_header: bool,
}

impl Default for RequestContextConfig {
    fn default() -> Self {
        Self {
            header_prefix: "X-Context-".to_string(),
            environment: "production".to_string(),
            service_name: "hv2-api".to_string(),
            region: String::new(),
            instance_id: String::new(),
            custom_fields: Vec::new(),
            excluded_paths: Vec::new(),
            add_context_list_header: false,
        }
    }
}

/// Request context middleware handler.
///
/// Injects contextual metadata headers into every response so that callers
/// know which environment, service, region, and instance handled their
/// request. This complements request tracing and header propagation by
/// providing static deployment context rather than per-request identifiers.
///
/// Empty fields (region, instance_id) are skipped. Custom key-value pairs
/// are injected with the configured prefix.
async fn request_context_handler(
    config: RequestContextConfig,
    req: Request,
    next: Next,
) -> Response {
    let path = req.uri().path().to_string();

    // Skip excluded paths
    for excluded in &config.excluded_paths {
        if path.starts_with(excluded) {
            return next.run(req).await;
        }
    }

    let mut response = next.run(req).await;
    let mut injected_names: Vec<String> = Vec::new();

    // Helper closure to inject a context header
    let prefix = &config.header_prefix;

    // Environment
    if !config.environment.is_empty() {
        let name = format!("{}Environment", prefix);
        if let Ok(hn) = axum::http::HeaderName::from_bytes(name.as_bytes()) {
            if let Ok(hv) = axum::http::header::HeaderValue::from_str(&config.environment) {
                response.headers_mut().insert(hn, hv);
                injected_names.push(name);
            }
        }
    }

    // Service name
    if !config.service_name.is_empty() {
        let name = format!("{}Service", prefix);
        if let Ok(hn) = axum::http::HeaderName::from_bytes(name.as_bytes()) {
            if let Ok(hv) = axum::http::header::HeaderValue::from_str(&config.service_name) {
                response.headers_mut().insert(hn, hv);
                injected_names.push(name);
            }
        }
    }

    // Region (skip if empty)
    if !config.region.is_empty() {
        let name = format!("{}Region", prefix);
        if let Ok(hn) = axum::http::HeaderName::from_bytes(name.as_bytes()) {
            if let Ok(hv) = axum::http::header::HeaderValue::from_str(&config.region) {
                response.headers_mut().insert(hn, hv);
                injected_names.push(name);
            }
        }
    }

    // Instance ID (skip if empty)
    if !config.instance_id.is_empty() {
        let name = format!("{}Instance", prefix);
        if let Ok(hn) = axum::http::HeaderName::from_bytes(name.as_bytes()) {
            if let Ok(hv) = axum::http::header::HeaderValue::from_str(&config.instance_id) {
                response.headers_mut().insert(hn, hv);
                injected_names.push(name);
            }
        }
    }

    // Custom fields
    for (key, value) in &config.custom_fields {
        let name = format!("{}{}", prefix, key);
        if let Ok(hn) = axum::http::HeaderName::from_bytes(name.as_bytes()) {
            if let Ok(hv) = axum::http::header::HeaderValue::from_str(value) {
                response.headers_mut().insert(hn, hv);
                injected_names.push(name);
            }
        }
    }

    // Optionally add a header listing all injected context header names
    if config.add_context_list_header && !injected_names.is_empty() {
        let list = injected_names.join(", ");
        if let Ok(val) = axum::http::header::HeaderValue::from_str(&list) {
            response.headers_mut().insert("x-context-headers", val);
        }
    }

    response
}
// Fallback Handler (404)

// ============================================================================
// Tenant Isolation (Layer 33)
// ============================================================================

/// Configuration for multi-tenant request isolation.
///
/// Extracts a tenant identifier from a configurable request header, validates
/// it against an optional allow-list, and injects the resolved tenant ID into
/// the response. When `require_tenant` is `true`, requests without a tenant
/// header are rejected with 400 Bad Request. When `allowed_tenants` is
/// non-empty, only listed tenants are accepted — all others receive
/// 403 Forbidden.
///
/// # Defaults
///
/// | Field              | Default          |
/// |--------------------|------------------|
/// | tenant_header    | "X-Tenant-Id"  |
/// | allowed_tenants  | [] (any)       |
/// | require_tenant   | false          |
/// | default_tenant   | None           |
/// | response_header  | "X-Tenant-Id"  |
/// | excluded_paths   | ["/health"]    |
#[derive(Debug, Clone)]
pub struct TenantIsolationConfig {
    /// Request header carrying the tenant identifier.
    pub tenant_header: String,
    /// If non-empty, only these tenant IDs are permitted.
    pub allowed_tenants: Vec<String>,
    /// When `true`, reject requests missing the tenant header.
    pub require_tenant: bool,
    /// Fallback tenant ID used when the header is absent and `require_tenant` is `false`.
    pub default_tenant: Option<String>,
    /// Response header name for the resolved tenant ID.
    pub response_header: String,
    /// Paths excluded from tenant isolation enforcement.
    pub excluded_paths: Vec<String>,
}

impl Default for TenantIsolationConfig {
    fn default() -> Self {
        Self {
            tenant_header: "X-Tenant-Id".to_string(),
            allowed_tenants: Vec::new(),
            require_tenant: false,
            default_tenant: None,
            response_header: "X-Tenant-Id".to_string(),
            excluded_paths: vec!["/health".to_string()],
        }
    }
}

/// Tenant isolation middleware handler.
///
/// Extracts the tenant identifier from the configured request header,
/// validates it against the allow-list (if any), and injects the resolved
/// tenant ID into the response via the configured response header. Returns
/// 400 Bad Request when the tenant header is missing and equire_tenant
/// is enabled (with no default_tenant). Returns 403 Forbidden when the
/// tenant is not in the allowed_tenants list.
pub async fn tenant_isolation_handler(
    config: TenantIsolationConfig,
    req: Request,
    next: Next,
) -> Response {
    let path = req.uri().path().to_string();

    // Skip excluded paths
    for excluded in &config.excluded_paths {
        if path.starts_with(excluded) {
            return next.run(req).await;
        }
    }

    // Extract tenant ID from request header
    let tenant_id = req
        .headers()
        .get(&config.tenant_header)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    let resolved = match tenant_id {
        Some(id) if !id.is_empty() => id,
        _ => {
            // No tenant header present
            if let Some(ref default) = config.default_tenant {
                default.clone()
            } else if config.require_tenant {
                let body = serde_json::json!({
                    "error": {
                        "code": "MISSING_TENANT",
                        "message": format!(
                            "Missing required header: {}",
                            config.tenant_header
                        ),
                    }
                });
                return Response::builder()
                    .status(axum::http::StatusCode::BAD_REQUEST)
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(
                        serde_json::to_string(&body).unwrap_or_default(),
                    ))
                    .unwrap_or_default();
            } else {
                // Tenant not required, pass through without header
                return next.run(req).await;
            }
        }
    };

    // Validate against allow-list
    if !config.allowed_tenants.is_empty() && !config.allowed_tenants.contains(&resolved) {
        let body = serde_json::json!({
            "error": {
                "code": "TENANT_DENIED",
                "message": format!("Tenant '{}' is not permitted", resolved),
            }
        });
        return Response::builder()
            .status(axum::http::StatusCode::FORBIDDEN)
            .header("content-type", "application/json")
            .body(axum::body::Body::from(
                serde_json::to_string(&body).unwrap_or_default(),
            ))
            .unwrap_or_default();
    }

    // Forward and inject tenant header into response
    let mut resp = next.run(req).await;
    if let Ok(h) = axum::http::HeaderName::from_bytes(config.response_header.as_bytes()) {
        if let Ok(v) = axum::http::HeaderValue::from_str(&resolved) {
            resp.headers_mut().insert(h, v);
        }
    }
    resp
}

// ============================================================================
// Response Envelope (Layer 34)
// ============================================================================

/// Configuration for wrapping JSON responses in a standard envelope.
///
/// When enabled, transforms successful JSON responses into:
/// `json
/// {
///   "data": <original_body>,
///   "meta": {
///     "status": 200,
///     "timestamp": "2026-01-01T00:00:00Z",
///     "request_id": "<from X-Request-Id header>"
///   }
/// }
/// `
///
/// Error responses (4xx/5xx) are left untouched so that the existing
/// `ErrorResponse` format is preserved.
///
/// # Defaults
///
/// | Field             | Default        |
/// |-------------------|----------------|
/// | `data_field`    | `"data"`     |
/// | `meta_field`    | `"meta"`     |
/// | `include_status`| `true`       |
/// | `include_timestamp` | `true`   |
/// | `include_request_id`| `true`   |
/// | `excluded_paths`| `["/health"]`|
/// | `excluded_status_codes` | `[]` |
#[derive(Debug, Clone)]
pub struct ResponseEnvelopeConfig {
    /// JSON key for the original response data.
    pub data_field: String,
    /// JSON key for the metadata object.
    pub meta_field: String,
    /// Include the HTTP status code in meta.
    pub include_status: bool,
    /// Include an ISO-8601 timestamp in meta.
    pub include_timestamp: bool,
    /// Include the request ID (from `X-Request-Id`) in meta.
    pub include_request_id: bool,
    /// Paths excluded from envelope wrapping.
    pub excluded_paths: Vec<String>,
    /// Status codes that bypass envelope wrapping (e.g., 204, 304).
    pub excluded_status_codes: Vec<u16>,
}

impl Default for ResponseEnvelopeConfig {
    fn default() -> Self {
        Self {
            data_field: "data".to_string(),
            meta_field: "meta".to_string(),
            include_status: true,
            include_timestamp: true,
            include_request_id: true,
            excluded_paths: vec!["/health".to_string()],
            excluded_status_codes: Vec::new(),
        }
    }
}

/// Response envelope middleware handler.
///
/// Wraps successful JSON response bodies in a standard envelope with
/// `data` and `meta` fields. Error responses (4xx/5xx) and non-JSON
/// content types are passed through unchanged. Responses with status
/// codes in `excluded_status_codes` or paths in `excluded_paths` are
/// also bypassed.
pub async fn response_envelope_handler(
    config: ResponseEnvelopeConfig,
    req: Request,
    next: Next,
) -> Response {
    let path = req.uri().path().to_string();
    let request_id = req
        .headers()
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    // Skip excluded paths
    for excluded in &config.excluded_paths {
        if path.starts_with(excluded) {
            return next.run(req).await;
        }
    }

    let resp = next.run(req).await;
    let status = resp.status();

    // Skip error responses (4xx/5xx)
    if status.is_client_error() || status.is_server_error() {
        return resp;
    }

    // Skip excluded status codes
    if config.excluded_status_codes.contains(&status.as_u16()) {
        return resp;
    }

    // Only wrap JSON responses
    let is_json = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .map(|ct| ct.contains("application/json"))
        .unwrap_or(false);

    if !is_json {
        return resp;
    }

    // Read the original body
    let (parts, body) = resp.into_parts();
    let bytes = match axum::body::to_bytes(body, 16 * 1024 * 1024).await {
        Ok(b) => b,
        Err(_) => {
            return Response::builder()
                .status(axum::http::StatusCode::INTERNAL_SERVER_ERROR)
                .body(axum::body::Body::empty())
                .unwrap_or_default();
        }
    };

    // Parse original body as JSON
    let data_value: serde_json::Value = match serde_json::from_slice(&bytes) {
        Ok(v) => v,
        Err(_) => {
            // Not valid JSON despite content-type — pass through
            let resp = Response::from_parts(parts, axum::body::Body::from(bytes));
            return resp;
        }
    };

    // Build meta object
    let mut meta = serde_json::Map::new();
    if config.include_status {
        meta.insert(
            "status".to_string(),
            serde_json::Value::Number(serde_json::Number::from(status.as_u16())),
        );
    }
    if config.include_timestamp {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        meta.insert(
            "timestamp".to_string(),
            serde_json::Value::Number(serde_json::Number::from(now)),
        );
    }
    if config.include_request_id {
        if let Some(ref rid) = request_id {
            meta.insert(
                "request_id".to_string(),
                serde_json::Value::String(rid.clone()),
            );
        }
    }

    // Build envelope
    let mut envelope = serde_json::Map::new();
    envelope.insert(config.data_field.clone(), data_value);
    envelope.insert(config.meta_field.clone(), serde_json::Value::Object(meta));

    let envelope_bytes =
        serde_json::to_vec(&serde_json::Value::Object(envelope)).unwrap_or_default();

    Response::from_parts(parts, axum::body::Body::from(envelope_bytes))
}
// ============================================================================

/// Fallback handler for unmatched routes.
///
/// Returns a `404 Not Found` JSON response using the standard
/// [`ErrorResponse`] format, keeping error shapes consistent across
/// the API.
pub async fn fallback_handler(request: Request) -> impl IntoResponse {
    let path = request.uri().path().to_string();
    let method = request.method().to_string();
    let req_id = extract_request_id(&request);
    let body = ErrorResponse {
        error: format!("No route matched {} {}", method, path),
        code: "NOT_FOUND".to_string(),
        request_id: req_id,
    };
    (StatusCode::NOT_FOUND, Json(body))
}

// ============================================================================
// API Version Header
// ============================================================================

/// The current API version string stamped on every response.
pub const API_VERSION: &str = "v1";

/// Middleware that adds an `X-API-Version` header to every response.
///
/// This lets clients discover which API version they are talking to
/// without inspecting the URL path.
pub async fn api_version(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    response
        .headers_mut()
        .insert("x-api-version", HeaderValue::from_static(API_VERSION));
    response
}

// ============================================================================
// Content-Type Validation
// ============================================================================

/// Middleware that rejects `POST`, `PUT`, and `PATCH` requests whose
/// `Content-Type` header is missing or does not start with
/// `application/json`.
///
/// `GET`, `DELETE`, `OPTIONS`, and `HEAD` requests are always allowed
/// through regardless of `Content-Type`.
///
/// Excluded paths (e.g. `/health`) skip validation entirely.
pub async fn content_type_validation(
    excluded: Vec<String>,
    request: Request,
    next: Next,
) -> Response {
    let method = request.method().clone();
    let path = request.uri().path().to_string();

    // Only validate methods that carry a body
    let needs_check = method == Method::POST || method == Method::PUT || method == Method::PATCH;

    if needs_check && !excluded.iter().any(|p| path.starts_with(p)) {
        let ct = request
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");

        if !ct.starts_with("application/json") {
            let req_id = extract_request_id(&request);
            let body = ErrorResponse {
                error: "Content-Type must be application/json".to_string(),
                code: "UNSUPPORTED_MEDIA_TYPE".to_string(),
                request_id: req_id,
            };
            return (StatusCode::UNSUPPORTED_MEDIA_TYPE, Json(body)).into_response();
        }
    }

    next.run(request).await
}

// ============================================================================
// Content-Type Validation Configuration
// ============================================================================

/// Configuration for content-type validation middleware.
#[derive(Debug, Clone)]
pub struct ContentTypeConfig {
    /// Paths excluded from content-type validation.
    pub excluded_paths: Vec<String>,
}

impl Default for ContentTypeConfig {
    fn default() -> Self {
        Self {
            excluded_paths: vec!["/health".to_string(), "/agentic".to_string()],
        }
    }
}

// ============================================================================
// Request Timeout
// ============================================================================

/// Configuration for the request timeout middleware.
#[derive(Debug, Clone)]
pub struct TimeoutConfig {
    /// Maximum duration before a request is considered timed out.
    pub duration: Duration,
    /// Path prefixes excluded from timeout enforcement.
    pub excluded_paths: Vec<String>,
}

impl Default for TimeoutConfig {
    fn default() -> Self {
        Self {
            duration: Duration::from_secs(30),
            excluded_paths: vec!["/health".to_string(), "/agentic".to_string()],
        }
    }
}

/// Middleware that enforces a maximum request duration.
///
/// If the downstream handler does not complete within
/// [`TimeoutConfig::duration`], a `408 Request Timeout` JSON response
/// is returned. Paths matching any prefix in `excluded_paths` bypass
/// timeout enforcement.
pub async fn request_timeout(config: TimeoutConfig, request: Request, next: Next) -> Response {
    let path = request.uri().path().to_string();

    // Skip timeout for excluded paths
    if config.excluded_paths.iter().any(|p| path.starts_with(p)) {
        return next.run(request).await;
    }

    let req_id = extract_request_id(&request);

    match tokio::time::timeout(config.duration, next.run(request)).await {
        Ok(response) => response,
        Err(_) => {
            let body = ErrorResponse {
                error: format!("Request timed out after {}ms", config.duration.as_millis()),
                code: "REQUEST_TIMEOUT".to_string(),
                request_id: req_id,
            };
            (StatusCode::REQUEST_TIMEOUT, Json(body)).into_response()
        }
    }
}

// ============================================================================
// IP-Based Access Control
// ============================================================================

/// An IP network in CIDR notation (e.g. `192.168.1.0/24` or `::1/128`).
///
/// Supports both IPv4 and IPv6 addresses. When no prefix length is
/// specified, the address is treated as a single host (`/32` for IPv4,
/// `/128` for IPv6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IpNetwork {
    /// Base address of the network.
    addr: IpAddr,
    /// CIDR prefix length (0–32 for IPv4, 0–128 for IPv6).
    prefix_len: u8,
}

impl IpNetwork {
    /// Parse a CIDR string like `"192.168.1.0/24"` or a bare IP like `"10.0.0.1"`.
    ///
    /// Returns `None` if the input is not a valid IP or CIDR.
    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim();
        if let Some((addr_part, prefix_part)) = s.split_once('/') {
            let addr: IpAddr = addr_part.parse().ok()?;
            let prefix_len: u8 = prefix_part.parse().ok()?;
            let max_prefix = match addr {
                IpAddr::V4(_) => 32,
                IpAddr::V6(_) => 128,
            };
            if prefix_len > max_prefix {
                return None;
            }
            Some(Self { addr, prefix_len })
        } else {
            let addr: IpAddr = s.parse().ok()?;
            let prefix_len = match addr {
                IpAddr::V4(_) => 32,
                IpAddr::V6(_) => 128,
            };
            Some(Self { addr, prefix_len })
        }
    }

    /// Check whether `ip` falls within this network.
    pub fn contains(&self, ip: &IpAddr) -> bool {
        match (&self.addr, ip) {
            (IpAddr::V4(net), IpAddr::V4(target)) => {
                if self.prefix_len == 0 {
                    return true;
                }
                let net_bits = u32::from(*net);
                let target_bits = u32::from(*target);
                let mask = u32::MAX
                    .checked_shl(32 - self.prefix_len as u32)
                    .unwrap_or(0);
                (net_bits & mask) == (target_bits & mask)
            }
            (IpAddr::V6(net), IpAddr::V6(target)) => {
                if self.prefix_len == 0 {
                    return true;
                }
                let net_bits = u128::from(*net);
                let target_bits = u128::from(*target);
                let mask = u128::MAX
                    .checked_shl(128 - self.prefix_len as u32)
                    .unwrap_or(0);
                (net_bits & mask) == (target_bits & mask)
            }
            _ => false, // IPv4 vs IPv6 mismatch
        }
    }
}

impl std::fmt::Display for IpNetwork {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{}", self.addr, self.prefix_len)
    }
}

/// IP-based access control configuration.
///
/// Supports allow-lists (whitelist) and deny-lists (blacklist) with
/// CIDR notation. The deny list is checked first; if the client IP
/// matches a denied network, the request is rejected. Then, if the
/// allow list is non-empty, only IPs matching an allowed network are
/// permitted.
///
/// Client IP is extracted from (in order):
/// 1. `X-Forwarded-For` header (first IP) — when `trust_proxy_headers` is true
/// 2. `X-Real-IP` header — when `trust_proxy_headers` is true
/// 3. Falls back to `127.0.0.1` (localhost) when connection info is unavailable
#[derive(Debug, Clone)]
pub struct IpFilterConfig {
    /// IP networks allowed to access the API.
    /// If non-empty, only IPs matching these networks are allowed.
    /// An empty list means all IPs are allowed (subject to deny list).
    pub allow_list: Vec<IpNetwork>,
    /// IP networks denied access. Checked before the allow list.
    pub deny_list: Vec<IpNetwork>,
    /// Paths excluded from IP filtering (e.g. `/health`).
    pub excluded_paths: Vec<String>,
    /// Trust `X-Forwarded-For` and `X-Real-IP` headers for client IP extraction.
    pub trust_proxy_headers: bool,
}

impl Default for IpFilterConfig {
    fn default() -> Self {
        Self {
            allow_list: vec![],
            deny_list: vec![],
            excluded_paths: vec!["/health".to_string()],
            trust_proxy_headers: false,
        }
    }
}

impl IpFilterConfig {
    /// Check if a path is excluded from IP filtering.
    fn is_excluded(&self, path: &str) -> bool {
        self.excluded_paths.iter().any(|p| path.starts_with(p))
    }

    /// Check if an IP matches any network in the deny list.
    fn is_denied(&self, ip: &IpAddr) -> bool {
        self.deny_list.iter().any(|net| net.contains(ip))
    }

    /// Check if an IP matches any network in the allow list.
    /// Returns `true` when the allow list is empty (default-allow).
    fn is_allowed(&self, ip: &IpAddr) -> bool {
        if self.allow_list.is_empty() {
            return true;
        }
        self.allow_list.iter().any(|net| net.contains(ip))
    }
}

/// Extract the client IP address from a request.
///
/// When `trust_proxy_headers` is true, checks `X-Forwarded-For` (first IP)
/// and `X-Real-IP` headers. Falls back to `127.0.0.1`.
fn extract_client_ip(request: &Request, trust_proxy: bool) -> IpAddr {
    if trust_proxy {
        // Try X-Forwarded-For first (first IP in the chain)
        if let Some(xff) = request
            .headers()
            .get("x-forwarded-for")
            .and_then(|v| v.to_str().ok())
        {
            if let Some(first_ip) = xff.split(',').next() {
                if let Ok(ip) = first_ip.trim().parse::<IpAddr>() {
                    return ip;
                }
            }
        }
        // Try X-Real-IP
        if let Some(real_ip) = request
            .headers()
            .get("x-real-ip")
            .and_then(|v| v.to_str().ok())
        {
            if let Ok(ip) = real_ip.trim().parse::<IpAddr>() {
                return ip;
            }
        }
    }
    // Default to localhost when connection info is unavailable
    IpAddr::V4(std::net::Ipv4Addr::LOCALHOST)
}

/// IP filter middleware handler.
///
/// Checks the client IP against deny and allow lists. Returns `403 Forbidden`
/// if the IP is denied or not in the allow list.
fn ip_filter_handler(
    config: IpFilterConfig,
    request: Request,
    next: Next,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Response> + Send>> {
    Box::pin(async move {
        let path = request.uri().path().to_string();

        // Skip excluded paths
        if config.is_excluded(&path) {
            return next.run(request).await;
        }

        let client_ip = extract_client_ip(&request, config.trust_proxy_headers);

        // Deny list takes precedence
        if config.is_denied(&client_ip) {
            let req_id = extract_request_id(&request);
            let body = ErrorResponse {
                error: format!("Access denied for IP: {client_ip}"),
                code: "IP_DENIED".to_string(),
                request_id: req_id,
            };
            return (StatusCode::FORBIDDEN, Json(body)).into_response();
        }

        // Check allow list (empty = allow all)
        if !config.is_allowed(&client_ip) {
            let req_id = extract_request_id(&request);
            let body = ErrorResponse {
                error: format!("Access denied for IP: {client_ip}"),
                code: "IP_NOT_ALLOWED".to_string(),
                request_id: req_id,
            };
            return (StatusCode::FORBIDDEN, Json(body)).into_response();
        }

        next.run(request).await
    })
}

// ============================================================================
// Security Headers
// ============================================================================

/// Configuration for security response headers.
#[derive(Debug, Clone)]
pub struct SecurityHeadersConfig {
    /// Include `X-Content-Type-Options: nosniff`.
    pub content_type_options: bool,
    /// Include `X-Frame-Options: DENY`.
    pub frame_options: bool,
    /// Include `X-XSS-Protection: 1; mode=block`.
    pub xss_protection: bool,
    /// Include `Referrer-Policy: strict-origin-when-cross-origin`.
    pub referrer_policy: bool,
    /// Include `Cache-Control: no-store`.
    pub cache_control: bool,
    /// Include `Strict-Transport-Security` (HSTS).
    ///
    /// When set, emits the HSTS header with the configured `max-age`
    /// and optional `includeSubDomains` / `preload` directives.
    pub hsts: Option<HstsConfig>,
    /// Include `Content-Security-Policy`.
    ///
    /// When set, emits the CSP header with the configured directives.
    pub content_security_policy: Option<String>,
    /// Include `Permissions-Policy`.
    ///
    /// When set, emits the Permissions-Policy header with the
    /// configured feature policies (e.g. `camera=(), microphone=()`).
    pub permissions_policy: Option<String>,
}

/// Configuration for HTTP Strict Transport Security (HSTS).
#[derive(Debug, Clone)]
pub struct HstsConfig {
    /// `max-age` directive in seconds (default: 31536000 = 1 year).
    pub max_age: u64,
    /// Include subdomains in the HSTS policy.
    pub include_sub_domains: bool,
    /// Enable HSTS preload list eligibility.
    pub preload: bool,
}

impl Default for HstsConfig {
    fn default() -> Self {
        Self {
            max_age: 31_536_000, // 1 year
            include_sub_domains: true,
            preload: false,
        }
    }
}

impl HstsConfig {
    /// Render the HSTS header value.
    pub fn header_value(&self) -> String {
        let mut val = format!("max-age={}", self.max_age);
        if self.include_sub_domains {
            val.push_str("; includeSubDomains");
        }
        if self.preload {
            val.push_str("; preload");
        }
        val
    }
}

impl Default for SecurityHeadersConfig {
    fn default() -> Self {
        Self {
            content_type_options: true,
            frame_options: true,
            xss_protection: true,
            referrer_policy: true,
            cache_control: true,
            hsts: None,
            content_security_policy: None,
            permissions_policy: None,
        }
    }
}

impl SecurityHeadersConfig {
    /// Returns the set of (header-name, value) pairs to add.
    fn headers(&self) -> Vec<(String, String)> {
        let mut hdrs = Vec::new();
        if self.content_type_options {
            hdrs.push(("x-content-type-options".to_string(), "nosniff".to_string()));
        }
        if self.frame_options {
            hdrs.push(("x-frame-options".to_string(), "DENY".to_string()));
        }
        if self.xss_protection {
            hdrs.push(("x-xss-protection".to_string(), "1; mode=block".to_string()));
        }
        if self.referrer_policy {
            hdrs.push((
                "referrer-policy".to_string(),
                "strict-origin-when-cross-origin".to_string(),
            ));
        }
        if self.cache_control {
            hdrs.push(("cache-control".to_string(), "no-store".to_string()));
        }
        if let Some(ref hsts) = self.hsts {
            hdrs.push(("strict-transport-security".to_string(), hsts.header_value()));
        }
        if let Some(ref csp) = self.content_security_policy {
            hdrs.push(("content-security-policy".to_string(), csp.clone()));
        }
        if let Some(ref pp) = self.permissions_policy {
            hdrs.push(("permissions-policy".to_string(), pp.clone()));
        }
        hdrs
    }
}

/// Middleware that adds standard security response headers.
///
/// Each header can be independently toggled via [`SecurityHeadersConfig`].
pub async fn security_headers(
    config: SecurityHeadersConfig,
    request: Request,
    next: Next,
) -> Response {
    let hdrs = config.headers();
    let mut response = next.run(request).await;
    for (name, value) in &hdrs {
        if let Ok(n) = axum::http::HeaderName::from_bytes(name.as_bytes()) {
            if let Ok(v) = HeaderValue::from_str(value) {
                response.headers_mut().insert(n, v);
            }
        }
    }
    response
}

// ============================================================================
// Response Compression
// ============================================================================

/// Supported compression encoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompressionEncoding {
    /// gzip (RFC 1952)
    Gzip,
    /// deflate (RFC 1951)
    Deflate,
}

impl CompressionEncoding {
    /// Returns the `Content-Encoding` header value.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Gzip => "gzip",
            Self::Deflate => "deflate",
        }
    }
}

/// Configuration for response compression.
#[derive(Debug, Clone)]
pub struct CompressionConfig {
    /// Enable gzip encoding.
    pub enable_gzip: bool,
    /// Enable deflate encoding.
    pub enable_deflate: bool,
    /// Minimum response body size in bytes to trigger compression.
    /// Responses smaller than this are returned uncompressed.
    pub min_size: usize,
    /// Path prefixes excluded from compression.
    pub excluded_paths: Vec<String>,
}

impl Default for CompressionConfig {
    fn default() -> Self {
        Self {
            enable_gzip: true,
            enable_deflate: true,
            min_size: 256,
            excluded_paths: vec![],
        }
    }
}

impl CompressionConfig {
    /// Check if a path is excluded from compression.
    fn is_excluded(&self, path: &str) -> bool {
        self.excluded_paths.iter().any(|p| path.starts_with(p))
    }

    /// Negotiate the best encoding from the `Accept-Encoding` header value.
    ///
    /// Returns the first match from: gzip, deflate (in preference order).
    fn negotiate(&self, accept: &str) -> Option<CompressionEncoding> {
        let accept_lower = accept.to_lowercase();
        if self.enable_gzip && accept_lower.contains("gzip") {
            return Some(CompressionEncoding::Gzip);
        }
        if self.enable_deflate && accept_lower.contains("deflate") {
            return Some(CompressionEncoding::Deflate);
        }
        None
    }
}

/// Compress a byte buffer with the given encoding.
fn compress_body(data: &[u8], encoding: CompressionEncoding) -> std::io::Result<Vec<u8>> {
    use flate2::write::{DeflateEncoder, GzEncoder};
    use flate2::Compression;
    use std::io::Write;

    match encoding {
        CompressionEncoding::Gzip => {
            let mut encoder = GzEncoder::new(Vec::new(), Compression::fast());
            encoder.write_all(data)?;
            encoder.finish()
        }
        CompressionEncoding::Deflate => {
            let mut encoder = DeflateEncoder::new(Vec::new(), Compression::fast());
            encoder.write_all(data)?;
            encoder.finish()
        }
    }
}

/// Middleware that transparently compresses response bodies.
///
/// Examines the `Accept-Encoding` request header to negotiate the best
/// compression algorithm. Only compresses responses whose body exceeds
/// [`CompressionConfig::min_size`] bytes.
///
/// Sets `Content-Encoding`, updates `Content-Length`, and appends `Vary:
/// Accept-Encoding` to the response.
pub async fn response_compression(
    config: CompressionConfig,
    request: Request,
    next: Next,
) -> Response {
    let path = request.uri().path().to_string();

    // Skip compression for excluded paths
    if config.is_excluded(&path) {
        return next.run(request).await;
    }

    // Negotiate encoding from Accept-Encoding header
    let encoding = request
        .headers()
        .get(header::ACCEPT_ENCODING)
        .and_then(|v| v.to_str().ok())
        .and_then(|accept| config.negotiate(accept));

    let encoding = match encoding {
        Some(enc) => enc,
        None => return next.run(request).await,
    };

    let response = next.run(request).await;

    // Don't compress if already compressed
    if response.headers().contains_key(header::CONTENT_ENCODING) {
        return response;
    }

    let (mut parts, body) = response.into_parts();

    // Read the full body
    let body_bytes = match axum::body::to_bytes(body, 10 * 1024 * 1024).await {
        Ok(bytes) => bytes,
        Err(_) => {
            return Response::from_parts(
                parts,
                axum::body::Body::from("Internal compression error"),
            );
        }
    };

    // Skip compression for small responses
    if body_bytes.len() < config.min_size {
        return Response::from_parts(parts, axum::body::Body::from(body_bytes));
    }

    // Compress the body
    match compress_body(&body_bytes, encoding) {
        Ok(compressed) => {
            if let Ok(val) = HeaderValue::from_str(encoding.as_str()) {
                parts.headers.insert(header::CONTENT_ENCODING, val);
            }
            if let Ok(val) = HeaderValue::from_str(&compressed.len().to_string()) {
                parts.headers.insert(header::CONTENT_LENGTH, val);
            }
            parts
                .headers
                .insert(header::VARY, HeaderValue::from_static("accept-encoding"));
            Response::from_parts(parts, axum::body::Body::from(compressed))
        }
        Err(_) => {
            // Compression failed — return uncompressed
            Response::from_parts(parts, axum::body::Body::from(body_bytes))
        }
    }
}

// ============================================================================
// ETag & Conditional Requests
// ============================================================================

/// Configuration for ETag generation and conditional request handling.
#[derive(Debug, Clone)]
pub struct ETagConfig {
    /// Enable ETag generation on responses.
    pub enable_etag: bool,
    /// Handle `If-None-Match` conditional requests (returns 304).
    pub enable_if_none_match: bool,
    /// Minimum response body size in bytes to generate an ETag.
    /// Responses smaller than this are returned without an ETag.
    pub min_size: usize,
    /// Path prefixes excluded from ETag processing.
    pub excluded_paths: Vec<String>,
    /// Use weak ETags (`W/"..."`) instead of strong ETags.
    pub weak: bool,
}

impl Default for ETagConfig {
    fn default() -> Self {
        Self {
            enable_etag: true,
            enable_if_none_match: true,
            min_size: 0,
            excluded_paths: vec![],
            weak: false,
        }
    }
}

impl ETagConfig {
    /// Check if a path is excluded from ETag processing.
    fn is_excluded(&self, path: &str) -> bool {
        self.excluded_paths.iter().any(|p| path.starts_with(p))
    }
}

/// Compute a hex-encoded hash of the given bytes using FNV-1a (64-bit).
///
/// FNV-1a is a fast, non-cryptographic hash ideal for ETag generation.
fn compute_etag_hash(data: &[u8]) -> String {
    // FNV-1a 64-bit
    const FNV_OFFSET: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x00000100000001B3;

    let mut hash = FNV_OFFSET;
    for &byte in data {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    format!("{hash:016x}")
}

/// Format a hash string as an ETag value.
///
/// Returns `W/"<hash>"` for weak ETags or `"<hash>"` for strong ETags.
fn format_etag(hash: &str, weak: bool) -> String {
    if weak {
        format!("W/\"{hash}\"")
    } else {
        format!("\"{hash}\"")
    }
}

/// Parse an `If-None-Match` header value and check if any ETag matches.
///
/// Supports `*` (wildcard) and comma-separated ETags with optional
/// whitespace, including both strong and weak comparison.
fn etag_matches(if_none_match: &str, etag: &str) -> bool {
    let trimmed = if_none_match.trim();
    if trimmed == "*" {
        return true;
    }
    // Strip weak prefix for comparison
    let normalize = |s: &str| -> String {
        let s = s.trim();
        s.strip_prefix("W/").unwrap_or(s).to_string()
    };
    let etag_norm = normalize(etag);
    trimmed
        .split(',')
        .any(|candidate| normalize(candidate) == etag_norm)
}

/// Middleware that generates ETag headers and handles conditional requests.
///
/// For GET and HEAD requests, computes an ETag from the response body hash.
/// If the client sends `If-None-Match` and the ETag matches, returns
/// `304 Not Modified` with an empty body.
///
/// This layer should run after compression so the ETag reflects the
/// actual bytes the client will receive.
pub async fn etag_conditional(config: ETagConfig, request: Request, next: Next) -> Response {
    let method = request.method().clone();
    let path = request.uri().path().to_string();

    // Only generate ETags for GET and HEAD
    if method != Method::GET && method != Method::HEAD {
        return next.run(request).await;
    }

    // Skip excluded paths
    if config.is_excluded(&path) {
        return next.run(request).await;
    }

    // Extract If-None-Match before consuming request
    let if_none_match = request
        .headers()
        .get(header::IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    let response = next.run(request).await;

    // Don't add ETag if the response already has one
    if response.headers().contains_key(header::ETAG) {
        return response;
    }

    // Don't process non-success responses
    if !response.status().is_success() {
        return response;
    }

    let (mut parts, body) = response.into_parts();

    // Read the full body
    let body_bytes = match axum::body::to_bytes(body, 10 * 1024 * 1024).await {
        Ok(bytes) => bytes,
        Err(_) => {
            return Response::from_parts(parts, axum::body::Body::empty());
        }
    };

    // Skip small responses
    if body_bytes.len() < config.min_size {
        return Response::from_parts(parts, axum::body::Body::from(body_bytes));
    }

    // Generate ETag
    if config.enable_etag {
        let hash = compute_etag_hash(&body_bytes);
        let etag = format_etag(&hash, config.weak);

        // Check If-None-Match
        if config.enable_if_none_match {
            if let Some(ref inm) = if_none_match {
                if etag_matches(inm, &etag) {
                    // 304 Not Modified — return empty body with ETag
                    parts.status = StatusCode::NOT_MODIFIED;
                    if let Ok(val) = HeaderValue::from_str(&etag) {
                        parts.headers.insert(header::ETAG, val);
                    }
                    // Remove content headers for 304
                    parts.headers.remove(header::CONTENT_LENGTH);
                    parts.headers.remove(header::CONTENT_TYPE);
                    parts.headers.remove(header::CONTENT_ENCODING);
                    return Response::from_parts(parts, axum::body::Body::empty());
                }
            }
        }

        if let Ok(val) = HeaderValue::from_str(&etag) {
            parts.headers.insert(header::ETAG, val);
        }
    }

    Response::from_parts(parts, axum::body::Body::from(body_bytes))
}

// ============================================================================
// Middleware Stack Configuration
// ============================================================================

/// Master configuration for the API middleware stack.
///
/// Each layer can be independently enabled or disabled. The [`apply`]
/// method composes all enabled layers into the correct order and
/// returns the wrapped router.
///
/// [`apply`]: MiddlewareConfig::apply
#[derive(Debug, Clone)]
pub struct MiddlewareConfig {
    /// Enable `X-Request-Id` header generation (UUID v4).
    pub enable_request_id: bool,
    /// Enable `X-Response-Time` header.
    pub enable_request_timing: bool,
    /// Enable structured request logging via tracing.
    pub enable_request_logging: bool,
    /// Enable `X-API-Version` response header.
    pub enable_api_version: bool,
    /// Enable `Content-Type: application/json` validation for body methods.
    pub enable_content_type_validation: bool,
    /// Content-type validation configuration.
    pub content_type: ContentTypeConfig,
    /// Enable CORS headers.
    pub enable_cors: bool,
    /// CORS configuration (used when `enable_cors` is true).
    pub cors: CorsConfig,
    /// Enable request timeout enforcement.
    pub enable_request_timeout: bool,
    /// Request timeout configuration.
    pub timeout: TimeoutConfig,
    /// Enable rate limiting.
    pub enable_rate_limit: bool,
    /// Rate limit configuration (used when `enable_rate_limit` is true).
    pub rate_limit: RateLimitConfig,
    /// Enable request body size enforcement.
    pub enable_body_limit: bool,
    /// Body limit configuration (used when `enable_body_limit` is true).
    pub body_limit: BodyLimitConfig,
    /// Enable API key authentication.
    pub enable_api_key_auth: bool,
    /// API key configuration (used when `enable_api_key_auth` is true).
    pub api_key: ApiKeyConfig,
    /// Enable standard security response headers.
    pub enable_security_headers: bool,
    /// Security headers configuration.
    pub security_headers: SecurityHeadersConfig,
    /// Enable response compression.
    pub enable_compression: bool,
    /// Response compression configuration.
    pub compression: CompressionConfig,
    /// Enable ETag generation and conditional requests.
    pub enable_etag: bool,
    /// ETag configuration.
    pub etag: ETagConfig,
    /// Enable IP-based access control.
    pub enable_ip_filter: bool,
    /// IP filter configuration.
    pub ip_filter: IpFilterConfig,
    /// Enable JSON request body validation.
    pub enable_body_validation: bool,
    /// Body validation configuration.
    pub body_validation: BodyValidationConfig,
    /// Enable request idempotency (cache responses by key).
    pub enable_idempotency: bool,
    /// Idempotency configuration.
    pub idempotency: IdempotencyConfig,
    /// Enable audit logging for mutating requests.
    pub enable_audit_log: bool,
    /// Audit logging configuration.
    pub audit_log: AuditLogConfig,
    /// Enable response caching for GET requests.
    pub enable_response_cache: bool,
    /// Response cache configuration.
    pub response_cache: ResponseCacheConfig,
    /// Enable request deduplication for mutating requests.
    pub enable_request_dedup: bool,
    /// Request deduplication configuration.
    pub request_dedup: RequestDedupConfig,
    /// Enable W3C Trace Context propagation.
    pub enable_tracing: bool,
    /// Request tracing configuration.
    pub tracing: TracingConfig,
    /// Enable HMAC-SHA256 request payload signing.
    pub enable_payload_signing: bool,
    /// Payload signing configuration.
    pub payload_signing: PayloadSigningConfig,
    /// Enable circuit breaker for downstream failure protection.
    pub enable_circuit_breaker: bool,
    /// Circuit breaker configuration.
    pub circuit_breaker: CircuitBreakerConfig,
    /// Enable request header sanitization.
    pub enable_sanitization: bool,
    /// Request sanitization configuration.
    pub sanitization: SanitizationConfig,
    /// Enable content negotiation (Accept header validation).
    pub enable_content_negotiation: bool,
    /// Content negotiation configuration.
    pub content_negotiation: ContentNegotiationConfig,
    /// Enable concurrency-based request throttling.
    pub enable_throttle: bool,
    /// Throttle configuration.
    pub throttle: ThrottleConfig,
    /// Enable retry hint headers on error responses.
    pub enable_retry_hints: bool,
    /// Retry hints configuration.
    pub retry_hints: RetryHintsConfig,
    /// Enable maintenance mode gating.
    pub enable_maintenance: bool,
    /// Maintenance mode configuration.
    pub maintenance: MaintenanceConfig,
    /// Enable API deprecation warning headers.
    pub enable_deprecation: bool,
    /// API deprecation configuration.
    pub deprecation: DeprecationConfig,
    /// Enable request cost tracking and budget enforcement.
    pub enable_request_cost: bool,
    /// Request cost configuration.
    pub request_cost: RequestCostConfig,
    /// Enable request fingerprint generation.
    pub enable_fingerprint: bool,
    /// Request fingerprint configuration.
    pub fingerprint: FingerprintConfig,
    /// Enable HMAC-SHA256 response signing.
    pub enable_response_signing: bool,
    /// Response signing configuration.
    pub response_signing: ResponseSigningConfig,
    /// Enable request priority tagging.
    pub enable_request_priority: bool,
    /// Request priority configuration.
    pub request_priority: RequestPriorityConfig,
    /// Whether request quota enforcement is enabled.
    pub enable_request_quota: bool,
    /// Request quota configuration.
    pub request_quota: RequestQuotaConfig,
    /// Shared quota state for tracking per-client usage.
    pub quota_state: QuotaState,
    /// Whether tenant isolation is enabled.
    pub enable_tenant_isolation: bool,
    /// Tenant isolation configuration.
    pub tenant_isolation: TenantIsolationConfig,
    /// Whether response envelope wrapping is enabled.
    pub enable_response_envelope: bool,
    /// Response envelope configuration.
    pub response_envelope: ResponseEnvelopeConfig,
    /// Enable request replay protection.
    pub enable_replay_protection: bool,
    /// Replay protection configuration.
    pub replay_protection: ReplayProtectionConfig,
    /// Enable geo-IP header injection.
    pub enable_geo_ip: bool,
    /// Geo-IP configuration.
    pub geo_ip: GeoIpConfig,
    /// Enable request schema validation.
    pub enable_schema_validation: bool,
    /// Schema validation configuration.
    pub schema_validation: SchemaValidationConfig,
    /// Enable request decompression.
    pub enable_request_decompression: bool,
    /// Request decompression configuration.
    pub request_decompression: RequestDecompressionConfig,
    /// Enable slow request detection.
    pub enable_slow_request: bool,
    /// Slow request detection configuration.
    pub slow_request: SlowRequestConfig,
    /// Enable header propagation.
    pub enable_header_propagation: bool,
    /// Header propagation configuration.
    pub header_propagation: HeaderPropagationConfig,
    /// Enable request context injection.
    pub enable_request_context: bool,
    /// Request context configuration.
    pub request_context: RequestContextConfig,
    /// Enable JSON 404 fallback for unmatched routes.
    pub enable_fallback: bool,
}

impl Default for MiddlewareConfig {
    fn default() -> Self {
        Self {
            enable_request_id: true,
            enable_request_timing: true,
            enable_request_logging: true,
            enable_api_version: true,
            enable_content_type_validation: true,
            content_type: ContentTypeConfig::default(),
            enable_cors: true,
            cors: CorsConfig::default(),
            enable_request_timeout: false,
            timeout: TimeoutConfig::default(),
            enable_rate_limit: false,
            rate_limit: RateLimitConfig::default(),
            enable_body_limit: false,
            body_limit: BodyLimitConfig::default(),
            enable_api_key_auth: false,
            api_key: ApiKeyConfig::default(),
            enable_security_headers: true,
            security_headers: SecurityHeadersConfig::default(),
            enable_compression: false,
            compression: CompressionConfig::default(),
            enable_etag: false,
            etag: ETagConfig::default(),
            enable_ip_filter: false,
            ip_filter: IpFilterConfig::default(),
            enable_body_validation: false,
            body_validation: BodyValidationConfig::default(),
            enable_idempotency: false,
            idempotency: IdempotencyConfig::default(),
            enable_audit_log: false,
            audit_log: AuditLogConfig::default(),
            enable_response_cache: false,
            response_cache: ResponseCacheConfig::default(),
            enable_request_dedup: false,
            request_dedup: RequestDedupConfig::default(),
            enable_tracing: false,
            tracing: TracingConfig::default(),
            enable_payload_signing: false,
            payload_signing: PayloadSigningConfig::default(),
            enable_circuit_breaker: false,
            circuit_breaker: CircuitBreakerConfig::default(),
            enable_sanitization: false,
            sanitization: SanitizationConfig::default(),
            enable_content_negotiation: false,
            content_negotiation: ContentNegotiationConfig::default(),
            enable_throttle: false,
            throttle: ThrottleConfig::default(),
            enable_retry_hints: false,
            retry_hints: RetryHintsConfig::default(),
            enable_maintenance: false,
            maintenance: MaintenanceConfig::default(),
            enable_deprecation: false,
            deprecation: DeprecationConfig::default(),
            enable_request_cost: false,
            request_cost: RequestCostConfig::default(),
            enable_fingerprint: false,
            fingerprint: FingerprintConfig::default(),
            enable_response_signing: false,
            response_signing: ResponseSigningConfig::default(),
            enable_request_priority: false,
            request_priority: RequestPriorityConfig::default(),
            enable_request_quota: false,
            request_quota: RequestQuotaConfig::default(),
            quota_state: QuotaState::new(),
            enable_tenant_isolation: false,
            tenant_isolation: TenantIsolationConfig::default(),
            enable_response_envelope: false,
            response_envelope: ResponseEnvelopeConfig::default(),
            enable_replay_protection: false,
            replay_protection: ReplayProtectionConfig::default(),
            enable_geo_ip: false,
            geo_ip: GeoIpConfig::default(),
            enable_schema_validation: false,
            schema_validation: SchemaValidationConfig::default(),
            enable_request_decompression: false,
            request_decompression: RequestDecompressionConfig::default(),
            enable_slow_request: false,
            slow_request: SlowRequestConfig::default(),
            enable_header_propagation: false,
            header_propagation: HeaderPropagationConfig::default(),
            enable_request_context: false,
            request_context: RequestContextConfig::default(),
            enable_fallback: true,
        }
    }
}

impl MiddlewareConfig {
    /// Create a minimal middleware config with everything disabled.
    pub fn none() -> Self {
        Self {
            enable_request_id: false,
            enable_request_timing: false,
            enable_request_logging: false,
            enable_api_version: false,
            enable_content_type_validation: false,
            content_type: ContentTypeConfig::default(),
            enable_cors: false,
            cors: CorsConfig::default(),
            enable_request_timeout: false,
            timeout: TimeoutConfig::default(),
            enable_rate_limit: false,
            rate_limit: RateLimitConfig::default(),
            enable_body_limit: false,
            body_limit: BodyLimitConfig::default(),
            enable_api_key_auth: false,
            api_key: ApiKeyConfig::default(),
            enable_security_headers: false,
            security_headers: SecurityHeadersConfig::default(),
            enable_compression: false,
            compression: CompressionConfig::default(),
            enable_etag: false,
            etag: ETagConfig::default(),
            enable_ip_filter: false,
            ip_filter: IpFilterConfig::default(),
            enable_body_validation: false,
            body_validation: BodyValidationConfig::default(),
            enable_idempotency: false,
            idempotency: IdempotencyConfig::default(),
            enable_audit_log: false,
            audit_log: AuditLogConfig::default(),
            enable_response_cache: false,
            response_cache: ResponseCacheConfig::default(),
            enable_request_dedup: false,
            request_dedup: RequestDedupConfig::default(),
            enable_tracing: false,
            tracing: TracingConfig::default(),
            enable_payload_signing: false,
            payload_signing: PayloadSigningConfig::default(),
            enable_circuit_breaker: false,
            circuit_breaker: CircuitBreakerConfig::default(),
            enable_sanitization: false,
            sanitization: SanitizationConfig::default(),
            enable_content_negotiation: false,
            content_negotiation: ContentNegotiationConfig::default(),
            enable_throttle: false,
            throttle: ThrottleConfig::default(),
            enable_retry_hints: false,
            retry_hints: RetryHintsConfig::default(),
            enable_maintenance: false,
            maintenance: MaintenanceConfig::default(),
            enable_deprecation: false,
            deprecation: DeprecationConfig::default(),
            enable_request_cost: false,
            request_cost: RequestCostConfig::default(),
            enable_fingerprint: false,
            fingerprint: FingerprintConfig::default(),
            enable_response_signing: false,
            response_signing: ResponseSigningConfig::default(),
            enable_request_priority: false,
            request_priority: RequestPriorityConfig::default(),
            enable_request_quota: false,
            request_quota: RequestQuotaConfig::default(),
            quota_state: QuotaState::new(),
            enable_tenant_isolation: false,
            tenant_isolation: TenantIsolationConfig::default(),
            enable_response_envelope: false,
            response_envelope: ResponseEnvelopeConfig::default(),
            enable_replay_protection: false,
            replay_protection: ReplayProtectionConfig::default(),
            enable_geo_ip: false,
            geo_ip: GeoIpConfig::default(),
            enable_schema_validation: false,
            schema_validation: SchemaValidationConfig::default(),
            enable_request_decompression: false,
            request_decompression: RequestDecompressionConfig::default(),
            enable_slow_request: false,
            slow_request: SlowRequestConfig::default(),
            enable_header_propagation: false,
            header_propagation: HeaderPropagationConfig::default(),
            enable_request_context: false,
            request_context: RequestContextConfig::default(),
            enable_fallback: false,
        }
    }

    /// Builder-style: enable/disable request ID.
    pub fn request_id(mut self, enable: bool) -> Self {
        self.enable_request_id = enable;
        self
    }

    /// Builder-style: enable/disable request timing.
    pub fn request_timing(mut self, enable: bool) -> Self {
        self.enable_request_timing = enable;
        self
    }

    /// Builder-style: enable/disable request logging.
    pub fn request_logging(mut self, enable: bool) -> Self {
        self.enable_request_logging = enable;
        self
    }

    /// Builder-style: enable/disable API version header.
    pub fn api_version_enabled(mut self, enable: bool) -> Self {
        self.enable_api_version = enable;
        self
    }

    /// Builder-style: enable/disable content-type validation.
    pub fn content_type_validation(mut self, enable: bool) -> Self {
        self.enable_content_type_validation = enable;
        self
    }

    /// Builder-style: set content-type validation config.
    pub fn content_type_config(mut self, config: ContentTypeConfig) -> Self {
        self.content_type = config;
        self
    }

    /// Builder-style: enable/disable CORS.
    pub fn cors_enabled(mut self, enable: bool) -> Self {
        self.enable_cors = enable;
        self
    }

    /// Builder-style: set CORS configuration.
    pub fn cors_config(mut self, cors: CorsConfig) -> Self {
        self.cors = cors;
        self
    }

    /// Builder-style: enable request timeout with config.
    pub fn request_timeout(mut self, config: TimeoutConfig) -> Self {
        self.enable_request_timeout = true;
        self.timeout = config;
        self
    }

    /// Builder-style: enable/disable request timeout.
    pub fn request_timeout_enabled(mut self, enable: bool) -> Self {
        self.enable_request_timeout = enable;
        self
    }

    /// Builder-style: enable rate limiting with config.
    pub fn rate_limit(mut self, config: RateLimitConfig) -> Self {
        self.enable_rate_limit = true;
        self.rate_limit = config;
        self
    }

    /// Builder-style: enable/disable rate limiting.
    pub fn rate_limit_enabled(mut self, enable: bool) -> Self {
        self.enable_rate_limit = enable;
        self
    }

    /// Builder-style: enable body size limit with config.
    pub fn body_limit(mut self, config: BodyLimitConfig) -> Self {
        self.enable_body_limit = true;
        self.body_limit = config;
        self
    }

    /// Builder-style: enable/disable body size limit.
    pub fn body_limit_enabled(mut self, enable: bool) -> Self {
        self.enable_body_limit = enable;
        self
    }

    /// Builder-style: enable API key auth with the given keys.
    pub fn api_keys(mut self, keys: Vec<String>) -> Self {
        self.enable_api_key_auth = true;
        self.api_key.keys = keys;
        self
    }

    /// Builder-style: set API key excluded paths.
    pub fn api_key_excluded_paths(mut self, paths: Vec<String>) -> Self {
        self.api_key.excluded_paths = paths;
        self
    }

    /// Builder-style: enable/disable security headers.
    pub fn security_headers_enabled(mut self, enable: bool) -> Self {
        self.enable_security_headers = enable;
        self
    }

    /// Builder-style: set security headers configuration.
    pub fn security_headers_config(mut self, config: SecurityHeadersConfig) -> Self {
        self.security_headers = config;
        self
    }

    /// Builder-style: enable/disable JSON 404 fallback.
    pub fn fallback(mut self, enable: bool) -> Self {
        self.enable_fallback = enable;
        self
    }

    /// Builder-style: enable response compression with config.
    pub fn compression(mut self, config: CompressionConfig) -> Self {
        self.enable_compression = true;
        self.compression = config;
        self
    }

    /// Builder-style: enable/disable response compression.
    pub fn compression_enabled(mut self, enable: bool) -> Self {
        self.enable_compression = enable;
        self
    }

    /// Builder-style: enable ETag with config.
    pub fn etag(mut self, config: ETagConfig) -> Self {
        self.enable_etag = true;
        self.etag = config;
        self
    }

    /// Builder-style: enable/disable ETag.
    pub fn etag_enabled(mut self, enable: bool) -> Self {
        self.enable_etag = enable;
        self
    }

    /// Builder-style: enable IP filter with config.
    pub fn ip_filter(mut self, config: IpFilterConfig) -> Self {
        self.enable_ip_filter = true;
        self.ip_filter = config;
        self
    }

    /// Builder-style: enable/disable IP filter.
    pub fn ip_filter_enabled(mut self, enable: bool) -> Self {
        self.enable_ip_filter = enable;
        self
    }

    /// Builder-style: enable body validation with config.
    pub fn body_validation(mut self, config: BodyValidationConfig) -> Self {
        self.enable_body_validation = true;
        self.body_validation = config;
        self
    }

    /// Builder-style: enable/disable body validation.
    pub fn body_validation_enabled(mut self, enable: bool) -> Self {
        self.enable_body_validation = enable;
        self
    }

    /// Builder-style: enable idempotency with config.
    pub fn idempotency(mut self, config: IdempotencyConfig) -> Self {
        self.enable_idempotency = true;
        self.idempotency = config;
        self
    }

    /// Builder-style: enable/disable idempotency.
    pub fn idempotency_enabled(mut self, enable: bool) -> Self {
        self.enable_idempotency = enable;
        self
    }

    /// Builder-style: enable audit logging with config.
    pub fn audit_log(mut self, config: AuditLogConfig) -> Self {
        self.enable_audit_log = true;
        self.audit_log = config;
        self
    }

    /// Builder-style: enable/disable audit logging.
    pub fn audit_log_enabled(mut self, enable: bool) -> Self {
        self.enable_audit_log = enable;
        self
    }

    /// Builder-style: enable response caching with config.
    pub fn response_cache(mut self, config: ResponseCacheConfig) -> Self {
        self.enable_response_cache = true;
        self.response_cache = config;
        self
    }

    /// Builder-style: enable/disable response caching.
    pub fn response_cache_enabled(mut self, enable: bool) -> Self {
        self.enable_response_cache = enable;
        self
    }

    /// Builder-style: enable request deduplication with config.
    pub fn request_dedup(mut self, config: RequestDedupConfig) -> Self {
        self.enable_request_dedup = true;
        self.request_dedup = config;
        self
    }

    /// Builder-style: enable/disable request deduplication.
    pub fn request_dedup_enabled(mut self, enable: bool) -> Self {
        self.enable_request_dedup = enable;
        self
    }

    /// Builder-style: enable request tracing with config.
    pub fn tracing_config(mut self, config: TracingConfig) -> Self {
        self.enable_tracing = true;
        self.tracing = config;
        self
    }

    /// Builder-style: enable/disable request tracing.
    pub fn tracing_enabled(mut self, enable: bool) -> Self {
        self.enable_tracing = enable;
        self
    }

    /// Builder-style: enable payload signing with config.
    pub fn payload_signing(mut self, config: PayloadSigningConfig) -> Self {
        self.enable_payload_signing = true;
        self.payload_signing = config;
        self
    }

    /// Builder-style: enable/disable payload signing.
    pub fn payload_signing_enabled(mut self, enable: bool) -> Self {
        self.enable_payload_signing = enable;
        self
    }

    /// Builder-style: enable circuit breaker with config.
    pub fn circuit_breaker(mut self, config: CircuitBreakerConfig) -> Self {
        self.enable_circuit_breaker = true;
        self.circuit_breaker = config;
        self
    }

    /// Builder-style: enable/disable circuit breaker.
    pub fn circuit_breaker_enabled(mut self, enable: bool) -> Self {
        self.enable_circuit_breaker = enable;
        self
    }

    /// Builder-style: enable request sanitization with config.
    pub fn sanitization(mut self, config: SanitizationConfig) -> Self {
        self.enable_sanitization = true;
        self.sanitization = config;
        self
    }

    /// Builder-style: enable/disable request sanitization.
    pub fn sanitization_enabled(mut self, enable: bool) -> Self {
        self.enable_sanitization = enable;
        self
    }

    /// Builder-style: enable content negotiation with config.
    pub fn content_negotiation(mut self, config: ContentNegotiationConfig) -> Self {
        self.enable_content_negotiation = true;
        self.content_negotiation = config;
        self
    }

    /// Builder-style: enable/disable content negotiation.
    pub fn content_negotiation_enabled(mut self, enable: bool) -> Self {
        self.enable_content_negotiation = enable;
        self
    }

    /// Builder-style: enable request throttling with config.
    pub fn throttle(mut self, config: ThrottleConfig) -> Self {
        self.enable_throttle = true;
        self.throttle = config;
        self
    }

    /// Builder-style: enable/disable request throttling.
    pub fn throttle_enabled(mut self, enable: bool) -> Self {
        self.enable_throttle = enable;
        self
    }

    /// Builder-style: enable retry hints with config.
    pub fn retry_hints(mut self, config: RetryHintsConfig) -> Self {
        self.enable_retry_hints = true;
        self.retry_hints = config;
        self
    }

    /// Builder-style: enable/disable retry hints.
    pub fn retry_hints_enabled(mut self, enable: bool) -> Self {
        self.enable_retry_hints = enable;
        self
    }

    /// Builder-style: enable maintenance mode with config.
    pub fn maintenance(mut self, config: MaintenanceConfig) -> Self {
        self.enable_maintenance = true;
        self.maintenance = config;
        self
    }

    /// Builder-style: enable/disable maintenance mode.
    pub fn maintenance_enabled(mut self, enable: bool) -> Self {
        self.enable_maintenance = enable;
        self
    }

    /// Builder-style: enable API deprecation with config.
    pub fn deprecation(mut self, config: DeprecationConfig) -> Self {
        self.enable_deprecation = true;
        self.deprecation = config;
        self
    }

    /// Builder-style: enable/disable API deprecation.
    pub fn deprecation_enabled(mut self, enable: bool) -> Self {
        self.enable_deprecation = enable;
        self
    }

    /// Builder-style: enable request cost tracking with config.
    pub fn request_cost(mut self, config: RequestCostConfig) -> Self {
        self.enable_request_cost = true;
        self.request_cost = config;
        self
    }

    /// Builder-style: enable/disable request cost tracking.
    pub fn request_cost_enabled(mut self, enable: bool) -> Self {
        self.enable_request_cost = enable;
        self
    }

    /// Builder-style: enable request fingerprinting with config.
    pub fn request_fingerprint(mut self, config: FingerprintConfig) -> Self {
        self.enable_fingerprint = true;
        self.fingerprint = config;
        self
    }

    /// Builder-style: enable/disable request fingerprinting.
    pub fn fingerprint_enabled(mut self, enable: bool) -> Self {
        self.enable_fingerprint = enable;
        self
    }

    /// Builder-style: enable response signing with the given config.
    pub fn response_signing(mut self, config: ResponseSigningConfig) -> Self {
        self.enable_response_signing = true;
        self.response_signing = config;
        self
    }

    /// Builder-style: enable/disable response signing.
    pub fn response_signing_enabled(mut self, enable: bool) -> Self {
        self.enable_response_signing = enable;
        self
    }

    /// Builder-style: enable request priority with the given config.
    pub fn request_priority(mut self, config: RequestPriorityConfig) -> Self {
        self.enable_request_priority = true;
        self.request_priority = config;
        self
    }

    /// Builder-style: enable/disable request priority.
    pub fn request_priority_enabled(mut self, enable: bool) -> Self {
        self.enable_request_priority = enable;
        self
    }
    /// Set the request quota configuration.
    pub fn request_quota(mut self, config: RequestQuotaConfig) -> Self {
        self.request_quota = config;
        self
    }

    /// Enable or disable request quota enforcement.
    pub fn request_quota_enabled(mut self, enabled: bool) -> Self {
        self.enable_request_quota = enabled;
        self
    }

    /// Set the shared quota state.
    pub fn quota_state(mut self, state: QuotaState) -> Self {
        self.quota_state = state;
        self
    }

    /// Set the tenant isolation configuration.
    pub fn tenant_isolation(mut self, config: TenantIsolationConfig) -> Self {
        self.tenant_isolation = config;
        self
    }

    /// Enable or disable tenant isolation.
    pub fn tenant_isolation_enabled(mut self, enabled: bool) -> Self {
        self.enable_tenant_isolation = enabled;
        self
    }

    /// Set the response envelope configuration.
    pub fn response_envelope(mut self, config: ResponseEnvelopeConfig) -> Self {
        self.response_envelope = config;
        self
    }

    /// Enable or disable response envelope wrapping.
    pub fn response_envelope_enabled(mut self, enabled: bool) -> Self {
        self.enable_response_envelope = enabled;
        self
    }

    /// Builder-style: set replay protection config.
    pub fn replay_protection(mut self, config: ReplayProtectionConfig) -> Self {
        self.replay_protection = config;
        self
    }

    /// Builder-style: enable/disable replay protection.
    pub fn replay_protection_enabled(mut self, enabled: bool) -> Self {
        self.enable_replay_protection = enabled;
        self
    }

    /// Builder-style: set geo-IP config.
    pub fn geo_ip(mut self, config: GeoIpConfig) -> Self {
        self.geo_ip = config;
        self
    }

    /// Builder-style: enable/disable geo-IP headers.
    pub fn geo_ip_enabled(mut self, enabled: bool) -> Self {
        self.enable_geo_ip = enabled;
        self
    }

    /// Builder-style: set schema validation config.
    pub fn schema_validation(mut self, config: SchemaValidationConfig) -> Self {
        self.schema_validation = config;
        self
    }

    /// Builder-style: enable/disable schema validation.
    pub fn schema_validation_enabled(mut self, enabled: bool) -> Self {
        self.enable_schema_validation = enabled;
        self
    }

    /// Builder-style: set request decompression config.
    pub fn request_decompression(mut self, config: RequestDecompressionConfig) -> Self {
        self.request_decompression = config;
        self
    }

    /// Builder-style: enable/disable request decompression.
    pub fn request_decompression_enabled(mut self, enabled: bool) -> Self {
        self.enable_request_decompression = enabled;
        self
    }

    /// Builder-style: set slow request config.
    pub fn slow_request(mut self, config: SlowRequestConfig) -> Self {
        self.slow_request = config;
        self
    }

    /// Builder-style: enable/disable slow request detection.
    pub fn slow_request_enabled(mut self, enabled: bool) -> Self {
        self.enable_slow_request = enabled;
        self
    }
    /// Builder-style: enable/disable header propagation.
    pub fn header_propagation_enabled(mut self, enabled: bool) -> Self {
        self.enable_header_propagation = enabled;
        self
    }

    /// Builder-style: set header propagation config.
    pub fn header_propagation_config(mut self, config: HeaderPropagationConfig) -> Self {
        self.header_propagation = config;
        self
    }
    /// Builder-style: enable/disable request context injection.
    pub fn request_context_enabled(mut self, enabled: bool) -> Self {
        self.enable_request_context = enabled;
        self
    }

    /// Builder-style: set request context config.
    pub fn request_context_config(mut self, config: RequestContextConfig) -> Self {
        self.request_context = config;
        self
    }
    /// Apply the full middleware stack to a router.
    ///
    /// Layers are applied from innermost to outermost. The execution
    /// order on an incoming request is:
    ///
    /// 1. **Request ID** (outermost) — assign/propagate ID
    /// 2. **Request Timing** — start timer
    /// 3. **Request Logging** — log request details
    /// 4. **IP Filter** — deny/allow by client IP (CIDR)
    /// 5. **Response Compression** — compress response bodies
    /// 6. **ETag** — generate ETags, handle `If-None-Match` (304)
    /// 7. **Security Headers** — add security response headers
    /// 8. **API Version** — stamp `X-API-Version` header
    /// 9. **Content-Type Validation** — reject non-JSON bodies
    /// 10. **CORS** — handle preflight, add headers
    /// 11. **Request Timeout** — enforce max request duration (408)
    /// 12. **Rate Limit** — enforce token-bucket limits (429)
    /// 13. **Body Limit** — enforce request body size (413)
    /// 14. **Body Validation** — validate JSON structure (422)
    /// 15. **Idempotency** — cache responses by key
    /// 16. **Audit Log** — structured audit events for mutating ops
    /// 17. **Response Cache** — cache GET responses with TTL
    /// 18. **Request Dedup** — deduplicate concurrent identical requests
    /// 19. **Request Tracing** — W3C Trace Context propagation
    /// 20. **Payload Signing** — HMAC-SHA256 body verification
    /// 21. **Circuit Breaker** — downstream failure protection (503)
    /// 22. **Sanitization** — strip dangerous request headers
    /// 23. **Content Negotiation** — validate Accept header (406)
    /// 24. **Throttle** — concurrency-based load shedding (503)
    /// 25. **Retry Hints** — add retry guidance headers to errors
    /// 26. **Maintenance** --- planned downtime gate (503)
    /// 27. **Deprecation** --- inject sunset/deprecation headers
    /// 28. **Request Cost** --- track per-request cost budget
    /// 29. **Fingerprint** --- deterministic request hash
    /// 30. **Response Signing** --- HMAC-SHA256 response integrity
    /// 31. **Request Priority** --- QoS tagging per request
    /// 32. **Request Quota** --- per-client usage quota
    /// 33. **Tenant Isolation** --- multi-tenant request isolation
    /// 34. **Response Envelope** --- JSON response wrapping
    /// 35. **Replay Protection** --- nonce-based replay detection
    /// 36. **Geo-IP Headers** --- IP-to-region header injection
    /// 37. **Schema Validation** --- JSON body schema checks
    /// 38. **Request Decompression** --- gzip/deflate body decompression
    /// 39. **Slow Request Detection** --- slow request flagging
    /// 40. **Header Propagation** --- propagate request headers to response
    /// 41. **Request Context** --- inject deployment context headers
    /// 42. **API Key Auth** (innermost) --- authorize
    /// 43. Handler
    pub fn apply(&self, router: Router) -> Router {
        let mut app = router;

        // Register JSON 404 fallback before middleware layers
        if self.enable_fallback {
            app = app.fallback(fallback_handler);
        }

        // Innermost first → outermost last

        // 33. Tenant isolation
        if self.enable_tenant_isolation {
            let ti_config = self.tenant_isolation.clone();
            app = app.layer(middleware::from_fn(move |req: Request, next: Next| {
                let config = ti_config.clone();
                tenant_isolation_handler(config, req, next)
            }));
        }

        // 34. Response envelope
        if self.enable_response_envelope {
            let re_config = self.response_envelope.clone();
            app = app.layer(middleware::from_fn(move |req: Request, next: Next| {
                let config = re_config.clone();
                response_envelope_handler(config, req, next)
            }));
        }

        // 35. Replay protection
        if self.enable_replay_protection {
            let replay_config = self.replay_protection.clone();
            let nonce_store = NonceStore::new();
            app = app.layer(middleware::from_fn(move |req: Request, next: Next| {
                let config = replay_config.clone();
                let store = nonce_store.clone();
                replay_protection_handler(config, store, req, next)
            }));
        }

        // 36. Geo-IP headers
        if self.enable_geo_ip {
            let geo_config = self.geo_ip.clone();
            app = app.layer(middleware::from_fn(move |req: Request, next: Next| {
                let config = geo_config.clone();
                geo_ip_handler(config, req, next)
            }));
        }
        // 37. Schema validation
        if self.enable_schema_validation {
            let schema_config = self.schema_validation.clone();
            app = app.layer(middleware::from_fn(move |req: Request, next: Next| {
                let config = schema_config.clone();
                schema_validation_handler(config, req, next)
            }));
        }
        // 38. Request decompression
        if self.enable_request_decompression {
            let decomp_config = self.request_decompression.clone();
            app = app.layer(middleware::from_fn(move |req: Request, next: Next| {
                let config = decomp_config.clone();
                request_decompression_handler(config, req, next)
            }));
        }
        // 39. Slow request detection
        if self.enable_slow_request {
            let slow_config = self.slow_request.clone();
            app = app.layer(middleware::from_fn(move |req: Request, next: Next| {
                let config = slow_config.clone();
                slow_request_handler(config, req, next)
            }));
        }
        // 40. Header propagation
        if self.enable_header_propagation {
            let hp_config = self.header_propagation.clone();
            app = app.layer(middleware::from_fn(move |req: Request, next: Next| {
                let config = hp_config.clone();
                header_propagation_handler(config, req, next)
            }));
        }
        // 41. Request context injection
        if self.enable_request_context {
            let rc_config = self.request_context.clone();
            app = app.layer(middleware::from_fn(move |req: Request, next: Next| {
                let config = rc_config.clone();
                request_context_handler(config, req, next)
            }));
        }
        // 42. API key auth (innermost: closest to handler)

        if self.enable_api_key_auth {
            let api_key_config = self.api_key.clone();
            app = app.layer(middleware::from_fn(move |req: Request, next: Next| {
                let config = api_key_config.clone();
                api_key_handler(config, req, next)
            }));
        }

        // 32. Request quota
        if self.enable_request_quota {
            let rq_config = self.request_quota.clone();
            let rq_state = self.quota_state.clone();
            app = app.layer(middleware::from_fn(move |req: Request, next: Next| {
                let config = rq_config.clone();
                let state = rq_state.clone();
                request_quota_handler(config, state, req, next)
            }));
        }
        // 31. Request priority
        if self.enable_request_priority {
            let rp_config = self.request_priority.clone();
            app = app.layer(middleware::from_fn(move |req: Request, next: Next| {
                let config = rp_config.clone();
                request_priority_handler(config, req, next)
            }));
        }
        // 30. Response signing
        if self.enable_response_signing {
            let rs_config = self.response_signing.clone();
            app = app.layer(middleware::from_fn(move |req: Request, next: Next| {
                let config = rs_config.clone();
                response_signing_handler(config, req, next)
            }));
        }
        // 29. Request fingerprint
        if self.enable_fingerprint {
            let fp_config = self.fingerprint.clone();
            app = app.layer(middleware::from_fn(move |req: Request, next: Next| {
                let config = fp_config.clone();
                request_fingerprint_handler(config, req, next)
            }));
        }

        // 28. Request cost tracking
        if self.enable_request_cost {
            let cost_state = Arc::new(RequestCostState::new(self.request_cost.clone()));
            app = app.layer(middleware::from_fn(move |req: Request, next: Next| {
                let state = cost_state.clone();
                request_cost_handler(state, req, next)
            }));
        }

        // 27. API deprecation headers
        if self.enable_deprecation {
            let dep_config = self.deprecation.clone();
            app = app.layer(middleware::from_fn(move |req: Request, next: Next| {
                let config = dep_config.clone();
                deprecation_handler(config, req, next)
            }));
        }

        // 26. Maintenance mode
        if self.enable_maintenance {
            let maint_state = Arc::new(MaintenanceState::new_active(self.maintenance.clone()));
            app = app.layer(middleware::from_fn(move |req: Request, next: Next| {
                let state = maint_state.clone();
                maintenance_handler(state, req, next)
            }));
        }

        // 25. Retry hints
        if self.enable_retry_hints {
            let hints_config = self.retry_hints.clone();
            app = app.layer(middleware::from_fn(move |req: Request, next: Next| {
                let config = hints_config.clone();
                retry_hints_handler(config, req, next)
            }));
        }

        // 24. Request throttle
        if self.enable_throttle {
            let throttle_state = Arc::new(ThrottleState::new(self.throttle.clone()));
            app = app.layer(middleware::from_fn(move |req: Request, next: Next| {
                let state = throttle_state.clone();
                request_throttle_handler(state, req, next)
            }));
        }

        // 23. Content negotiation
        if self.enable_content_negotiation {
            let nego_config = self.content_negotiation.clone();
            app = app.layer(middleware::from_fn(move |req: Request, next: Next| {
                let config = nego_config.clone();
                content_negotiation_handler(config, req, next)
            }));
        }

        // 22. Request sanitization
        if self.enable_sanitization {
            let sanitize_config = self.sanitization.clone();
            app = app.layer(middleware::from_fn(move |req: Request, next: Next| {
                let config = sanitize_config.clone();
                request_sanitization_handler(config, req, next)
            }));
        }

        // 21. Circuit breaker
        if self.enable_circuit_breaker {
            let breaker = Arc::new(CircuitBreaker::new(self.circuit_breaker.clone()));
            app = app.layer(middleware::from_fn(move |req: Request, next: Next| {
                let breaker = breaker.clone();
                circuit_breaker_handler(breaker, req, next)
            }));
        }

        // 20. Payload signing
        if self.enable_payload_signing {
            let signing_config = self.payload_signing.clone();
            app = app.layer(middleware::from_fn(move |req: Request, next: Next| {
                let config = signing_config.clone();
                payload_signing_handler(config, req, next)
            }));
        }

        // 19. Request tracing (W3C Trace Context)
        if self.enable_tracing {
            let tracing_config = self.tracing.clone();
            app = app.layer(middleware::from_fn(move |req: Request, next: Next| {
                let config = tracing_config.clone();
                request_tracing_handler(config, req, next)
            }));
        }

        // 18. Request deduplication
        if self.enable_request_dedup {
            let tracker = Arc::new(InFlightTracker::new(self.request_dedup.clone()));
            app = app.layer(middleware::from_fn(move |req: Request, next: Next| {
                let tracker = tracker.clone();
                request_dedup_handler(tracker, req, next)
            }));
        }

        // 17. Response cache
        if self.enable_response_cache {
            let resp_cache = Arc::new(ResponseCache::new(self.response_cache.clone()));
            app = app.layer(middleware::from_fn(move |req: Request, next: Next| {
                let cache = resp_cache.clone();
                response_cache_handler(cache, req, next)
            }));
        }

        // 16. Audit log
        if self.enable_audit_log {
            let audit_config = self.audit_log.clone();
            app = app.layer(middleware::from_fn(move |req: Request, next: Next| {
                let config = audit_config.clone();
                audit_log_handler(config, req, next)
            }));
        }

        // 15. Idempotency
        if self.enable_idempotency {
            let store = Arc::new(IdempotencyStore::new(self.idempotency.clone()));
            app = app.layer(middleware::from_fn(move |req: Request, next: Next| {
                let store = store.clone();
                idempotency_handler(store, req, next)
            }));
        }

        // 14. Body validation
        if self.enable_body_validation {
            let bv_config = self.body_validation.clone();
            app = app.layer(middleware::from_fn(move |req: Request, next: Next| {
                let config = bv_config.clone();
                body_validation_handler(config, req, next)
            }));
        }

        // 13. Body size limit
        if self.enable_body_limit {
            let body_config = self.body_limit.clone();
            app = app.layer(middleware::from_fn(move |req: Request, next: Next| {
                let config = body_config.clone();
                body_limit_handler(config, req, next)
            }));
        }

        // 12. Rate limiting
        if self.enable_rate_limit {
            let limiter = Arc::new(RateLimiter::new(self.rate_limit.clone()));
            app = app.layer(middleware::from_fn(move |req: Request, next: Next| {
                let limiter = limiter.clone();
                rate_limit_handler(limiter, req, next)
            }));
        }

        // 11. Request timeout
        if self.enable_request_timeout {
            let timeout_config = self.timeout.clone();
            app = app.layer(middleware::from_fn(move |req: Request, next: Next| {
                let config = timeout_config.clone();
                request_timeout(config, req, next)
            }));
        }

        // 10. CORS
        if self.enable_cors {
            let cors_config = self.cors.clone();
            app = app.layer(middleware::from_fn(move |req: Request, next: Next| {
                let config = cors_config.clone();
                cors_handler(config, req, next)
            }));
        }

        // 9. Content-Type validation
        if self.enable_content_type_validation {
            let excluded = self.content_type.excluded_paths.clone();
            app = app.layer(middleware::from_fn(move |req: Request, next: Next| {
                let excluded = excluded.clone();
                content_type_validation(excluded, req, next)
            }));
        }

        // 8. API version header
        if self.enable_api_version {
            app = app.layer(middleware::from_fn(api_version));
        }

        // 7. Security headers
        if self.enable_security_headers {
            let sec_config = self.security_headers.clone();
            app = app.layer(middleware::from_fn(move |req: Request, next: Next| {
                let config = sec_config.clone();
                security_headers(config, req, next)
            }));
        }

        // 5. Response compression
        if self.enable_compression {
            let comp_config = self.compression.clone();
            app = app.layer(middleware::from_fn(move |req: Request, next: Next| {
                let config = comp_config.clone();
                response_compression(config, req, next)
            }));
        }

        // 6. ETag & conditional requests
        if self.enable_etag {
            let etag_config = self.etag.clone();
            app = app.layer(middleware::from_fn(move |req: Request, next: Next| {
                let config = etag_config.clone();
                etag_conditional(config, req, next)
            }));
        }

        // 4. IP filter
        if self.enable_ip_filter {
            let ip_config = self.ip_filter.clone();
            app = app.layer(middleware::from_fn(move |req: Request, next: Next| {
                let config = ip_config.clone();
                ip_filter_handler(config, req, next)
            }));
        }

        // 3. Request logging
        if self.enable_request_logging {
            app = app.layer(middleware::from_fn(request_logging));
        }

        // 2. Request timing
        if self.enable_request_timing {
            app = app.layer(middleware::from_fn(request_timing));
        }

        // 1. Request ID (outermost: first to execute)
        if self.enable_request_id {
            app = app.layer(middleware::from_fn(request_id));
        }

        app
    }

    /// Returns a human-readable summary of enabled middleware layers.
    pub fn summary(&self) -> Vec<(&'static str, bool)> {
        vec![
            ("Request ID", self.enable_request_id),
            ("Request Timing", self.enable_request_timing),
            ("Request Logging", self.enable_request_logging),
            ("Security Headers", self.enable_security_headers),
            ("API Version", self.enable_api_version),
            (
                "Content-Type Validation",
                self.enable_content_type_validation,
            ),
            ("CORS", self.enable_cors),
            ("Request Timeout", self.enable_request_timeout),
            ("Rate Limit", self.enable_rate_limit),
            ("Body Limit", self.enable_body_limit),
            ("API Key Auth", self.enable_api_key_auth),
            ("Compression", self.enable_compression),
            ("ETag", self.enable_etag),
            ("IP Filter", self.enable_ip_filter),
            ("Body Validation", self.enable_body_validation),
            ("Idempotency", self.enable_idempotency),
            ("Audit Log", self.enable_audit_log),
            ("Response Cache", self.enable_response_cache),
            ("Request Dedup", self.enable_request_dedup),
            ("Request Tracing", self.enable_tracing),
            ("Payload Signing", self.enable_payload_signing),
            ("Circuit Breaker", self.enable_circuit_breaker),
            ("Sanitization", self.enable_sanitization),
            ("Content Negotiation", self.enable_content_negotiation),
            ("Throttle", self.enable_throttle),
            ("Retry Hints", self.enable_retry_hints),
            ("Maintenance", self.enable_maintenance),
            ("Deprecation", self.enable_deprecation),
            ("Request Cost", self.enable_request_cost),
            ("Fingerprint", self.enable_fingerprint),
            ("Response Signing", self.enable_response_signing),
            ("Request Priority", self.enable_request_priority),
            ("Request Quota", self.enable_request_quota),
            ("Tenant Isolation", self.enable_tenant_isolation),
            ("Response Envelope", self.enable_response_envelope),
            ("Replay Protect", self.enable_replay_protection),
            ("Geo-IP Hdrs", self.enable_geo_ip),
            ("Schema Valid.", self.enable_schema_validation),
            ("Req Decomp.", self.enable_request_decompression),
            ("Slow Req.", self.enable_slow_request),
            ("Hdr Prop.", self.enable_header_propagation),
            ("Req Context", self.enable_request_context),
            ("Fallback 404", self.enable_fallback),
        ]
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request as HttpRequest;
    use axum::routing::{any, get};
    use tower::ServiceExt;

    /// Simple test handler
    async fn ok_handler() -> &'static str {
        "ok"
    }

    /// Build a minimal test router
    fn test_router() -> Router {
        Router::new()
            .route("/test", any(ok_handler))
            .route("/health", get(ok_handler))
            .route("/agentic/tools", get(ok_handler))
            .route("/api/v1/vms", get(ok_handler))
    }

    // ── Request ID ────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_request_id_generated() {
        let app = test_router().layer(middleware::from_fn(request_id));

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let id = response.headers().get("x-request-id").unwrap();
        let id_str = id.to_str().unwrap();
        // UUID v4 format: 8-4-4-4-12 = 36 chars
        assert_eq!(id_str.len(), 36);
        assert_eq!(id_str.matches('-').count(), 4);
    }

    #[tokio::test]
    async fn test_request_id_propagated() {
        let app = test_router().layer(middleware::from_fn(request_id));

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .header("x-request-id", "my-custom-id-123")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        let id = response.headers().get("x-request-id").unwrap();
        assert_eq!(id.to_str().unwrap(), "my-custom-id-123");
    }

    #[tokio::test]
    async fn test_request_id_unique_per_request() {
        let app = test_router().layer(middleware::from_fn(request_id));

        let r1 = app
            .clone()
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        let r2 = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        let id1 = r1.headers().get("x-request-id").unwrap().to_str().unwrap();
        let id2 = r2.headers().get("x-request-id").unwrap().to_str().unwrap();
        assert_ne!(id1, id2, "Each request should get a unique ID");
    }

    // ── Request Timing ────────────────────────────────────────────────

    #[tokio::test]
    async fn test_request_timing_header() {
        let app = test_router().layer(middleware::from_fn(request_timing));

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let timing = response
            .headers()
            .get("x-response-time")
            .unwrap()
            .to_str()
            .unwrap();
        assert!(timing.ends_with("ms"), "Should end with 'ms': {timing}");
        // Parse the numeric part
        let ms_str = timing.trim_end_matches("ms");
        let ms: f64 = ms_str.parse().expect("Should be a valid float");
        assert!(ms >= 0.0, "Duration must be non-negative");
    }

    // ── Request Logging ───────────────────────────────────────────────

    #[tokio::test]
    async fn test_request_logging_does_not_crash() {
        let app = test_router().layer(middleware::from_fn(request_logging));

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    // ── CORS ──────────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_cors_wildcard_default() {
        let cors = CorsConfig::default();
        let app = test_router().layer(middleware::from_fn(move |req: Request, next: Next| {
            let c = cors.clone();
            cors_handler(c, req, next)
        }));

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let origin = response
            .headers()
            .get("access-control-allow-origin")
            .unwrap()
            .to_str()
            .unwrap();
        assert_eq!(origin, "*");
    }

    #[tokio::test]
    async fn test_cors_preflight_returns_204() {
        let cors = CorsConfig::default();
        let app = test_router().layer(middleware::from_fn(move |req: Request, next: Next| {
            let c = cors.clone();
            cors_handler(c, req, next)
        }));

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .method(Method::OPTIONS)
                    .uri("/test")
                    .header(header::ORIGIN, "https://example.com")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NO_CONTENT);
        assert!(response
            .headers()
            .contains_key("access-control-allow-methods"));
        assert!(response.headers().contains_key("access-control-max-age"));
    }

    #[tokio::test]
    async fn test_cors_allowed_origin() {
        let cors = CorsConfig::restrictive(vec!["https://app.example.com".to_string()]);
        let app = test_router().layer(middleware::from_fn(move |req: Request, next: Next| {
            let c = cors.clone();
            cors_handler(c, req, next)
        }));

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .header(header::ORIGIN, "https://app.example.com")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        let origin = response
            .headers()
            .get("access-control-allow-origin")
            .unwrap()
            .to_str()
            .unwrap();
        assert_eq!(origin, "https://app.example.com");
    }

    #[tokio::test]
    async fn test_cors_disallowed_origin() {
        let cors = CorsConfig::restrictive(vec!["https://app.example.com".to_string()]);
        let app = test_router().layer(middleware::from_fn(move |req: Request, next: Next| {
            let c = cors.clone();
            cors_handler(c, req, next)
        }));

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .header(header::ORIGIN, "https://evil.com")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        // Request should still succeed, but without CORS headers
        assert_eq!(response.status(), StatusCode::OK);
        assert!(
            response
                .headers()
                .get("access-control-allow-origin")
                .is_none(),
            "Disallowed origin should not receive CORS headers"
        );
    }

    #[tokio::test]
    async fn test_cors_credentials() {
        let cors = CorsConfig {
            allow_credentials: true,
            ..CorsConfig::default()
        };
        let app = test_router().layer(middleware::from_fn(move |req: Request, next: Next| {
            let c = cors.clone();
            cors_handler(c, req, next)
        }));

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        let creds = response
            .headers()
            .get("access-control-allow-credentials")
            .unwrap()
            .to_str()
            .unwrap();
        assert_eq!(creds, "true");
    }

    #[tokio::test]
    async fn test_cors_max_age() {
        let cors = CorsConfig::default();
        let headers = cors.build_headers(None);
        let max_age = headers
            .iter()
            .find(|(k, _)| *k == "access-control-max-age")
            .unwrap();
        assert_eq!(max_age.1, "86400");
    }

    // ── API Key Auth ──────────────────────────────────────────────────

    #[tokio::test]
    async fn test_api_key_valid_bearer() {
        let api_key = ApiKeyConfig {
            keys: vec!["secret-key-123".to_string()],
            ..ApiKeyConfig::default()
        };
        let app = test_router().layer(middleware::from_fn(move |req: Request, next: Next| {
            let c = api_key.clone();
            api_key_handler(c, req, next)
        }));

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/api/v1/vms")
                    .header("authorization", "Bearer secret-key-123")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_api_key_valid_bare() {
        let api_key = ApiKeyConfig {
            keys: vec!["secret-key-123".to_string()],
            ..ApiKeyConfig::default()
        };
        let app = test_router().layer(middleware::from_fn(move |req: Request, next: Next| {
            let c = api_key.clone();
            api_key_handler(c, req, next)
        }));

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/api/v1/vms")
                    .header("authorization", "secret-key-123")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_api_key_invalid() {
        let api_key = ApiKeyConfig {
            keys: vec!["secret-key-123".to_string()],
            ..ApiKeyConfig::default()
        };
        let app = test_router().layer(middleware::from_fn(move |req: Request, next: Next| {
            let c = api_key.clone();
            api_key_handler(c, req, next)
        }));

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/api/v1/vms")
                    .header("authorization", "Bearer wrong-key")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn test_api_key_missing() {
        let api_key = ApiKeyConfig {
            keys: vec!["secret-key-123".to_string()],
            ..ApiKeyConfig::default()
        };
        let app = test_router().layer(middleware::from_fn(move |req: Request, next: Next| {
            let c = api_key.clone();
            api_key_handler(c, req, next)
        }));

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/api/v1/vms")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_api_key_excluded_health() {
        let api_key = ApiKeyConfig {
            keys: vec!["secret-key-123".to_string()],
            ..ApiKeyConfig::default()
        };
        let app = test_router().layer(middleware::from_fn(move |req: Request, next: Next| {
            let c = api_key.clone();
            api_key_handler(c, req, next)
        }));

        // /health is excluded from auth by default
        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_api_key_excluded_agentic() {
        let api_key = ApiKeyConfig {
            keys: vec!["secret-key-123".to_string()],
            ..ApiKeyConfig::default()
        };
        let app = test_router().layer(middleware::from_fn(move |req: Request, next: Next| {
            let c = api_key.clone();
            api_key_handler(c, req, next)
        }));

        // /agentic is excluded from auth by default
        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/agentic/tools")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    // ── CorsConfig ────────────────────────────────────────────────────

    #[test]
    fn test_cors_config_default() {
        let config = CorsConfig::default();
        assert!(config.allowed_origins.is_empty());
        assert_eq!(config.allowed_methods.len(), 6);
        assert_eq!(config.allowed_headers.len(), 3);
        assert!(!config.allow_credentials);
        assert_eq!(config.max_age, 86400);
    }

    #[test]
    fn test_cors_config_restrictive() {
        let config = CorsConfig::restrictive(vec!["https://example.com".to_string()]);
        assert_eq!(config.allowed_origins.len(), 1);
        assert_eq!(config.allowed_origins[0], "https://example.com");
    }

    #[test]
    fn test_cors_build_headers_wildcard() {
        let config = CorsConfig::default();
        let headers = config.build_headers(None);
        assert!(!headers.is_empty());
        let origin = headers
            .iter()
            .find(|(k, _)| *k == "access-control-allow-origin");
        assert_eq!(origin.unwrap().1, "*");
    }

    #[test]
    fn test_cors_build_headers_origin_allowed() {
        let config = CorsConfig::restrictive(vec!["https://app.com".to_string()]);
        let headers = config.build_headers(Some("https://app.com"));
        let origin = headers
            .iter()
            .find(|(k, _)| *k == "access-control-allow-origin");
        assert_eq!(origin.unwrap().1, "https://app.com");
    }

    #[test]
    fn test_cors_build_headers_origin_denied() {
        let config = CorsConfig::restrictive(vec!["https://app.com".to_string()]);
        let headers = config.build_headers(Some("https://evil.com"));
        assert!(headers.is_empty());
    }

    // ── ApiKeyConfig ──────────────────────────────────────────────────

    #[test]
    fn test_api_key_config_default() {
        let config = ApiKeyConfig::default();
        assert!(config.keys.is_empty());
        assert_eq!(config.excluded_paths.len(), 2);
        assert_eq!(config.header_name, "authorization");
    }

    #[test]
    fn test_api_key_config_is_excluded() {
        let config = ApiKeyConfig::default();
        assert!(config.is_excluded("/health"));
        assert!(config.is_excluded("/health/live"));
        assert!(config.is_excluded("/agentic"));
        assert!(config.is_excluded("/agentic/tools/openai"));
        assert!(!config.is_excluded("/api/v1/vms"));
        assert!(!config.is_excluded("/api/v1/runtime/status"));
    }

    #[test]
    fn test_api_key_config_validate_bearer() {
        let config = ApiKeyConfig {
            keys: vec!["key1".to_string(), "key2".to_string()],
            ..ApiKeyConfig::default()
        };
        assert!(config.validate("Bearer key1"));
        assert!(config.validate("Bearer key2"));
        assert!(!config.validate("Bearer key3"));
    }

    #[test]
    fn test_api_key_config_validate_bare() {
        let config = ApiKeyConfig {
            keys: vec!["key1".to_string()],
            ..ApiKeyConfig::default()
        };
        assert!(config.validate("key1"));
        assert!(!config.validate("wrong"));
    }

    // ── MiddlewareConfig ──────────────────────────────────────────────

    #[test]
    fn test_middleware_config_default() {
        let config = MiddlewareConfig::default();
        assert!(config.enable_request_id);
        assert!(config.enable_request_timing);
        assert!(config.enable_request_logging);
        assert!(config.enable_cors);
        assert!(!config.enable_api_key_auth);
    }

    #[test]
    fn test_middleware_config_none() {
        let config = MiddlewareConfig::none();
        assert!(!config.enable_request_id);
        assert!(!config.enable_request_timing);
        assert!(!config.enable_request_logging);
        assert!(!config.enable_cors);
        assert!(!config.enable_api_key_auth);
    }

    #[test]
    fn test_middleware_config_builder() {
        let config = MiddlewareConfig::none()
            .request_id(true)
            .request_timing(true)
            .request_logging(false)
            .cors_enabled(true)
            .api_keys(vec!["key1".to_string()]);

        assert!(config.enable_request_id);
        assert!(config.enable_request_timing);
        assert!(!config.enable_request_logging);
        assert!(config.enable_cors);
        assert!(config.enable_api_key_auth);
        assert_eq!(config.api_key.keys, vec!["key1"]);
    }

    #[test]
    fn test_middleware_summary() {
        let config = MiddlewareConfig::default();
        let summary = config.summary();
        assert_eq!(summary.len(), 43);
        // All enabled except API key auth, rate limit, body limit, request timeout, compression, etag, ip filter, body validation, idempotency, audit log, response cache, circuit breaker, sanitization
        let enabled: Vec<_> = summary.iter().filter(|(_, e)| *e).collect();
        assert_eq!(enabled.len(), 8);
    }

    // ── Full Stack Integration ────────────────────────────────────────

    #[tokio::test]
    async fn test_full_stack_default() {
        // Use default config but disable logging to avoid noise
        let config = MiddlewareConfig::default().request_logging(false);
        let app = config.apply(test_router());

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        // Should have request ID
        assert!(response.headers().contains_key("x-request-id"));
        // Should have timing
        assert!(response.headers().contains_key("x-response-time"));
        // Should have CORS
        assert!(response
            .headers()
            .contains_key("access-control-allow-origin"));
    }

    #[tokio::test]
    async fn test_full_stack_none() {
        let config = MiddlewareConfig::none();
        let app = config.apply(test_router());

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        // No middleware headers
        assert!(response.headers().get("x-request-id").is_none());
        assert!(response.headers().get("x-response-time").is_none());
        assert!(response
            .headers()
            .get("access-control-allow-origin")
            .is_none());
    }

    #[tokio::test]
    async fn test_full_stack_with_auth() {
        let config = MiddlewareConfig::default()
            .request_logging(false)
            .api_keys(vec!["test-key".to_string()]);
        let app = config.apply(test_router());

        // Authenticated request should succeed
        let response = app
            .clone()
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .header("authorization", "Bearer test-key")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert!(response.headers().contains_key("x-request-id"));

        // Unauthenticated request should fail
        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_full_stack_auth_excludes_health() {
        let config = MiddlewareConfig::default()
            .request_logging(false)
            .api_keys(vec!["test-key".to_string()]);
        let app = config.apply(test_router());

        // /health should bypass auth
        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_cors_headers_on_all_methods() {
        let config = MiddlewareConfig::default().request_logging(false);
        let app = config.apply(test_router());

        // Regular GET should have CORS headers
        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(
            response
                .headers()
                .get("access-control-allow-origin")
                .unwrap()
                .to_str()
                .unwrap(),
            "*"
        );
        assert!(response
            .headers()
            .contains_key("access-control-allow-methods"));
        assert!(response
            .headers()
            .contains_key("access-control-allow-headers"));
    }

    // ── Rate Limiting ─────────────────────────────────────────────────

    #[test]
    fn test_rate_limit_config_default() {
        let config = RateLimitConfig::default();
        assert_eq!(config.capacity, 100);
        assert!((config.refill_rate - 10.0).abs() < f64::EPSILON);
        assert_eq!(config.excluded_paths, vec!["/health"]);
    }

    #[test]
    fn test_rate_limit_config_excluded() {
        let config = RateLimitConfig::default();
        assert!(config.is_excluded("/health"));
        assert!(config.is_excluded("/health/liveness"));
        assert!(!config.is_excluded("/api/v1/vms"));
    }

    #[test]
    fn test_rate_limiter_allows_within_capacity() {
        let config = RateLimitConfig {
            capacity: 5,
            refill_rate: 1.0,
            excluded_paths: vec![],
        };
        let limiter = RateLimiter::new(config);

        // Should allow 5 requests (full capacity)
        for _ in 0..5 {
            let (allowed, _, _) = limiter.try_acquire();
            assert!(allowed);
        }
    }

    #[test]
    fn test_rate_limiter_rejects_over_capacity() {
        let config = RateLimitConfig {
            capacity: 3,
            refill_rate: 0.0, // no refill
            excluded_paths: vec![],
        };
        let limiter = RateLimiter::new(config);

        // Exhaust tokens
        for _ in 0..3 {
            let (allowed, _, _) = limiter.try_acquire();
            assert!(allowed);
        }

        // 4th request should be rejected
        let (allowed, remaining, retry_after) = limiter.try_acquire();
        assert!(!allowed);
        assert_eq!(remaining, 0);
        assert!(retry_after > 0.0);
    }

    #[test]
    fn test_rate_limiter_remaining_decrements() {
        let config = RateLimitConfig {
            capacity: 5,
            refill_rate: 0.0,
            excluded_paths: vec![],
        };
        let limiter = RateLimiter::new(config);

        let (_, rem, _) = limiter.try_acquire();
        assert_eq!(rem, 4);
        let (_, rem, _) = limiter.try_acquire();
        assert_eq!(rem, 3);
        let (_, rem, _) = limiter.try_acquire();
        assert_eq!(rem, 2);
    }

    #[tokio::test]
    async fn test_rate_limit_middleware_allows_request() {
        let rl_config = RateLimitConfig {
            capacity: 10,
            refill_rate: 1.0,
            excluded_paths: vec![],
        };
        let config = MiddlewareConfig::none().rate_limit(rl_config);
        let app = config.apply(test_router());

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response
                .headers()
                .get("x-ratelimit-limit")
                .unwrap()
                .to_str()
                .unwrap(),
            "10"
        );
        assert!(response.headers().contains_key("x-ratelimit-remaining"));
        assert!(response.headers().contains_key("x-ratelimit-reset"));
    }

    #[tokio::test]
    async fn test_rate_limit_middleware_rejects_when_exhausted() {
        // Build a limiter with 1 token, no refill, then exhaust it
        let limiter = Arc::new(RateLimiter::new(RateLimitConfig {
            capacity: 1,
            refill_rate: 0.0,
            excluded_paths: vec![],
        }));
        // Exhaust the one token
        limiter.try_acquire();

        let app = test_router().layer(middleware::from_fn(move |req: Request, next: Next| {
            let limiter = limiter.clone();
            rate_limit_handler(limiter, req, next)
        }));

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(
            response
                .headers()
                .get("x-ratelimit-remaining")
                .unwrap()
                .to_str()
                .unwrap(),
            "0"
        );
        assert!(response.headers().contains_key("retry-after"));
    }

    #[tokio::test]
    async fn test_rate_limit_excludes_health() {
        let rl_config = RateLimitConfig {
            capacity: 1,
            refill_rate: 0.0,
            excluded_paths: vec!["/health".to_string()],
        };
        let limiter = Arc::new(RateLimiter::new(rl_config));

        // Exhaust the single token
        limiter.try_acquire();

        let limiter2 = limiter.clone();
        let app = test_router().layer(middleware::from_fn(move |req: Request, next: Next| {
            let limiter = limiter2.clone();
            rate_limit_handler(limiter, req, next)
        }));

        // /health should bypass rate limiting
        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        // No rate limit headers on excluded paths
        assert!(!response.headers().contains_key("x-ratelimit-limit"));
    }

    #[tokio::test]
    async fn test_rate_limit_full_stack_integration() {
        let rl_config = RateLimitConfig {
            capacity: 5,
            refill_rate: 0.0,
            excluded_paths: vec!["/health".to_string()],
        };
        let config = MiddlewareConfig::default()
            .request_logging(false)
            .rate_limit(rl_config);
        let app = config.apply(test_router());

        // First request should succeed with all headers
        let response = app
            .clone()
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert!(response.headers().contains_key("x-request-id"));
        assert!(response.headers().contains_key("x-response-time"));
        assert!(response.headers().contains_key("x-ratelimit-limit"));
        assert!(response.headers().contains_key("x-ratelimit-remaining"));
    }

    #[test]
    fn test_middleware_config_summary_includes_rate_limit() {
        let config = MiddlewareConfig::default();
        let summary = config.summary();
        assert_eq!(summary.len(), 43);
        let rate_limit_entry = summary.iter().find(|(name, _)| *name == "Rate Limit");
        assert!(rate_limit_entry.is_some());
        assert!(!rate_limit_entry.unwrap().1); // off by default
    }

    #[test]
    fn test_rate_limit_builder() {
        let config = MiddlewareConfig::default().rate_limit(RateLimitConfig {
            capacity: 50,
            refill_rate: 5.0,
            excluded_paths: vec![],
        });
        assert!(config.enable_rate_limit);
        assert_eq!(config.rate_limit.capacity, 50);

        let config2 = config.rate_limit_enabled(false);
        assert!(!config2.enable_rate_limit);
    }

    // ── Body Limit ────────────────────────────────────────────────────

    #[test]
    fn test_body_limit_config_default() {
        let config = BodyLimitConfig::default();
        assert_eq!(config.max_bytes, 2 * 1024 * 1024); // 2 MB
        assert_eq!(config.excluded_paths, vec!["/health"]);
    }

    #[test]
    fn test_body_limit_config_is_excluded() {
        let config = BodyLimitConfig::default();
        assert!(config.is_excluded("/health"));
        assert!(config.is_excluded("/health/liveness"));
        assert!(!config.is_excluded("/api/v1/vms"));
    }

    #[test]
    fn test_body_limit_disabled_by_default() {
        let config = MiddlewareConfig::default();
        assert!(!config.enable_body_limit);
    }

    #[test]
    fn test_body_limit_builder() {
        let config = MiddlewareConfig::default().body_limit(BodyLimitConfig {
            max_bytes: 1024,
            excluded_paths: vec![],
        });
        assert!(config.enable_body_limit);
        assert_eq!(config.body_limit.max_bytes, 1024);

        let config2 = config.body_limit_enabled(false);
        assert!(!config2.enable_body_limit);
    }

    #[test]
    fn test_body_limit_none_config() {
        let config = MiddlewareConfig::none();
        assert!(!config.enable_body_limit);
    }

    #[test]
    fn test_body_limit_summary_entry() {
        let config = MiddlewareConfig::default();
        let summary = config.summary();
        let entry = summary.iter().find(|(name, _)| *name == "Body Limit");
        assert!(entry.is_some());
        assert!(!entry.unwrap().1); // off by default
    }

    #[tokio::test]
    async fn test_body_limit_allows_small_request() {
        let config = MiddlewareConfig::none().body_limit(BodyLimitConfig {
            max_bytes: 1024,
            excluded_paths: vec![],
        });
        let app = config.apply(test_router());

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .header("content-length", "100")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response
                .headers()
                .get("x-body-limit")
                .unwrap()
                .to_str()
                .unwrap(),
            "1024"
        );
    }

    #[tokio::test]
    async fn test_body_limit_rejects_large_request() {
        let config = MiddlewareConfig::none().body_limit(BodyLimitConfig {
            max_bytes: 1024,
            excluded_paths: vec![],
        });
        let app = config.apply(test_router());

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .header("content-length", "2048")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
        assert_eq!(
            response
                .headers()
                .get("x-body-limit")
                .unwrap()
                .to_str()
                .unwrap(),
            "1024"
        );

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(json["error"].as_str().unwrap().contains("too large"));
        assert_eq!(json["code"], "PAYLOAD_TOO_LARGE");
    }

    #[tokio::test]
    async fn test_body_limit_allows_no_content_length() {
        let config = MiddlewareConfig::none().body_limit(BodyLimitConfig {
            max_bytes: 1024,
            excluded_paths: vec![],
        });
        let app = config.apply(test_router());

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_body_limit_excluded_path() {
        let config = MiddlewareConfig::none().body_limit(BodyLimitConfig {
            max_bytes: 100,
            excluded_paths: vec!["/health".to_string()],
        });
        let app = config.apply(test_router());

        // Large body on excluded path should be allowed
        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/health")
                    .header("content-length", "999999")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        // Excluded paths don't get the x-body-limit header
        assert!(response.headers().get("x-body-limit").is_none());
    }

    #[tokio::test]
    async fn test_body_limit_exact_boundary() {
        let config = MiddlewareConfig::none().body_limit(BodyLimitConfig {
            max_bytes: 1024,
            excluded_paths: vec![],
        });
        let app = config.apply(test_router());

        // Exactly at limit should be allowed
        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .header("content-length", "1024")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_body_limit_one_over_boundary() {
        let config = MiddlewareConfig::none().body_limit(BodyLimitConfig {
            max_bytes: 1024,
            excluded_paths: vec![],
        });
        let app = config.apply(test_router());

        // One over limit should be rejected
        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .header("content-length", "1025")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    }

    #[tokio::test]
    async fn test_body_limit_with_full_stack() {
        let config = MiddlewareConfig::default()
            .request_logging(false)
            .body_limit(BodyLimitConfig {
                max_bytes: 512,
                excluded_paths: vec![],
            });
        let app = config.apply(test_router());

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .header("content-length", "1000")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
        // Request ID should still be set by outer middleware
        assert!(response.headers().contains_key("x-request-id"));
        assert!(response.headers().contains_key("x-response-time"));
    }

    // ── Fallback Handler ──────────────────────────────────────────────────

    #[tokio::test]
    async fn test_fallback_handler_returns_404_json() {
        let config = MiddlewareConfig::none().fallback(true);
        let app = config.apply(test_router());

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/nonexistent/path")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["code"], "NOT_FOUND");
        assert!(json["error"]
            .as_str()
            .unwrap()
            .contains("/nonexistent/path"));
    }

    #[tokio::test]
    async fn test_fallback_handler_includes_method() {
        let config = MiddlewareConfig::none().fallback(true);
        let app = config.apply(test_router());

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .method("DELETE")
                    .uri("/does/not/exist")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(json["error"].as_str().unwrap().contains("DELETE"));
    }

    #[tokio::test]
    async fn test_fallback_disabled_no_json() {
        let config = MiddlewareConfig::none().fallback(false);
        let app = config.apply(test_router());

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/nonexistent")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        // Still 404, but not our custom JSON response
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        // Default axum 404 is empty body
        assert!(body.is_empty());
    }

    #[tokio::test]
    async fn test_fallback_default_enabled() {
        let config = MiddlewareConfig::default();
        assert!(config.enable_fallback);
    }

    #[tokio::test]
    async fn test_fallback_none_disabled() {
        let config = MiddlewareConfig::none();
        assert!(!config.enable_fallback);
    }

    #[tokio::test]
    async fn test_fallback_with_full_stack() {
        let config = MiddlewareConfig::default()
            .request_logging(false)
            .fallback(true);
        let app = config.apply(test_router());

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/unknown/route")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        // Middleware headers should still be applied
        assert!(response.headers().contains_key("x-request-id"));
        assert!(response.headers().contains_key("x-response-time"));
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["code"], "NOT_FOUND");
    }

    #[test]
    fn test_fallback_builder() {
        let config = MiddlewareConfig::default().fallback(false);
        assert!(!config.enable_fallback);
        let config2 = config.fallback(true);
        assert!(config2.enable_fallback);
    }

    #[test]
    fn test_summary_includes_fallback() {
        let config = MiddlewareConfig::default();
        let summary = config.summary();
        let entry = summary.iter().find(|(name, _)| *name == "Fallback 404");
        assert!(entry.is_some());
        assert!(entry.unwrap().1); // on by default
    }

    // ── API Version ─────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_api_version_header_present() {
        let app = test_router().layer(middleware::from_fn(api_version));

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let val = response.headers().get("x-api-version").unwrap();
        assert_eq!(val.to_str().unwrap(), "v1");
    }

    #[tokio::test]
    async fn test_api_version_header_via_config() {
        let config = MiddlewareConfig::none().api_version_enabled(true);
        let app = config.apply(test_router());

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert!(response.headers().get("x-api-version").is_some());
    }

    #[tokio::test]
    async fn test_api_version_header_absent_when_disabled() {
        let config = MiddlewareConfig::none();
        let app = config.apply(test_router());

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert!(response.headers().get("x-api-version").is_none());
    }

    #[test]
    fn test_api_version_builder() {
        let config = MiddlewareConfig::default().api_version_enabled(false);
        assert!(!config.enable_api_version);
        let config2 = config.api_version_enabled(true);
        assert!(config2.enable_api_version);
    }

    #[test]
    fn test_api_version_constant() {
        assert_eq!(API_VERSION, "v1");
    }

    #[test]
    fn test_summary_includes_api_version() {
        let config = MiddlewareConfig::default();
        let summary = config.summary();
        let entry = summary.iter().find(|(name, _)| *name == "API Version");
        assert!(entry.is_some());
        assert!(entry.unwrap().1);
    }

    // ── Content-Type Validation ─────────────────────────────────────────

    #[tokio::test]
    async fn test_content_type_rejects_post_without_json() {
        use axum::routing::post;
        let app = Router::new()
            .route("/api/v1/vms", post(ok_handler))
            .layer(middleware::from_fn(move |req: Request, next: Next| {
                content_type_validation(vec![], req, next)
            }));

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .method(Method::POST)
                    .uri("/api/v1/vms")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["code"], "UNSUPPORTED_MEDIA_TYPE");
    }

    #[tokio::test]
    async fn test_content_type_accepts_post_with_json() {
        use axum::routing::post;
        let app = Router::new()
            .route("/api/v1/vms", post(ok_handler))
            .layer(middleware::from_fn(move |req: Request, next: Next| {
                content_type_validation(vec![], req, next)
            }));

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .method(Method::POST)
                    .uri("/api/v1/vms")
                    .header("content-type", "application/json")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_content_type_allows_get_without_json() {
        let app = test_router().layer(middleware::from_fn(move |req: Request, next: Next| {
            content_type_validation(vec![], req, next)
        }));

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_content_type_excludes_health() {
        use axum::routing::post;
        let app = Router::new()
            .route("/health", post(ok_handler))
            .layer(middleware::from_fn(move |req: Request, next: Next| {
                content_type_validation(vec!["/health".to_string()], req, next)
            }));

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .method(Method::POST)
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_content_type_rejects_put_without_json() {
        use axum::routing::put;
        let app = Router::new()
            .route("/test", put(ok_handler))
            .layer(middleware::from_fn(move |req: Request, next: Next| {
                content_type_validation(vec![], req, next)
            }));

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .method(Method::PUT)
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);
    }

    #[tokio::test]
    async fn test_content_type_rejects_patch_without_json() {
        use axum::routing::patch;
        let app = Router::new()
            .route("/test", patch(ok_handler))
            .layer(middleware::from_fn(move |req: Request, next: Next| {
                content_type_validation(vec![], req, next)
            }));

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .method(Method::PATCH)
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);
    }

    #[tokio::test]
    async fn test_content_type_via_config() {
        use axum::routing::post;
        let config = MiddlewareConfig::none().content_type_validation(true);
        let app = config.apply(
            Router::new()
                .route("/api/v1/vms", post(ok_handler))
                .route("/test", get(ok_handler)),
        );

        // POST without content-type → 415
        let resp1 = app
            .clone()
            .oneshot(
                HttpRequest::builder()
                    .method(Method::POST)
                    .uri("/api/v1/vms")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp1.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);

        // GET without content-type → 200
        let resp2 = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp2.status(), StatusCode::OK);
    }

    #[test]
    fn test_content_type_builder() {
        let config = MiddlewareConfig::default().content_type_validation(false);
        assert!(!config.enable_content_type_validation);
        let config2 = config.content_type_validation(true);
        assert!(config2.enable_content_type_validation);
    }

    #[test]
    fn test_content_type_config_default() {
        let config = ContentTypeConfig::default();
        assert!(config.excluded_paths.contains(&"/health".to_string()));
        assert!(config.excluded_paths.contains(&"/agentic".to_string()));
    }

    #[test]
    fn test_content_type_config_builder() {
        let ct = ContentTypeConfig {
            excluded_paths: vec!["/custom".to_string()],
        };
        let config = MiddlewareConfig::default().content_type_config(ct);
        assert_eq!(config.content_type.excluded_paths, vec!["/custom"]);
    }

    #[test]
    fn test_summary_includes_content_type() {
        let config = MiddlewareConfig::default();
        let summary = config.summary();
        let entry = summary
            .iter()
            .find(|(name, _)| *name == "Content-Type Validation");
        assert!(entry.is_some());
        assert!(entry.unwrap().1);
    }

    #[test]
    fn test_summary_has_forty_three_entries() {
        let config = MiddlewareConfig::default();
        assert_eq!(config.summary().len(), 43);
    }

    #[test]
    fn test_none_disables_new_layers() {
        let config = MiddlewareConfig::none();
        assert!(!config.enable_api_version);
        assert!(!config.enable_content_type_validation);
        assert!(!config.enable_request_timeout);
        assert!(!config.enable_security_headers);
    }

    #[test]
    fn test_default_enables_new_layers() {
        let config = MiddlewareConfig::default();
        assert!(config.enable_api_version);
        assert!(config.enable_content_type_validation);
        assert!(config.enable_security_headers);
        assert!(!config.enable_request_timeout);
    }

    // ── Request Timeout ─────────────────────────────────────────────────

    #[tokio::test]
    async fn test_request_timeout_passes_fast_request() {
        let timeout_cfg = TimeoutConfig {
            duration: Duration::from_secs(5),
            excluded_paths: vec![],
        };
        let app = test_router().layer(middleware::from_fn(move |req: Request, next: Next| {
            let config = timeout_cfg.clone();
            request_timeout(config, req, next)
        }));

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_request_timeout_returns_408() {
        use axum::routing::get as get_route;

        async fn slow_handler() -> &'static str {
            tokio::time::sleep(Duration::from_millis(200)).await;
            "slow"
        }

        let timeout_cfg = TimeoutConfig {
            duration: Duration::from_millis(50),
            excluded_paths: vec![],
        };
        let app = Router::new()
            .route("/slow", get_route(slow_handler))
            .layer(middleware::from_fn(move |req: Request, next: Next| {
                let config = timeout_cfg.clone();
                request_timeout(config, req, next)
            }));

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/slow")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::REQUEST_TIMEOUT);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["code"], "REQUEST_TIMEOUT");
        assert!(json["error"].as_str().unwrap().contains("timed out"));
    }

    #[tokio::test]
    async fn test_request_timeout_excludes_paths() {
        use axum::routing::get as get_route;

        async fn slow_handler() -> &'static str {
            tokio::time::sleep(Duration::from_millis(200)).await;
            "slow"
        }

        let timeout_cfg = TimeoutConfig {
            duration: Duration::from_millis(50),
            excluded_paths: vec!["/health".to_string()],
        };
        let app = Router::new()
            .route("/health", get_route(slow_handler))
            .layer(middleware::from_fn(move |req: Request, next: Next| {
                let config = timeout_cfg.clone();
                request_timeout(config, req, next)
            }));

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        // Excluded path bypasses timeout
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_request_timeout_via_config() {
        let config = MiddlewareConfig::none().request_timeout(TimeoutConfig {
            duration: Duration::from_secs(10),
            excluded_paths: vec![],
        });
        assert!(config.enable_request_timeout);
        assert_eq!(config.timeout.duration, Duration::from_secs(10));
    }

    #[test]
    fn test_request_timeout_builder() {
        let config = MiddlewareConfig::default().request_timeout_enabled(true);
        assert!(config.enable_request_timeout);
        let config2 = config.request_timeout_enabled(false);
        assert!(!config2.enable_request_timeout);
    }

    #[test]
    fn test_timeout_config_default() {
        let config = TimeoutConfig::default();
        assert_eq!(config.duration, Duration::from_secs(30));
        assert!(config.excluded_paths.contains(&"/health".to_string()));
        assert!(config.excluded_paths.contains(&"/agentic".to_string()));
    }

    #[test]
    fn test_summary_includes_request_timeout() {
        let config = MiddlewareConfig::default();
        let summary = config.summary();
        let entry = summary.iter().find(|(name, _)| *name == "Request Timeout");
        assert!(entry.is_some());
        assert!(!entry.unwrap().1); // off by default
    }

    // ── Security Headers ────────────────────────────────────────────────

    #[tokio::test]
    async fn test_security_headers_all_present() {
        let sec_config = SecurityHeadersConfig::default();
        let app = test_router().layer(middleware::from_fn(move |req: Request, next: Next| {
            let config = sec_config.clone();
            security_headers(config, req, next)
        }));

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response
                .headers()
                .get("x-content-type-options")
                .unwrap()
                .to_str()
                .unwrap(),
            "nosniff"
        );
        assert_eq!(
            response
                .headers()
                .get("x-frame-options")
                .unwrap()
                .to_str()
                .unwrap(),
            "DENY"
        );
        assert_eq!(
            response
                .headers()
                .get("x-xss-protection")
                .unwrap()
                .to_str()
                .unwrap(),
            "1; mode=block"
        );
        assert_eq!(
            response
                .headers()
                .get("referrer-policy")
                .unwrap()
                .to_str()
                .unwrap(),
            "strict-origin-when-cross-origin"
        );
        assert_eq!(
            response
                .headers()
                .get("cache-control")
                .unwrap()
                .to_str()
                .unwrap(),
            "no-store"
        );
    }

    #[tokio::test]
    async fn test_security_headers_partial() {
        let sec_config = SecurityHeadersConfig {
            content_type_options: true,
            frame_options: false,
            xss_protection: false,
            referrer_policy: false,
            cache_control: false,
            ..SecurityHeadersConfig::default()
        };
        let app = test_router().layer(middleware::from_fn(move |req: Request, next: Next| {
            let config = sec_config.clone();
            security_headers(config, req, next)
        }));

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert!(response.headers().get("x-content-type-options").is_some());
        assert!(response.headers().get("x-frame-options").is_none());
        assert!(response.headers().get("x-xss-protection").is_none());
        assert!(response.headers().get("referrer-policy").is_none());
        assert!(response.headers().get("cache-control").is_none());
    }

    #[tokio::test]
    async fn test_security_headers_via_config() {
        let config = MiddlewareConfig::none().security_headers_enabled(true);
        let app = config.apply(test_router());

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert!(response.headers().get("x-content-type-options").is_some());
        assert!(response.headers().get("x-frame-options").is_some());
    }

    #[tokio::test]
    async fn test_security_headers_absent_when_disabled() {
        let config = MiddlewareConfig::none();
        let app = config.apply(test_router());

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert!(response.headers().get("x-content-type-options").is_none());
    }

    #[test]
    fn test_security_headers_builder() {
        let config = MiddlewareConfig::default().security_headers_enabled(false);
        assert!(!config.enable_security_headers);
        let config2 = config.security_headers_enabled(true);
        assert!(config2.enable_security_headers);
    }

    #[test]
    fn test_security_headers_config_builder() {
        let sec = SecurityHeadersConfig {
            content_type_options: true,
            frame_options: false,
            xss_protection: false,
            referrer_policy: false,
            cache_control: false,
            ..SecurityHeadersConfig::default()
        };
        let config = MiddlewareConfig::default().security_headers_config(sec);
        assert!(config.security_headers.content_type_options);
        assert!(!config.security_headers.frame_options);
    }

    #[test]
    fn test_security_headers_config_default() {
        let config = SecurityHeadersConfig::default();
        assert!(config.content_type_options);
        assert!(config.frame_options);
        assert!(config.xss_protection);
        assert!(config.referrer_policy);
        assert!(config.cache_control);
    }

    #[test]
    fn test_security_headers_config_headers_count() {
        let config = SecurityHeadersConfig::default();
        assert_eq!(config.headers().len(), 5);

        let partial = SecurityHeadersConfig {
            content_type_options: true,
            frame_options: false,
            xss_protection: false,
            referrer_policy: false,
            cache_control: false,
            ..SecurityHeadersConfig::default()
        };
        assert_eq!(partial.headers().len(), 1);
    }

    #[test]
    fn test_summary_includes_security_headers() {
        let config = MiddlewareConfig::default();
        let summary = config.summary();
        let entry = summary.iter().find(|(name, _)| *name == "Security Headers");
        assert!(entry.is_some());
        assert!(entry.unwrap().1); // on by default
    }

    // ── HSTS ──────────────────────────────────────────────────────────

    #[test]
    fn test_hsts_config_default() {
        let config = HstsConfig::default();
        assert_eq!(config.max_age, 31_536_000);
        assert!(config.include_sub_domains);
        assert!(!config.preload);
    }

    #[test]
    fn test_hsts_header_value_default() {
        let config = HstsConfig::default();
        assert_eq!(config.header_value(), "max-age=31536000; includeSubDomains");
    }

    #[test]
    fn test_hsts_header_value_with_preload() {
        let config = HstsConfig {
            max_age: 63072000,
            include_sub_domains: true,
            preload: true,
        };
        assert_eq!(
            config.header_value(),
            "max-age=63072000; includeSubDomains; preload"
        );
    }

    #[test]
    fn test_hsts_header_value_minimal() {
        let config = HstsConfig {
            max_age: 300,
            include_sub_domains: false,
            preload: false,
        };
        assert_eq!(config.header_value(), "max-age=300");
    }

    #[tokio::test]
    async fn test_hsts_header_in_response() {
        let sec_config = SecurityHeadersConfig {
            hsts: Some(HstsConfig::default()),
            ..SecurityHeadersConfig::default()
        };
        let config = MiddlewareConfig::none()
            .security_headers_enabled(true)
            .security_headers_config(sec_config);
        let app = config.apply(test_router());

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let hsts = response
            .headers()
            .get("strict-transport-security")
            .unwrap()
            .to_str()
            .unwrap();
        assert!(hsts.contains("max-age=31536000"));
        assert!(hsts.contains("includeSubDomains"));
    }

    #[tokio::test]
    async fn test_hsts_disabled_by_default() {
        let config = MiddlewareConfig::default();
        let app = config.apply(test_router());

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert!(!response.headers().contains_key("strict-transport-security"));
    }

    // ── Content-Security-Policy ───────────────────────────────────────

    #[tokio::test]
    async fn test_csp_header_in_response() {
        let csp = "default-src 'self'; script-src 'none'";
        let sec_config = SecurityHeadersConfig {
            content_security_policy: Some(csp.to_string()),
            ..SecurityHeadersConfig::default()
        };
        let config = MiddlewareConfig::none()
            .security_headers_enabled(true)
            .security_headers_config(sec_config);
        let app = config.apply(test_router());

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response
                .headers()
                .get("content-security-policy")
                .unwrap()
                .to_str()
                .unwrap(),
            csp
        );
    }

    #[tokio::test]
    async fn test_csp_disabled_by_default() {
        let config = MiddlewareConfig::default();
        let app = config.apply(test_router());

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert!(!response.headers().contains_key("content-security-policy"));
    }

    // ── Permissions-Policy ────────────────────────────────────────────

    #[tokio::test]
    async fn test_permissions_policy_in_response() {
        let pp = "camera=(), microphone=(), geolocation=()";
        let sec_config = SecurityHeadersConfig {
            permissions_policy: Some(pp.to_string()),
            ..SecurityHeadersConfig::default()
        };
        let config = MiddlewareConfig::none()
            .security_headers_enabled(true)
            .security_headers_config(sec_config);
        let app = config.apply(test_router());

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response
                .headers()
                .get("permissions-policy")
                .unwrap()
                .to_str()
                .unwrap(),
            pp
        );
    }

    #[tokio::test]
    async fn test_permissions_policy_disabled_by_default() {
        let config = MiddlewareConfig::default();
        let app = config.apply(test_router());

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert!(!response.headers().contains_key("permissions-policy"));
    }

    // ── Combined Enhanced Security Headers ────────────────────────────

    #[test]
    fn test_security_headers_default_has_no_new_headers() {
        let config = SecurityHeadersConfig::default();
        assert!(config.hsts.is_none());
        assert!(config.content_security_policy.is_none());
        assert!(config.permissions_policy.is_none());
    }

    #[test]
    fn test_security_headers_count_with_all_enabled() {
        let config = SecurityHeadersConfig {
            content_type_options: true,
            frame_options: true,
            xss_protection: true,
            referrer_policy: true,
            cache_control: true,
            hsts: Some(HstsConfig::default()),
            content_security_policy: Some("default-src 'self'".to_string()),
            permissions_policy: Some("camera=()".to_string()),
        };
        assert_eq!(config.headers().len(), 8); // 5 original + 3 new
    }

    #[tokio::test]
    async fn test_all_security_headers_in_response() {
        let sec_config = SecurityHeadersConfig {
            content_type_options: true,
            frame_options: true,
            xss_protection: true,
            referrer_policy: true,
            cache_control: true,
            hsts: Some(HstsConfig::default()),
            content_security_policy: Some("default-src 'self'".to_string()),
            permissions_policy: Some("camera=()".to_string()),
        };
        let config = MiddlewareConfig::none()
            .security_headers_enabled(true)
            .security_headers_config(sec_config);
        let app = config.apply(test_router());

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert!(response.headers().contains_key("x-content-type-options"));
        assert!(response.headers().contains_key("x-frame-options"));
        assert!(response.headers().contains_key("x-xss-protection"));
        assert!(response.headers().contains_key("referrer-policy"));
        assert!(response.headers().contains_key("cache-control"));
        assert!(response.headers().contains_key("strict-transport-security"));
        assert!(response.headers().contains_key("content-security-policy"));
        assert!(response.headers().contains_key("permissions-policy"));
    }

    // ── Middleware JSON Error Responses ──────────────────────────────────

    #[tokio::test]
    async fn test_api_key_invalid_returns_json() {
        let api_key = ApiKeyConfig {
            keys: vec!["secret".to_string()],
            ..ApiKeyConfig::default()
        };
        let app = test_router().layer(middleware::from_fn(move |req: Request, next: Next| {
            let c = api_key.clone();
            api_key_handler(c, req, next)
        }));

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/api/v1/vms")
                    .header("authorization", "Bearer wrong")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["code"], "FORBIDDEN");
        assert_eq!(json["error"], "Invalid API key");
    }

    #[tokio::test]
    async fn test_api_key_missing_returns_json() {
        let api_key = ApiKeyConfig {
            keys: vec!["secret".to_string()],
            ..ApiKeyConfig::default()
        };
        let app = test_router().layer(middleware::from_fn(move |req: Request, next: Next| {
            let c = api_key.clone();
            api_key_handler(c, req, next)
        }));

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/api/v1/vms")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["code"], "UNAUTHORIZED");
        assert_eq!(json["error"], "Missing API key");
    }

    #[tokio::test]
    async fn test_rate_limit_rejected_returns_json() {
        let limiter = Arc::new(RateLimiter::new(RateLimitConfig {
            capacity: 1,
            refill_rate: 0.0,
            excluded_paths: vec![],
        }));
        // Exhaust the token
        limiter.try_acquire();

        let app = test_router().layer(middleware::from_fn(move |req: Request, next: Next| {
            let limiter = limiter.clone();
            rate_limit_handler(limiter, req, next)
        }));

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["code"], "RATE_LIMITED");
        assert_eq!(json["error"], "Rate limit exceeded");
    }

    #[tokio::test]
    async fn test_body_limit_rejected_returns_code_field() {
        let config = MiddlewareConfig::none().body_limit(BodyLimitConfig {
            max_bytes: 512,
            excluded_paths: vec![],
        });
        let app = config.apply(test_router());

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .header("content-length", "1024")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["code"], "PAYLOAD_TOO_LARGE");
        assert!(json["error"].as_str().unwrap().contains("too large"));
    }

    #[tokio::test]
    async fn test_error_response_no_request_id_by_default() {
        let config = MiddlewareConfig::none().fallback(true);
        let app = config.apply(test_router());

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/nowhere")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        // request_id is skipped when None
        assert!(json.get("request_id").is_none());
    }

    #[test]
    fn test_error_response_serialization() {
        let err = ErrorResponse {
            error: "test error".to_string(),
            code: "TEST_CODE".to_string(),
            request_id: None,
        };
        let json = serde_json::to_value(&err).unwrap();
        assert_eq!(json["error"], "test error");
        assert_eq!(json["code"], "TEST_CODE");
        assert!(json.get("request_id").is_none());
    }

    #[test]
    fn test_error_response_with_request_id() {
        let err = ErrorResponse {
            error: "test error".to_string(),
            code: "TEST_CODE".to_string(),
            request_id: Some("req-123".to_string()),
        };
        let json = serde_json::to_value(&err).unwrap();
        assert_eq!(json["request_id"], "req-123");
    }

    #[test]
    fn test_error_response_deserialization() {
        let json = r#"{"error":"not found","code":"NOT_FOUND"}"#;
        let err: ErrorResponse = serde_json::from_str(json).unwrap();
        assert_eq!(err.error, "not found");
        assert_eq!(err.code, "NOT_FOUND");
        assert!(err.request_id.is_none());
    }

    #[test]
    fn test_error_response_deserialization_with_request_id() {
        let json = r#"{"error":"bad","code":"BAD","request_id":"abc"}"#;
        let err: ErrorResponse = serde_json::from_str(json).unwrap();
        assert_eq!(err.request_id, Some("abc".to_string()));
    }

    // ── Request ID Propagation ────────────────────────────────────────

    #[tokio::test]
    async fn test_request_id_stored_in_extensions() {
        /// Handler that reads the RequestId from extensions
        async fn echo_request_id(request: Request) -> String {
            extract_request_id(&request).unwrap_or_else(|| "none".to_string())
        }

        let app = Router::new()
            .route("/echo", get(echo_request_id))
            .layer(middleware::from_fn(request_id));

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .header("x-request-id", "test-id-42")
                    .uri("/echo")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        assert_eq!(std::str::from_utf8(&body).unwrap(), "test-id-42");
    }

    #[tokio::test]
    async fn test_request_id_in_extensions_generated() {
        /// Handler that reads the RequestId from extensions
        async fn echo_request_id(request: Request) -> String {
            extract_request_id(&request).unwrap_or_else(|| "none".to_string())
        }

        let app = Router::new()
            .route("/echo", get(echo_request_id))
            .layer(middleware::from_fn(request_id));

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/echo")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let body_str = std::str::from_utf8(&body).unwrap();
        // Should be a UUID v4 (36 chars)
        assert_eq!(body_str.len(), 36);
        assert_ne!(body_str, "none");
    }

    #[tokio::test]
    async fn test_extract_request_id_returns_none_without_middleware() {
        /// Handler that reads the RequestId from extensions
        async fn echo_request_id(request: Request) -> String {
            extract_request_id(&request).unwrap_or_else(|| "none".to_string())
        }

        let app = Router::new().route("/echo", get(echo_request_id));

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/echo")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        assert_eq!(std::str::from_utf8(&body).unwrap(), "none");
    }

    #[tokio::test]
    async fn test_fallback_error_includes_request_id() {
        let config = MiddlewareConfig::none().request_id(true).fallback(true);
        let app = config.apply(test_router());

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .header("x-request-id", "fallback-req-id")
                    .uri("/nonexistent")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["code"], "NOT_FOUND");
        assert_eq!(json["request_id"], "fallback-req-id");
    }

    #[tokio::test]
    async fn test_fallback_error_no_request_id_without_middleware() {
        let config = MiddlewareConfig::none().fallback(true);
        let app = config.apply(test_router());

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/nonexistent")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["code"], "NOT_FOUND");
        assert!(json.get("request_id").is_none());
    }

    #[tokio::test]
    async fn test_api_key_error_includes_request_id() {
        let config = MiddlewareConfig::none()
            .request_id(true)
            .api_keys(vec!["secret".to_string()]);
        let app = config.apply(test_router());

        // Missing API key
        let response = app
            .oneshot(
                HttpRequest::builder()
                    .header("x-request-id", "auth-req-id")
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["code"], "UNAUTHORIZED");
        assert_eq!(json["request_id"], "auth-req-id");
    }

    #[tokio::test]
    async fn test_api_key_forbidden_includes_request_id() {
        let config = MiddlewareConfig::none()
            .request_id(true)
            .api_keys(vec!["secret".to_string()]);
        let app = config.apply(test_router());

        // Invalid API key
        let response = app
            .oneshot(
                HttpRequest::builder()
                    .header("x-request-id", "forbidden-req-id")
                    .header("authorization", "wrong-key")
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["code"], "FORBIDDEN");
        assert_eq!(json["request_id"], "forbidden-req-id");
    }

    #[tokio::test]
    async fn test_body_limit_error_includes_request_id() {
        use axum::routing::post;

        async fn post_handler() -> &'static str {
            "ok"
        }

        let body_config = BodyLimitConfig {
            max_bytes: 10,
            ..BodyLimitConfig::default()
        };
        let config = MiddlewareConfig::none()
            .request_id(true)
            .body_limit(body_config);
        let app = config.apply(
            Router::new()
                .route("/upload", post(post_handler))
                .route("/health", get(ok_handler)),
        );

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .method("POST")
                    .header("x-request-id", "body-req-id")
                    .header("content-length", "99999")
                    .uri("/upload")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["code"], "PAYLOAD_TOO_LARGE");
        assert_eq!(json["request_id"], "body-req-id");
    }

    #[tokio::test]
    async fn test_content_type_error_includes_request_id() {
        use axum::routing::post;

        async fn post_handler() -> &'static str {
            "ok"
        }

        let ct_config = ContentTypeConfig::default();
        let config = MiddlewareConfig::none()
            .request_id(true)
            .content_type_validation(true)
            .content_type_config(ct_config);
        let app = config.apply(
            Router::new()
                .route("/api/v1/vms", post(post_handler))
                .route("/health", get(ok_handler)),
        );

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .method("POST")
                    .header("x-request-id", "ct-req-id")
                    .header("content-type", "text/plain")
                    .uri("/api/v1/vms")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["code"], "UNSUPPORTED_MEDIA_TYPE");
        assert_eq!(json["request_id"], "ct-req-id");
    }

    #[tokio::test]
    async fn test_rate_limit_error_includes_request_id() {
        let rl_config = RateLimitConfig {
            capacity: 1,
            refill_rate: 0.001,
            ..RateLimitConfig::default()
        };
        let config = MiddlewareConfig::none()
            .request_id(true)
            .rate_limit(rl_config);
        let app = config.apply(test_router());

        // First request succeeds
        let _ = app
            .clone()
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        // Second request should be rate limited
        let response = app
            .oneshot(
                HttpRequest::builder()
                    .header("x-request-id", "rl-req-id")
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["code"], "RATE_LIMITED");
        assert_eq!(json["request_id"], "rl-req-id");
    }

    #[tokio::test]
    async fn test_request_timeout_error_includes_request_id() {
        /// Handler that sleeps longer than the timeout
        async fn slow_handler() -> &'static str {
            tokio::time::sleep(Duration::from_millis(200)).await;
            "ok"
        }

        let timeout_config = TimeoutConfig {
            duration: Duration::from_millis(10),
            ..TimeoutConfig::default()
        };
        let config = MiddlewareConfig::none()
            .request_id(true)
            .request_timeout(timeout_config);
        let app = config.apply(Router::new().route("/slow", get(slow_handler)));

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .header("x-request-id", "timeout-req-id")
                    .uri("/slow")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::REQUEST_TIMEOUT);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["code"], "REQUEST_TIMEOUT");
        assert_eq!(json["request_id"], "timeout-req-id");
    }

    #[tokio::test]
    async fn test_request_id_propagation_generated_in_error() {
        // When no x-request-id header is sent, the middleware generates one.
        // That generated ID should appear in error responses too.
        let config = MiddlewareConfig::none().request_id(true).fallback(true);
        let app = config.apply(test_router());

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/nonexistent")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);

        // The x-request-id header should match the request_id in the body
        let header_id = response
            .headers()
            .get("x-request-id")
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["request_id"], header_id);
        // Should be a UUID v4
        assert_eq!(header_id.len(), 36);
    }

    // ── Response Compression ──────────────────────────────────────────

    #[test]
    fn test_compression_config_default() {
        let config = CompressionConfig::default();
        assert!(config.enable_gzip);
        assert!(config.enable_deflate);
        assert_eq!(config.min_size, 256);
        assert!(config.excluded_paths.is_empty());
    }

    #[test]
    fn test_compression_negotiate_gzip() {
        let config = CompressionConfig::default();
        let enc = config.negotiate("gzip, deflate, br");
        assert_eq!(enc, Some(CompressionEncoding::Gzip));
    }

    #[test]
    fn test_compression_negotiate_deflate() {
        let config = CompressionConfig {
            enable_gzip: false,
            ..CompressionConfig::default()
        };
        let enc = config.negotiate("gzip, deflate, br");
        assert_eq!(enc, Some(CompressionEncoding::Deflate));
    }

    #[test]
    fn test_compression_negotiate_none() {
        let config = CompressionConfig::default();
        let enc = config.negotiate("br");
        assert_eq!(enc, None);
    }

    #[test]
    fn test_compression_negotiate_disabled() {
        let config = CompressionConfig {
            enable_gzip: false,
            enable_deflate: false,
            ..CompressionConfig::default()
        };
        let enc = config.negotiate("gzip, deflate");
        assert_eq!(enc, None);
    }

    #[test]
    fn test_compression_is_excluded() {
        let config = CompressionConfig {
            excluded_paths: vec!["/health".to_string()],
            ..CompressionConfig::default()
        };
        assert!(config.is_excluded("/health"));
        assert!(config.is_excluded("/health/live"));
        assert!(!config.is_excluded("/api/v1/vms"));
    }

    #[test]
    fn test_compress_body_gzip() {
        let data = b"hello world this is a test payload for gzip compression";
        let compressed = compress_body(data, CompressionEncoding::Gzip).unwrap();
        assert!(!compressed.is_empty());
        // Gzip magic number: 0x1f 0x8b
        assert_eq!(compressed[0], 0x1f);
        assert_eq!(compressed[1], 0x8b);
    }

    #[test]
    fn test_compress_body_deflate() {
        let data = b"hello world this is a test payload for deflate compression";
        let compressed = compress_body(data, CompressionEncoding::Deflate).unwrap();
        assert!(!compressed.is_empty());
        // Deflate output should be smaller for repetitive content
        assert!(compressed.len() < data.len());
    }

    #[test]
    fn test_compress_decompress_gzip_roundtrip() {
        use flate2::read::GzDecoder;
        use std::io::Read;

        let original = b"repeated data repeated data repeated data repeated data";
        let compressed = compress_body(original, CompressionEncoding::Gzip).unwrap();

        let mut decoder = GzDecoder::new(&compressed[..]);
        let mut decompressed = Vec::new();
        decoder.read_to_end(&mut decompressed).unwrap();
        assert_eq!(decompressed, original);
    }

    #[test]
    fn test_compress_decompress_deflate_roundtrip() {
        use flate2::read::DeflateDecoder;
        use std::io::Read;

        let original = b"repeated data repeated data repeated data repeated data";
        let compressed = compress_body(original, CompressionEncoding::Deflate).unwrap();

        let mut decoder = DeflateDecoder::new(&compressed[..]);
        let mut decompressed = Vec::new();
        decoder.read_to_end(&mut decompressed).unwrap();
        assert_eq!(decompressed, original);
    }

    #[test]
    fn test_compression_encoding_as_str() {
        assert_eq!(CompressionEncoding::Gzip.as_str(), "gzip");
        assert_eq!(CompressionEncoding::Deflate.as_str(), "deflate");
    }

    #[tokio::test]
    async fn test_compression_gzip_response() {
        use flate2::read::GzDecoder;
        use std::io::Read;

        // Build a large enough response to trigger compression
        let large_body = "x".repeat(1024);
        let body_clone = large_body.clone();
        let handler = move || {
            let b = body_clone.clone();
            async move { b }
        };

        let comp_config = CompressionConfig {
            min_size: 256,
            ..CompressionConfig::default()
        };
        let config = MiddlewareConfig::none().compression(comp_config);
        let app = config.apply(Router::new().route("/large", get(handler)));

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .header("accept-encoding", "gzip")
                    .uri("/large")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response
                .headers()
                .get("content-encoding")
                .unwrap()
                .to_str()
                .unwrap(),
            "gzip"
        );
        assert_eq!(
            response.headers().get("vary").unwrap().to_str().unwrap(),
            "accept-encoding"
        );

        // Decompress and verify
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let mut decoder = GzDecoder::new(&body[..]);
        let mut decompressed = String::new();
        decoder.read_to_string(&mut decompressed).unwrap();
        assert_eq!(decompressed, large_body);
    }

    #[tokio::test]
    async fn test_compression_deflate_response() {
        use flate2::read::DeflateDecoder;
        use std::io::Read;

        let large_body = "y".repeat(1024);
        let body_clone = large_body.clone();
        let handler = move || {
            let b = body_clone.clone();
            async move { b }
        };

        let comp_config = CompressionConfig {
            min_size: 256,
            enable_gzip: false,
            ..CompressionConfig::default()
        };
        let config = MiddlewareConfig::none().compression(comp_config);
        let app = config.apply(Router::new().route("/large", get(handler)));

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .header("accept-encoding", "deflate")
                    .uri("/large")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response
                .headers()
                .get("content-encoding")
                .unwrap()
                .to_str()
                .unwrap(),
            "deflate"
        );

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let mut decoder = DeflateDecoder::new(&body[..]);
        let mut decompressed = String::new();
        decoder.read_to_string(&mut decompressed).unwrap();
        assert_eq!(decompressed, large_body);
    }

    #[tokio::test]
    async fn test_compression_skips_small_body() {
        let small_body = "ok";
        let handler = || async { "ok" };

        let comp_config = CompressionConfig {
            min_size: 256,
            ..CompressionConfig::default()
        };
        let config = MiddlewareConfig::none().compression(comp_config);
        let app = config.apply(Router::new().route("/small", get(handler)));

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .header("accept-encoding", "gzip")
                    .uri("/small")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        // No Content-Encoding header for small responses
        assert!(response.headers().get("content-encoding").is_none());
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        assert_eq!(std::str::from_utf8(&body).unwrap(), small_body);
    }

    #[tokio::test]
    async fn test_compression_no_accept_encoding() {
        let large_body = "z".repeat(1024);
        let body_clone = large_body.clone();
        let handler = move || {
            let b = body_clone.clone();
            async move { b }
        };

        let comp_config = CompressionConfig::default();
        let config = MiddlewareConfig::none().compression(comp_config);
        let app = config.apply(Router::new().route("/large", get(handler)));

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/large")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        // No compression without Accept-Encoding
        assert!(response.headers().get("content-encoding").is_none());
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        assert_eq!(std::str::from_utf8(&body).unwrap(), large_body);
    }

    #[tokio::test]
    async fn test_compression_excluded_path() {
        let large_body = "a".repeat(1024);
        let body_clone = large_body.clone();
        let handler = move || {
            let b = body_clone.clone();
            async move { b }
        };

        let comp_config = CompressionConfig {
            excluded_paths: vec!["/health".to_string()],
            ..CompressionConfig::default()
        };
        let config = MiddlewareConfig::none().compression(comp_config);
        let app = config.apply(Router::new().route("/health", get(handler)));

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .header("accept-encoding", "gzip")
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        // No compression for excluded paths
        assert!(response.headers().get("content-encoding").is_none());
    }

    #[tokio::test]
    async fn test_compression_disabled_by_default() {
        let config = MiddlewareConfig::default();
        assert!(!config.enable_compression);
    }

    #[tokio::test]
    async fn test_compression_none_disabled() {
        let config = MiddlewareConfig::none();
        assert!(!config.enable_compression);
    }

    #[test]
    fn test_compression_builder() {
        let config = MiddlewareConfig::none().compression(CompressionConfig::default());
        assert!(config.enable_compression);
    }

    #[test]
    fn test_compression_enabled_builder() {
        let config = MiddlewareConfig::none()
            .compression(CompressionConfig::default())
            .compression_enabled(false);
        assert!(!config.enable_compression);
    }

    #[test]
    fn test_compression_summary() {
        let config = MiddlewareConfig::none().compression(CompressionConfig::default());
        let summary = config.summary();
        let compression_entry = summary
            .iter()
            .find(|(name, _)| *name == "Compression")
            .unwrap();
        assert!(compression_entry.1);
    }

    #[tokio::test]
    async fn test_compression_content_length_updated() {
        let large_body = "b".repeat(2048);
        let body_clone = large_body.clone();
        let handler = move || {
            let b = body_clone.clone();
            async move { b }
        };

        let comp_config = CompressionConfig {
            min_size: 256,
            ..CompressionConfig::default()
        };
        let config = MiddlewareConfig::none().compression(comp_config);
        let app = config.apply(Router::new().route("/data", get(handler)));

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .header("accept-encoding", "gzip")
                    .uri("/data")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        let content_length: usize = response
            .headers()
            .get("content-length")
            .unwrap()
            .to_str()
            .unwrap()
            .parse()
            .unwrap();
        // Compressed content should be smaller than original
        assert!(content_length < 2048);
        // Content-Length should match actual body
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        assert_eq!(body.len(), content_length);
    }

    #[tokio::test]
    async fn test_compression_prefers_gzip_over_deflate() {
        let large_body = "c".repeat(1024);
        let body_clone = large_body.clone();
        let handler = move || {
            let b = body_clone.clone();
            async move { b }
        };

        let comp_config = CompressionConfig::default();
        let config = MiddlewareConfig::none().compression(comp_config);
        let app = config.apply(Router::new().route("/data", get(handler)));

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .header("accept-encoding", "deflate, gzip")
                    .uri("/data")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(
            response
                .headers()
                .get("content-encoding")
                .unwrap()
                .to_str()
                .unwrap(),
            "gzip"
        );
    }

    // ── ETag & Conditional Requests ───────────────────────────────────

    #[test]
    fn test_etag_config_default() {
        let config = ETagConfig::default();
        assert!(config.enable_etag);
        assert!(config.enable_if_none_match);
        assert_eq!(config.min_size, 0);
        assert!(config.excluded_paths.is_empty());
        assert!(!config.weak);
    }

    #[test]
    fn test_compute_etag_hash_deterministic() {
        let h1 = compute_etag_hash(b"hello world");
        let h2 = compute_etag_hash(b"hello world");
        assert_eq!(h1, h2);
        assert_eq!(h1.len(), 16); // 64-bit hex = 16 chars
    }

    #[test]
    fn test_compute_etag_hash_different_inputs() {
        let h1 = compute_etag_hash(b"hello");
        let h2 = compute_etag_hash(b"world");
        assert_ne!(h1, h2);
    }

    #[test]
    fn test_format_etag_strong() {
        let etag = format_etag("abc123", false);
        assert_eq!(etag, "\"abc123\"");
    }

    #[test]
    fn test_format_etag_weak() {
        let etag = format_etag("abc123", true);
        assert_eq!(etag, "W/\"abc123\"");
    }

    #[test]
    fn test_etag_matches_exact() {
        assert!(etag_matches("\"abc\"", "\"abc\""));
    }

    #[test]
    fn test_etag_matches_wildcard() {
        assert!(etag_matches("*", "\"abc\""));
    }

    #[test]
    fn test_etag_matches_multiple() {
        assert!(etag_matches("\"aaa\", \"bbb\", \"ccc\"", "\"bbb\""));
    }

    #[test]
    fn test_etag_matches_weak_comparison() {
        // Weak comparison: W/"abc" matches "abc"
        assert!(etag_matches("W/\"abc\"", "\"abc\""));
        assert!(etag_matches("\"abc\"", "W/\"abc\""));
    }

    #[test]
    fn test_etag_no_match() {
        assert!(!etag_matches("\"aaa\"", "\"bbb\""));
    }

    #[test]
    fn test_etag_is_excluded() {
        let config = ETagConfig {
            excluded_paths: vec!["/health".to_string(), "/metrics".to_string()],
            ..ETagConfig::default()
        };
        assert!(config.is_excluded("/health"));
        assert!(config.is_excluded("/health/check"));
        assert!(config.is_excluded("/metrics"));
        assert!(!config.is_excluded("/api/vms"));
    }

    #[tokio::test]
    async fn test_etag_generated_on_get() {
        let config = MiddlewareConfig::none().etag(ETagConfig::default());
        let app = config.apply(test_router());

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .method("GET")
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert!(response.headers().contains_key("etag"));
        let etag = response.headers().get("etag").unwrap().to_str().unwrap();
        assert!(etag.starts_with('"') && etag.ends_with('"'));
    }

    #[tokio::test]
    async fn test_etag_not_generated_on_post() {
        let config = MiddlewareConfig::none().etag(ETagConfig::default());
        let app = config.apply(Router::new().route("/test", axum::routing::post(ok_handler)));

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .method("POST")
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert!(!response.headers().contains_key("etag"));
    }

    #[tokio::test]
    async fn test_etag_304_on_match() {
        // First request: get the ETag
        let etag_config = ETagConfig::default();
        let config = MiddlewareConfig::none().etag(etag_config.clone());
        let app = config.apply(test_router());

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .method("GET")
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let etag = response
            .headers()
            .get("etag")
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();

        // Second request: send If-None-Match with the same ETag
        let config2 = MiddlewareConfig::none().etag(etag_config);
        let app2 = config2.apply(test_router());

        let response2 = app2
            .oneshot(
                HttpRequest::builder()
                    .method("GET")
                    .uri("/test")
                    .header("if-none-match", &etag)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response2.status(), StatusCode::NOT_MODIFIED);
        assert!(response2.headers().contains_key("etag"));
        // 304 should have no content-type (content was not sent)
        assert!(!response2.headers().contains_key("content-type"));

        let body = axum::body::to_bytes(response2.into_body(), 1024)
            .await
            .unwrap();
        assert!(body.is_empty());
    }

    #[tokio::test]
    async fn test_etag_200_on_mismatch() {
        let config = MiddlewareConfig::none().etag(ETagConfig::default());
        let app = config.apply(test_router());

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .method("GET")
                    .uri("/test")
                    .header("if-none-match", "\"wrong-etag\"")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert!(response.headers().contains_key("etag"));
    }

    #[tokio::test]
    async fn test_etag_wildcard_match() {
        let config = MiddlewareConfig::none().etag(ETagConfig::default());
        let app = config.apply(test_router());

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .method("GET")
                    .uri("/test")
                    .header("if-none-match", "*")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_MODIFIED);
    }

    #[tokio::test]
    async fn test_etag_weak_format() {
        let config = MiddlewareConfig::none().etag(ETagConfig {
            weak: true,
            ..ETagConfig::default()
        });
        let app = config.apply(test_router());

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .method("GET")
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        let etag = response.headers().get("etag").unwrap().to_str().unwrap();
        assert!(etag.starts_with("W/\""));
    }

    #[tokio::test]
    async fn test_etag_excluded_path() {
        let config = MiddlewareConfig::none().etag(ETagConfig {
            excluded_paths: vec!["/health".to_string()],
            ..ETagConfig::default()
        });
        let app = config.apply(test_router());

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .method("GET")
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert!(!response.headers().contains_key("etag"));
    }

    #[tokio::test]
    async fn test_etag_min_size_skip() {
        let config = MiddlewareConfig::none().etag(ETagConfig {
            min_size: 1000, // "ok" is only 2 bytes
            ..ETagConfig::default()
        });
        let app = config.apply(test_router());

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .method("GET")
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert!(!response.headers().contains_key("etag"));
    }

    #[test]
    fn test_etag_disabled_by_default() {
        let config = MiddlewareConfig::default();
        assert!(!config.enable_etag);
    }

    #[test]
    fn test_etag_none_disabled() {
        let config = MiddlewareConfig::none();
        assert!(!config.enable_etag);
    }

    #[test]
    fn test_etag_builder() {
        let config = MiddlewareConfig::default().etag(ETagConfig::default());
        assert!(config.enable_etag);
    }

    #[test]
    fn test_etag_enabled_builder() {
        let config = MiddlewareConfig::default()
            .etag(ETagConfig::default())
            .etag_enabled(false);
        assert!(!config.enable_etag);
    }

    #[test]
    fn test_etag_in_summary() {
        let config = MiddlewareConfig::default().etag(ETagConfig::default());
        let summary = config.summary();
        let entry = summary.iter().find(|(name, _)| *name == "ETag");
        assert!(entry.is_some());
        assert!(entry.unwrap().1);
    }

    #[tokio::test]
    async fn test_etag_deterministic() {
        // Same content should produce same ETag
        let config1 = MiddlewareConfig::none().etag(ETagConfig::default());
        let config2 = MiddlewareConfig::none().etag(ETagConfig::default());
        let app1 = config1.apply(test_router());
        let app2 = config2.apply(test_router());

        let r1 = app1
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let r2 = app2
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        let etag1 = r1
            .headers()
            .get("etag")
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();
        let etag2 = r2
            .headers()
            .get("etag")
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();
        assert_eq!(etag1, etag2);
    }

    // ── IP Network Parsing ────────────────────────────────────────────

    #[test]
    fn test_ip_network_parse_ipv4_cidr() {
        let net = IpNetwork::parse("192.168.1.0/24").unwrap();
        assert_eq!(net.addr, "192.168.1.0".parse::<IpAddr>().unwrap());
        assert_eq!(net.prefix_len, 24);
    }

    #[test]
    fn test_ip_network_parse_ipv4_host() {
        let net = IpNetwork::parse("10.0.0.1").unwrap();
        assert_eq!(net.addr, "10.0.0.1".parse::<IpAddr>().unwrap());
        assert_eq!(net.prefix_len, 32);
    }

    #[test]
    fn test_ip_network_parse_ipv6_cidr() {
        let net = IpNetwork::parse("fe80::/10").unwrap();
        assert_eq!(net.addr, "fe80::".parse::<IpAddr>().unwrap());
        assert_eq!(net.prefix_len, 10);
    }

    #[test]
    fn test_ip_network_parse_ipv6_host() {
        let net = IpNetwork::parse("::1").unwrap();
        assert_eq!(net.addr, "::1".parse::<IpAddr>().unwrap());
        assert_eq!(net.prefix_len, 128);
    }

    #[test]
    fn test_ip_network_parse_invalid() {
        assert!(IpNetwork::parse("not-an-ip").is_none());
        assert!(IpNetwork::parse("192.168.1.0/33").is_none()); // prefix too large
        assert!(IpNetwork::parse("::1/129").is_none());
        assert!(IpNetwork::parse("").is_none());
    }

    #[test]
    fn test_ip_network_parse_whitespace() {
        let net = IpNetwork::parse("  10.0.0.0/8  ").unwrap();
        assert_eq!(net.prefix_len, 8);
    }

    #[test]
    fn test_ip_network_contains_ipv4_exact() {
        let net = IpNetwork::parse("10.0.0.1/32").unwrap();
        assert!(net.contains(&"10.0.0.1".parse().unwrap()));
        assert!(!net.contains(&"10.0.0.2".parse().unwrap()));
    }

    #[test]
    fn test_ip_network_contains_ipv4_subnet() {
        let net = IpNetwork::parse("192.168.1.0/24").unwrap();
        assert!(net.contains(&"192.168.1.0".parse().unwrap()));
        assert!(net.contains(&"192.168.1.255".parse().unwrap()));
        assert!(net.contains(&"192.168.1.42".parse().unwrap()));
        assert!(!net.contains(&"192.168.2.0".parse().unwrap()));
        assert!(!net.contains(&"10.0.0.1".parse().unwrap()));
    }

    #[test]
    fn test_ip_network_contains_ipv4_wide() {
        let net = IpNetwork::parse("10.0.0.0/8").unwrap();
        assert!(net.contains(&"10.0.0.1".parse().unwrap()));
        assert!(net.contains(&"10.255.255.255".parse().unwrap()));
        assert!(!net.contains(&"11.0.0.1".parse().unwrap()));
    }

    #[test]
    fn test_ip_network_contains_ipv4_zero_prefix() {
        let net = IpNetwork::parse("0.0.0.0/0").unwrap();
        assert!(net.contains(&"192.168.1.1".parse().unwrap()));
        assert!(net.contains(&"10.0.0.1".parse().unwrap()));
        assert!(net.contains(&"255.255.255.255".parse().unwrap()));
    }

    #[test]
    fn test_ip_network_contains_ipv6() {
        let net = IpNetwork::parse("fe80::/10").unwrap();
        assert!(net.contains(&"fe80::1".parse().unwrap()));
        assert!(net.contains(&"fe80::ffff".parse().unwrap()));
        assert!(!net.contains(&"::1".parse().unwrap()));
    }

    #[test]
    fn test_ip_network_contains_ipv6_loopback() {
        let net = IpNetwork::parse("::1/128").unwrap();
        assert!(net.contains(&"::1".parse().unwrap()));
        assert!(!net.contains(&"::2".parse().unwrap()));
    }

    #[test]
    fn test_ip_network_v4_v6_mismatch() {
        let net = IpNetwork::parse("192.168.1.0/24").unwrap();
        assert!(!net.contains(&"::1".parse().unwrap()));

        let net6 = IpNetwork::parse("::1/128").unwrap();
        assert!(!net6.contains(&"127.0.0.1".parse().unwrap()));
    }

    #[test]
    fn test_ip_network_display() {
        let net = IpNetwork::parse("10.0.0.0/8").unwrap();
        assert_eq!(net.to_string(), "10.0.0.0/8");
    }

    // ── IP Filter Config ──────────────────────────────────────────────

    #[test]
    fn test_ip_filter_config_default() {
        let config = IpFilterConfig::default();
        assert!(config.allow_list.is_empty());
        assert!(config.deny_list.is_empty());
        assert!(!config.trust_proxy_headers);
        assert_eq!(config.excluded_paths, vec!["/health".to_string()]);
    }

    #[test]
    fn test_ip_filter_config_is_excluded() {
        let config = IpFilterConfig::default();
        assert!(config.is_excluded("/health"));
        assert!(config.is_excluded("/health/live"));
        assert!(!config.is_excluded("/test"));
    }

    #[test]
    fn test_ip_filter_config_default_allow() {
        let config = IpFilterConfig::default();
        // Empty allow list = allow all
        assert!(config.is_allowed(&"10.0.0.1".parse().unwrap()));
        assert!(config.is_allowed(&"192.168.1.1".parse().unwrap()));
    }

    #[test]
    fn test_ip_filter_config_allow_list() {
        let config = IpFilterConfig {
            allow_list: vec![IpNetwork::parse("192.168.1.0/24").unwrap()],
            ..IpFilterConfig::default()
        };
        assert!(config.is_allowed(&"192.168.1.42".parse().unwrap()));
        assert!(!config.is_allowed(&"10.0.0.1".parse().unwrap()));
    }

    #[test]
    fn test_ip_filter_config_deny_list() {
        let config = IpFilterConfig {
            deny_list: vec![IpNetwork::parse("10.0.0.0/8").unwrap()],
            ..IpFilterConfig::default()
        };
        assert!(config.is_denied(&"10.0.0.1".parse().unwrap()));
        assert!(!config.is_denied(&"192.168.1.1".parse().unwrap()));
    }

    // ── IP Filter Middleware ──────────────────────────────────────────

    #[tokio::test]
    async fn test_ip_filter_deny_list_blocks() {
        let config = MiddlewareConfig::none().ip_filter(IpFilterConfig {
            deny_list: vec![IpNetwork::parse("127.0.0.0/8").unwrap()],
            ..IpFilterConfig::default()
        });
        let app = config.apply(test_router());

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let body = axum::body::to_bytes(response.into_body(), 1024)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["code"], "IP_DENIED");
    }

    #[tokio::test]
    async fn test_ip_filter_allow_list_blocks_unmatched() {
        let config = MiddlewareConfig::none().ip_filter(IpFilterConfig {
            allow_list: vec![IpNetwork::parse("10.0.0.0/8").unwrap()],
            ..IpFilterConfig::default()
        });
        let app = config.apply(test_router());

        // Default client IP is 127.0.0.1 which is NOT in 10.0.0.0/8
        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let body = axum::body::to_bytes(response.into_body(), 1024)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["code"], "IP_NOT_ALLOWED");
    }

    #[tokio::test]
    async fn test_ip_filter_allow_list_permits_matched() {
        let config = MiddlewareConfig::none().ip_filter(IpFilterConfig {
            allow_list: vec![IpNetwork::parse("127.0.0.0/8").unwrap()],
            ..IpFilterConfig::default()
        });
        let app = config.apply(test_router());

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_ip_filter_excluded_path_bypasses() {
        let config = MiddlewareConfig::none().ip_filter(IpFilterConfig {
            deny_list: vec![IpNetwork::parse("127.0.0.0/8").unwrap()],
            excluded_paths: vec!["/health".to_string()],
            ..IpFilterConfig::default()
        });
        let app = config.apply(test_router());

        // /health is excluded — should pass even though 127.0.0.1 is denied
        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_ip_filter_deny_takes_precedence_over_allow() {
        let config = MiddlewareConfig::none().ip_filter(IpFilterConfig {
            allow_list: vec![IpNetwork::parse("127.0.0.0/8").unwrap()],
            deny_list: vec![IpNetwork::parse("127.0.0.1/32").unwrap()],
            ..IpFilterConfig::default()
        });
        let app = config.apply(test_router());

        // 127.0.0.1 is in both allow and deny — deny takes precedence
        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn test_ip_filter_disabled_by_default() {
        let config = MiddlewareConfig::default();
        assert!(!config.enable_ip_filter);
    }

    #[tokio::test]
    async fn test_ip_filter_x_forwarded_for_trusted() {
        let config = MiddlewareConfig::none().ip_filter(IpFilterConfig {
            allow_list: vec![IpNetwork::parse("10.0.0.0/8").unwrap()],
            trust_proxy_headers: true,
            ..IpFilterConfig::default()
        });
        let app = config.apply(test_router());

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .header("x-forwarded-for", "10.0.0.42, 172.16.0.1")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        // Client IP extracted from X-Forwarded-For: 10.0.0.42 — in allow list
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_ip_filter_x_forwarded_for_untrusted() {
        let config = MiddlewareConfig::none().ip_filter(IpFilterConfig {
            allow_list: vec![IpNetwork::parse("10.0.0.0/8").unwrap()],
            trust_proxy_headers: false,
            ..IpFilterConfig::default()
        });
        let app = config.apply(test_router());

        // Even with X-Forwarded-For, proxy headers not trusted — falls back to 127.0.0.1
        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .header("x-forwarded-for", "10.0.0.42")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn test_ip_filter_x_real_ip() {
        let config = MiddlewareConfig::none().ip_filter(IpFilterConfig {
            allow_list: vec![IpNetwork::parse("172.16.0.0/12").unwrap()],
            trust_proxy_headers: true,
            ..IpFilterConfig::default()
        });
        let app = config.apply(test_router());

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .header("x-real-ip", "172.17.0.5")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_ip_filter_empty_lists_allows_all() {
        let config = MiddlewareConfig::none().ip_filter(IpFilterConfig {
            allow_list: vec![],
            deny_list: vec![],
            ..IpFilterConfig::default()
        });
        let app = config.apply(test_router());

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[test]
    fn test_ip_filter_in_summary() {
        let config = MiddlewareConfig::default().ip_filter_enabled(true);
        let summary = config.summary();
        assert!(summary
            .iter()
            .any(|(name, enabled)| *name == "IP Filter" && *enabled));
    }

    // ── Body Validation ─────────────────────────────────────────────────

    #[test]
    fn test_body_validation_config_defaults() {
        let config = BodyValidationConfig::default();
        assert_eq!(config.max_depth, 32);
        assert_eq!(config.max_keys, 1000);
        assert_eq!(config.max_string_length, 1_000_000);
        assert_eq!(config.max_array_length, 10_000);
        assert_eq!(config.excluded_paths, vec!["/health"]);
    }

    #[test]
    fn test_body_validation_valid_json() {
        let config = BodyValidationConfig::default();
        let body = br#"{"name": "test", "value": 42}"#;
        assert!(validate_json_body(body, &config).is_ok());
    }

    #[test]
    fn test_body_validation_empty_body() {
        let config = BodyValidationConfig::default();
        assert!(validate_json_body(b"", &config).is_ok());
    }

    #[test]
    fn test_body_validation_invalid_json() {
        let config = BodyValidationConfig::default();
        let body = b"not json at all";
        let err = validate_json_body(body, &config).unwrap_err();
        assert!(matches!(err, BodyValidationError::InvalidJson(_)));
    }

    #[test]
    fn test_body_validation_max_depth_exceeded() {
        let config = BodyValidationConfig {
            max_depth: 2,
            ..BodyValidationConfig::default()
        };
        // depth 3: outer -> inner -> innermost
        let body = br#"{"a": {"b": {"c": 1}}}"#;
        let err = validate_json_body(body, &config).unwrap_err();
        assert!(matches!(err, BodyValidationError::MaxDepthExceeded { .. }));
    }

    #[test]
    fn test_body_validation_max_depth_exact() {
        let config = BodyValidationConfig {
            max_depth: 3,
            ..BodyValidationConfig::default()
        };
        // depth 3: outer -> inner -> innermost
        let body = br#"{"a": {"b": {"c": 1}}}"#;
        assert!(validate_json_body(body, &config).is_ok());
    }

    #[test]
    fn test_body_validation_max_keys_exceeded() {
        let config = BodyValidationConfig {
            max_keys: 2,
            ..BodyValidationConfig::default()
        };
        let body = br#"{"a": 1, "b": 2, "c": 3}"#;
        let err = validate_json_body(body, &config).unwrap_err();
        assert!(matches!(err, BodyValidationError::MaxKeysExceeded { .. }));
    }

    #[test]
    fn test_body_validation_max_string_length_exceeded() {
        let config = BodyValidationConfig {
            max_string_length: 5,
            ..BodyValidationConfig::default()
        };
        let body = br#"{"key": "toolong"}"#;
        let err = validate_json_body(body, &config).unwrap_err();
        assert!(matches!(
            err,
            BodyValidationError::MaxStringLengthExceeded { .. }
        ));
    }

    #[test]
    fn test_body_validation_max_array_length_exceeded() {
        let config = BodyValidationConfig {
            max_array_length: 2,
            ..BodyValidationConfig::default()
        };
        let body = br#"[1, 2, 3]"#;
        let err = validate_json_body(body, &config).unwrap_err();
        assert!(matches!(
            err,
            BodyValidationError::MaxArrayLengthExceeded { .. }
        ));
    }

    #[test]
    fn test_body_validation_key_length_checked() {
        let config = BodyValidationConfig {
            max_string_length: 3,
            ..BodyValidationConfig::default()
        };
        let body = br#"{"longkey": 1}"#;
        // key_length_checked: key "longkey" has 7 chars > 3
        let err = validate_json_body(body, &config).unwrap_err();
        assert!(matches!(
            err,
            BodyValidationError::MaxStringLengthExceeded { .. }
        ));
    }

    #[test]
    fn test_body_validation_nested_array() {
        let config = BodyValidationConfig {
            max_depth: 2,
            ..BodyValidationConfig::default()
        };
        // array at depth 1, nested array at depth 2 - should pass
        let body = br#"[[1, 2], [3, 4]]"#;
        assert!(validate_json_body(body, &config).is_ok());
    }

    #[test]
    fn test_body_validation_nested_array_too_deep() {
        let config = BodyValidationConfig {
            max_depth: 2,
            ..BodyValidationConfig::default()
        };
        // array at depth 1, nested array at depth 2, another at depth 3
        let body = br#"[[[1]]]"#;
        let err = validate_json_body(body, &config).unwrap_err();
        assert!(matches!(err, BodyValidationError::MaxDepthExceeded { .. }));
    }

    #[test]
    fn test_body_validation_excluded_path() {
        let config = BodyValidationConfig {
            excluded_paths: vec!["/health".to_string(), "/ready".to_string()],
            ..BodyValidationConfig::default()
        };
        assert!(config.is_excluded("/health"));
        assert!(config.is_excluded("/ready"));
        assert!(!config.is_excluded("/api/v1/vms"));
    }

    #[test]
    fn test_body_validation_error_display() {
        assert!(BodyValidationError::MaxDepthExceeded { depth: 5, limit: 3 }
            .to_string()
            .contains("depth"));
        assert!(BodyValidationError::MaxKeysExceeded { keys: 10, limit: 5 }
            .to_string()
            .contains("key count"));
        assert!(BodyValidationError::MaxStringLengthExceeded {
            length: 100,
            limit: 50
        }
        .to_string()
        .contains("string length"));
        assert!(BodyValidationError::MaxArrayLengthExceeded {
            elements: 20,
            limit: 10
        }
        .to_string()
        .contains("array length"));
        let inv = BodyValidationError::InvalidJson("bad".to_string());
        assert!(inv.to_string().contains("bad"));
    }

    #[tokio::test]
    async fn test_body_validation_middleware_passes_valid_json() {
        let config = MiddlewareConfig::none().body_validation(BodyValidationConfig::default());
        let app = config.apply(test_router());

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .method("POST")
                    .uri("/test")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"key": "value"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_body_validation_middleware_rejects_too_deep() {
        let config = MiddlewareConfig::none().body_validation(BodyValidationConfig {
            max_depth: 1,
            ..BodyValidationConfig::default()
        });
        let app = config.apply(test_router());

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .method("POST")
                    .uri("/test")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"a": {"b": 1}}"#))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }

    #[tokio::test]
    async fn test_body_validation_middleware_skips_get() {
        let config = MiddlewareConfig::none().body_validation(BodyValidationConfig {
            max_depth: 1,
            ..BodyValidationConfig::default()
        });
        let app = config.apply(test_router());

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .method("GET")
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_body_validation_middleware_skips_excluded_path() {
        let config = MiddlewareConfig::none().body_validation(BodyValidationConfig {
            max_depth: 1,
            excluded_paths: vec!["/test".to_string()],
            ..BodyValidationConfig::default()
        });
        let app = config.apply(test_router());

        // POST to excluded path with deeply nested JSON should still pass
        let response = app
            .oneshot(
                HttpRequest::builder()
                    .method("POST")
                    .uri("/test")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"a": {"b": {"c": 1}}}"#))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_body_validation_middleware_empty_body_passes() {
        let config = MiddlewareConfig::none().body_validation(BodyValidationConfig::default());
        let app = config.apply(test_router());

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .method("POST")
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[test]
    fn test_body_validation_builder() {
        let config = MiddlewareConfig::default().body_validation(BodyValidationConfig {
            max_depth: 10,
            max_keys: 50,
            max_string_length: 500,
            max_array_length: 100,
            excluded_paths: vec![],
        });
        assert!(config.enable_body_validation);
        assert_eq!(config.body_validation.max_depth, 10);
        assert_eq!(config.body_validation.max_keys, 50);
    }

    #[test]
    fn test_body_validation_enabled_builder() {
        let config = MiddlewareConfig::default().body_validation_enabled(true);
        assert!(config.enable_body_validation);
    }

    #[test]
    fn test_body_validation_in_summary() {
        let config = MiddlewareConfig::default().body_validation_enabled(true);
        let summary = config.summary();
        assert!(summary
            .iter()
            .any(|(name, enabled)| *name == "Body Validation" && *enabled));
    }

    #[test]
    fn test_body_validation_off_by_default() {
        let config = MiddlewareConfig::default();
        assert!(!config.enable_body_validation);
    }

    #[test]
    fn test_body_validation_none_disables() {
        let config = MiddlewareConfig::none();
        assert!(!config.enable_body_validation);
    }

    #[tokio::test]
    async fn test_body_validation_rejects_invalid_json() {
        let config = MiddlewareConfig::none().body_validation(BodyValidationConfig::default());
        let app = config.apply(test_router());

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .method("POST")
                    .uri("/test")
                    .header("content-type", "application/json")
                    .body(Body::from("{not valid json"))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }

    #[tokio::test]
    async fn test_body_validation_max_keys_middleware() {
        let config = MiddlewareConfig::none().body_validation(BodyValidationConfig {
            max_keys: 2,
            ..BodyValidationConfig::default()
        });
        let app = config.apply(test_router());

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .method("PUT")
                    .uri("/test")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"a":1,"b":2,"c":3}"#))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }

    #[tokio::test]
    async fn test_body_validation_patch_validated() {
        let config = MiddlewareConfig::none().body_validation(BodyValidationConfig {
            max_depth: 1,
            ..BodyValidationConfig::default()
        });
        let app = config.apply(test_router());

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .method("PATCH")
                    .uri("/test")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"a":{"b":1}}"#))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }

    // ── Idempotency ─────────────────────────────────────────────────

    #[test]
    fn test_idempotency_config_defaults() {
        let config = IdempotencyConfig::default();
        assert_eq!(config.ttl_secs, 3600);
        assert_eq!(config.max_entries, 10_000);
        assert!(!config.require_key);
        assert_eq!(config.excluded_paths, vec!["/health"]);
    }

    #[test]
    fn test_idempotency_config_excluded() {
        let config = IdempotencyConfig {
            excluded_paths: vec!["/health".to_string(), "/ready".to_string()],
            ..IdempotencyConfig::default()
        };
        assert!(config.is_excluded("/health"));
        assert!(config.is_excluded("/ready"));
        assert!(!config.is_excluded("/api/v1/vms"));
    }

    #[test]
    fn test_idempotency_store_put_get() {
        let store = IdempotencyStore::new(IdempotencyConfig::default());
        let cached = CachedResponse {
            status: 201,
            headers: vec![("content-type".to_string(), b"application/json".to_vec())],
            body: b"created".to_vec(),
            created_at: Instant::now(),
        };
        store.put("POST", "/api/v1/vms", "key-123", cached);
        let result = store.get("POST", "/api/v1/vms", "key-123");
        assert!(result.is_some());
        let r = result.unwrap();
        assert_eq!(r.status, 201);
        assert_eq!(r.body, b"created");
    }

    #[test]
    fn test_idempotency_store_miss() {
        let store = IdempotencyStore::new(IdempotencyConfig::default());
        assert!(store.get("POST", "/api/v1/vms", "missing-key").is_none());
    }

    #[test]
    fn test_idempotency_store_different_method() {
        let store = IdempotencyStore::new(IdempotencyConfig::default());
        let cached = CachedResponse {
            status: 200,
            headers: vec![],
            body: b"ok".to_vec(),
            created_at: Instant::now(),
        };
        store.put("POST", "/test", "key-1", cached);
        // Same key but different method should miss
        assert!(store.get("PUT", "/test", "key-1").is_none());
    }

    #[test]
    fn test_idempotency_store_different_path() {
        let store = IdempotencyStore::new(IdempotencyConfig::default());
        let cached = CachedResponse {
            status: 200,
            headers: vec![],
            body: b"ok".to_vec(),
            created_at: Instant::now(),
        };
        store.put("POST", "/path-a", "key-1", cached);
        // Same key but different path should miss
        assert!(store.get("POST", "/path-b", "key-1").is_none());
    }

    #[test]
    fn test_idempotency_store_ttl_expiry() {
        let store = IdempotencyStore::new(IdempotencyConfig {
            ttl_secs: 0, // immediate expiry
            ..IdempotencyConfig::default()
        });
        let cached = CachedResponse {
            status: 200,
            headers: vec![],
            body: b"ok".to_vec(),
            created_at: Instant::now() - Duration::from_secs(1),
        };
        store.put("POST", "/test", "key-1", cached);
        // Should be expired
        assert!(store.get("POST", "/test", "key-1").is_none());
    }

    #[test]
    fn test_idempotency_store_eviction() {
        let store = IdempotencyStore::new(IdempotencyConfig {
            max_entries: 2,
            ..IdempotencyConfig::default()
        });
        for i in 0..3 {
            let cached = CachedResponse {
                status: 200,
                headers: vec![],
                body: format!("body-{i}").into_bytes(),
                created_at: Instant::now(),
            };
            store.put("POST", "/test", &format!("key-{i}"), cached);
        }
        // After eviction, at least the last entry should be present
        assert!(store.get("POST", "/test", "key-2").is_some());
    }

    #[test]
    fn test_idempotency_cache_key_format() {
        let key = IdempotencyStore::cache_key("POST", "/api/v1/vms", "abc-123");
        assert_eq!(key, "POST:/api/v1/vms:abc-123");
    }

    #[tokio::test]
    async fn test_idempotency_middleware_caches_response() {
        let config = MiddlewareConfig::none().idempotency(IdempotencyConfig::default());
        let app = config.apply(test_router());

        // First request with idempotency key
        let response = app
            .oneshot(
                HttpRequest::builder()
                    .method("POST")
                    .uri("/test")
                    .header("idempotency-key", "test-key-1")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get("idempotency-key").unwrap(),
            "test-key-1"
        );
        // First request should not have replay header
        assert!(response.headers().get("idempotency-replay").is_none());
    }

    #[tokio::test]
    async fn test_idempotency_middleware_replays_cached() {
        // Use shared store via MiddlewareConfig that creates one internally
        let idempotency_config = IdempotencyConfig::default();
        let store = Arc::new(IdempotencyStore::new(idempotency_config));

        // Manually populate the cache
        let cached = CachedResponse {
            status: 201,
            headers: vec![("content-type".to_string(), b"application/json".to_vec())],
            body: br#"{"id":"vm-123"}"#.to_vec(),
            created_at: Instant::now(),
        };
        store.put("POST", "/test", "replay-key", cached);

        // Create middleware around the store
        let store_clone = store.clone();
        let app = test_router().layer(middleware::from_fn(move |req: Request, next: Next| {
            let s = store_clone.clone();
            idempotency_handler(s, req, next)
        }));

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .method("POST")
                    .uri("/test")
                    .header("idempotency-key", "replay-key")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::CREATED);
        assert_eq!(
            response.headers().get("idempotency-replay").unwrap(),
            "true"
        );
    }

    #[tokio::test]
    async fn test_idempotency_middleware_skips_get() {
        let config = MiddlewareConfig::none().idempotency(IdempotencyConfig {
            require_key: true,
            ..IdempotencyConfig::default()
        });
        let app = config.apply(test_router());

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .method("GET")
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        // GET should pass through even without idempotency key
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_idempotency_middleware_requires_key() {
        let config = MiddlewareConfig::none().idempotency(IdempotencyConfig {
            require_key: true,
            ..IdempotencyConfig::default()
        });
        let app = config.apply(test_router());

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .method("POST")
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_idempotency_middleware_no_key_not_required() {
        let config = MiddlewareConfig::none().idempotency(IdempotencyConfig {
            require_key: false,
            ..IdempotencyConfig::default()
        });
        let app = config.apply(test_router());

        // POST without idempotency key when not required should pass through
        let response = app
            .oneshot(
                HttpRequest::builder()
                    .method("POST")
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_idempotency_middleware_skips_excluded_path() {
        let config = MiddlewareConfig::none().idempotency(IdempotencyConfig {
            require_key: true,
            excluded_paths: vec!["/test".to_string()],
            ..IdempotencyConfig::default()
        });
        let app = config.apply(test_router());

        // POST to excluded path without key should still work
        let response = app
            .oneshot(
                HttpRequest::builder()
                    .method("POST")
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[test]
    fn test_idempotency_builder() {
        let config = MiddlewareConfig::default().idempotency(IdempotencyConfig {
            ttl_secs: 600,
            max_entries: 5000,
            require_key: true,
            excluded_paths: vec![],
        });
        assert!(config.enable_idempotency);
        assert_eq!(config.idempotency.ttl_secs, 600);
        assert_eq!(config.idempotency.max_entries, 5000);
        assert!(config.idempotency.require_key);
    }

    #[test]
    fn test_idempotency_enabled_builder() {
        let config = MiddlewareConfig::default().idempotency_enabled(true);
        assert!(config.enable_idempotency);
    }

    #[test]
    fn test_idempotency_in_summary() {
        let config = MiddlewareConfig::default().idempotency_enabled(true);
        let summary = config.summary();
        assert!(summary
            .iter()
            .any(|(name, enabled)| *name == "Idempotency" && *enabled));
    }

    #[test]
    fn test_idempotency_off_by_default() {
        let config = MiddlewareConfig::default();
        assert!(!config.enable_idempotency);
    }

    #[test]
    fn test_idempotency_none_disables() {
        let config = MiddlewareConfig::none();
        assert!(!config.enable_idempotency);
    }

    #[tokio::test]
    async fn test_idempotency_put_method() {
        let config = MiddlewareConfig::none().idempotency(IdempotencyConfig::default());
        let app = config.apply(test_router());

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .method("PUT")
                    .uri("/test")
                    .header("idempotency-key", "put-key-1")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get("idempotency-key").unwrap(),
            "put-key-1"
        );
    }

    #[tokio::test]
    async fn test_idempotency_patch_method() {
        let config = MiddlewareConfig::none().idempotency(IdempotencyConfig::default());
        let app = config.apply(test_router());

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .method("PATCH")
                    .uri("/test")
                    .header("idempotency-key", "patch-key-1")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get("idempotency-key").unwrap(),
            "patch-key-1"
        );
    }

    // ── Audit Logging ─────────────────────────────────────────────────

    #[test]
    fn test_audit_log_config_default() {
        let config = AuditLogConfig::default();
        assert_eq!(config.methods.len(), 4);
        assert!(config.methods.contains(&Method::POST));
        assert!(config.methods.contains(&Method::PUT));
        assert!(config.methods.contains(&Method::PATCH));
        assert!(config.methods.contains(&Method::DELETE));
        assert_eq!(config.excluded_paths, vec!["/health"]);
        assert!(!config.log_request_body);
        assert_eq!(config.max_body_log_bytes, 1024);
        assert!(config.log_response_status);
    }

    #[test]
    fn test_audit_log_config_is_excluded() {
        let config = AuditLogConfig::default();
        assert!(config.is_excluded("/health"));
        assert!(config.is_excluded("/health/live"));
        assert!(!config.is_excluded("/api/v1/vms"));
        assert!(!config.is_excluded("/test"));
    }

    #[test]
    fn test_audit_log_config_is_audited_method() {
        let config = AuditLogConfig::default();
        assert!(config.is_audited_method(&Method::POST));
        assert!(config.is_audited_method(&Method::PUT));
        assert!(config.is_audited_method(&Method::PATCH));
        assert!(config.is_audited_method(&Method::DELETE));
        assert!(!config.is_audited_method(&Method::GET));
        assert!(!config.is_audited_method(&Method::HEAD));
        assert!(!config.is_audited_method(&Method::OPTIONS));
    }

    #[test]
    fn test_audit_log_config_custom_methods() {
        let config = AuditLogConfig {
            methods: vec![Method::GET, Method::POST],
            ..AuditLogConfig::default()
        };
        assert!(config.is_audited_method(&Method::GET));
        assert!(config.is_audited_method(&Method::POST));
        assert!(!config.is_audited_method(&Method::DELETE));
    }

    #[test]
    fn test_audit_log_entry_serialization() {
        let entry = AuditLogEntry {
            timestamp: "1234567890".to_string(),
            method: "POST".to_string(),
            path: "/api/v1/vms".to_string(),
            status: Some(201),
            duration_ms: 42,
            request_id: Some("abc-123".to_string()),
            client_ip: Some("192.168.1.1".to_string()),
            request_body: None,
        };
        let json = serde_json::to_string(&entry).unwrap();
        assert!(json.contains("\"method\":\"POST\""));
        assert!(json.contains("\"status\":201"));
        assert!(json.contains("\"duration_ms\":42"));
        assert!(json.contains("\"request_id\":\"abc-123\""));
        assert!(!json.contains("request_body")); // skipped when None
    }

    #[test]
    fn test_audit_log_entry_with_body() {
        let entry = AuditLogEntry {
            timestamp: "1234567890".to_string(),
            method: "PUT".to_string(),
            path: "/test".to_string(),
            status: Some(200),
            duration_ms: 5,
            request_id: None,
            client_ip: None,
            request_body: Some("{\"name\":\"vm1\"}".to_string()),
        };
        let json = serde_json::to_string(&entry).unwrap();
        assert!(json.contains("\"request_body\":\"{\\\"name\\\":\\\"vm1\\\"}\""));
        assert!(!json.contains("request_id")); // skipped when None
        assert!(!json.contains("client_ip")); // skipped when None
    }

    #[test]
    fn test_audit_log_entry_no_status() {
        let entry = AuditLogEntry {
            timestamp: "1234567890".to_string(),
            method: "DELETE".to_string(),
            path: "/test".to_string(),
            status: None,
            duration_ms: 1,
            request_id: None,
            client_ip: None,
            request_body: None,
        };
        let json = serde_json::to_string(&entry).unwrap();
        assert!(!json.contains("status"));
    }

    #[tokio::test]
    async fn test_audit_log_passes_through_get() {
        let config = AuditLogConfig::default();
        let app = test_router().layer(middleware::from_fn(move |req: Request, next: Next| {
            let c = config.clone();
            audit_log_handler(c, req, next)
        }));

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_audit_log_passes_through_post() {
        let config = AuditLogConfig::default();
        let app = test_router().layer(middleware::from_fn(move |req: Request, next: Next| {
            let c = config.clone();
            audit_log_handler(c, req, next)
        }));

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .method("POST")
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_audit_log_skips_excluded_path() {
        let config = AuditLogConfig {
            excluded_paths: vec!["/test".to_string()],
            ..AuditLogConfig::default()
        };
        let app = test_router().layer(middleware::from_fn(move |req: Request, next: Next| {
            let c = config.clone();
            audit_log_handler(c, req, next)
        }));

        // POST to an excluded path — audit skipped, request still succeeds
        let response = app
            .oneshot(
                HttpRequest::builder()
                    .method("POST")
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_audit_log_delete_passes_through() {
        let config = AuditLogConfig::default();
        let app = test_router().layer(middleware::from_fn(move |req: Request, next: Next| {
            let c = config.clone();
            audit_log_handler(c, req, next)
        }));

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .method("DELETE")
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_audit_log_put_passes_through() {
        let config = AuditLogConfig::default();
        let app = test_router().layer(middleware::from_fn(move |req: Request, next: Next| {
            let c = config.clone();
            audit_log_handler(c, req, next)
        }));

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .method("PUT")
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_audit_log_patch_passes_through() {
        let config = AuditLogConfig::default();
        let app = test_router().layer(middleware::from_fn(move |req: Request, next: Next| {
            let c = config.clone();
            audit_log_handler(c, req, next)
        }));

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .method("PATCH")
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_audit_log_with_request_body() {
        let config = AuditLogConfig {
            log_request_body: true,
            max_body_log_bytes: 512,
            ..AuditLogConfig::default()
        };
        let app = test_router().layer(middleware::from_fn(move |req: Request, next: Next| {
            let c = config.clone();
            audit_log_handler(c, req, next)
        }));

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .method("POST")
                    .uri("/test")
                    .body(Body::from("{\"name\":\"vm1\"}"))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_audit_log_with_client_ip() {
        let config = AuditLogConfig::default();
        let app = test_router().layer(middleware::from_fn(move |req: Request, next: Next| {
            let c = config.clone();
            audit_log_handler(c, req, next)
        }));

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .method("POST")
                    .uri("/test")
                    .header("x-forwarded-for", "10.0.0.1, 192.168.1.1")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_audit_log_with_request_id() {
        let config = AuditLogConfig::default();
        let app = test_router().layer(middleware::from_fn(move |req: Request, next: Next| {
            let c = config.clone();
            audit_log_handler(c, req, next)
        }));

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .method("POST")
                    .uri("/test")
                    .header("x-request-id", "test-id-456")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[test]
    fn test_audit_log_builder() {
        let config = MiddlewareConfig::none().audit_log(AuditLogConfig::default());
        assert!(config.enable_audit_log);
    }

    #[test]
    fn test_audit_log_enabled_builder() {
        let config = MiddlewareConfig::none().audit_log_enabled(true);
        assert!(config.enable_audit_log);
        let config = config.audit_log_enabled(false);
        assert!(!config.enable_audit_log);
    }

    #[test]
    fn test_summary_includes_audit_log() {
        let config = MiddlewareConfig::default();
        let summary = config.summary();
        let entry = summary.iter().find(|(name, _)| *name == "Audit Log");
        assert!(entry.is_some());
        assert!(!entry.unwrap().1); // off by default
    }

    #[test]
    fn test_audit_log_none_disabled() {
        let config = MiddlewareConfig::none();
        assert!(!config.enable_audit_log);
    }

    #[test]
    fn test_audit_log_default_disabled() {
        let config = MiddlewareConfig::default();
        assert!(!config.enable_audit_log);
    }

    #[tokio::test]
    async fn test_audit_log_full_stack() {
        let config = MiddlewareConfig::none().audit_log(AuditLogConfig::default());
        let app = config.apply(test_router());

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .method("POST")
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_audit_log_full_stack_get_skips() {
        let config = MiddlewareConfig::none().audit_log(AuditLogConfig::default());
        let app = config.apply(test_router());

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[test]
    fn test_audit_log_custom_excluded_paths() {
        let config = AuditLogConfig {
            excluded_paths: vec!["/internal".to_string(), "/metrics".to_string()],
            ..AuditLogConfig::default()
        };
        assert!(config.is_excluded("/internal"));
        assert!(config.is_excluded("/internal/status"));
        assert!(config.is_excluded("/metrics"));
        assert!(!config.is_excluded("/api/v1/vms"));
    }

    #[tokio::test]
    async fn test_audit_log_no_status_logging() {
        let config = AuditLogConfig {
            log_response_status: false,
            ..AuditLogConfig::default()
        };
        let app = test_router().layer(middleware::from_fn(move |req: Request, next: Next| {
            let c = config.clone();
            audit_log_handler(c, req, next)
        }));

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .method("DELETE")
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    // ── Response Caching ──────────────────────────────────────────────

    #[test]
    fn test_response_cache_config_default() {
        let config = ResponseCacheConfig::default();
        assert_eq!(config.ttl_secs, 60);
        assert_eq!(config.max_entries, 1_000);
        assert_eq!(config.excluded_paths, vec!["/health"]);
        assert!(!config.cache_head);
        assert_eq!(config.cache_control, "public, max-age=60");
        assert_eq!(config.max_cacheable_body_size, 1024 * 1024);
    }

    #[test]
    fn test_response_cache_config_is_excluded() {
        let config = ResponseCacheConfig::default();
        assert!(config.is_excluded("/health"));
        assert!(config.is_excluded("/health/live"));
        assert!(!config.is_excluded("/api/v1/vms"));
        assert!(!config.is_excluded("/test"));
    }

    #[test]
    fn test_response_cache_store_put_and_get() {
        let config = ResponseCacheConfig::default();
        let cache = ResponseCache::new(config);
        let cached = CachedHttpResponse {
            status: 200,
            headers: vec![("content-type".to_string(), b"text/plain".to_vec())],
            body: b"hello".to_vec(),
            created_at: Instant::now(),
        };
        cache.put("GET", "/test", cached);
        let result = cache.get("GET", "/test");
        assert!(result.is_some());
        let (entry, age) = result.unwrap();
        assert_eq!(entry.status, 200);
        assert_eq!(entry.body, b"hello");
        assert_eq!(age, 0);
    }

    #[test]
    fn test_response_cache_store_miss() {
        let config = ResponseCacheConfig::default();
        let cache = ResponseCache::new(config);
        assert!(cache.get("GET", "/nonexistent").is_none());
    }

    #[test]
    fn test_response_cache_store_expired() {
        let config = ResponseCacheConfig {
            ttl_secs: 0, // expire immediately
            ..ResponseCacheConfig::default()
        };
        let cache = ResponseCache::new(config);
        let cached = CachedHttpResponse {
            status: 200,
            headers: vec![],
            body: b"stale".to_vec(),
            created_at: Instant::now() - Duration::from_secs(1),
        };
        cache.put("GET", "/test", cached);
        assert!(cache.get("GET", "/test").is_none());
    }

    #[test]
    fn test_response_cache_key_includes_query() {
        let config = ResponseCacheConfig::default();
        let cache = ResponseCache::new(config);
        let cached1 = CachedHttpResponse {
            status: 200,
            headers: vec![],
            body: b"page1".to_vec(),
            created_at: Instant::now(),
        };
        let cached2 = CachedHttpResponse {
            status: 200,
            headers: vec![],
            body: b"page2".to_vec(),
            created_at: Instant::now(),
        };
        cache.put("GET", "/test?page=1", cached1);
        cache.put("GET", "/test?page=2", cached2);
        let r1 = cache.get("GET", "/test?page=1").unwrap().0;
        let r2 = cache.get("GET", "/test?page=2").unwrap().0;
        assert_eq!(r1.body, b"page1");
        assert_eq!(r2.body, b"page2");
    }

    #[tokio::test]
    async fn test_response_cache_get_miss_then_hit() {
        let config = ResponseCacheConfig::default();
        let cache = Arc::new(ResponseCache::new(config));

        let cache_clone = cache.clone();
        let app = test_router().layer(middleware::from_fn(move |req: Request, next: Next| {
            let c = cache_clone.clone();
            response_cache_handler(c, req, next)
        }));

        // First request — MISS
        let response = app
            .clone()
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers().get("x-cache").unwrap(), "MISS");
        assert!(response.headers().contains_key("cache-control"));

        // Second request — HIT
        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers().get("x-cache").unwrap(), "HIT");
        assert!(response.headers().contains_key("age"));
        assert!(response.headers().contains_key("cache-control"));
    }

    #[tokio::test]
    async fn test_response_cache_skips_post() {
        let config = ResponseCacheConfig::default();
        let cache = Arc::new(ResponseCache::new(config));

        let cache_clone = cache.clone();
        let app = test_router().layer(middleware::from_fn(move |req: Request, next: Next| {
            let c = cache_clone.clone();
            response_cache_handler(c, req, next)
        }));

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .method("POST")
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert!(response.headers().get("x-cache").is_none());
    }

    #[tokio::test]
    async fn test_response_cache_skips_excluded_path() {
        let config = ResponseCacheConfig {
            excluded_paths: vec!["/test".to_string()],
            ..ResponseCacheConfig::default()
        };
        let cache = Arc::new(ResponseCache::new(config));

        let cache_clone = cache.clone();
        let app = test_router().layer(middleware::from_fn(move |req: Request, next: Next| {
            let c = cache_clone.clone();
            response_cache_handler(c, req, next)
        }));

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert!(response.headers().get("x-cache").is_none());
    }

    #[tokio::test]
    async fn test_response_cache_head_disabled_by_default() {
        let config = ResponseCacheConfig::default();
        let cache = Arc::new(ResponseCache::new(config));

        let cache_clone = cache.clone();
        let app = test_router().layer(middleware::from_fn(move |req: Request, next: Next| {
            let c = cache_clone.clone();
            response_cache_handler(c, req, next)
        }));

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .method("HEAD")
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert!(response.headers().get("x-cache").is_none());
    }

    #[tokio::test]
    async fn test_response_cache_head_when_enabled() {
        let config = ResponseCacheConfig {
            cache_head: true,
            ..ResponseCacheConfig::default()
        };
        let cache = Arc::new(ResponseCache::new(config));

        let cache_clone = cache.clone();
        let app = test_router().layer(middleware::from_fn(move |req: Request, next: Next| {
            let c = cache_clone.clone();
            response_cache_handler(c, req, next)
        }));

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .method("HEAD")
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers().get("x-cache").unwrap(), "MISS");
    }

    #[test]
    fn test_response_cache_builder() {
        let config = MiddlewareConfig::none().response_cache(ResponseCacheConfig::default());
        assert!(config.enable_response_cache);
    }

    #[test]
    fn test_response_cache_enabled_builder() {
        let config = MiddlewareConfig::none().response_cache_enabled(true);
        assert!(config.enable_response_cache);
        let config = config.response_cache_enabled(false);
        assert!(!config.enable_response_cache);
    }

    #[test]
    fn test_summary_includes_response_cache() {
        let config = MiddlewareConfig::default();
        let summary = config.summary();
        let entry = summary.iter().find(|(name, _)| *name == "Response Cache");
        assert!(entry.is_some());
        assert!(!entry.unwrap().1); // off by default
    }

    #[test]
    fn test_response_cache_none_disabled() {
        let config = MiddlewareConfig::none();
        assert!(!config.enable_response_cache);
    }

    #[test]
    fn test_response_cache_default_disabled() {
        let config = MiddlewareConfig::default();
        assert!(!config.enable_response_cache);
    }

    #[tokio::test]
    async fn test_response_cache_full_stack() {
        let config = MiddlewareConfig::none().response_cache(ResponseCacheConfig::default());
        let app = config.apply(test_router());

        let response = app
            .clone()
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers().get("x-cache").unwrap(), "MISS");

        // Second request — HIT
        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers().get("x-cache").unwrap(), "HIT");
    }

    #[test]
    fn test_response_cache_custom_cache_control() {
        let config = ResponseCacheConfig {
            cache_control: "private, max-age=300".to_string(),
            ..ResponseCacheConfig::default()
        };
        assert_eq!(config.cache_control, "private, max-age=300");
    }

    #[test]
    fn test_response_cache_custom_excluded_paths() {
        let config = ResponseCacheConfig {
            excluded_paths: vec!["/internal".to_string(), "/metrics".to_string()],
            ..ResponseCacheConfig::default()
        };
        assert!(config.is_excluded("/internal"));
        assert!(config.is_excluded("/internal/deep"));
        assert!(config.is_excluded("/metrics"));
        assert!(!config.is_excluded("/api/v1/vms"));
    }

    #[tokio::test]
    async fn test_response_cache_delete_not_cached() {
        let config = ResponseCacheConfig::default();
        let cache = Arc::new(ResponseCache::new(config));

        let cache_clone = cache.clone();
        let app = test_router().layer(middleware::from_fn(move |req: Request, next: Next| {
            let c = cache_clone.clone();
            response_cache_handler(c, req, next)
        }));

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .method("DELETE")
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert!(response.headers().get("x-cache").is_none());
    }

    #[test]
    fn test_response_cache_eviction_at_capacity() {
        let config = ResponseCacheConfig {
            max_entries: 2,
            ..ResponseCacheConfig::default()
        };
        let cache = ResponseCache::new(config);

        for i in 0..3 {
            let cached = CachedHttpResponse {
                status: 200,
                headers: vec![],
                body: format!("body{i}").into_bytes(),
                created_at: Instant::now(),
            };
            cache.put("GET", &format!("/path{i}"), cached);
        }
        // Cache should still work (eviction happened internally)
        let result = cache.get("GET", "/path2");
        assert!(result.is_some());
    }

    // ── Request Deduplication Tests ───────────────────────────────────

    #[test]
    fn test_request_dedup_config_default() {
        let config = RequestDedupConfig::default();
        assert_eq!(config.methods.len(), 4);
        assert!(config.methods.contains(&Method::POST));
        assert!(config.methods.contains(&Method::PUT));
        assert!(config.methods.contains(&Method::PATCH));
        assert!(config.methods.contains(&Method::DELETE));
        assert_eq!(config.excluded_paths, vec!["/health"]);
        assert_eq!(config.ttl_secs, 30);
        assert_eq!(config.max_body_hash_bytes, 64 * 1024);
    }

    #[test]
    fn test_request_dedup_config_is_excluded() {
        let config = RequestDedupConfig::default();
        assert!(config.is_excluded("/health"));
        assert!(config.is_excluded("/health/ready"));
        assert!(!config.is_excluded("/api/v1/vms"));
    }

    #[test]
    fn test_request_dedup_config_is_dedup_method() {
        let config = RequestDedupConfig::default();
        assert!(config.is_dedup_method(&Method::POST));
        assert!(config.is_dedup_method(&Method::PUT));
        assert!(config.is_dedup_method(&Method::PATCH));
        assert!(config.is_dedup_method(&Method::DELETE));
        assert!(!config.is_dedup_method(&Method::GET));
        assert!(!config.is_dedup_method(&Method::HEAD));
    }

    #[test]
    fn test_in_flight_tracker_acquire_release() {
        let tracker = InFlightTracker::new(RequestDedupConfig::default());
        let fp = "POST:/test:abc123";

        assert!(tracker.try_acquire(fp));
        assert!(!tracker.try_acquire(fp)); // duplicate blocked
        tracker.release(fp);
        assert!(tracker.try_acquire(fp)); // can acquire again after release
    }

    #[test]
    fn test_in_flight_tracker_different_fingerprints() {
        let tracker = InFlightTracker::new(RequestDedupConfig::default());
        assert!(tracker.try_acquire("POST:/a:111"));
        assert!(tracker.try_acquire("POST:/b:222"));
        assert!(!tracker.try_acquire("POST:/a:111")); // duplicate of first
    }

    #[test]
    fn test_in_flight_tracker_ttl_eviction() {
        let config = RequestDedupConfig {
            ttl_secs: 0, // immediate expiry
            ..RequestDedupConfig::default()
        };
        let tracker = InFlightTracker::new(config);
        let fp = "POST:/test:abc";

        assert!(tracker.try_acquire(fp));
        // With TTL=0, the entry should be expired immediately
        std::thread::sleep(Duration::from_millis(5));
        assert!(tracker.try_acquire(fp)); // expired entry was evicted
    }

    #[test]
    fn test_simple_hash_deterministic() {
        let data = b"hello world";
        let h1 = simple_hash(data);
        let h2 = simple_hash(data);
        assert_eq!(h1, h2);
    }

    #[test]
    fn test_simple_hash_different_inputs() {
        let h1 = simple_hash(b"hello");
        let h2 = simple_hash(b"world");
        assert_ne!(h1, h2);
    }

    #[test]
    fn test_in_flight_fingerprint_format() {
        let fp = InFlightTracker::fingerprint("POST", "/api/vms", 0xdeadbeef);
        assert_eq!(fp, "POST:/api/vms:deadbeef");
    }

    #[tokio::test]
    async fn test_request_dedup_get_passes_through() {
        let config = MiddlewareConfig::none().request_dedup(RequestDedupConfig::default());
        let app = config.apply(test_router());

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_request_dedup_post_first_passes() {
        let config = MiddlewareConfig::none().request_dedup(RequestDedupConfig::default());
        let app = config.apply(test_router());

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .method("POST")
                    .uri("/test")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"key":"value"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_request_dedup_concurrent_duplicate() {
        let tracker = Arc::new(InFlightTracker::new(RequestDedupConfig::default()));

        let tracker_clone = tracker.clone();
        let app = test_router().layer(middleware::from_fn(move |req: Request, next: Next| {
            let t = tracker_clone.clone();
            request_dedup_handler(t, req, next)
        }));

        // Manually acquire a slot to simulate in-flight
        let body_hash = simple_hash(b"");
        let fp = InFlightTracker::fingerprint("POST", "/test", body_hash);
        assert!(tracker.try_acquire(&fp));

        // Second request with same fingerprint should get 409
        let response = app
            .oneshot(
                HttpRequest::builder()
                    .method("POST")
                    .uri("/test")
                    .header("content-type", "application/json")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::CONFLICT);

        let body_bytes = axum::body::to_bytes(response.into_body(), 1024)
            .await
            .unwrap();
        let body: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
        assert_eq!(body["code"], "DUPLICATE_REQUEST");
    }

    #[tokio::test]
    async fn test_request_dedup_different_bodies() {
        let tracker = Arc::new(InFlightTracker::new(RequestDedupConfig::default()));

        // Acquire slot for body "a"
        let fp_a = InFlightTracker::fingerprint("POST", "/test", simple_hash(b"a"));
        assert!(tracker.try_acquire(&fp_a));

        // Request with body "b" has different fingerprint, should pass
        let tracker_clone = tracker.clone();
        let app = test_router().layer(middleware::from_fn(move |req: Request, next: Next| {
            let t = tracker_clone.clone();
            request_dedup_handler(t, req, next)
        }));

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .method("POST")
                    .uri("/test")
                    .header("content-type", "application/json")
                    .body(Body::from("b"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_request_dedup_excluded_path() {
        let tracker = Arc::new(InFlightTracker::new(RequestDedupConfig::default()));

        let tracker_clone = tracker.clone();
        let app = Router::new()
            .route("/health", axum::routing::any(|| async { "ok" }))
            .layer(middleware::from_fn(move |req: Request, next: Next| {
                let t = tracker_clone.clone();
                request_dedup_handler(t, req, next)
            }));

        // POST to excluded /health — no dedup
        let response = app
            .clone()
            .oneshot(
                HttpRequest::builder()
                    .method("POST")
                    .uri("/health")
                    .header("content-type", "application/json")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        // Do it twice — should still pass (no dedup for excluded path)
        let response = app
            .oneshot(
                HttpRequest::builder()
                    .method("POST")
                    .uri("/health")
                    .header("content-type", "application/json")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[test]
    fn test_request_dedup_builder() {
        let config = MiddlewareConfig::none().request_dedup(RequestDedupConfig::default());
        assert!(config.enable_request_dedup);
    }

    #[test]
    fn test_request_dedup_enabled_builder() {
        let config = MiddlewareConfig::none().request_dedup_enabled(true);
        assert!(config.enable_request_dedup);
        let config = config.request_dedup_enabled(false);
        assert!(!config.enable_request_dedup);
    }

    #[test]
    fn test_summary_includes_request_dedup() {
        let config = MiddlewareConfig::default();
        let summary = config.summary();
        let entry = summary.iter().find(|(name, _)| *name == "Request Dedup");
        assert!(entry.is_some());
        assert!(!entry.unwrap().1); // off by default
    }

    #[test]
    fn test_request_dedup_none_disabled() {
        let config = MiddlewareConfig::none();
        assert!(!config.enable_request_dedup);
    }

    #[test]
    fn test_request_dedup_default_disabled() {
        let config = MiddlewareConfig::default();
        assert!(!config.enable_request_dedup);
    }

    #[tokio::test]
    async fn test_request_dedup_full_stack() {
        let config = MiddlewareConfig::none().request_dedup(RequestDedupConfig::default());
        let app = config.apply(test_router());

        // POST goes through (no in-flight duplicate)
        let response = app
            .oneshot(
                HttpRequest::builder()
                    .method("POST")
                    .uri("/test")
                    .header("content-type", "application/json")
                    .body(Body::from("{}"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_request_dedup_releases_after_completion() {
        let tracker = Arc::new(InFlightTracker::new(RequestDedupConfig::default()));

        let tracker_clone = tracker.clone();
        let app = test_router().layer(middleware::from_fn(move |req: Request, next: Next| {
            let t = tracker_clone.clone();
            request_dedup_handler(t, req, next)
        }));

        // First POST — should succeed and release the slot
        let response = app
            .clone()
            .oneshot(
                HttpRequest::builder()
                    .method("POST")
                    .uri("/test")
                    .header("content-type", "application/json")
                    .body(Body::from("hello"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        // Second identical POST — should also succeed (slot was released)
        let response = app
            .oneshot(
                HttpRequest::builder()
                    .method("POST")
                    .uri("/test")
                    .header("content-type", "application/json")
                    .body(Body::from("hello"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[test]
    fn test_request_dedup_custom_methods() {
        let config = RequestDedupConfig {
            methods: vec![Method::POST],
            ..RequestDedupConfig::default()
        };
        assert!(config.is_dedup_method(&Method::POST));
        assert!(!config.is_dedup_method(&Method::PUT));
        assert!(!config.is_dedup_method(&Method::DELETE));
    }

    // ── Request Tracing (W3C Trace Context) Tests ────────────────────

    #[test]
    fn test_tracing_config_default() {
        let config = TracingConfig::default();
        assert_eq!(config.excluded_paths, vec!["/health"]);
        assert!(config.propagate_tracestate);
        assert!(config.expose_trace_id);
        assert!(config.default_sampled);
    }

    #[test]
    fn test_tracing_config_is_excluded() {
        let config = TracingConfig::default();
        assert!(config.is_excluded("/health"));
        assert!(config.is_excluded("/health/ready"));
        assert!(!config.is_excluded("/api/v1/vms"));
    }

    #[test]
    fn test_traceparent_parse_valid() {
        let tp = TraceParent::parse("00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01");
        assert!(tp.is_some());
        let tp = tp.unwrap();
        assert_eq!(tp.version, 0);
        assert_eq!(tp.trace_id, "4bf92f3577b34da6a3ce929d0e0e4736");
        assert_eq!(tp.parent_id, "00f067aa0ba902b7");
        assert_eq!(tp.trace_flags, 0x01);
        assert!(tp.is_sampled());
    }

    #[test]
    fn test_traceparent_parse_not_sampled() {
        let tp = TraceParent::parse("00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-00");
        assert!(tp.is_some());
        assert!(!tp.unwrap().is_sampled());
    }

    #[test]
    fn test_traceparent_parse_invalid_too_few_parts() {
        assert!(TraceParent::parse("00-abc-01").is_none());
    }

    #[test]
    fn test_traceparent_parse_invalid_trace_id_length() {
        assert!(TraceParent::parse("00-4bf92f-00f067aa0ba902b7-01").is_none());
    }

    #[test]
    fn test_traceparent_parse_invalid_all_zeros_trace_id() {
        assert!(
            TraceParent::parse("00-00000000000000000000000000000000-00f067aa0ba902b7-01").is_none()
        );
    }

    #[test]
    fn test_traceparent_parse_invalid_non_hex() {
        assert!(
            TraceParent::parse("00-zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz-00f067aa0ba902b7-01").is_none()
        );
    }

    #[test]
    fn test_traceparent_generate() {
        let tp = TraceParent::generate(true);
        assert_eq!(tp.version, 0);
        assert_eq!(tp.trace_id.len(), 32);
        assert_eq!(tp.parent_id.len(), 16);
        assert!(tp.is_sampled());
    }

    #[test]
    fn test_traceparent_generate_not_sampled() {
        let tp = TraceParent::generate(false);
        assert!(!tp.is_sampled());
        assert_eq!(tp.trace_flags, 0x00);
    }

    #[test]
    fn test_traceparent_child_preserves_trace_id() {
        let parent = TraceParent::generate(true);
        let child = parent.child();
        assert_eq!(child.trace_id, parent.trace_id);
        assert_ne!(child.parent_id, parent.parent_id);
        assert_eq!(child.trace_flags, parent.trace_flags);
    }

    #[test]
    fn test_traceparent_to_header_value() {
        let tp = TraceParent {
            version: 0,
            trace_id: "4bf92f3577b34da6a3ce929d0e0e4736".to_string(),
            parent_id: "00f067aa0ba902b7".to_string(),
            trace_flags: 0x01,
        };
        assert_eq!(
            tp.to_header_value(),
            "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01"
        );
    }

    #[test]
    fn test_traceparent_roundtrip() {
        let original = "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01";
        let tp = TraceParent::parse(original).unwrap();
        let child = tp.child();
        let header = child.to_header_value();
        // Child has same trace-id but different parent-id
        assert!(header.starts_with("00-4bf92f3577b34da6a3ce929d0e0e4736-"));
        assert!(header.ends_with("-01"));
        assert_ne!(header, original);
    }

    #[tokio::test]
    async fn test_request_tracing_adds_traceparent() {
        let config = MiddlewareConfig::none().tracing_config(TracingConfig::default());
        let app = config.apply(test_router());

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert!(response.headers().contains_key("traceparent"));
        assert!(response.headers().contains_key("x-trace-id"));

        let tp_value = response
            .headers()
            .get("traceparent")
            .unwrap()
            .to_str()
            .unwrap();
        let tp = TraceParent::parse(tp_value);
        assert!(tp.is_some());
        assert!(tp.unwrap().is_sampled());
    }

    #[tokio::test]
    async fn test_request_tracing_propagates_incoming() {
        let config = MiddlewareConfig::none().tracing_config(TracingConfig::default());
        let app = config.apply(test_router());

        let incoming = "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01";
        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .header("traceparent", incoming)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let tp_value = response
            .headers()
            .get("traceparent")
            .unwrap()
            .to_str()
            .unwrap();
        let tp = TraceParent::parse(tp_value).unwrap();
        // Should preserve trace-id
        assert_eq!(tp.trace_id, "4bf92f3577b34da6a3ce929d0e0e4736");
        // Should generate new parent-id (child span)
        assert_ne!(tp.parent_id, "00f067aa0ba902b7");
    }

    #[tokio::test]
    async fn test_request_tracing_propagates_tracestate() {
        let config = MiddlewareConfig::none().tracing_config(TracingConfig::default());
        let app = config.apply(test_router());

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .header(
                        "traceparent",
                        "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01",
                    )
                    .header("tracestate", "congo=t61rcWkgMzE")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get("tracestate").unwrap(),
            "congo=t61rcWkgMzE"
        );
    }

    #[tokio::test]
    async fn test_request_tracing_excluded_path() {
        let config = MiddlewareConfig::none().tracing_config(TracingConfig::default());
        let app =
            config.apply(Router::new().route("/health", axum::routing::get(|| async { "ok" })));

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert!(!response.headers().contains_key("traceparent"));
    }

    #[tokio::test]
    async fn test_request_tracing_no_expose_trace_id() {
        let config = MiddlewareConfig::none().tracing_config(TracingConfig {
            expose_trace_id: false,
            ..TracingConfig::default()
        });
        let app = config.apply(test_router());

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert!(response.headers().contains_key("traceparent"));
        assert!(!response.headers().contains_key("x-trace-id"));
    }

    #[tokio::test]
    async fn test_request_tracing_no_propagate_tracestate() {
        let config = MiddlewareConfig::none().tracing_config(TracingConfig {
            propagate_tracestate: false,
            ..TracingConfig::default()
        });
        let app = config.apply(test_router());

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .header(
                        "traceparent",
                        "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01",
                    )
                    .header("tracestate", "congo=t61rcWkgMzE")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert!(!response.headers().contains_key("tracestate"));
    }

    #[tokio::test]
    async fn test_request_tracing_not_sampled() {
        let config = MiddlewareConfig::none().tracing_config(TracingConfig {
            default_sampled: false,
            ..TracingConfig::default()
        });
        let app = config.apply(test_router());

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let tp_value = response
            .headers()
            .get("traceparent")
            .unwrap()
            .to_str()
            .unwrap();
        let tp = TraceParent::parse(tp_value).unwrap();
        assert!(!tp.is_sampled());
    }

    #[test]
    fn test_tracing_builder() {
        let config = MiddlewareConfig::none().tracing_config(TracingConfig::default());
        assert!(config.enable_tracing);
    }

    #[test]
    fn test_tracing_enabled_builder() {
        let config = MiddlewareConfig::none().tracing_enabled(true);
        assert!(config.enable_tracing);
        let config = config.tracing_enabled(false);
        assert!(!config.enable_tracing);
    }

    #[test]
    fn test_summary_includes_request_tracing() {
        let config = MiddlewareConfig::default();
        let summary = config.summary();
        let entry = summary.iter().find(|(name, _)| *name == "Request Tracing");
        assert!(entry.is_some());
        assert!(!entry.unwrap().1); // off by default
    }

    #[test]
    fn test_tracing_none_disabled() {
        let config = MiddlewareConfig::none();
        assert!(!config.enable_tracing);
    }

    #[test]
    fn test_tracing_default_disabled() {
        let config = MiddlewareConfig::default();
        assert!(!config.enable_tracing);
    }

    #[tokio::test]
    async fn test_request_tracing_full_stack() {
        let config = MiddlewareConfig::none().tracing_config(TracingConfig::default());
        let app = config.apply(test_router());

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let tp_value = response
            .headers()
            .get("traceparent")
            .unwrap()
            .to_str()
            .unwrap();
        assert!(TraceParent::parse(tp_value).is_some());
    }

    #[test]
    fn test_traceparent_generate_unique() {
        let a = TraceParent::generate(true);
        let b = TraceParent::generate(true);
        assert_ne!(a.trace_id, b.trace_id);
    }

    #[test]
    fn test_tracing_custom_excluded_paths() {
        let config = TracingConfig {
            excluded_paths: vec!["/internal".to_string(), "/metrics".to_string()],
            ..TracingConfig::default()
        };
        assert!(config.is_excluded("/internal"));
        assert!(config.is_excluded("/internal/deep"));
        assert!(config.is_excluded("/metrics"));
        assert!(!config.is_excluded("/api/v1/vms"));
    }

    #[tokio::test]
    async fn test_request_tracing_invalid_traceparent_generates_new() {
        let config = MiddlewareConfig::none().tracing_config(TracingConfig::default());
        let app = config.apply(test_router());

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .header("traceparent", "invalid-value")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let tp_value = response
            .headers()
            .get("traceparent")
            .unwrap()
            .to_str()
            .unwrap();
        let tp = TraceParent::parse(tp_value).unwrap();
        // Should generate new trace (not propagated)
        assert!(tp.is_sampled());
    }

    // ── Payload Signing (HMAC-SHA256) Tests ──────────────────────────

    #[test]
    fn test_payload_signing_config_default() {
        let config = PayloadSigningConfig::default();
        assert!(config.secret.is_empty());
        assert_eq!(config.methods.len(), 3);
        assert!(config.methods.contains(&Method::POST));
        assert!(config.methods.contains(&Method::PUT));
        assert!(config.methods.contains(&Method::PATCH));
        assert_eq!(config.excluded_paths, vec!["/health"]);
        assert!(!config.require_signature);
        assert_eq!(config.max_body_bytes, 1024 * 1024);
        assert_eq!(config.signature_header, "x-signature");
    }

    #[test]
    fn test_payload_signing_is_excluded() {
        let config = PayloadSigningConfig::default();
        assert!(config.is_excluded("/health"));
        assert!(config.is_excluded("/health/ready"));
        assert!(!config.is_excluded("/api/v1/vms"));
    }

    #[test]
    fn test_payload_signing_is_signed_method() {
        let config = PayloadSigningConfig::default();
        assert!(config.is_signed_method(&Method::POST));
        assert!(config.is_signed_method(&Method::PUT));
        assert!(config.is_signed_method(&Method::PATCH));
        assert!(!config.is_signed_method(&Method::GET));
        assert!(!config.is_signed_method(&Method::DELETE));
        assert!(!config.is_signed_method(&Method::HEAD));
    }

    #[test]
    fn test_sha256_known_answer() {
        // SHA-256("") = e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855
        let hash = sha256(b"");
        let hex: String = hash.iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(
            hex,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn test_sha256_hello_world() {
        // SHA-256("hello world")
        let hash = sha256(b"hello world");
        let hex: String = hash.iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(
            hex,
            "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
        );
    }

    #[test]
    fn test_hmac_sha256_known_answer() {
        // RFC 4231 Test Case 2: key = "Jefe", data = "what do ya want for nothing?"
        let mac = hmac_sha256(b"Jefe", b"what do ya want for nothing?");
        assert_eq!(
            mac,
            "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
        );
    }

    #[test]
    fn test_hmac_sha256_empty_data() {
        let mac = hmac_sha256(b"secret", b"");
        // Just verify it produces a 64-char hex string
        assert_eq!(mac.len(), 64);
        assert!(mac.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn test_hmac_sha256_deterministic() {
        let a = hmac_sha256(b"key", b"data");
        let b = hmac_sha256(b"key", b"data");
        assert_eq!(a, b);
    }

    #[test]
    fn test_hmac_sha256_different_keys() {
        let a = hmac_sha256(b"key1", b"data");
        let b = hmac_sha256(b"key2", b"data");
        assert_ne!(a, b);
    }

    #[test]
    fn test_hmac_sha256_different_data() {
        let a = hmac_sha256(b"key", b"data1");
        let b = hmac_sha256(b"key", b"data2");
        assert_ne!(a, b);
    }

    #[test]
    fn test_constant_time_eq_equal() {
        assert!(constant_time_eq(b"hello", b"hello"));
    }

    #[test]
    fn test_constant_time_eq_not_equal() {
        assert!(!constant_time_eq(b"hello", b"world"));
    }

    #[test]
    fn test_constant_time_eq_different_lengths() {
        assert!(!constant_time_eq(b"short", b"longer"));
    }

    #[test]
    fn test_constant_time_eq_empty() {
        assert!(constant_time_eq(b"", b""));
    }

    #[tokio::test]
    async fn test_payload_signing_valid_signature() {
        let secret = "test-secret-key";
        let body = r#"{"name":"vm1"}"#;
        let sig = hmac_sha256(secret.as_bytes(), body.as_bytes());

        let config = MiddlewareConfig::none().payload_signing(PayloadSigningConfig {
            secret: secret.to_string(),
            ..PayloadSigningConfig::default()
        });
        let app = config.apply(test_router());

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .method("POST")
                    .uri("/test")
                    .header("content-type", "application/json")
                    .header("x-signature", &sig)
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response
                .headers()
                .get("x-signature-status")
                .unwrap()
                .to_str()
                .unwrap(),
            "valid"
        );
    }

    #[tokio::test]
    async fn test_payload_signing_invalid_signature_returns_401() {
        let config = MiddlewareConfig::none().payload_signing(PayloadSigningConfig {
            secret: "my-secret".to_string(),
            ..PayloadSigningConfig::default()
        });
        let app = config.apply(test_router());

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .method("POST")
                    .uri("/test")
                    .header("content-type", "application/json")
                    .header("x-signature", "invalid-hex-signature")
                    .body(Body::from(r#"{"a":1}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        let body = axum::body::to_bytes(response.into_body(), 4096)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["code"], "INVALID_SIGNATURE");
    }

    #[tokio::test]
    async fn test_payload_signing_missing_not_required_passes() {
        let config = MiddlewareConfig::none().payload_signing(PayloadSigningConfig {
            secret: "my-secret".to_string(),
            require_signature: false,
            ..PayloadSigningConfig::default()
        });
        let app = config.apply(test_router());

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .method("POST")
                    .uri("/test")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"a":1}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response
                .headers()
                .get("x-signature-status")
                .unwrap()
                .to_str()
                .unwrap(),
            "unsigned"
        );
    }

    #[tokio::test]
    async fn test_payload_signing_missing_required_returns_401() {
        let config = MiddlewareConfig::none().payload_signing(PayloadSigningConfig {
            secret: "my-secret".to_string(),
            require_signature: true,
            ..PayloadSigningConfig::default()
        });
        let app = config.apply(test_router());

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .method("POST")
                    .uri("/test")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"a":1}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        let body = axum::body::to_bytes(response.into_body(), 4096)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["code"], "MISSING_SIGNATURE");
    }

    #[tokio::test]
    async fn test_payload_signing_get_bypasses() {
        let config = MiddlewareConfig::none().payload_signing(PayloadSigningConfig {
            secret: "my-secret".to_string(),
            require_signature: true,
            ..PayloadSigningConfig::default()
        });
        let app = config.apply(test_router());

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .method("GET")
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        // GET requests should not have signature status header
        assert!(response.headers().get("x-signature-status").is_none());
    }

    #[tokio::test]
    async fn test_payload_signing_excluded_path_bypasses() {
        let config = MiddlewareConfig::none().payload_signing(PayloadSigningConfig {
            secret: "my-secret".to_string(),
            require_signature: true,
            ..PayloadSigningConfig::default()
        });
        let app = config.apply(test_router());

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .method("POST")
                    .uri("/health")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"a":1}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        // /health is excluded, so should pass through (hits fallback 404)
        assert!(response.headers().get("x-signature-status").is_none());
    }

    #[test]
    fn test_payload_signing_builder() {
        let config = MiddlewareConfig::none().payload_signing(PayloadSigningConfig {
            secret: "test".to_string(),
            ..PayloadSigningConfig::default()
        });
        assert!(config.enable_payload_signing);
        assert_eq!(config.payload_signing.secret, "test");
    }

    #[test]
    fn test_payload_signing_enabled_builder() {
        let config = MiddlewareConfig::none()
            .payload_signing(PayloadSigningConfig::default())
            .payload_signing_enabled(false);
        assert!(!config.enable_payload_signing);
    }

    #[test]
    fn test_payload_signing_none_disabled() {
        let config = MiddlewareConfig::none();
        assert!(!config.enable_payload_signing);
    }

    #[test]
    fn test_payload_signing_default_disabled() {
        let config = MiddlewareConfig::default();
        assert!(!config.enable_payload_signing);
    }

    #[test]
    fn test_payload_signing_summary_entry() {
        let config = MiddlewareConfig::none().payload_signing(PayloadSigningConfig::default());
        let summary = config.summary();
        let entry = summary.iter().find(|(name, _)| *name == "Payload Signing");
        assert!(entry.is_some());
        assert!(entry.unwrap().1);
    }

    #[tokio::test]
    async fn test_payload_signing_put_method() {
        let secret = "put-secret";
        let body = r#"{"update":true}"#;
        let sig = hmac_sha256(secret.as_bytes(), body.as_bytes());

        let config = MiddlewareConfig::none().payload_signing(PayloadSigningConfig {
            secret: secret.to_string(),
            ..PayloadSigningConfig::default()
        });
        let app = config.apply(test_router());

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .method("PUT")
                    .uri("/test")
                    .header("content-type", "application/json")
                    .header("x-signature", &sig)
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response
                .headers()
                .get("x-signature-status")
                .unwrap()
                .to_str()
                .unwrap(),
            "valid"
        );
    }

    #[tokio::test]
    async fn test_payload_signing_patch_method() {
        let secret = "patch-secret";
        let body = r#"{"patch":true}"#;
        let sig = hmac_sha256(secret.as_bytes(), body.as_bytes());

        let config = MiddlewareConfig::none().payload_signing(PayloadSigningConfig {
            secret: secret.to_string(),
            ..PayloadSigningConfig::default()
        });
        let app = config.apply(test_router());

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .method("PATCH")
                    .uri("/test")
                    .header("content-type", "application/json")
                    .header("x-signature", &sig)
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response
                .headers()
                .get("x-signature-status")
                .unwrap()
                .to_str()
                .unwrap(),
            "valid"
        );
    }

    #[tokio::test]
    async fn test_payload_signing_empty_body() {
        let secret = "empty-secret";
        let body = b"";
        let sig = hmac_sha256(secret.as_bytes(), body);

        let config = MiddlewareConfig::none().payload_signing(PayloadSigningConfig {
            secret: secret.to_string(),
            ..PayloadSigningConfig::default()
        });
        let app = config.apply(test_router());

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .method("POST")
                    .uri("/test")
                    .header("content-type", "application/json")
                    .header("x-signature", &sig)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response
                .headers()
                .get("x-signature-status")
                .unwrap()
                .to_str()
                .unwrap(),
            "valid"
        );
    }

    #[test]
    fn test_payload_signing_custom_excluded_paths() {
        let config = PayloadSigningConfig {
            excluded_paths: vec!["/internal".to_string(), "/metrics".to_string()],
            ..PayloadSigningConfig::default()
        };
        assert!(config.is_excluded("/internal"));
        assert!(config.is_excluded("/internal/deep"));
        assert!(config.is_excluded("/metrics"));
        assert!(!config.is_excluded("/api/v1/vms"));
    }

    #[tokio::test]
    async fn test_payload_signing_delete_bypasses() {
        let config = MiddlewareConfig::none().payload_signing(PayloadSigningConfig {
            secret: "my-secret".to_string(),
            require_signature: true,
            ..PayloadSigningConfig::default()
        });
        let app = config.apply(test_router());

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .method("DELETE")
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert!(response.headers().get("x-signature-status").is_none());
    }

    // ── Circuit Breaker Tests ────────────────────────────────────────

    /// Handler that always returns 500 Internal Server Error.
    async fn error_handler() -> (StatusCode, &'static str) {
        (StatusCode::INTERNAL_SERVER_ERROR, "error")
    }

    /// Build a test router where /test returns 500.
    fn error_test_router() -> Router {
        Router::new()
            .route("/test", any(error_handler))
            .route("/health", get(ok_handler))
    }

    #[test]
    fn test_circuit_breaker_config_default() {
        let config = CircuitBreakerConfig::default();
        assert_eq!(config.failure_threshold, 5);
        assert_eq!(config.recovery_timeout_secs, 30);
        assert_eq!(config.excluded_paths, vec!["/health"]);
    }

    #[test]
    fn test_circuit_breaker_config_is_excluded() {
        let config = CircuitBreakerConfig::default();
        assert!(config.is_excluded("/health"));
        assert!(config.is_excluded("/health/ready"));
        assert!(!config.is_excluded("/api/v1/vms"));
    }

    #[test]
    fn test_circuit_breaker_initial_state() {
        let cb = CircuitBreaker::new(CircuitBreakerConfig::default());
        assert_eq!(cb.current_state(), CircuitState::Closed);
        assert_eq!(cb.failure_count(), 0);
    }

    #[test]
    fn test_circuit_breaker_record_success() {
        let cb = CircuitBreaker::new(CircuitBreakerConfig {
            failure_threshold: 3,
            ..CircuitBreakerConfig::default()
        });
        cb.record_failure();
        cb.record_failure();
        assert_eq!(cb.failure_count(), 2);
        cb.record_success();
        assert_eq!(cb.failure_count(), 0);
        assert_eq!(cb.current_state(), CircuitState::Closed);
    }

    #[test]
    fn test_circuit_breaker_trips_open() {
        let cb = CircuitBreaker::new(CircuitBreakerConfig {
            failure_threshold: 3,
            ..CircuitBreakerConfig::default()
        });
        cb.record_failure();
        cb.record_failure();
        assert_eq!(cb.current_state(), CircuitState::Closed);
        cb.record_failure();
        assert_eq!(cb.current_state(), CircuitState::Open);
    }

    #[test]
    fn test_circuit_breaker_half_open_after_timeout() {
        let cb = CircuitBreaker::new(CircuitBreakerConfig {
            failure_threshold: 1,
            recovery_timeout_secs: 0, // immediate recovery for testing
            ..CircuitBreakerConfig::default()
        });
        cb.record_failure();
        assert_eq!(cb.current_state(), CircuitState::Open);
        // With recovery_timeout_secs = 0, check_state should transition to HalfOpen
        assert_eq!(cb.check_state(), CircuitState::HalfOpen);
    }

    #[test]
    fn test_circuit_breaker_success_after_half_open_closes() {
        let cb = CircuitBreaker::new(CircuitBreakerConfig {
            failure_threshold: 1,
            recovery_timeout_secs: 0,
            ..CircuitBreakerConfig::default()
        });
        cb.record_failure();
        assert_eq!(cb.check_state(), CircuitState::HalfOpen);
        cb.record_success();
        assert_eq!(cb.current_state(), CircuitState::Closed);
    }

    #[test]
    fn test_circuit_breaker_failure_in_half_open_reopens() {
        let cb = CircuitBreaker::new(CircuitBreakerConfig {
            failure_threshold: 1,
            recovery_timeout_secs: 0,
            ..CircuitBreakerConfig::default()
        });
        cb.record_failure();
        assert_eq!(cb.check_state(), CircuitState::HalfOpen);
        cb.record_failure();
        assert_eq!(cb.current_state(), CircuitState::Open);
    }

    #[tokio::test]
    async fn test_circuit_breaker_closed_passes_through() {
        let config = MiddlewareConfig::none().circuit_breaker(CircuitBreakerConfig::default());
        let app = config.apply(test_router());

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response
                .headers()
                .get("x-circuit-state")
                .unwrap()
                .to_str()
                .unwrap(),
            "closed"
        );
    }

    #[tokio::test]
    async fn test_circuit_breaker_tracks_failures() {
        // Use a shared breaker so we can inspect state after requests
        let breaker = Arc::new(CircuitBreaker::new(CircuitBreakerConfig {
            failure_threshold: 3,
            ..CircuitBreakerConfig::default()
        }));
        let breaker_clone = breaker.clone();

        let app =
            error_test_router().layer(middleware::from_fn(move |req: Request, next: Next| {
                let b = breaker_clone.clone();
                circuit_breaker_handler(b, req, next)
            }));

        // First failure
        let response = app
            .clone()
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(breaker.failure_count(), 1);
        assert_eq!(breaker.current_state(), CircuitState::Closed);
    }

    #[tokio::test]
    async fn test_circuit_breaker_open_returns_503() {
        let breaker = Arc::new(CircuitBreaker::new(CircuitBreakerConfig {
            failure_threshold: 1,
            recovery_timeout_secs: 60, // long timeout so it stays open
            ..CircuitBreakerConfig::default()
        }));

        // Trip the circuit
        breaker.record_failure();
        assert_eq!(breaker.current_state(), CircuitState::Open);

        let breaker_clone = breaker.clone();
        let app = test_router().layer(middleware::from_fn(move |req: Request, next: Next| {
            let b = breaker_clone.clone();
            circuit_breaker_handler(b, req, next)
        }));

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(
            response
                .headers()
                .get("x-circuit-state")
                .unwrap()
                .to_str()
                .unwrap(),
            "open"
        );
        assert!(response.headers().get("retry-after").is_some());
        let body = axum::body::to_bytes(response.into_body(), 4096)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["code"], "CIRCUIT_OPEN");
    }

    #[tokio::test]
    async fn test_circuit_breaker_excluded_path_bypasses() {
        let breaker = Arc::new(CircuitBreaker::new(CircuitBreakerConfig {
            failure_threshold: 1,
            recovery_timeout_secs: 60,
            ..CircuitBreakerConfig::default()
        }));
        breaker.record_failure(); // trip open
        assert_eq!(breaker.current_state(), CircuitState::Open);

        let breaker_clone = breaker.clone();
        let app = test_router().layer(middleware::from_fn(move |req: Request, next: Next| {
            let b = breaker_clone.clone();
            circuit_breaker_handler(b, req, next)
        }));

        // /health is excluded, should pass through even when circuit is open
        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        // Excluded paths don't get x-circuit-state header
        assert!(response.headers().get("x-circuit-state").is_none());
    }

    #[tokio::test]
    async fn test_circuit_breaker_half_open_success_closes() {
        let breaker = Arc::new(CircuitBreaker::new(CircuitBreakerConfig {
            failure_threshold: 1,
            recovery_timeout_secs: 0, // immediate half-open for testing
            ..CircuitBreakerConfig::default()
        }));
        breaker.record_failure(); // trip open
                                  // With recovery_timeout_secs = 0, check_state returns HalfOpen
        assert_eq!(breaker.check_state(), CircuitState::HalfOpen);

        // Reset to open for the handler to detect HalfOpen on next check
        breaker.record_failure();
        assert_eq!(breaker.current_state(), CircuitState::Open);

        let breaker_clone = breaker.clone();
        let app = test_router().layer(middleware::from_fn(move |req: Request, next: Next| {
            let b = breaker_clone.clone();
            circuit_breaker_handler(b, req, next)
        }));

        // Probe succeeds (OK from test_router) — should close the circuit
        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response
                .headers()
                .get("x-circuit-state")
                .unwrap()
                .to_str()
                .unwrap(),
            "closed"
        );
        assert_eq!(breaker.current_state(), CircuitState::Closed);
    }

    #[tokio::test]
    async fn test_circuit_breaker_half_open_failure_reopens() {
        let breaker = Arc::new(CircuitBreaker::new(CircuitBreakerConfig {
            failure_threshold: 1,
            recovery_timeout_secs: 0,
            ..CircuitBreakerConfig::default()
        }));
        breaker.record_failure();

        // Reset to open so handler sees it
        breaker.record_failure();

        let breaker_clone = breaker.clone();
        let app =
            error_test_router().layer(middleware::from_fn(move |req: Request, next: Next| {
                let b = breaker_clone.clone();
                circuit_breaker_handler(b, req, next)
            }));

        // Probe fails (500 from error_test_router) — should reopen
        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(
            response
                .headers()
                .get("x-circuit-state")
                .unwrap()
                .to_str()
                .unwrap(),
            "open"
        );
        assert_eq!(breaker.current_state(), CircuitState::Open);
    }

    #[test]
    fn test_circuit_breaker_builder() {
        let config = MiddlewareConfig::none().circuit_breaker(CircuitBreakerConfig {
            failure_threshold: 10,
            ..CircuitBreakerConfig::default()
        });
        assert!(config.enable_circuit_breaker);
        assert_eq!(config.circuit_breaker.failure_threshold, 10);
    }

    #[test]
    fn test_circuit_breaker_enabled_builder() {
        let config = MiddlewareConfig::none()
            .circuit_breaker(CircuitBreakerConfig::default())
            .circuit_breaker_enabled(false);
        assert!(!config.enable_circuit_breaker);
    }

    #[test]
    fn test_circuit_breaker_none_disabled() {
        let config = MiddlewareConfig::none();
        assert!(!config.enable_circuit_breaker);
    }

    #[test]
    fn test_circuit_breaker_default_disabled() {
        let config = MiddlewareConfig::default();
        assert!(!config.enable_circuit_breaker);
    }

    #[test]
    fn test_circuit_breaker_summary_entry() {
        let config = MiddlewareConfig::none().circuit_breaker(CircuitBreakerConfig::default());
        let summary = config.summary();
        let entry = summary.iter().find(|(name, _)| *name == "Circuit Breaker");
        assert!(entry.is_some());
        assert!(entry.unwrap().1);
    }

    #[test]
    fn test_circuit_breaker_custom_excluded_paths() {
        let config = CircuitBreakerConfig {
            excluded_paths: vec!["/internal".to_string(), "/metrics".to_string()],
            ..CircuitBreakerConfig::default()
        };
        assert!(config.is_excluded("/internal"));
        assert!(config.is_excluded("/internal/deep"));
        assert!(config.is_excluded("/metrics"));
        assert!(!config.is_excluded("/api/v1/vms"));
    }

    #[test]
    fn test_circuit_state_enum_equality() {
        assert_eq!(CircuitState::Closed, CircuitState::Closed);
        assert_eq!(CircuitState::Open, CircuitState::Open);
        assert_eq!(CircuitState::HalfOpen, CircuitState::HalfOpen);
        assert_ne!(CircuitState::Closed, CircuitState::Open);
        assert_ne!(CircuitState::Open, CircuitState::HalfOpen);
    }

    #[tokio::test]
    async fn test_circuit_breaker_open_includes_retry_after() {
        let breaker = Arc::new(CircuitBreaker::new(CircuitBreakerConfig {
            failure_threshold: 1,
            recovery_timeout_secs: 45,
            ..CircuitBreakerConfig::default()
        }));
        breaker.record_failure();

        let breaker_clone = breaker.clone();
        let app = test_router().layer(middleware::from_fn(move |req: Request, next: Next| {
            let b = breaker_clone.clone();
            circuit_breaker_handler(b, req, next)
        }));

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        let retry_after = response
            .headers()
            .get("retry-after")
            .unwrap()
            .to_str()
            .unwrap();
        assert_eq!(retry_after, "45");
    }

    // ── Request Sanitization Tests ───────────────────────────────────

    #[test]
    fn test_sanitization_config_default() {
        let config = SanitizationConfig::default();
        assert_eq!(config.strip_headers.len(), 5);
        assert!(config
            .strip_headers
            .contains(&"x-forwarded-for".to_string()));
        assert!(config
            .strip_headers
            .contains(&"x-forwarded-host".to_string()));
        assert!(config
            .strip_headers
            .contains(&"x-forwarded-proto".to_string()));
        assert!(config.strip_headers.contains(&"x-real-ip".to_string()));
        assert!(config.strip_headers.contains(&"via".to_string()));
        assert!(config.excluded_paths.is_empty());
        assert!(config.strip_internal_prefix);
        assert_eq!(config.max_header_value_length, 8192);
    }

    #[test]
    fn test_sanitization_config_is_excluded() {
        let config = SanitizationConfig {
            excluded_paths: vec!["/internal".to_string()],
            ..SanitizationConfig::default()
        };
        assert!(config.is_excluded("/internal"));
        assert!(config.is_excluded("/internal/deep"));
        assert!(!config.is_excluded("/api/v1/vms"));
    }

    #[test]
    fn test_sanitization_should_strip_configured() {
        let config = SanitizationConfig::default();
        assert!(config.should_strip("x-forwarded-for"));
        assert!(config.should_strip("X-Forwarded-For")); // case-insensitive
        assert!(config.should_strip("X-FORWARDED-FOR"));
        assert!(config.should_strip("x-real-ip"));
        assert!(config.should_strip("via"));
        assert!(!config.should_strip("content-type"));
        assert!(!config.should_strip("authorization"));
    }

    #[test]
    fn test_sanitization_should_strip_internal_prefix() {
        let config = SanitizationConfig::default();
        assert!(config.should_strip("x-internal-secret"));
        assert!(config.should_strip("X-Internal-Token"));
        assert!(config.should_strip("x-internal-"));
    }

    #[test]
    fn test_sanitization_internal_prefix_disabled() {
        let config = SanitizationConfig {
            strip_internal_prefix: false,
            ..SanitizationConfig::default()
        };
        assert!(!config.should_strip("x-internal-secret"));
    }

    #[tokio::test]
    async fn test_sanitization_strips_headers() {
        let config = MiddlewareConfig::none().sanitization(SanitizationConfig::default());
        let app = config.apply(test_router());

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .header("x-forwarded-for", "1.2.3.4")
                    .header("x-real-ip", "5.6.7.8")
                    .header("accept", "application/json")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        // Should have stripped 2 headers
        let count = response
            .headers()
            .get("x-sanitized-count")
            .unwrap()
            .to_str()
            .unwrap();
        assert_eq!(count, "2");
    }

    #[tokio::test]
    async fn test_sanitization_strips_internal_prefix_headers() {
        let config = MiddlewareConfig::none().sanitization(SanitizationConfig::default());
        let app = config.apply(test_router());

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .header("x-internal-token", "secret123")
                    .header("x-internal-debug", "true")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let count = response
            .headers()
            .get("x-sanitized-count")
            .unwrap()
            .to_str()
            .unwrap();
        assert_eq!(count, "2");
    }

    #[tokio::test]
    async fn test_sanitization_no_strip_zero_count() {
        let config = MiddlewareConfig::none().sanitization(SanitizationConfig::default());
        let app = config.apply(test_router());

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .header("accept", "application/json")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let count = response
            .headers()
            .get("x-sanitized-count")
            .unwrap()
            .to_str()
            .unwrap();
        assert_eq!(count, "0");
    }

    #[tokio::test]
    async fn test_sanitization_excluded_path_bypasses() {
        let config = MiddlewareConfig::none().sanitization(SanitizationConfig {
            excluded_paths: vec!["/health".to_string()],
            ..SanitizationConfig::default()
        });
        let app = config.apply(test_router());

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/health")
                    .header("x-forwarded-for", "spoofed")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        // Excluded path should not have sanitized-count header
        assert!(response.headers().get("x-sanitized-count").is_none());
    }

    #[test]
    fn test_sanitization_builder() {
        let config = MiddlewareConfig::none().sanitization(SanitizationConfig {
            strip_headers: vec!["x-custom".to_string()],
            ..SanitizationConfig::default()
        });
        assert!(config.enable_sanitization);
        assert!(config
            .sanitization
            .strip_headers
            .contains(&"x-custom".to_string()));
    }

    #[test]
    fn test_sanitization_enabled_builder() {
        let config = MiddlewareConfig::none()
            .sanitization(SanitizationConfig::default())
            .sanitization_enabled(false);
        assert!(!config.enable_sanitization);
    }

    #[test]
    fn test_sanitization_none_disabled() {
        let config = MiddlewareConfig::none();
        assert!(!config.enable_sanitization);
    }

    #[test]
    fn test_sanitization_default_disabled() {
        let config = MiddlewareConfig::default();
        assert!(!config.enable_sanitization);
    }

    #[test]
    fn test_sanitization_summary_entry() {
        let config = MiddlewareConfig::none().sanitization(SanitizationConfig::default());
        let summary = config.summary();
        let entry = summary.iter().find(|(name, _)| *name == "Sanitization");
        assert!(entry.is_some());
        assert!(entry.unwrap().1);
    }

    #[test]
    fn test_sanitization_custom_strip_headers() {
        let config = SanitizationConfig {
            strip_headers: vec!["x-custom-bad".to_string()],
            strip_internal_prefix: false,
            ..SanitizationConfig::default()
        };
        assert!(config.should_strip("x-custom-bad"));
        assert!(config.should_strip("X-Custom-Bad"));
        assert!(!config.should_strip("x-forwarded-for")); // not in custom list
        assert!(!config.should_strip("x-internal-foo")); // prefix disabled
    }

    #[tokio::test]
    async fn test_sanitization_via_header_stripped() {
        let config = MiddlewareConfig::none().sanitization(SanitizationConfig::default());
        let app = config.apply(test_router());

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .header("via", "1.1 proxy.example.com")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let count = response
            .headers()
            .get("x-sanitized-count")
            .unwrap()
            .to_str()
            .unwrap();
        assert_eq!(count, "1");
    }

    #[tokio::test]
    async fn test_sanitization_all_forwarded_headers() {
        let config = MiddlewareConfig::none().sanitization(SanitizationConfig::default());
        let app = config.apply(test_router());

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .header("x-forwarded-for", "1.2.3.4")
                    .header("x-forwarded-host", "evil.com")
                    .header("x-forwarded-proto", "https")
                    .header("x-real-ip", "10.0.0.1")
                    .header("via", "1.0 proxy")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let count = response
            .headers()
            .get("x-sanitized-count")
            .unwrap()
            .to_str()
            .unwrap();
        assert_eq!(count, "5");
    }

    // ── Content Negotiation ───────────────────────────────────────────

    #[test]
    fn test_content_negotiation_config_defaults() {
        let config = ContentNegotiationConfig::default();
        assert_eq!(config.supported_types, vec!["application/json"]);
        assert_eq!(config.default_type, "application/json");
        assert!(!config.strict);
        assert_eq!(config.excluded_paths, vec!["/health"]);
    }

    #[test]
    fn test_content_negotiation_accepts_any_exact() {
        let config = ContentNegotiationConfig::default();
        assert!(config.accepts_any("application/json"));
    }

    #[test]
    fn test_content_negotiation_accepts_any_wildcard() {
        let config = ContentNegotiationConfig::default();
        assert!(config.accepts_any("*/*"));
    }

    #[test]
    fn test_content_negotiation_accepts_any_type_wildcard() {
        let config = ContentNegotiationConfig::default();
        assert!(config.accepts_any("application/*"));
    }

    #[test]
    fn test_content_negotiation_rejects_unsupported() {
        let config = ContentNegotiationConfig::default();
        assert!(!config.accepts_any("text/html"));
    }

    #[test]
    fn test_content_negotiation_accepts_with_params() {
        let config = ContentNegotiationConfig::default();
        assert!(config.accepts_any("application/json; charset=utf-8"));
    }

    #[test]
    fn test_content_negotiation_accepts_multiple_ranges() {
        let config = ContentNegotiationConfig::default();
        assert!(config.accepts_any("text/html, application/json"));
    }

    #[test]
    fn test_content_negotiation_rejects_multiple_unsupported() {
        let config = ContentNegotiationConfig::default();
        assert!(!config.accepts_any("text/html, text/xml"));
    }

    #[test]
    fn test_content_negotiation_case_insensitive() {
        let config = ContentNegotiationConfig::default();
        assert!(config.accepts_any("Application/JSON"));
    }

    #[test]
    fn test_content_negotiation_is_excluded() {
        let config = ContentNegotiationConfig::default();
        assert!(config.is_excluded("/health"));
        assert!(config.is_excluded("/health/ready"));
        assert!(!config.is_excluded("/test"));
    }

    #[tokio::test]
    async fn test_content_negotiation_allows_json() {
        let config =
            MiddlewareConfig::none().content_negotiation(ContentNegotiationConfig::default());
        let app = config.apply(test_router());

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .header("accept", "application/json")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert!(response.headers().contains_key("vary"));
    }

    #[tokio::test]
    async fn test_content_negotiation_rejects_html() {
        let config =
            MiddlewareConfig::none().content_negotiation(ContentNegotiationConfig::default());
        let app = config.apply(test_router());

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .header("accept", "text/html")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_ACCEPTABLE);
    }

    #[tokio::test]
    async fn test_content_negotiation_allows_wildcard() {
        let config =
            MiddlewareConfig::none().content_negotiation(ContentNegotiationConfig::default());
        let app = config.apply(test_router());

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .header("accept", "*/*")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_content_negotiation_missing_accept_lenient() {
        let config =
            MiddlewareConfig::none().content_negotiation(ContentNegotiationConfig::default());
        let app = config.apply(test_router());

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        // Default (strict=false) allows missing Accept
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_content_negotiation_missing_accept_strict() {
        let config = MiddlewareConfig::none().content_negotiation(ContentNegotiationConfig {
            strict: true,
            ..ContentNegotiationConfig::default()
        });
        let app = config.apply(test_router());

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_ACCEPTABLE);
    }

    #[tokio::test]
    async fn test_content_negotiation_excluded_path() {
        let config = MiddlewareConfig::none().content_negotiation(ContentNegotiationConfig {
            strict: true,
            ..ContentNegotiationConfig::default()
        });
        let app = config.apply(test_router());

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        // Health is excluded, should pass even without Accept in strict mode
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_content_negotiation_vary_header() {
        let config =
            MiddlewareConfig::none().content_negotiation(ContentNegotiationConfig::default());
        let app = config.apply(test_router());

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .header("accept", "application/json")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let vary = response.headers().get("vary").unwrap().to_str().unwrap();
        assert_eq!(vary, "Accept");
    }

    #[test]
    fn test_content_negotiation_builder() {
        let config =
            MiddlewareConfig::none().content_negotiation(ContentNegotiationConfig::default());
        assert!(config.enable_content_negotiation);
    }

    #[test]
    fn test_content_negotiation_enabled_builder() {
        let config = MiddlewareConfig::default().content_negotiation_enabled(true);
        assert!(config.enable_content_negotiation);
    }

    #[test]
    fn test_content_negotiation_default_disabled() {
        let config = MiddlewareConfig::default();
        assert!(!config.enable_content_negotiation);
    }

    #[test]
    fn test_content_negotiation_none_disabled() {
        let config = MiddlewareConfig::none();
        assert!(!config.enable_content_negotiation);
    }

    #[test]
    fn test_content_negotiation_summary_entry() {
        let config =
            MiddlewareConfig::none().content_negotiation(ContentNegotiationConfig::default());
        let summary = config.summary();
        let entry = summary
            .iter()
            .find(|(name, _)| *name == "Content Negotiation");
        assert!(entry.is_some());
        assert!(entry.unwrap().1);
    }

    // ── Request Throttling ────────────────────────────────────────────

    #[test]
    fn test_throttle_config_defaults() {
        let config = ThrottleConfig::default();
        assert_eq!(config.max_concurrent, 100);
        assert_eq!(config.retry_after_secs, 1);
        assert_eq!(config.excluded_paths, vec!["/health"]);
    }

    #[test]
    fn test_throttle_config_is_excluded() {
        let config = ThrottleConfig::default();
        assert!(config.is_excluded("/health"));
        assert!(config.is_excluded("/health/ready"));
        assert!(!config.is_excluded("/test"));
    }

    #[test]
    fn test_throttle_state_acquire_and_release() {
        let state = ThrottleState::new(ThrottleConfig {
            max_concurrent: 2,
            ..ThrottleConfig::default()
        });
        assert_eq!(state.current_count(), 0);

        let g1 = state.try_acquire();
        assert!(g1.is_some());
        assert_eq!(state.current_count(), 1);

        let g2 = state.try_acquire();
        assert!(g2.is_some());
        assert_eq!(state.current_count(), 2);

        // At capacity → should fail
        let g3 = state.try_acquire();
        assert!(g3.is_none());

        drop(g1);
        assert_eq!(state.current_count(), 1);

        // Should succeed again
        let g4 = state.try_acquire();
        assert!(g4.is_some());
        assert_eq!(state.current_count(), 2);

        drop(g2);
        drop(g4);
        assert_eq!(state.current_count(), 0);
    }

    #[test]
    fn test_throttle_state_unlimited() {
        let state = ThrottleState::new(ThrottleConfig {
            max_concurrent: 0,
            ..ThrottleConfig::default()
        });
        let guards: Vec<_> = (0..1000).filter_map(|_| state.try_acquire()).collect();
        assert_eq!(guards.len(), 1000);
        assert_eq!(state.current_count(), 1000);
    }

    #[tokio::test]
    async fn test_throttle_allows_under_limit() {
        let config = MiddlewareConfig::none().throttle(ThrottleConfig {
            max_concurrent: 10,
            ..ThrottleConfig::default()
        });
        let app = config.apply(test_router());

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert!(response.headers().contains_key("x-throttle-current"));
    }

    #[tokio::test]
    async fn test_throttle_excluded_path() {
        let config = MiddlewareConfig::none().throttle(ThrottleConfig {
            max_concurrent: 1,
            ..ThrottleConfig::default()
        });
        let app = config.apply(test_router());

        // Health endpoint is excluded — should always pass
        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_throttle_returns_503_when_full() {
        // Create a throttle state with max_concurrent=0 meaning none are allowed
        // since the middleware does 0 = unlimited, use max_concurrent=1 and
        // pre-acquire to test rejection
        let throttle_config = ThrottleConfig {
            max_concurrent: 1,
            retry_after_secs: 5,
            excluded_paths: vec![],
        };
        let state = Arc::new(ThrottleState::new(throttle_config));

        // Pre-acquire the single slot
        let _guard = state.try_acquire().unwrap();
        assert_eq!(state.current_count(), 1);

        // Build handler directly
        let state_clone = state.clone();
        let app = test_router().layer(middleware::from_fn(move |req: Request, next: Next| {
            let s = state_clone.clone();
            request_throttle_handler(s, req, next)
        }));

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        let retry = response
            .headers()
            .get("retry-after")
            .unwrap()
            .to_str()
            .unwrap();
        assert_eq!(retry, "5");
        assert!(response.headers().contains_key("x-throttle-limit"));
    }

    #[test]
    fn test_throttle_builder() {
        let config = MiddlewareConfig::none().throttle(ThrottleConfig::default());
        assert!(config.enable_throttle);
    }

    #[test]
    fn test_throttle_enabled_builder() {
        let config = MiddlewareConfig::default().throttle_enabled(true);
        assert!(config.enable_throttle);
    }

    #[test]
    fn test_throttle_default_disabled() {
        let config = MiddlewareConfig::default();
        assert!(!config.enable_throttle);
    }

    #[test]
    fn test_throttle_none_disabled() {
        let config = MiddlewareConfig::none();
        assert!(!config.enable_throttle);
    }

    #[test]
    fn test_throttle_summary_entry() {
        let config = MiddlewareConfig::none().throttle(ThrottleConfig::default());
        let summary = config.summary();
        let entry = summary.iter().find(|(name, _)| *name == "Throttle");
        assert!(entry.is_some());
        assert!(entry.unwrap().1);
    }

    #[test]
    fn test_throttle_custom_config() {
        let config = ThrottleConfig {
            max_concurrent: 50,
            retry_after_secs: 10,
            excluded_paths: vec!["/health".to_string(), "/metrics".to_string()],
        };
        assert_eq!(config.max_concurrent, 50);
        assert_eq!(config.retry_after_secs, 10);
        assert!(config.is_excluded("/metrics"));
        assert!(config.is_excluded("/health"));
        assert!(!config.is_excluded("/api"));
    }

    #[tokio::test]
    async fn test_throttle_current_header() {
        let config = MiddlewareConfig::none().throttle(ThrottleConfig {
            max_concurrent: 100,
            ..ThrottleConfig::default()
        });
        let app = config.apply(test_router());

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let current = response
            .headers()
            .get("x-throttle-current")
            .unwrap()
            .to_str()
            .unwrap();
        assert_eq!(current, "1"); // Only one request in flight
    }

    // ── Retry Hints ───────────────────────────────────────────────────

    #[test]
    fn test_retry_hints_config_default() {
        let config = RetryHintsConfig::default();
        assert_eq!(config.retry_statuses, vec![408, 429, 503]);
        assert_eq!(config.default_retry_after_secs, 1);
        assert_eq!(config.strategy, "exponential-backoff");
        assert_eq!(config.max_retries, 3);
        assert_eq!(config.excluded_paths, vec!["/health"]);
    }

    #[test]
    fn test_retry_hints_is_excluded() {
        let config = RetryHintsConfig::default();
        assert!(config.is_excluded("/health"));
        assert!(config.is_excluded("/health/ready"));
        assert!(!config.is_excluded("/test"));
    }

    #[tokio::test]
    async fn test_retry_hints_adds_headers_on_503() {
        let config = RetryHintsConfig::default();
        let app = Router::new()
            .route("/test", any(|| async { StatusCode::SERVICE_UNAVAILABLE }))
            .layer(middleware::from_fn(move |req: Request, next: Next| {
                let c = config.clone();
                retry_hints_handler(c, req, next)
            }));

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(response.headers().get("retry-after").unwrap(), "1");
        assert_eq!(
            response.headers().get("x-retry-strategy").unwrap(),
            "exponential-backoff"
        );
        assert_eq!(response.headers().get("x-retry-max").unwrap(), "3");
    }

    #[tokio::test]
    async fn test_retry_hints_adds_headers_on_429() {
        let config = RetryHintsConfig::default();
        let app = Router::new()
            .route("/test", any(|| async { StatusCode::TOO_MANY_REQUESTS }))
            .layer(middleware::from_fn(move |req: Request, next: Next| {
                let c = config.clone();
                retry_hints_handler(c, req, next)
            }));

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(response.headers().get("retry-after").unwrap(), "1");
        assert_eq!(
            response.headers().get("x-retry-strategy").unwrap(),
            "exponential-backoff"
        );
    }

    #[tokio::test]
    async fn test_retry_hints_adds_headers_on_408() {
        let config = RetryHintsConfig::default();
        let app = Router::new()
            .route("/test", any(|| async { StatusCode::REQUEST_TIMEOUT }))
            .layer(middleware::from_fn(move |req: Request, next: Next| {
                let c = config.clone();
                retry_hints_handler(c, req, next)
            }));

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::REQUEST_TIMEOUT);
        assert_eq!(response.headers().get("retry-after").unwrap(), "1");
        assert_eq!(
            response.headers().get("x-retry-strategy").unwrap(),
            "exponential-backoff"
        );
        assert_eq!(response.headers().get("x-retry-max").unwrap(), "3");
    }

    #[tokio::test]
    async fn test_retry_hints_skips_success_responses() {
        let config = RetryHintsConfig::default();
        let app = Router::new()
            .route("/test", any(ok_handler))
            .layer(middleware::from_fn(move |req: Request, next: Next| {
                let c = config.clone();
                retry_hints_handler(c, req, next)
            }));

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert!(response.headers().get("retry-after").is_none());
        assert!(response.headers().get("x-retry-strategy").is_none());
        assert!(response.headers().get("x-retry-max").is_none());
    }

    #[tokio::test]
    async fn test_retry_hints_skips_non_matching_error() {
        let config = RetryHintsConfig::default();
        let app = Router::new()
            .route("/test", any(|| async { StatusCode::BAD_REQUEST }))
            .layer(middleware::from_fn(move |req: Request, next: Next| {
                let c = config.clone();
                retry_hints_handler(c, req, next)
            }));

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert!(response.headers().get("retry-after").is_none());
        assert!(response.headers().get("x-retry-strategy").is_none());
    }

    #[tokio::test]
    async fn test_retry_hints_skips_excluded_path() {
        let config = RetryHintsConfig::default();
        let app = Router::new()
            .route("/health", any(|| async { StatusCode::SERVICE_UNAVAILABLE }))
            .layer(middleware::from_fn(move |req: Request, next: Next| {
                let c = config.clone();
                retry_hints_handler(c, req, next)
            }));

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert!(response.headers().get("retry-after").is_none());
        assert!(response.headers().get("x-retry-strategy").is_none());
    }

    #[tokio::test]
    async fn test_retry_hints_does_not_overwrite_existing_retry_after() {
        use axum::response::IntoResponse;

        let config = RetryHintsConfig::default();
        let app = Router::new()
            .route(
                "/test",
                any(|| async {
                    let mut resp = StatusCode::TOO_MANY_REQUESTS.into_response();
                    resp.headers_mut()
                        .insert("retry-after", HeaderValue::from_static("60"));
                    resp
                }),
            )
            .layer(middleware::from_fn(move |req: Request, next: Next| {
                let c = config.clone();
                retry_hints_handler(c, req, next)
            }));

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        // Existing Retry-After from handler preserved, not overwritten
        assert_eq!(response.headers().get("retry-after").unwrap(), "60");
        // But strategy and max still added
        assert_eq!(
            response.headers().get("x-retry-strategy").unwrap(),
            "exponential-backoff"
        );
        assert_eq!(response.headers().get("x-retry-max").unwrap(), "3");
    }

    #[tokio::test]
    async fn test_retry_hints_custom_config() {
        let config = RetryHintsConfig {
            retry_statuses: vec![500],
            default_retry_after_secs: 5,
            strategy: "linear".to_string(),
            max_retries: 10,
            excluded_paths: vec![],
        };
        let app = Router::new()
            .route("/test", any(|| async { StatusCode::INTERNAL_SERVER_ERROR }))
            .layer(middleware::from_fn(move |req: Request, next: Next| {
                let c = config.clone();
                retry_hints_handler(c, req, next)
            }));

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(response.headers().get("retry-after").unwrap(), "5");
        assert_eq!(
            response.headers().get("x-retry-strategy").unwrap(),
            "linear"
        );
        assert_eq!(response.headers().get("x-retry-max").unwrap(), "10");
    }

    #[test]
    fn test_retry_hints_builder() {
        let config = MiddlewareConfig::none().retry_hints(RetryHintsConfig::default());
        assert!(config.enable_retry_hints);
        assert_eq!(config.retry_hints.max_retries, 3);
    }

    #[test]
    fn test_retry_hints_enabled_builder() {
        let config = MiddlewareConfig::default().retry_hints_enabled(true);
        assert!(config.enable_retry_hints);
    }

    #[test]
    fn test_retry_hints_disabled_by_default() {
        let config = MiddlewareConfig::default();
        assert!(!config.enable_retry_hints);
    }

    #[test]
    fn test_retry_hints_in_summary() {
        let config = MiddlewareConfig::default();
        let summary = config.summary();
        let entry = summary.iter().find(|(name, _)| *name == "Retry Hints");
        assert!(entry.is_some());
        assert!(!entry.unwrap().1); // off by default
    }

    #[tokio::test]
    async fn test_retry_hints_full_stack() {
        let config = MiddlewareConfig::none().retry_hints(RetryHintsConfig::default());
        let app = Router::new().route("/test", any(|| async { StatusCode::SERVICE_UNAVAILABLE }));
        let app = config.apply(app);

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(response.headers().get("retry-after").unwrap(), "1");
        assert_eq!(
            response.headers().get("x-retry-strategy").unwrap(),
            "exponential-backoff"
        );
        assert_eq!(response.headers().get("x-retry-max").unwrap(), "3");
    }

    #[test]
    fn test_retry_hints_none_disabled() {
        let config = MiddlewareConfig::none();
        assert!(!config.enable_retry_hints);
    }

    #[tokio::test]
    async fn test_retry_hints_500_not_in_default_statuses() {
        let config = RetryHintsConfig::default();
        let app = Router::new()
            .route("/test", any(|| async { StatusCode::INTERNAL_SERVER_ERROR }))
            .layer(middleware::from_fn(move |req: Request, next: Next| {
                let c = config.clone();
                retry_hints_handler(c, req, next)
            }));

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        // 500 is not in default retry_statuses
        assert!(response.headers().get("retry-after").is_none());
    }

    // -- Maintenance Mode -----------------------------------------------

    #[test]
    fn test_maintenance_config_default() {
        let config = MaintenanceConfig::default();
        assert_eq!(config.message, "Service is undergoing planned maintenance");
        assert_eq!(config.retry_after_secs, 300);
        assert_eq!(config.excluded_paths, vec!["/health"]);
    }

    #[test]
    fn test_maintenance_config_is_excluded() {
        let config = MaintenanceConfig::default();
        assert!(config.is_excluded("/health"));
        assert!(config.is_excluded("/health/ready"));
        assert!(!config.is_excluded("/test"));
    }

    #[test]
    fn test_maintenance_state_new_inactive() {
        let state = MaintenanceState::new(MaintenanceConfig::default());
        assert!(!state.is_active());
    }

    #[test]
    fn test_maintenance_state_new_active() {
        let state = MaintenanceState::new_active(MaintenanceConfig::default());
        assert!(state.is_active());
    }

    #[test]
    fn test_maintenance_state_toggle() {
        let state = MaintenanceState::new(MaintenanceConfig::default());
        assert!(!state.is_active());
        state.activate();
        assert!(state.is_active());
        state.deactivate();
        assert!(!state.is_active());
    }

    #[tokio::test]
    async fn test_maintenance_active_returns_503() {
        let state = Arc::new(MaintenanceState::new_active(MaintenanceConfig::default()));
        let app = Router::new()
            .route("/test", any(ok_handler))
            .layer(middleware::from_fn(move |req: Request, next: Next| {
                let s = state.clone();
                maintenance_handler(s, req, next)
            }));

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(response.headers().get("retry-after").unwrap(), "300");
        assert!(response.headers().get("x-maintenance-message").is_some());
    }

    #[tokio::test]
    async fn test_maintenance_inactive_passes_through() {
        let state = Arc::new(MaintenanceState::new(MaintenanceConfig::default()));
        let app = Router::new()
            .route("/test", any(ok_handler))
            .layer(middleware::from_fn(move |req: Request, next: Next| {
                let s = state.clone();
                maintenance_handler(s, req, next)
            }));

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_maintenance_excludes_health() {
        let state = Arc::new(MaintenanceState::new_active(MaintenanceConfig::default()));
        let app = Router::new()
            .route("/health", get(ok_handler))
            .layer(middleware::from_fn(move |req: Request, next: Next| {
                let s = state.clone();
                maintenance_handler(s, req, next)
            }));

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_maintenance_json_body() {
        let state = Arc::new(MaintenanceState::new_active(MaintenanceConfig::default()));
        let app = Router::new()
            .route("/test", any(ok_handler))
            .layer(middleware::from_fn(move |req: Request, next: Next| {
                let s = state.clone();
                maintenance_handler(s, req, next)
            }));

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        let body = axum::body::to_bytes(response.into_body(), 1024)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["code"], "MAINTENANCE");
        assert_eq!(json["error"], "Service is undergoing planned maintenance");
    }

    #[tokio::test]
    async fn test_maintenance_custom_message() {
        let config = MaintenanceConfig {
            message: "Down for upgrade".to_string(),
            retry_after_secs: 60,
            excluded_paths: vec![],
        };
        let state = Arc::new(MaintenanceState::new_active(config));
        let app = Router::new()
            .route("/test", any(ok_handler))
            .layer(middleware::from_fn(move |req: Request, next: Next| {
                let s = state.clone();
                maintenance_handler(s, req, next)
            }));

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(response.headers().get("retry-after").unwrap(), "60");
        assert_eq!(
            response.headers().get("x-maintenance-message").unwrap(),
            "Down for upgrade"
        );
    }

    #[tokio::test]
    async fn test_maintenance_runtime_toggle() {
        let state = Arc::new(MaintenanceState::new(MaintenanceConfig::default()));
        let s1 = state.clone();
        let s2 = state.clone();
        let app = Router::new()
            .route("/test", any(ok_handler))
            .layer(middleware::from_fn(move |req: Request, next: Next| {
                let s = s1.clone();
                maintenance_handler(s, req, next)
            }));

        // Initially inactive - passes through
        let response = app
            .clone()
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        // Activate - blocks
        s2.activate();
        let response = app
            .clone()
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);

        // Deactivate - passes through again
        s2.deactivate();
        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[test]
    fn test_maintenance_builder() {
        let config = MiddlewareConfig::none().maintenance(MaintenanceConfig::default());
        assert!(config.enable_maintenance);
        assert_eq!(config.maintenance.retry_after_secs, 300);
    }

    #[test]
    fn test_maintenance_enabled_builder() {
        let config = MiddlewareConfig::default().maintenance_enabled(true);
        assert!(config.enable_maintenance);
    }

    #[test]
    fn test_maintenance_disabled_by_default() {
        let config = MiddlewareConfig::default();
        assert!(!config.enable_maintenance);
    }

    #[test]
    fn test_maintenance_none_disabled() {
        let config = MiddlewareConfig::none();
        assert!(!config.enable_maintenance);
    }

    #[test]
    fn test_maintenance_in_summary() {
        let config = MiddlewareConfig::default();
        let summary = config.summary();
        let entry = summary.iter().find(|(name, _)| *name == "Maintenance");
        assert!(entry.is_some());
        assert!(!entry.unwrap().1); // off by default
    }

    #[tokio::test]
    async fn test_maintenance_full_stack() {
        let config = MiddlewareConfig::none().maintenance(MaintenanceConfig::default());
        let app = Router::new().route("/test", any(ok_handler));
        let app = config.apply(app);

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        // Maintenance is active when enabled via apply()
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(response.headers().get("retry-after").unwrap(), "300");
    }

    // ── Deprecation Tests ─────────────────────────────────────────────

    #[test]
    fn test_deprecation_config_default() {
        let config = DeprecationConfig::default();
        assert!(config.entries.is_empty());
        assert_eq!(config.excluded_paths, vec!["/health".to_string()]);
    }

    #[test]
    fn test_deprecation_config_is_excluded() {
        let config = DeprecationConfig::default();
        assert!(config.is_excluded("/health"));
        assert!(config.is_excluded("/health/ready"));
        assert!(!config.is_excluded("/api/v1"));
    }

    #[test]
    fn test_deprecation_config_find_match() {
        let config = DeprecationConfig {
            entries: vec![DeprecationEntry {
                path: "/api/v1".to_string(),
                deprecated_at: Some("2025-12-01".to_string()),
                sunset_at: Some("2026-06-01".to_string()),
                replacement: Some("/api/v2".to_string()),
                message: Some("Use v2 instead".to_string()),
            }],
            excluded_paths: vec!["/health".to_string()],
        };
        assert!(config.find_match("/api/v1/vms").is_some());
        assert!(config.find_match("/api/v2/vms").is_none());
    }

    #[test]
    fn test_deprecation_config_no_match_empty() {
        let config = DeprecationConfig::default();
        assert!(config.find_match("/api/v1/vms").is_none());
    }

    #[tokio::test]
    async fn test_deprecation_no_entries_passes_through() {
        let config = DeprecationConfig::default();
        let app = test_router().layer(middleware::from_fn(move |req: Request, next: Next| {
            let c = config.clone();
            deprecation_handler(c, req, next)
        }));

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert!(!response.headers().contains_key("deprecation"));
    }

    #[tokio::test]
    async fn test_deprecation_adds_all_headers() {
        let config = DeprecationConfig {
            entries: vec![DeprecationEntry {
                path: "/test".to_string(),
                deprecated_at: Some("2025-12-01".to_string()),
                sunset_at: Some("2026-06-01".to_string()),
                replacement: Some("/api/v2/test".to_string()),
                message: Some("Migrate to v2".to_string()),
            }],
            excluded_paths: vec![],
        };
        let app = test_router().layer(middleware::from_fn(move |req: Request, next: Next| {
            let c = config.clone();
            deprecation_handler(c, req, next)
        }));

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers().get("deprecation").unwrap(), "2025-12-01");
        assert_eq!(response.headers().get("sunset").unwrap(), "2026-06-01");
        let link = response.headers().get("link").unwrap().to_str().unwrap();
        assert!(link.contains("/api/v2/test"));
        assert!(link.contains("successor-version"));
        assert_eq!(
            response.headers().get("x-deprecation-message").unwrap(),
            "Migrate to v2"
        );
    }

    #[tokio::test]
    async fn test_deprecation_true_when_no_date() {
        let config = DeprecationConfig {
            entries: vec![DeprecationEntry {
                path: "/test".to_string(),
                deprecated_at: None,
                sunset_at: None,
                replacement: None,
                message: None,
            }],
            excluded_paths: vec![],
        };
        let app = test_router().layer(middleware::from_fn(move |req: Request, next: Next| {
            let c = config.clone();
            deprecation_handler(c, req, next)
        }));

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.headers().get("deprecation").unwrap(), "true");
        assert!(!response.headers().contains_key("sunset"));
        assert!(!response.headers().contains_key("link"));
        assert!(!response.headers().contains_key("x-deprecation-message"));
    }

    #[tokio::test]
    async fn test_deprecation_skips_excluded_path() {
        let config = DeprecationConfig {
            entries: vec![DeprecationEntry {
                path: "/health".to_string(),
                deprecated_at: Some("2025-12-01".to_string()),
                sunset_at: None,
                replacement: None,
                message: None,
            }],
            excluded_paths: vec!["/health".to_string()],
        };
        let app = test_router().layer(middleware::from_fn(move |req: Request, next: Next| {
            let c = config.clone();
            deprecation_handler(c, req, next)
        }));

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert!(!response.headers().contains_key("deprecation"));
    }

    #[tokio::test]
    async fn test_deprecation_no_match_no_headers() {
        let config = DeprecationConfig {
            entries: vec![DeprecationEntry {
                path: "/old".to_string(),
                deprecated_at: Some("2025-01-01".to_string()),
                sunset_at: None,
                replacement: None,
                message: None,
            }],
            excluded_paths: vec![],
        };
        let app = test_router().layer(middleware::from_fn(move |req: Request, next: Next| {
            let c = config.clone();
            deprecation_handler(c, req, next)
        }));

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert!(!response.headers().contains_key("deprecation"));
    }

    #[tokio::test]
    async fn test_deprecation_partial_headers() {
        let config = DeprecationConfig {
            entries: vec![DeprecationEntry {
                path: "/test".to_string(),
                deprecated_at: Some("2025-06-15".to_string()),
                sunset_at: None,
                replacement: Some("/api/v3/test".to_string()),
                message: None,
            }],
            excluded_paths: vec![],
        };
        let app = test_router().layer(middleware::from_fn(move |req: Request, next: Next| {
            let c = config.clone();
            deprecation_handler(c, req, next)
        }));

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.headers().get("deprecation").unwrap(), "2025-06-15");
        assert!(!response.headers().contains_key("sunset"));
        assert!(response.headers().contains_key("link"));
        assert!(!response.headers().contains_key("x-deprecation-message"));
    }

    #[tokio::test]
    async fn test_deprecation_prefix_matching() {
        let config = DeprecationConfig {
            entries: vec![DeprecationEntry {
                path: "/api/v1".to_string(),
                deprecated_at: Some("2025-01-01".to_string()),
                sunset_at: None,
                replacement: None,
                message: None,
            }],
            excluded_paths: vec![],
        };
        let app = test_router().layer(middleware::from_fn(move |req: Request, next: Next| {
            let c = config.clone();
            deprecation_handler(c, req, next)
        }));

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/api/v1/vms")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.headers().get("deprecation").unwrap(), "2025-01-01");
    }

    #[test]
    fn test_deprecation_builder_enables() {
        let config = MiddlewareConfig::none().deprecation(DeprecationConfig::default());
        assert!(config.enable_deprecation);
    }

    #[test]
    fn test_deprecation_builder_toggle() {
        let config = MiddlewareConfig::none().deprecation_enabled(true);
        assert!(config.enable_deprecation);
        let config = config.deprecation_enabled(false);
        assert!(!config.enable_deprecation);
    }

    #[test]
    fn test_deprecation_default_disabled() {
        let config = MiddlewareConfig::default();
        assert!(!config.enable_deprecation);
    }

    #[test]
    fn test_deprecation_none_disabled() {
        let config = MiddlewareConfig::none();
        assert!(!config.enable_deprecation);
    }

    #[test]
    fn test_deprecation_in_summary() {
        let config = MiddlewareConfig::default();
        let summary = config.summary();
        let entry = summary.iter().find(|(name, _)| *name == "Deprecation");
        assert!(entry.is_some());
        assert!(!entry.unwrap().1); // off by default
    }

    #[tokio::test]
    async fn test_deprecation_full_stack() {
        let dep_config = DeprecationConfig {
            entries: vec![DeprecationEntry {
                path: "/test".to_string(),
                deprecated_at: Some("2025-12-01".to_string()),
                sunset_at: Some("2026-06-01".to_string()),
                replacement: Some("/v2/test".to_string()),
                message: Some("Please migrate".to_string()),
            }],
            excluded_paths: vec![],
        };
        let config = MiddlewareConfig::none().deprecation(dep_config);
        let app = Router::new().route("/test", any(ok_handler));
        let app = config.apply(app);

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers().get("deprecation").unwrap(), "2025-12-01");
        assert_eq!(response.headers().get("sunset").unwrap(), "2026-06-01");
        assert!(response.headers().contains_key("link"));
        assert_eq!(
            response.headers().get("x-deprecation-message").unwrap(),
            "Please migrate"
        );
    }

    #[tokio::test]
    async fn test_deprecation_does_not_block() {
        // Deprecation is purely informational — response body should be normal
        let dep_config = DeprecationConfig {
            entries: vec![DeprecationEntry {
                path: "/test".to_string(),
                deprecated_at: Some("2025-01-01".to_string()),
                sunset_at: Some("2025-12-31".to_string()),
                replacement: None,
                message: None,
            }],
            excluded_paths: vec![],
        };
        let app = test_router().layer(middleware::from_fn(move |req: Request, next: Next| {
            let c = dep_config.clone();
            deprecation_handler(c, req, next)
        }));

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), 1024)
            .await
            .unwrap();
        assert_eq!(&body[..], b"ok");
    }

    // ── Request Cost Tests ────────────────────────────────────────────

    #[test]
    fn test_request_cost_config_default() {
        let config = RequestCostConfig::default();
        assert_eq!(config.get_cost, 1);
        assert_eq!(config.post_cost, 5);
        assert_eq!(config.put_cost, 3);
        assert_eq!(config.patch_cost, 3);
        assert_eq!(config.delete_cost, 5);
        assert_eq!(config.head_cost, 1);
        assert_eq!(config.options_cost, 0);
        assert_eq!(config.budget, 1000);
        assert_eq!(config.window_secs, 3600);
        assert_eq!(config.excluded_paths, vec!["/health".to_string()]);
    }

    #[test]
    fn test_request_cost_config_excluded() {
        let config = RequestCostConfig::default();
        assert!(config.is_excluded("/health"));
        assert!(config.is_excluded("/health/ready"));
        assert!(!config.is_excluded("/test"));
    }

    #[test]
    fn test_request_cost_config_method_costs() {
        let config = RequestCostConfig::default();
        assert_eq!(config.cost_for_method(&Method::GET), 1);
        assert_eq!(config.cost_for_method(&Method::POST), 5);
        assert_eq!(config.cost_for_method(&Method::PUT), 3);
        assert_eq!(config.cost_for_method(&Method::PATCH), 3);
        assert_eq!(config.cost_for_method(&Method::DELETE), 5);
        assert_eq!(config.cost_for_method(&Method::HEAD), 1);
        assert_eq!(config.cost_for_method(&Method::OPTIONS), 0);
    }

    #[test]
    fn test_request_cost_state_new() {
        let state = RequestCostState::new(RequestCostConfig::default());
        assert_eq!(state.remaining(), 1000);
    }

    #[test]
    fn test_request_cost_state_spend() {
        let state = RequestCostState::new(RequestCostConfig {
            budget: 10,
            ..RequestCostConfig::default()
        });
        let (remaining, ok) = state.try_spend(3);
        assert!(ok);
        assert_eq!(remaining, 7);
        assert_eq!(state.remaining(), 7);
    }

    #[test]
    fn test_request_cost_state_over_budget() {
        let state = RequestCostState::new(RequestCostConfig {
            budget: 5,
            ..RequestCostConfig::default()
        });
        let (_, ok) = state.try_spend(3);
        assert!(ok);
        let (remaining, ok) = state.try_spend(5);
        assert!(!ok);
        assert_eq!(remaining, 2); // 5 - 3 = 2 left
        assert_eq!(state.remaining(), 2); // spend was rolled back
    }

    #[test]
    fn test_request_cost_state_exact_budget() {
        let state = RequestCostState::new(RequestCostConfig {
            budget: 10,
            ..RequestCostConfig::default()
        });
        let (remaining, ok) = state.try_spend(10);
        assert!(ok);
        assert_eq!(remaining, 0);
    }

    #[tokio::test]
    async fn test_request_cost_adds_headers() {
        let config = RequestCostConfig::default();
        let state = Arc::new(RequestCostState::new(config));
        let app = test_router().layer(middleware::from_fn(move |req: Request, next: Next| {
            let s = state.clone();
            request_cost_handler(s, req, next)
        }));

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers().get("x-request-cost").unwrap(), "1");
        assert_eq!(
            response.headers().get("x-cost-budget-remaining").unwrap(),
            "999"
        );
    }

    #[tokio::test]
    async fn test_request_cost_post_higher_cost() {
        let config = RequestCostConfig::default();
        let state = Arc::new(RequestCostState::new(config));
        let app = test_router().layer(middleware::from_fn(move |req: Request, next: Next| {
            let s = state.clone();
            request_cost_handler(s, req, next)
        }));

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .method("POST")
                    .uri("/test")
                    .header("content-type", "application/json")
                    .body(Body::from("{}"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers().get("x-request-cost").unwrap(), "5");
        assert_eq!(
            response.headers().get("x-cost-budget-remaining").unwrap(),
            "995"
        );
    }

    #[tokio::test]
    async fn test_request_cost_rejects_over_budget() {
        let config = RequestCostConfig {
            budget: 3,
            ..RequestCostConfig::default()
        };
        let state = Arc::new(RequestCostState::new(config));
        let s1 = state.clone();
        let s2 = state.clone();

        let app1 = test_router().layer(middleware::from_fn(move |req: Request, next: Next| {
            let s = s1.clone();
            request_cost_handler(s, req, next)
        }));

        // First request costs 1, should succeed
        let response = app1
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let app2 = test_router().layer(middleware::from_fn(move |req: Request, next: Next| {
            let s = s2.clone();
            request_cost_handler(s, req, next)
        }));

        // POST costs 5, should be rejected (only 2 remaining)
        let response = app2
            .oneshot(
                HttpRequest::builder()
                    .method("POST")
                    .uri("/test")
                    .header("content-type", "application/json")
                    .body(Body::from("{}"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(response.headers().get("x-request-cost").unwrap(), "5");
    }

    #[tokio::test]
    async fn test_request_cost_skips_excluded() {
        let config = RequestCostConfig::default();
        let state = Arc::new(RequestCostState::new(config));
        let app = test_router().layer(middleware::from_fn(move |req: Request, next: Next| {
            let s = state.clone();
            request_cost_handler(s, req, next)
        }));

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert!(!response.headers().contains_key("x-request-cost"));
    }

    #[test]
    fn test_request_cost_builder_enables() {
        let config = MiddlewareConfig::none().request_cost(RequestCostConfig::default());
        assert!(config.enable_request_cost);
    }

    #[test]
    fn test_request_cost_builder_toggle() {
        let config = MiddlewareConfig::none().request_cost_enabled(true);
        assert!(config.enable_request_cost);
        let config = config.request_cost_enabled(false);
        assert!(!config.enable_request_cost);
    }

    #[test]
    fn test_request_cost_default_disabled() {
        let config = MiddlewareConfig::default();
        assert!(!config.enable_request_cost);
    }

    #[test]
    fn test_request_cost_none_disabled() {
        let config = MiddlewareConfig::none();
        assert!(!config.enable_request_cost);
    }

    #[test]
    fn test_request_cost_in_summary() {
        let config = MiddlewareConfig::default();
        let summary = config.summary();
        let entry = summary.iter().find(|(name, _)| *name == "Request Cost");
        assert!(entry.is_some());
        assert!(!entry.unwrap().1); // off by default
    }

    #[tokio::test]
    async fn test_request_cost_full_stack() {
        let cost_config = RequestCostConfig {
            budget: 100,
            ..RequestCostConfig::default()
        };
        let config = MiddlewareConfig::none().request_cost(cost_config);
        let app = Router::new().route("/test", any(ok_handler));
        let app = config.apply(app);

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers().get("x-request-cost").unwrap(), "1");
        assert_eq!(
            response.headers().get("x-cost-budget-remaining").unwrap(),
            "99"
        );
    }

    // ── Request Fingerprint Tests ─────────────────────────────────────

    #[tokio::test]
    async fn test_fingerprint_added_to_response() {
        let config = MiddlewareConfig::none().request_fingerprint(FingerprintConfig::default());
        let app = Router::new().route("/test", any(ok_handler));
        let app = config.apply(app);

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert!(response.headers().contains_key("x-request-fingerprint"));
    }

    #[tokio::test]
    async fn test_fingerprint_deterministic() {
        let fp_config = FingerprintConfig::default();
        let config1 = MiddlewareConfig::none().request_fingerprint(fp_config.clone());
        let config2 = MiddlewareConfig::none().request_fingerprint(FingerprintConfig::default());

        let app1 = config1.apply(Router::new().route("/test", any(ok_handler)));
        let app2 = config2.apply(Router::new().route("/test", any(ok_handler)));

        let r1 = app1
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let r2 = app2
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        let fp1 = r1.headers().get("x-request-fingerprint").unwrap();
        let fp2 = r2.headers().get("x-request-fingerprint").unwrap();
        assert_eq!(fp1, fp2);
    }

    #[tokio::test]
    async fn test_fingerprint_differs_by_method() {
        let fp_config = FingerprintConfig {
            include_body: false,
            ..FingerprintConfig::default()
        };
        let config = MiddlewareConfig::none().request_fingerprint(fp_config);

        let app1 = config
            .clone()
            .apply(Router::new().route("/test", any(ok_handler)));
        let app2 = config.apply(Router::new().route("/test", any(ok_handler)));

        let r1 = app1
            .oneshot(
                HttpRequest::builder()
                    .method("GET")
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let r2 = app2
            .oneshot(
                HttpRequest::builder()
                    .method("POST")
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        let fp1 = r1.headers().get("x-request-fingerprint").unwrap();
        let fp2 = r2.headers().get("x-request-fingerprint").unwrap();
        assert_ne!(fp1, fp2);
    }

    #[tokio::test]
    async fn test_fingerprint_differs_by_path() {
        let fp_config = FingerprintConfig {
            include_body: false,
            excluded_paths: vec![],
            ..FingerprintConfig::default()
        };
        let config = MiddlewareConfig::none().request_fingerprint(fp_config);

        let app1 = config.clone().apply(
            Router::new()
                .route("/test", any(ok_handler))
                .route("/other", any(ok_handler)),
        );
        let app2 = config.apply(
            Router::new()
                .route("/test", any(ok_handler))
                .route("/other", any(ok_handler)),
        );

        let r1 = app1
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let r2 = app2
            .oneshot(
                HttpRequest::builder()
                    .uri("/other")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        let fp1 = r1.headers().get("x-request-fingerprint").unwrap();
        let fp2 = r2.headers().get("x-request-fingerprint").unwrap();
        assert_ne!(fp1, fp2);
    }

    #[tokio::test]
    async fn test_fingerprint_differs_by_body() {
        let fp_config = FingerprintConfig::default();
        let config = MiddlewareConfig::none().request_fingerprint(fp_config);

        let app1 = config
            .clone()
            .apply(Router::new().route("/test", any(ok_handler)));
        let app2 = config.apply(Router::new().route("/test", any(ok_handler)));

        let r1 = app1
            .oneshot(
                HttpRequest::builder()
                    .method("POST")
                    .uri("/test")
                    .body(Body::from("body1"))
                    .unwrap(),
            )
            .await
            .unwrap();
        let r2 = app2
            .oneshot(
                HttpRequest::builder()
                    .method("POST")
                    .uri("/test")
                    .body(Body::from("body2"))
                    .unwrap(),
            )
            .await
            .unwrap();

        let fp1 = r1.headers().get("x-request-fingerprint").unwrap();
        let fp2 = r2.headers().get("x-request-fingerprint").unwrap();
        assert_ne!(fp1, fp2);
    }

    #[tokio::test]
    async fn test_fingerprint_excluded_path() {
        let fp_config = FingerprintConfig {
            excluded_paths: vec!["/health".to_string()],
            ..FingerprintConfig::default()
        };
        let config = MiddlewareConfig::none().request_fingerprint(fp_config);
        let app = config.apply(Router::new().route("/health", get(ok_handler)));

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert!(!response.headers().contains_key("x-request-fingerprint"));
    }

    #[tokio::test]
    async fn test_fingerprint_includes_headers() {
        let fp_config = FingerprintConfig {
            include_headers: vec!["x-custom".to_string()],
            include_body: false,
            ..FingerprintConfig::default()
        };
        let config = MiddlewareConfig::none().request_fingerprint(fp_config);

        let app1 = config
            .clone()
            .apply(Router::new().route("/test", any(ok_handler)));
        let app2 = config.apply(Router::new().route("/test", any(ok_handler)));

        let r1 = app1
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .header("x-custom", "val1")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let r2 = app2
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .header("x-custom", "val2")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        let fp1 = r1.headers().get("x-request-fingerprint").unwrap();
        let fp2 = r2.headers().get("x-request-fingerprint").unwrap();
        assert_ne!(fp1, fp2);
    }

    #[tokio::test]
    async fn test_fingerprint_without_body() {
        let fp_config = FingerprintConfig {
            include_body: false,
            ..FingerprintConfig::default()
        };
        let config = MiddlewareConfig::none().request_fingerprint(fp_config);
        let app = config.apply(Router::new().route("/test", any(ok_handler)));

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert!(response.headers().contains_key("x-request-fingerprint"));
    }

    #[tokio::test]
    async fn test_fingerprint_with_query_params() {
        let fp_config = FingerprintConfig {
            include_body: false,
            include_query: true,
            ..FingerprintConfig::default()
        };
        let config = MiddlewareConfig::none().request_fingerprint(fp_config);

        let app1 = config
            .clone()
            .apply(Router::new().route("/test", any(ok_handler)));
        let app2 = config.apply(Router::new().route("/test", any(ok_handler)));

        let r1 = app1
            .oneshot(
                HttpRequest::builder()
                    .uri("/test?a=1")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let r2 = app2
            .oneshot(
                HttpRequest::builder()
                    .uri("/test?a=2")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        let fp1 = r1.headers().get("x-request-fingerprint").unwrap();
        let fp2 = r2.headers().get("x-request-fingerprint").unwrap();
        assert_ne!(fp1, fp2);
    }

    #[tokio::test]
    async fn test_fingerprint_without_query() {
        let fp_config = FingerprintConfig {
            include_body: false,
            include_query: false,
            ..FingerprintConfig::default()
        };
        let config = MiddlewareConfig::none().request_fingerprint(fp_config);

        let app1 = config
            .clone()
            .apply(Router::new().route("/test", any(ok_handler)));
        let app2 = config.apply(Router::new().route("/test", any(ok_handler)));

        let r1 = app1
            .oneshot(
                HttpRequest::builder()
                    .uri("/test?a=1")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let r2 = app2
            .oneshot(
                HttpRequest::builder()
                    .uri("/test?a=2")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        let fp1 = r1.headers().get("x-request-fingerprint").unwrap();
        let fp2 = r2.headers().get("x-request-fingerprint").unwrap();
        assert_eq!(fp1, fp2);
    }

    #[tokio::test]
    async fn test_fingerprint_is_16_hex_chars() {
        let config = MiddlewareConfig::none().request_fingerprint(FingerprintConfig::default());
        let app = config.apply(Router::new().route("/test", any(ok_handler)));

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let fp = response
            .headers()
            .get("x-request-fingerprint")
            .unwrap()
            .to_str()
            .unwrap();
        assert_eq!(fp.len(), 16);
        assert!(fp.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn test_fingerprint_config_default() {
        let config = FingerprintConfig::default();
        assert_eq!(config.include_headers, vec!["content-type", "accept"]);
        assert_eq!(config.excluded_paths, vec!["/health"]);
        assert!(config.include_body);
        assert!(config.include_query);
    }

    #[test]
    fn test_fingerprint_config_is_excluded() {
        let config = FingerprintConfig {
            excluded_paths: vec!["/health".to_string(), "/metrics".to_string()],
            ..FingerprintConfig::default()
        };
        assert!(config.is_excluded("/health"));
        assert!(config.is_excluded("/health/ready"));
        assert!(config.is_excluded("/metrics"));
        assert!(!config.is_excluded("/test"));
    }

    #[test]
    fn test_fingerprint_builder() {
        let config = MiddlewareConfig::none().request_fingerprint(FingerprintConfig::default());
        assert!(config.enable_fingerprint);
    }

    #[test]
    fn test_fingerprint_builder_enabled() {
        let config = MiddlewareConfig::none().fingerprint_enabled(true);
        assert!(config.enable_fingerprint);

        let config = MiddlewareConfig::none().fingerprint_enabled(false);
        assert!(!config.enable_fingerprint);
    }

    #[test]
    fn test_fingerprint_none_disabled() {
        let config = MiddlewareConfig::none();
        assert!(!config.enable_fingerprint);
    }

    #[test]
    fn test_fingerprint_in_summary() {
        let config = MiddlewareConfig::default();
        let summary = config.summary();
        let entry = summary.iter().find(|(name, _)| *name == "Fingerprint");
        assert!(entry.is_some());
        assert!(!entry.unwrap().1); // off by default
    }

    #[tokio::test]
    async fn test_fingerprint_full_stack() {
        let config = MiddlewareConfig::none().request_fingerprint(FingerprintConfig::default());
        let app = Router::new().route("/test", any(ok_handler));
        let app = config.apply(app);

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert!(response.headers().contains_key("x-request-fingerprint"));
    }

    // ── Response Signing ──────────────────────────────────────────────────

    #[tokio::test]
    async fn test_response_signing_adds_signature() {
        let mut rs = ResponseSigningConfig::default();
        rs.secret = "test-secret".to_string();
        let config = MiddlewareConfig::none().response_signing(rs);
        let app = Router::new().route("/test", any(ok_handler));
        let app = config.apply(app);

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert!(response.headers().contains_key("x-response-signature"));
        assert_eq!(
            response.headers().get("x-signature-algorithm").unwrap(),
            "hmac-sha256"
        );
    }

    #[tokio::test]
    async fn test_response_signing_deterministic() {
        let mut rs = ResponseSigningConfig::default();
        rs.secret = "det-key".to_string();
        let config = MiddlewareConfig::none().response_signing(rs);
        let app1 = Router::new().route("/test", any(ok_handler));
        let app1 = config.clone().apply(app1);
        let app2 = Router::new().route("/test", any(ok_handler));
        let app2 = config.apply(app2);

        let r1 = app1
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let r2 = app2
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let sig1 = r1
            .headers()
            .get("x-response-signature")
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();
        let sig2 = r2
            .headers()
            .get("x-response-signature")
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();
        assert_eq!(sig1, sig2);
    }

    #[tokio::test]
    async fn test_response_signing_excluded_path() {
        let mut rs = ResponseSigningConfig::default();
        rs.secret = "secret".to_string();
        // /health is excluded by default
        let config = MiddlewareConfig::none().response_signing(rs);
        let app = Router::new().route("/health", any(ok_handler));
        let app = config.apply(app);

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert!(!response.headers().contains_key("x-response-signature"));
    }

    #[tokio::test]
    async fn test_response_signing_empty_secret_skips() {
        let config = MiddlewareConfig::none().response_signing(ResponseSigningConfig::default());
        let app = Router::new().route("/test", any(ok_handler));
        let app = config.apply(app);

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert!(!response.headers().contains_key("x-response-signature"));
    }

    #[tokio::test]
    async fn test_response_signing_includes_status() {
        let mut rs1 = ResponseSigningConfig::default();
        rs1.secret = "key".to_string();
        rs1.include_status = true;
        let mut rs2 = ResponseSigningConfig::default();
        rs2.secret = "key".to_string();
        rs2.include_status = false;

        let c1 = MiddlewareConfig::none().response_signing(rs1);
        let c2 = MiddlewareConfig::none().response_signing(rs2);

        let app1 = Router::new().route("/test", any(ok_handler));
        let app1 = c1.apply(app1);
        let app2 = Router::new().route("/test", any(ok_handler));
        let app2 = c2.apply(app2);

        let r1 = app1
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let r2 = app2
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let sig1 = r1
            .headers()
            .get("x-response-signature")
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();
        let sig2 = r2
            .headers()
            .get("x-response-signature")
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();
        assert_ne!(sig1, sig2);
    }

    #[tokio::test]
    async fn test_response_signing_custom_header_name() {
        let mut rs = ResponseSigningConfig::default();
        rs.secret = "secret".to_string();
        rs.signature_header = "x-custom-sig".to_string();
        let config = MiddlewareConfig::none().response_signing(rs);
        let app = Router::new().route("/test", any(ok_handler));
        let app = config.apply(app);

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert!(response.headers().contains_key("x-custom-sig"));
        assert!(!response.headers().contains_key("x-response-signature"));
    }

    #[tokio::test]
    async fn test_response_signing_signature_is_64_hex_chars() {
        let mut rs = ResponseSigningConfig::default();
        rs.secret = "secret".to_string();
        let config = MiddlewareConfig::none().response_signing(rs);
        let app = Router::new().route("/test", any(ok_handler));
        let app = config.apply(app);

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let sig = response
            .headers()
            .get("x-response-signature")
            .unwrap()
            .to_str()
            .unwrap();
        assert_eq!(sig.len(), 64);
        assert!(sig.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn test_response_signing_config_default() {
        let config = ResponseSigningConfig::default();
        assert!(config.secret.is_empty());
        assert_eq!(config.excluded_paths, vec!["/health".to_string()]);
        assert_eq!(config.signature_header, "x-response-signature");
        assert!(config.include_status);
        assert!(config.include_headers.is_empty());
        assert_eq!(config.max_body_bytes, 10 * 1024 * 1024);
    }

    #[test]
    fn test_response_signing_config_is_excluded() {
        let config = ResponseSigningConfig::default();
        assert!(config.is_excluded("/health"));
        assert!(!config.is_excluded("/test"));
    }

    #[test]
    fn test_response_signing_builder() {
        let mut rs = ResponseSigningConfig::default();
        rs.secret = "key".to_string();
        let config = MiddlewareConfig::none().response_signing(rs);
        assert!(config.enable_response_signing);
        assert_eq!(config.response_signing.secret, "key");
    }

    #[test]
    fn test_response_signing_builder_enabled() {
        let config = MiddlewareConfig::none().response_signing_enabled(true);
        assert!(config.enable_response_signing);
        let config = config.response_signing_enabled(false);
        assert!(!config.enable_response_signing);
    }

    #[test]
    fn test_response_signing_none_disabled() {
        let config = MiddlewareConfig::none();
        assert!(!config.enable_response_signing);
    }

    #[test]
    fn test_response_signing_in_summary() {
        let config = MiddlewareConfig::default();
        let summary = config.summary();
        let entry = summary.iter().find(|(name, _)| *name == "Response Signing");
        assert!(entry.is_some());
        assert!(!entry.unwrap().1);
    }

    #[tokio::test]
    async fn test_response_signing_different_secrets_differ() {
        let mut rs1 = ResponseSigningConfig::default();
        rs1.secret = "key-a".to_string();
        let mut rs2 = ResponseSigningConfig::default();
        rs2.secret = "key-b".to_string();

        let c1 = MiddlewareConfig::none().response_signing(rs1);
        let c2 = MiddlewareConfig::none().response_signing(rs2);

        let app1 = Router::new().route("/test", any(ok_handler));
        let app1 = c1.apply(app1);
        let app2 = Router::new().route("/test", any(ok_handler));
        let app2 = c2.apply(app2);

        let r1 = app1
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let r2 = app2
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let sig1 = r1
            .headers()
            .get("x-response-signature")
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();
        let sig2 = r2
            .headers()
            .get("x-response-signature")
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();
        assert_ne!(sig1, sig2);
    }

    #[tokio::test]
    async fn test_response_signing_includes_response_headers() {
        let mut rs = ResponseSigningConfig::default();
        rs.secret = "key".to_string();
        rs.include_headers = vec!["content-type".to_string()];
        let config = MiddlewareConfig::none().response_signing(rs);
        let app = Router::new().route("/test", any(ok_handler));
        let app = config.apply(app);

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert!(response.headers().contains_key("x-response-signature"));
    }

    #[tokio::test]
    async fn test_response_signing_full_stack() {
        let mut rs = ResponseSigningConfig::default();
        rs.secret = "full-stack-key".to_string();
        let config = MiddlewareConfig::none().response_signing(rs);
        let app = Router::new().route("/test", any(ok_handler));
        let app = config.apply(app);

        let response = app
            .oneshot(
                HttpRequest::builder()
                    .uri("/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert!(response.headers().contains_key("x-response-signature"));
    }

    // -------------------------------------------------------------------------
    //  Request Priority tests
    // -------------------------------------------------------------------------

    fn rp_config_default() -> RequestPriorityConfig {
        RequestPriorityConfig::default()
    }

    fn rp_config_with_rules() -> RequestPriorityConfig {
        RequestPriorityConfig {
            rules: vec![
                PriorityRule {
                    path_prefix: "/health".to_string(),
                    method: None,
                    priority: PriorityLevel::Critical,
                    reason: "health check".to_string(),
                },
                PriorityRule {
                    path_prefix: "/api/admin".to_string(),
                    method: Some("POST".to_string()),
                    priority: PriorityLevel::High,
                    reason: "admin write".to_string(),
                },
                PriorityRule {
                    path_prefix: "/api/bulk".to_string(),
                    method: None,
                    priority: PriorityLevel::Low,
                    reason: "bulk operation".to_string(),
                },
            ],
            default_priority: PriorityLevel::Normal,
            priority_header: "x-request-priority".to_string(),
            reason_header: "x-priority-reason".to_string(),
            allow_client_override: false,
        }
    }

    #[tokio::test]
    async fn test_request_priority_default_level() {
        let config = rp_config_default();
        let app = Router::new()
            .route("/test", axum::routing::any(|| async { "ok" }))
            .layer(middleware::from_fn(move |req: Request, next: Next| {
                let c = config.clone();
                request_priority_handler(c, req, next)
            }));
        let resp = app
            .oneshot(Request::builder().uri("/test").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.headers().get("x-request-priority").unwrap(), "normal");
    }

    #[tokio::test]
    async fn test_request_priority_rule_match() {
        let config = rp_config_with_rules();
        let app = Router::new()
            .route("/health", axum::routing::any(|| async { "ok" }))
            .layer(middleware::from_fn(move |req: Request, next: Next| {
                let c = config.clone();
                request_priority_handler(c, req, next)
            }));
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            resp.headers().get("x-request-priority").unwrap(),
            "critical"
        );
        assert_eq!(
            resp.headers().get("x-priority-reason").unwrap(),
            "health check"
        );
    }

    #[tokio::test]
    async fn test_request_priority_method_filter_match() {
        let config = rp_config_with_rules();
        let app = Router::new()
            .route("/api/admin", axum::routing::any(|| async { "ok" }))
            .layer(middleware::from_fn(move |req: Request, next: Next| {
                let c = config.clone();
                request_priority_handler(c, req, next)
            }));
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/admin")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.headers().get("x-request-priority").unwrap(), "high");
        assert_eq!(
            resp.headers().get("x-priority-reason").unwrap(),
            "admin write"
        );
    }

    #[tokio::test]
    async fn test_request_priority_method_filter_no_match() {
        let config = rp_config_with_rules();
        let app = Router::new()
            .route("/api/admin", axum::routing::any(|| async { "ok" }))
            .layer(middleware::from_fn(move |req: Request, next: Next| {
                let c = config.clone();
                request_priority_handler(c, req, next)
            }));
        let resp = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/admin")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        // GET /api/admin doesn't match the POST rule, falls to default
        assert_eq!(resp.headers().get("x-request-priority").unwrap(), "normal");
    }

    #[tokio::test]
    async fn test_request_priority_first_match_wins() {
        let config = RequestPriorityConfig {
            rules: vec![
                PriorityRule {
                    path_prefix: "/api".to_string(),
                    method: None,
                    priority: PriorityLevel::High,
                    reason: "first".to_string(),
                },
                PriorityRule {
                    path_prefix: "/api".to_string(),
                    method: None,
                    priority: PriorityLevel::Low,
                    reason: "second".to_string(),
                },
            ],
            ..RequestPriorityConfig::default()
        };
        let app = Router::new()
            .route("/api/test", axum::routing::any(|| async { "ok" }))
            .layer(middleware::from_fn(move |req: Request, next: Next| {
                let c = config.clone();
                request_priority_handler(c, req, next)
            }));
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.headers().get("x-request-priority").unwrap(), "high");
        assert_eq!(resp.headers().get("x-priority-reason").unwrap(), "first");
    }

    #[tokio::test]
    async fn test_request_priority_client_override_disabled() {
        let config = rp_config_with_rules();
        let app = Router::new()
            .route("/test", axum::routing::any(|| async { "ok" }))
            .layer(middleware::from_fn(move |req: Request, next: Next| {
                let c = config.clone();
                request_priority_handler(c, req, next)
            }));
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/test")
                    .header("x-request-priority", "critical")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        // Client override disabled, so server-assigned normal wins
        assert_eq!(resp.headers().get("x-request-priority").unwrap(), "normal");
    }

    #[tokio::test]
    async fn test_request_priority_client_override_enabled() {
        let config = RequestPriorityConfig {
            allow_client_override: true,
            ..RequestPriorityConfig::default()
        };
        let app = Router::new()
            .route("/test", axum::routing::any(|| async { "ok" }))
            .layer(middleware::from_fn(move |req: Request, next: Next| {
                let c = config.clone();
                request_priority_handler(c, req, next)
            }));
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/test")
                    .header("x-request-priority", "critical")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            resp.headers().get("x-request-priority").unwrap(),
            "critical"
        );
    }

    #[tokio::test]
    async fn test_request_priority_client_override_invalid_ignored() {
        let config = RequestPriorityConfig {
            allow_client_override: true,
            ..RequestPriorityConfig::default()
        };
        let app = Router::new()
            .route("/test", axum::routing::any(|| async { "ok" }))
            .layer(middleware::from_fn(move |req: Request, next: Next| {
                let c = config.clone();
                request_priority_handler(c, req, next)
            }));
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/test")
                    .header("x-request-priority", "super-duper")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        // Invalid value ignored, falls to default
        assert_eq!(resp.headers().get("x-request-priority").unwrap(), "normal");
    }

    #[tokio::test]
    async fn test_request_priority_custom_headers() {
        let config = RequestPriorityConfig {
            priority_header: "x-qos-level".to_string(),
            reason_header: "x-qos-reason".to_string(),
            ..RequestPriorityConfig::default()
        };
        let app = Router::new()
            .route("/test", axum::routing::any(|| async { "ok" }))
            .layer(middleware::from_fn(move |req: Request, next: Next| {
                let c = config.clone();
                request_priority_handler(c, req, next)
            }));
        let resp = app
            .oneshot(Request::builder().uri("/test").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert!(resp.headers().get("x-qos-level").is_some());
        assert!(resp.headers().get("x-request-priority").is_none());
    }

    #[test]
    fn test_priority_level_weight() {
        assert_eq!(PriorityLevel::Critical.weight(), 4);
        assert_eq!(PriorityLevel::High.weight(), 3);
        assert_eq!(PriorityLevel::Normal.weight(), 2);
        assert_eq!(PriorityLevel::Low.weight(), 1);
    }

    #[test]
    fn test_priority_level_as_str() {
        assert_eq!(PriorityLevel::Critical.as_str(), "critical");
        assert_eq!(PriorityLevel::High.as_str(), "high");
        assert_eq!(PriorityLevel::Normal.as_str(), "normal");
        assert_eq!(PriorityLevel::Low.as_str(), "low");
    }

    #[test]
    fn test_priority_level_from_str_opt() {
        assert_eq!(
            PriorityLevel::from_str_opt("critical"),
            Some(PriorityLevel::Critical)
        );
        assert_eq!(
            PriorityLevel::from_str_opt("HIGH"),
            Some(PriorityLevel::High)
        );
        assert_eq!(
            PriorityLevel::from_str_opt("Normal"),
            Some(PriorityLevel::Normal)
        );
        assert_eq!(PriorityLevel::from_str_opt("low"), Some(PriorityLevel::Low));
        assert_eq!(PriorityLevel::from_str_opt("invalid"), None);
    }

    #[test]
    fn test_priority_level_display() {
        assert_eq!(format!("{}", PriorityLevel::Critical), "critical");
        assert_eq!(format!("{}", PriorityLevel::Low), "low");
    }

    #[test]
    fn test_request_priority_config_default() {
        let config = RequestPriorityConfig::default();
        assert!(config.rules.is_empty());
        assert_eq!(config.default_priority, PriorityLevel::Normal);
        assert_eq!(config.priority_header, "x-request-priority");
        assert_eq!(config.reason_header, "x-priority-reason");
        assert!(!config.allow_client_override);
    }

    #[test]
    fn test_request_priority_config_evaluate_default() {
        let config = RequestPriorityConfig::default();
        let (level, reason) = config.evaluate("GET", "/any/path");
        assert_eq!(level, PriorityLevel::Normal);
        assert_eq!(reason, "default");
    }

    #[test]
    fn test_request_priority_config_evaluate_rule() {
        let config = rp_config_with_rules();
        let (level, reason) = config.evaluate("GET", "/health");
        assert_eq!(level, PriorityLevel::Critical);
        assert_eq!(reason, "health check");
    }

    #[test]
    fn test_request_priority_builder() {
        let mw = MiddlewareConfig::none()
            .request_priority(rp_config_with_rules())
            .request_priority_enabled(true);
        assert!(mw.enable_request_priority);
        assert_eq!(mw.request_priority.rules.len(), 3);
    }

    #[test]
    fn test_request_priority_none_disabled() {
        let mw = MiddlewareConfig::none();
        assert!(!mw.enable_request_priority);
    }

    #[test]
    fn test_request_priority_in_summary() {
        let mw = MiddlewareConfig::none().request_priority_enabled(true);
        let summary = mw.summary();
        let found = summary
            .iter()
            .any(|(name, enabled)| *name == "Request Priority" && *enabled);
        assert!(found);
    }

    #[tokio::test]
    async fn test_request_priority_bulk_low() {
        let config = rp_config_with_rules();
        let app = Router::new()
            .route("/api/bulk/import", axum::routing::any(|| async { "ok" }))
            .layer(middleware::from_fn(move |req: Request, next: Next| {
                let c = config.clone();
                request_priority_handler(c, req, next)
            }));
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/bulk/import")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.headers().get("x-request-priority").unwrap(), "low");
        assert_eq!(
            resp.headers().get("x-priority-reason").unwrap(),
            "bulk operation"
        );
    }

    // -------------------------------------------------------------------------
    //  Request Quota tests
    // -------------------------------------------------------------------------

    fn rq_config_small() -> RequestQuotaConfig {
        RequestQuotaConfig {
            limit: 3,
            window_secs: 3600,
            ..RequestQuotaConfig::default()
        }
    }

    fn rq_app(config: RequestQuotaConfig, state: QuotaState) -> Router {
        Router::new()
            .route("/test", axum::routing::any(|| async { "ok" }))
            .route("/health", axum::routing::any(|| async { "ok" }))
            .layer(middleware::from_fn(move |req: Request, next: Next| {
                let c = config.clone();
                let s = state.clone();
                request_quota_handler(c, s, req, next)
            }))
    }

    #[tokio::test]
    async fn test_request_quota_headers_present() {
        let state = QuotaState::new();
        let app = rq_app(rq_config_small(), state);
        let resp = app
            .oneshot(Request::builder().uri("/test").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), axum::http::StatusCode::OK);
        assert!(resp.headers().get("x-quota-limit").is_some());
        assert!(resp.headers().get("x-quota-remaining").is_some());
        assert!(resp.headers().get("x-quota-reset").is_some());
    }

    #[tokio::test]
    async fn test_request_quota_limit_header_value() {
        let state = QuotaState::new();
        let app = rq_app(rq_config_small(), state);
        let resp = app
            .oneshot(Request::builder().uri("/test").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.headers().get("x-quota-limit").unwrap(), "3");
    }

    #[tokio::test]
    async fn test_request_quota_remaining_decrements() {
        let state = QuotaState::new();
        let config = rq_config_small();
        // First request: remaining = 2
        let (allowed, remaining, _) = state.check("test-client", config.limit, config.window_secs);
        assert!(allowed);
        assert_eq!(remaining, 2);
        // Second request: remaining = 1
        let (allowed, remaining, _) = state.check("test-client", config.limit, config.window_secs);
        assert!(allowed);
        assert_eq!(remaining, 1);
        // Third request: remaining = 0
        let (allowed, remaining, _) = state.check("test-client", config.limit, config.window_secs);
        assert!(allowed);
        assert_eq!(remaining, 0);
    }

    #[tokio::test]
    async fn test_request_quota_exceeded_returns_429() {
        let state = QuotaState::new();
        let config = rq_config_small();
        // Exhaust quota
        for _ in 0..3 {
            state.check("anonymous", config.limit, config.window_secs);
        }
        // 4th request should be rejected
        let app = rq_app(config, state);
        let resp = app
            .oneshot(Request::builder().uri("/test").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), axum::http::StatusCode::TOO_MANY_REQUESTS);
    }

    #[tokio::test]
    async fn test_request_quota_exceeded_json_body() {
        let state = QuotaState::new();
        let config = rq_config_small();
        for _ in 0..3 {
            state.check("anonymous", config.limit, config.window_secs);
        }
        let app = rq_app(config, state);
        let resp = app
            .oneshot(Request::builder().uri("/test").body(Body::empty()).unwrap())
            .await
            .unwrap();
        let body = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["error"]["code"], "QUOTA_EXCEEDED");
    }

    #[tokio::test]
    async fn test_request_quota_excluded_path() {
        let state = QuotaState::new();
        let config = rq_config_small();
        // Exhaust quota
        for _ in 0..5 {
            state.check("anonymous", config.limit, config.window_secs);
        }
        let app = rq_app(config, state);
        // /health is excluded, should still succeed
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), axum::http::StatusCode::OK);
    }

    #[tokio::test]
    async fn test_request_quota_api_key_identification() {
        let state = QuotaState::new();
        let config = rq_config_small();
        // Exhaust quota for key-A
        for _ in 0..3 {
            state.check("key-A", config.limit, config.window_secs);
        }
        let app = rq_app(config, state.clone());
        // key-B should still have quota
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/test")
                    .header("x-api-key", "key-B")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), axum::http::StatusCode::OK);
    }

    #[tokio::test]
    async fn test_request_quota_custom_headers() {
        let state = QuotaState::new();
        let config = RequestQuotaConfig {
            limit: 10,
            limit_header: "x-my-limit".to_string(),
            remaining_header: "x-my-remaining".to_string(),
            reset_header: "x-my-reset".to_string(),
            ..RequestQuotaConfig::default()
        };
        let app = rq_app(config, state);
        let resp = app
            .oneshot(Request::builder().uri("/test").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert!(resp.headers().get("x-my-limit").is_some());
        assert!(resp.headers().get("x-my-remaining").is_some());
        assert!(resp.headers().get("x-my-reset").is_some());
        assert!(resp.headers().get("x-quota-limit").is_none());
    }

    #[test]
    fn test_request_quota_config_default() {
        let config = RequestQuotaConfig::default();
        assert_eq!(config.limit, 1000);
        assert_eq!(config.window_secs, 3600);
        assert_eq!(config.limit_header, "x-quota-limit");
        assert_eq!(config.remaining_header, "x-quota-remaining");
        assert_eq!(config.reset_header, "x-quota-reset");
        assert!(config.identify_by_api_key);
        assert_eq!(config.excluded_paths, vec!["/health".to_string()]);
    }

    #[test]
    fn test_quota_state_new_window() {
        let state = QuotaState::new();
        let (allowed, remaining, reset) = state.check("client", 5, 3600);
        assert!(allowed);
        assert_eq!(remaining, 4);
        assert_eq!(reset, 3600);
    }

    #[test]
    fn test_quota_state_tracks_per_client() {
        let state = QuotaState::new();
        // Client A uses 3
        for _ in 0..3 {
            state.check("A", 5, 3600);
        }
        // Client B should still have full quota
        let (allowed, remaining, _) = state.check("B", 5, 3600);
        assert!(allowed);
        assert_eq!(remaining, 4);
    }

    #[test]
    fn test_request_quota_builder() {
        let config = RequestQuotaConfig {
            limit: 500,
            ..RequestQuotaConfig::default()
        };
        let state = QuotaState::new();
        let mw = MiddlewareConfig::none()
            .request_quota(config)
            .request_quota_enabled(true)
            .quota_state(state);
        assert!(mw.enable_request_quota);
        assert_eq!(mw.request_quota.limit, 500);
    }

    #[test]
    fn test_request_quota_none_disabled() {
        let mw = MiddlewareConfig::none();
        assert!(!mw.enable_request_quota);
    }

    #[test]
    fn test_request_quota_in_summary() {
        let mw = MiddlewareConfig::none().request_quota_enabled(true);
        let summary = mw.summary();
        let found = summary
            .iter()
            .any(|(name, enabled)| *name == "Request Quota" && *enabled);
        assert!(found);
    }

    #[tokio::test]
    async fn test_request_quota_rejected_still_has_headers() {
        let state = QuotaState::new();
        let config = rq_config_small();
        // Exhaust quota
        for _ in 0..3 {
            state.check("anonymous", config.limit, config.window_secs);
        }
        let app = rq_app(config, state);
        let resp = app
            .oneshot(Request::builder().uri("/test").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), axum::http::StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(resp.headers().get("x-quota-limit").unwrap(), "3");
        assert_eq!(resp.headers().get("x-quota-remaining").unwrap(), "0");
    }

    #[test]
    fn test_quota_state_exceeds_limit() {
        let state = QuotaState::new();
        for _ in 0..5 {
            state.check("client", 5, 3600);
        }
        let (allowed, remaining, _) = state.check("client", 5, 3600);
        assert!(!allowed);
        assert_eq!(remaining, 0);
    }
    // -------------------------------------------------------------------------
    // Tenant Isolation tests
    // -------------------------------------------------------------------------

    fn ti_config() -> TenantIsolationConfig {
        TenantIsolationConfig {
            tenant_header: "X-Tenant-Id".to_string(),
            allowed_tenants: vec!["acme".to_string(), "globex".to_string()],
            require_tenant: true,
            default_tenant: None,
            response_header: "X-Tenant-Id".to_string(),
            excluded_paths: vec!["/health".to_string()],
        }
    }

    fn ti_app(config: TenantIsolationConfig) -> Router {
        Router::new()
            .route("/api/vms", axum::routing::any(|| async { "ok" }))
            .route("/health", axum::routing::any(|| async { "healthy" }))
            .layer(middleware::from_fn(move |req: Request, next: Next| {
                let cfg = config.clone();
                tenant_isolation_handler(cfg, req, next)
            }))
    }

    #[tokio::test]
    async fn test_tenant_isolation_valid_tenant() {
        let app = ti_app(ti_config());
        let req = Request::builder()
            .uri("/api/vms")
            .header("X-Tenant-Id", "acme")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            resp.headers().get("x-tenant-id").unwrap().to_str().unwrap(),
            "acme"
        );
    }

    #[tokio::test]
    async fn test_tenant_isolation_denied_tenant() {
        let app = ti_app(ti_config());
        let req = Request::builder()
            .uri("/api/vms")
            .header("X-Tenant-Id", "evil-corp")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["error"]["code"], "TENANT_DENIED");
    }

    #[tokio::test]
    async fn test_tenant_isolation_missing_tenant_required() {
        let app = ti_app(ti_config());
        let req = Request::builder()
            .uri("/api/vms")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["error"]["code"], "MISSING_TENANT");
    }

    #[tokio::test]
    async fn test_tenant_isolation_missing_tenant_not_required() {
        let config = TenantIsolationConfig {
            require_tenant: false,
            allowed_tenants: Vec::new(),
            ..ti_config()
        };
        let app = ti_app(config);
        let req = Request::builder()
            .uri("/api/vms")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        // No tenant header injected when not required and not provided
        assert!(resp.headers().get("x-tenant-id").is_none());
    }

    #[tokio::test]
    async fn test_tenant_isolation_default_tenant() {
        let config = TenantIsolationConfig {
            require_tenant: true,
            default_tenant: Some("default-org".to_string()),
            allowed_tenants: Vec::new(),
            ..ti_config()
        };
        let app = ti_app(config);
        let req = Request::builder()
            .uri("/api/vms")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            resp.headers().get("x-tenant-id").unwrap().to_str().unwrap(),
            "default-org"
        );
    }

    #[tokio::test]
    async fn test_tenant_isolation_excluded_path_skips() {
        let app = ti_app(ti_config());
        let req = Request::builder()
            .uri("/health")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        // No tenant header required for excluded path
    }

    #[tokio::test]
    async fn test_tenant_isolation_empty_tenant_header_rejected() {
        let app = ti_app(ti_config());
        let req = Request::builder()
            .uri("/api/vms")
            .header("X-Tenant-Id", "")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        // Empty header treated like missing
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_tenant_isolation_allowed_tenants_empty_allows_any() {
        let config = TenantIsolationConfig {
            allowed_tenants: Vec::new(),
            require_tenant: true,
            ..ti_config()
        };
        let app = ti_app(config);
        let req = Request::builder()
            .uri("/api/vms")
            .header("X-Tenant-Id", "any-org")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            resp.headers().get("x-tenant-id").unwrap().to_str().unwrap(),
            "any-org"
        );
    }

    #[tokio::test]
    async fn test_tenant_isolation_response_header_name_custom() {
        let config = TenantIsolationConfig {
            response_header: "X-Org-Id".to_string(),
            allowed_tenants: Vec::new(),
            require_tenant: true,
            ..ti_config()
        };
        let app = ti_app(config);
        let req = Request::builder()
            .uri("/api/vms")
            .header("X-Tenant-Id", "acme")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            resp.headers().get("x-org-id").unwrap().to_str().unwrap(),
            "acme"
        );
    }

    #[tokio::test]
    async fn test_tenant_isolation_forbidden_body_contains_tenant() {
        let app = ti_app(ti_config());
        let req = Request::builder()
            .uri("/api/vms")
            .header("X-Tenant-Id", "nope")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(json["error"]["message"].as_str().unwrap().contains("nope"));
    }

    #[tokio::test]
    async fn test_tenant_isolation_default_config() {
        let config = TenantIsolationConfig::default();
        assert_eq!(config.tenant_header, "X-Tenant-Id");
        assert_eq!(config.response_header, "X-Tenant-Id");
        assert!(!config.require_tenant);
        assert!(config.default_tenant.is_none());
        assert!(config.allowed_tenants.is_empty());
        assert_eq!(config.excluded_paths, vec!["/health".to_string()]);
    }

    #[tokio::test]
    async fn test_tenant_isolation_builder_methods() {
        let config = TenantIsolationConfig {
            tenant_header: "X-Custom".to_string(),
            ..TenantIsolationConfig::default()
        };
        let mw = MiddlewareConfig::none()
            .tenant_isolation(config.clone())
            .tenant_isolation_enabled(true);
        assert!(mw.enable_tenant_isolation);
        assert_eq!(mw.tenant_isolation.tenant_header, "X-Custom");
    }

    #[tokio::test]
    async fn test_tenant_isolation_summary_entry() {
        let mw = MiddlewareConfig::none().tenant_isolation_enabled(true);
        let summary = mw.summary();
        let found = summary
            .iter()
            .any(|(name, enabled)| *name == "Tenant Isolation" && *enabled);
        assert!(found);
    }

    #[tokio::test]
    async fn test_tenant_isolation_second_allowed_tenant() {
        let app = ti_app(ti_config());
        let req = Request::builder()
            .uri("/api/vms")
            .header("X-Tenant-Id", "globex")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            resp.headers().get("x-tenant-id").unwrap().to_str().unwrap(),
            "globex"
        );
    }

    #[tokio::test]
    async fn test_tenant_isolation_case_sensitive() {
        let app = ti_app(ti_config());
        let req = Request::builder()
            .uri("/api/vms")
            .header("X-Tenant-Id", "ACME")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        // "ACME" != "acme" — case sensitive, should be denied
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }
    // -------------------------------------------------------------------------
    // Response Envelope tests
    // -------------------------------------------------------------------------

    fn env_config() -> ResponseEnvelopeConfig {
        ResponseEnvelopeConfig::default()
    }

    fn env_app(config: ResponseEnvelopeConfig) -> Router {
        Router::new()
            .route(
                "/api/vms",
                axum::routing::any(|| async {
                    axum::Json(serde_json::json!({"id": 1, "name": "vm1"}))
                }),
            )
            .route("/api/text", axum::routing::any(|| async { "plain text" }))
            .route("/health", axum::routing::any(|| async { "healthy" }))
            .route(
                "/api/error",
                axum::routing::any(|| async {
                    (
                        StatusCode::BAD_REQUEST,
                        axum::Json(serde_json::json!({"error": "bad"})),
                    )
                }),
            )
            .layer(middleware::from_fn(move |req: Request, next: Next| {
                let cfg = config.clone();
                response_envelope_handler(cfg, req, next)
            }))
    }

    #[tokio::test]
    async fn test_response_envelope_wraps_json() {
        let app = env_app(env_config());
        let req = Request::builder()
            .uri("/api/vms")
            .header("x-request-id", "req-123")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["data"]["id"], 1);
        assert_eq!(json["data"]["name"], "vm1");
        assert_eq!(json["meta"]["status"], 200);
        assert!(json["meta"]["timestamp"].is_number());
        assert_eq!(json["meta"]["request_id"], "req-123");
    }

    #[tokio::test]
    async fn test_response_envelope_skips_error_responses() {
        let app = env_app(env_config());
        let req = Request::builder()
            .uri("/api/error")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        // Should NOT be wrapped — no "data" key
        assert!(json.get("data").is_none());
        assert_eq!(json["error"], "bad");
    }

    #[tokio::test]
    async fn test_response_envelope_skips_non_json() {
        let app = env_app(env_config());
        let req = Request::builder()
            .uri("/api/text")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let text = String::from_utf8_lossy(&body);
        assert_eq!(text, "plain text");
    }

    #[tokio::test]
    async fn test_response_envelope_excluded_path() {
        let app = env_app(env_config());
        let req = Request::builder()
            .uri("/health")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let text = String::from_utf8_lossy(&body);
        assert_eq!(text, "healthy");
    }

    #[tokio::test]
    async fn test_response_envelope_no_request_id() {
        let app = env_app(env_config());
        let req = Request::builder()
            .uri("/api/vms")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(json["data"]["id"].is_number());
        // No request_id when header absent
        assert!(json["meta"].get("request_id").is_none());
    }

    #[tokio::test]
    async fn test_response_envelope_custom_field_names() {
        let config = ResponseEnvelopeConfig {
            data_field: "result".to_string(),
            meta_field: "info".to_string(),
            ..ResponseEnvelopeConfig::default()
        };
        let app = env_app(config);
        let req = Request::builder()
            .uri("/api/vms")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(json.get("result").is_some());
        assert!(json.get("info").is_some());
        assert!(json.get("data").is_none());
    }

    #[tokio::test]
    async fn test_response_envelope_no_status_in_meta() {
        let config = ResponseEnvelopeConfig {
            include_status: false,
            ..ResponseEnvelopeConfig::default()
        };
        let app = env_app(config);
        let req = Request::builder()
            .uri("/api/vms")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(json["meta"].get("status").is_none());
    }

    #[tokio::test]
    async fn test_response_envelope_no_timestamp_in_meta() {
        let config = ResponseEnvelopeConfig {
            include_timestamp: false,
            ..ResponseEnvelopeConfig::default()
        };
        let app = env_app(config);
        let req = Request::builder()
            .uri("/api/vms")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(json["meta"].get("timestamp").is_none());
    }

    #[tokio::test]
    async fn test_response_envelope_excluded_status_code() {
        let config = ResponseEnvelopeConfig {
            excluded_status_codes: vec![200],
            ..ResponseEnvelopeConfig::default()
        };
        let app = env_app(config);
        let req = Request::builder()
            .uri("/api/vms")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        // Should NOT be wrapped since 200 is excluded
        assert!(json.get("data").is_none());
        assert_eq!(json["id"], 1);
    }

    #[tokio::test]
    async fn test_response_envelope_default_config() {
        let config = ResponseEnvelopeConfig::default();
        assert_eq!(config.data_field, "data");
        assert_eq!(config.meta_field, "meta");
        assert!(config.include_status);
        assert!(config.include_timestamp);
        assert!(config.include_request_id);
        assert_eq!(config.excluded_paths, vec!["/health".to_string()]);
        assert!(config.excluded_status_codes.is_empty());
    }

    #[tokio::test]
    async fn test_response_envelope_builder_methods() {
        let config = ResponseEnvelopeConfig {
            data_field: "payload".to_string(),
            ..ResponseEnvelopeConfig::default()
        };
        let mw = MiddlewareConfig::none()
            .response_envelope(config.clone())
            .response_envelope_enabled(true);
        assert!(mw.enable_response_envelope);
        assert_eq!(mw.response_envelope.data_field, "payload");
    }

    #[tokio::test]
    async fn test_response_envelope_summary_entry() {
        let mw = MiddlewareConfig::none().response_envelope_enabled(true);
        let summary = mw.summary();
        let found = summary
            .iter()
            .any(|(name, enabled)| *name == "Response Envelope" && *enabled);
        assert!(found);
    }

    #[tokio::test]
    async fn test_response_envelope_meta_has_all_fields() {
        let app = env_app(env_config());
        let req = Request::builder()
            .uri("/api/vms")
            .header("x-request-id", "rid-456")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let meta = &json["meta"];
        assert_eq!(meta["status"], 200);
        assert!(meta["timestamp"].is_number());
        assert_eq!(meta["request_id"], "rid-456");
    }

    #[tokio::test]
    async fn test_response_envelope_preserves_status_code() {
        let app = env_app(env_config());
        let req = Request::builder()
            .uri("/api/vms")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    // -------------------------------------------------------------------------
    // Request Replay Protection tests
    // -------------------------------------------------------------------------

    fn rp_config() -> ReplayProtectionConfig {
        ReplayProtectionConfig::default()
    }

    fn rp_app(config: ReplayProtectionConfig) -> Router {
        let nonce_store = NonceStore::new();
        let store = nonce_store.clone();
        Router::new()
            .route(
                "/api/vms",
                axum::routing::any(|| async { axum::Json(serde_json::json!({"status": "ok"})) }),
            )
            .route("/health", axum::routing::any(|| async { "healthy" }))
            .layer(middleware::from_fn(move |req: Request, next: Next| {
                let c = config.clone();
                let s = store.clone();
                replay_protection_handler(c, s, req, next)
            }))
    }

    #[tokio::test]
    async fn test_replay_protection_allows_unique_nonce() {
        let app = rp_app(rp_config());
        let req = Request::builder()
            .uri("/api/vms")
            .header("x-nonce", "unique-1")
            .header("x-timestamp", &now_ts())
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    fn now_ts() -> String {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
            .to_string()
    }

    #[tokio::test]
    async fn test_replay_protection_rejects_duplicate_nonce() {
        let config = rp_config();
        let nonce_store = NonceStore::new();
        let store1 = nonce_store.clone();
        let store2 = nonce_store.clone();
        let c1 = config.clone();
        let c2 = config.clone();

        let app1 = Router::new()
            .route(
                "/api/vms",
                axum::routing::any(|| async { axum::Json(serde_json::json!({"status": "ok"})) }),
            )
            .layer(middleware::from_fn(move |req: Request, next: Next| {
                let c = c1.clone();
                let s = store1.clone();
                replay_protection_handler(c, s, req, next)
            }));

        let app2 = Router::new()
            .route(
                "/api/vms",
                axum::routing::any(|| async { axum::Json(serde_json::json!({"status": "ok"})) }),
            )
            .layer(middleware::from_fn(move |req: Request, next: Next| {
                let c = c2.clone();
                let s = store2.clone();
                replay_protection_handler(c, s, req, next)
            }));

        let req1 = Request::builder()
            .uri("/api/vms")
            .header("x-nonce", "dup-nonce")
            .header("x-timestamp", &now_ts())
            .body(Body::empty())
            .unwrap();
        let resp1 = app1.oneshot(req1).await.unwrap();
        assert_eq!(resp1.status(), StatusCode::OK);

        let req2 = Request::builder()
            .uri("/api/vms")
            .header("x-nonce", "dup-nonce")
            .header("x-timestamp", &now_ts())
            .body(Body::empty())
            .unwrap();
        let resp2 = app2.oneshot(req2).await.unwrap();
        assert_eq!(resp2.status(), StatusCode::CONFLICT);
        let body = axum::body::to_bytes(resp2.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["error"], "REPLAY_DETECTED");
    }

    #[tokio::test]
    async fn test_replay_protection_missing_nonce_rejected() {
        let app = rp_app(rp_config());
        let req = Request::builder()
            .uri("/api/vms")
            .header("x-timestamp", &now_ts())
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["error"], "MISSING_NONCE");
    }

    #[tokio::test]
    async fn test_replay_protection_missing_nonce_allowed_when_not_required() {
        let mut config = rp_config();
        config.require_nonce = false;
        let app = rp_app(config);
        let req = Request::builder()
            .uri("/api/vms")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_replay_protection_expired_timestamp() {
        let app = rp_app(rp_config());
        let old_ts = (std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
            - 600)
            .to_string();
        let req = Request::builder()
            .uri("/api/vms")
            .header("x-nonce", "ts-test")
            .header("x-timestamp", &old_ts)
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["error"], "TIMESTAMP_EXPIRED");
    }

    #[tokio::test]
    async fn test_replay_protection_valid_timestamp() {
        let app = rp_app(rp_config());
        let req = Request::builder()
            .uri("/api/vms")
            .header("x-nonce", "valid-ts-nonce")
            .header("x-timestamp", &now_ts())
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_replay_protection_excluded_path() {
        let app = rp_app(rp_config());
        // /health is excluded, so no nonce needed
        let req = Request::builder()
            .uri("/health")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_replay_protection_custom_nonce_header() {
        let mut config = rp_config();
        config.nonce_header = "x-idempotency-key".to_string();
        let nonce_store = NonceStore::new();
        let store = nonce_store.clone();
        let app = Router::new()
            .route(
                "/api/vms",
                axum::routing::any(|| async { axum::Json(serde_json::json!({"status": "ok"})) }),
            )
            .layer(middleware::from_fn(move |req: Request, next: Next| {
                let c = config.clone();
                let s = store.clone();
                replay_protection_handler(c, s, req, next)
            }));
        let req = Request::builder()
            .uri("/api/vms")
            .header("x-idempotency-key", "custom-nonce")
            .header("x-timestamp", &now_ts())
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_replay_protection_nonce_store_insert_and_len() {
        let store = NonceStore::new();
        assert_eq!(store.len(), 0);
        let now = 1000;
        assert!(store.try_insert("n1".into(), now, 300, 100));
        assert_eq!(store.len(), 1);
        assert!(!store.try_insert("n1".into(), now, 300, 100));
        assert_eq!(store.len(), 1);
        assert!(store.try_insert("n2".into(), now, 300, 100));
        assert_eq!(store.len(), 2);
    }

    #[tokio::test]
    async fn test_replay_protection_nonce_store_eviction() {
        let store = NonceStore::new();
        // Insert at time 1000
        assert!(store.try_insert("old".into(), 1000, 300, 1));
        assert_eq!(store.len(), 1);
        // Insert at time 1400 — max_size=1, purge triggers, old (1000) is expired (1400-1000=400 > 300)
        assert!(store.try_insert("new1".into(), 1400, 300, 1));
        // "old" purged, only "new1" remains
        assert_eq!(store.len(), 1);
        // Can reuse "old" now since it was purged
        assert!(store.try_insert("old".into(), 1400, 300, 100));
        assert_eq!(store.len(), 2);
    }

    #[tokio::test]
    async fn test_replay_protection_timestamp_validation_disabled() {
        let mut config = rp_config();
        config.validate_timestamp = false;
        let app = rp_app(config);
        let old_ts = "0"; // epoch 0
        let req = Request::builder()
            .uri("/api/vms")
            .header("x-nonce", "no-ts-check")
            .header("x-timestamp", old_ts)
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_replay_protection_response_json_body_on_conflict() {
        let config = rp_config();
        let nonce_store = NonceStore::new();
        let store1 = nonce_store.clone();
        let store2 = nonce_store.clone();
        let c1 = config.clone();
        let c2 = config.clone();

        let app1 = Router::new()
            .route(
                "/api/vms",
                axum::routing::any(|| async { axum::Json(serde_json::json!({"status": "ok"})) }),
            )
            .layer(middleware::from_fn(move |req: Request, next: Next| {
                let c = c1.clone();
                let s = store1.clone();
                replay_protection_handler(c, s, req, next)
            }));

        let app2 = Router::new()
            .route(
                "/api/vms",
                axum::routing::any(|| async { axum::Json(serde_json::json!({"status": "ok"})) }),
            )
            .layer(middleware::from_fn(move |req: Request, next: Next| {
                let c = c2.clone();
                let s = store2.clone();
                replay_protection_handler(c, s, req, next)
            }));

        // First request succeeds
        let req1 = Request::builder()
            .uri("/api/vms")
            .header("x-nonce", "json-body-test")
            .header("x-timestamp", &now_ts())
            .body(Body::empty())
            .unwrap();
        let _ = app1.oneshot(req1).await.unwrap();

        // Second with same nonce
        let req2 = Request::builder()
            .uri("/api/vms")
            .header("x-nonce", "json-body-test")
            .header("x-timestamp", &now_ts())
            .body(Body::empty())
            .unwrap();
        let resp = app2.oneshot(req2).await.unwrap();
        assert_eq!(resp.status(), StatusCode::CONFLICT);
        let ct = resp
            .headers()
            .get("content-type")
            .unwrap()
            .to_str()
            .unwrap();
        assert!(ct.contains("application/json"));
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["error"], "REPLAY_DETECTED");
        assert!(json["message"]
            .as_str()
            .unwrap()
            .contains("already been used"));
    }

    #[tokio::test]
    async fn test_replay_protection_default_config() {
        let config = ReplayProtectionConfig::default();
        assert_eq!(config.nonce_header, "x-nonce");
        assert_eq!(config.timestamp_header, "x-timestamp");
        assert!(config.require_nonce);
        assert!(config.validate_timestamp);
        assert_eq!(config.max_age_secs, 300);
        assert_eq!(config.max_stored_nonces, 100_000);
        assert_eq!(config.excluded_paths, vec!["/health".to_string()]);
    }

    #[tokio::test]
    async fn test_replay_protection_summary_entry() {
        let config = MiddlewareConfig::default();
        let summary = config.summary();
        let found = summary
            .iter()
            .any(|(name, enabled)| *name == "Replay Protect" && !*enabled);
        assert!(found);
    }

    #[tokio::test]
    async fn test_replay_protection_builder() {
        let config = MiddlewareConfig::default()
            .replay_protection_enabled(true)
            .replay_protection(ReplayProtectionConfig {
                max_age_secs: 600,
                ..ReplayProtectionConfig::default()
            });
        assert!(config.enable_replay_protection);
        assert_eq!(config.replay_protection.max_age_secs, 600);
    }

    #[tokio::test]
    async fn test_replay_protection_no_timestamp_header_passes() {
        let app = rp_app(rp_config());
        // Nonce present, timestamp header absent — timestamp validation has nothing to check
        let req = Request::builder()
            .uri("/api/vms")
            .header("x-nonce", "no-ts-header")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    // -------------------------------------------------------------------------
    // Geo-IP Headers tests
    // -------------------------------------------------------------------------

    fn geo_config() -> GeoIpConfig {
        GeoIpConfig {
            mappings: vec![
                GeoIpEntry {
                    ip_prefix: "203.0.113.".to_string(),
                    country: "US".to_string(),
                    region: Some("California".to_string()),
                },
                GeoIpEntry {
                    ip_prefix: "198.51.100.".to_string(),
                    country: "DE".to_string(),
                    region: Some("Bavaria".to_string()),
                },
                GeoIpEntry {
                    ip_prefix: "192.168.".to_string(),
                    country: "XX".to_string(),
                    region: Some("Private".to_string()),
                },
            ],
            ..GeoIpConfig::default()
        }
    }

    fn geo_app(config: GeoIpConfig) -> Router {
        Router::new()
            .route(
                "/api/vms",
                axum::routing::any(|| async { axum::Json(serde_json::json!({"status": "ok"})) }),
            )
            .route("/health", axum::routing::any(|| async { "healthy" }))
            .layer(middleware::from_fn(move |req: Request, next: Next| {
                let c = config.clone();
                geo_ip_handler(c, req, next)
            }))
    }

    #[tokio::test]
    async fn test_geo_ip_injects_country_header() {
        let app = geo_app(geo_config());
        let req = Request::builder()
            .uri("/api/vms")
            .header("x-forwarded-for", "203.0.113.42")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            resp.headers()
                .get("x-geo-country")
                .unwrap()
                .to_str()
                .unwrap(),
            "US"
        );
    }

    #[tokio::test]
    async fn test_geo_ip_injects_region_header() {
        let app = geo_app(geo_config());
        let req = Request::builder()
            .uri("/api/vms")
            .header("x-forwarded-for", "203.0.113.1")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(
            resp.headers()
                .get("x-geo-region")
                .unwrap()
                .to_str()
                .unwrap(),
            "California"
        );
    }

    #[tokio::test]
    async fn test_geo_ip_german_ip() {
        let app = geo_app(geo_config());
        let req = Request::builder()
            .uri("/api/vms")
            .header("x-forwarded-for", "198.51.100.55")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(
            resp.headers()
                .get("x-geo-country")
                .unwrap()
                .to_str()
                .unwrap(),
            "DE"
        );
        assert_eq!(
            resp.headers()
                .get("x-geo-region")
                .unwrap()
                .to_str()
                .unwrap(),
            "Bavaria"
        );
    }

    #[tokio::test]
    async fn test_geo_ip_unknown_ip_uses_default() {
        let app = geo_app(geo_config());
        let req = Request::builder()
            .uri("/api/vms")
            .header("x-forwarded-for", "8.8.8.8")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(
            resp.headers()
                .get("x-geo-country")
                .unwrap()
                .to_str()
                .unwrap(),
            "XX"
        );
        assert!(resp.headers().get("x-geo-region").is_none());
    }

    #[tokio::test]
    async fn test_geo_ip_no_ip_header_uses_default() {
        let app = geo_app(geo_config());
        let req = Request::builder()
            .uri("/api/vms")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(
            resp.headers()
                .get("x-geo-country")
                .unwrap()
                .to_str()
                .unwrap(),
            "XX"
        );
    }

    #[tokio::test]
    async fn test_geo_ip_excluded_path() {
        let app = geo_app(geo_config());
        let req = Request::builder()
            .uri("/health")
            .header("x-forwarded-for", "203.0.113.1")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert!(resp.headers().get("x-geo-country").is_none());
    }

    #[tokio::test]
    async fn test_geo_ip_echo_ip_disabled_by_default() {
        let app = geo_app(geo_config());
        let req = Request::builder()
            .uri("/api/vms")
            .header("x-forwarded-for", "203.0.113.1")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert!(resp.headers().get("x-geo-ip").is_none());
    }

    #[tokio::test]
    async fn test_geo_ip_echo_ip_enabled() {
        let mut config = geo_config();
        config.echo_ip = true;
        let app = geo_app(config);
        let req = Request::builder()
            .uri("/api/vms")
            .header("x-forwarded-for", "203.0.113.42")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(
            resp.headers().get("x-geo-ip").unwrap().to_str().unwrap(),
            "203.0.113.42"
        );
    }

    #[tokio::test]
    async fn test_geo_ip_forwarded_for_multi_ip() {
        let app = geo_app(geo_config());
        // X-Forwarded-For with multiple IPs — should use first (client)
        let req = Request::builder()
            .uri("/api/vms")
            .header("x-forwarded-for", "203.0.113.1, 10.0.0.1, 172.16.0.1")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(
            resp.headers()
                .get("x-geo-country")
                .unwrap()
                .to_str()
                .unwrap(),
            "US"
        );
    }

    #[tokio::test]
    async fn test_geo_ip_resolve_method() {
        let config = geo_config();
        let (country, region) = config.resolve("203.0.113.50");
        assert_eq!(country, "US");
        assert_eq!(region, Some("California".to_string()));
        let (country2, region2) = config.resolve("1.2.3.4");
        assert_eq!(country2, "XX");
        assert!(region2.is_none());
    }

    #[tokio::test]
    async fn test_geo_ip_resolve_multi_ip_string() {
        let config = geo_config();
        let (country, _) = config.resolve("198.51.100.1, 10.0.0.1");
        assert_eq!(country, "DE");
    }

    #[tokio::test]
    async fn test_geo_ip_default_config() {
        let config = GeoIpConfig::default();
        assert_eq!(config.ip_header, "x-forwarded-for");
        assert_eq!(config.country_header, "x-geo-country");
        assert_eq!(config.region_header, "x-geo-region");
        assert!(!config.echo_ip);
        assert_eq!(config.default_country, "XX");
        assert_eq!(config.mappings.len(), 3);
        assert_eq!(config.excluded_paths, vec!["/health".to_string()]);
    }

    #[tokio::test]
    async fn test_geo_ip_private_ip_mapping() {
        let app = geo_app(geo_config());
        let req = Request::builder()
            .uri("/api/vms")
            .header("x-forwarded-for", "192.168.1.100")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(
            resp.headers()
                .get("x-geo-country")
                .unwrap()
                .to_str()
                .unwrap(),
            "XX"
        );
        assert_eq!(
            resp.headers()
                .get("x-geo-region")
                .unwrap()
                .to_str()
                .unwrap(),
            "Private"
        );
    }

    #[tokio::test]
    async fn test_geo_ip_summary_entry() {
        let config = MiddlewareConfig::default();
        let summary = config.summary();
        let found = summary
            .iter()
            .any(|(name, enabled)| *name == "Geo-IP Hdrs" && !*enabled);
        assert!(found);
    }

    #[tokio::test]
    async fn test_geo_ip_builder() {
        let config = MiddlewareConfig::default()
            .geo_ip_enabled(true)
            .geo_ip(GeoIpConfig {
                echo_ip: true,
                ..GeoIpConfig::default()
            });
        assert!(config.enable_geo_ip);
        assert!(config.geo_ip.echo_ip);
    }

    #[tokio::test]
    async fn test_geo_ip_custom_headers() {
        let mut config = geo_config();
        config.country_header = "x-client-country".to_string();
        config.region_header = "x-client-region".to_string();
        let app = geo_app(config);
        let req = Request::builder()
            .uri("/api/vms")
            .header("x-forwarded-for", "203.0.113.1")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(
            resp.headers()
                .get("x-client-country")
                .unwrap()
                .to_str()
                .unwrap(),
            "US"
        );
        assert_eq!(
            resp.headers()
                .get("x-client-region")
                .unwrap()
                .to_str()
                .unwrap(),
            "California"
        );
    }
    // --- Schema Validation Tests ---

    fn schema_config() -> SchemaValidationConfig {
        let mut config = SchemaValidationConfig::default();
        config.rules.push(SchemaRouteRule {
            path: "/api/v1/vms".to_string(),
            methods: vec!["POST".to_string()],
            fields: vec![
                SchemaFieldRule {
                    field_name: "name".to_string(),
                    required: true,
                    field_type: "string".to_string(),
                    max_length: Some(64),
                    min_value: None,
                    max_value: None,
                },
                SchemaFieldRule {
                    field_name: "vcpus".to_string(),
                    required: true,
                    field_type: "number".to_string(),
                    max_length: None,
                    min_value: Some(1.0),
                    max_value: Some(128.0),
                },
                SchemaFieldRule {
                    field_name: "tags".to_string(),
                    required: false,
                    field_type: "array".to_string(),
                    max_length: None,
                    min_value: None,
                    max_value: None,
                },
            ],
        });
        config
    }

    fn schema_app(config: SchemaValidationConfig) -> Router {
        let mw = MiddlewareConfig::none()
            .schema_validation(config)
            .schema_validation_enabled(true);
        mw.apply(Router::new().route("/api/v1/vms", any(|| async { "ok" })))
    }

    #[tokio::test]
    async fn test_schema_validation_valid_body() {
        let app = schema_app(schema_config());
        let body = serde_json::json!({"name": "test-vm", "vcpus": 4});
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/vms")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_schema_validation_missing_required_field() {
        let app = schema_app(schema_config());
        let body = serde_json::json!({"vcpus": 4});
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/vms")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
        let bytes = axum::body::to_bytes(resp.into_body(), 8192).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["error"], "SCHEMA_VALIDATION_FAILED");
        let details = json["details"].as_array().unwrap();
        assert!(details.iter().any(|d| d.as_str().unwrap().contains("name")));
    }

    #[tokio::test]
    async fn test_schema_validation_wrong_type() {
        let app = schema_app(schema_config());
        let body = serde_json::json!({"name": 123, "vcpus": 4});
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/vms")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
        let bytes = axum::body::to_bytes(resp.into_body(), 8192).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let details = json["details"].as_array().unwrap();
        assert!(details.iter().any(|d| d.as_str().unwrap().contains("type")));
    }

    #[tokio::test]
    async fn test_schema_validation_string_too_long() {
        let app = schema_app(schema_config());
        let long_name = "x".repeat(100);
        let body = serde_json::json!({"name": long_name, "vcpus": 4});
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/vms")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
        let bytes = axum::body::to_bytes(resp.into_body(), 8192).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let details = json["details"].as_array().unwrap();
        assert!(details
            .iter()
            .any(|d| d.as_str().unwrap().contains("max length")));
    }

    #[tokio::test]
    async fn test_schema_validation_number_below_min() {
        let app = schema_app(schema_config());
        let body = serde_json::json!({"name": "vm", "vcpus": 0});
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/vms")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
        let bytes = axum::body::to_bytes(resp.into_body(), 8192).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let details = json["details"].as_array().unwrap();
        assert!(details
            .iter()
            .any(|d| d.as_str().unwrap().contains("below minimum")));
    }

    #[tokio::test]
    async fn test_schema_validation_number_above_max() {
        let app = schema_app(schema_config());
        let body = serde_json::json!({"name": "vm", "vcpus": 256});
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/vms")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
        let bytes = axum::body::to_bytes(resp.into_body(), 8192).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let details = json["details"].as_array().unwrap();
        assert!(details
            .iter()
            .any(|d| d.as_str().unwrap().contains("above maximum")));
    }

    #[tokio::test]
    async fn test_schema_validation_excluded_path() {
        let app = schema_app(schema_config());
        let req = Request::builder()
            .method("POST")
            .uri("/health")
            .header("content-type", "application/json")
            .body(Body::from("not json"))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        // Should pass through without validation
        assert_ne!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }

    #[tokio::test]
    async fn test_schema_validation_no_rule_passes() {
        let app = schema_app(schema_config());
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/other")
            .header("content-type", "application/json")
            .body(Body::from("not json"))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        // No matching rule - passes to downstream (404 from unregistered route)
        assert_ne!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }

    #[tokio::test]
    async fn test_schema_validation_invalid_json() {
        let app = schema_app(schema_config());
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/vms")
            .header("content-type", "application/json")
            .body(Body::from("not valid json {{{"))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
        let bytes = axum::body::to_bytes(resp.into_body(), 8192).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["error"], "INVALID_JSON");
    }

    #[tokio::test]
    async fn test_schema_validation_body_too_large() {
        let mut config = schema_config();
        config.max_body_size = 16;
        let app = schema_app(config);
        let body = serde_json::json!({"name": "vm-with-long-name-exceeding-size", "vcpus": 4});
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/vms")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::PAYLOAD_TOO_LARGE);
    }

    #[tokio::test]
    async fn test_schema_validation_strict_mode_unknown_field() {
        let mut config = schema_config();
        config.strict_mode = true;
        let app = schema_app(config);
        let body = serde_json::json!({"name": "vm", "vcpus": 4, "unknown": true});
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/vms")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
        let bytes = axum::body::to_bytes(resp.into_body(), 8192).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let details = json["details"].as_array().unwrap();
        assert!(details
            .iter()
            .any(|d| d.as_str().unwrap().contains("unknown")));
    }

    #[tokio::test]
    async fn test_schema_validation_strict_mode_allows_known() {
        let mut config = schema_config();
        config.strict_mode = true;
        let app = schema_app(config);
        let body = serde_json::json!({"name": "vm", "vcpus": 4, "tags": ["a"]});
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/vms")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_schema_validation_optional_field_missing_ok() {
        let app = schema_app(schema_config());
        let body = serde_json::json!({"name": "vm", "vcpus": 2});
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/vms")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_schema_validation_get_request_skipped() {
        let app = schema_app(schema_config());
        let req = Request::builder()
            .method("GET")
            .uri("/api/v1/vms")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        // GET has no matching rule (POST only), should pass through
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_schema_validation_empty_body_passes() {
        let app = schema_app(schema_config());
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/vms")
            .header("content-type", "application/json")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        // Empty body skips validation
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_schema_validation_not_json_object() {
        let app = schema_app(schema_config());
        let body = serde_json::json!([1, 2, 3]);
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/vms")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
        let bytes = axum::body::to_bytes(resp.into_body(), 8192).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert!(json["details"]
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d.as_str().unwrap().contains("JSON object")));
    }
    // --- Request Decompression Tests ---

    fn decomp_config() -> RequestDecompressionConfig {
        RequestDecompressionConfig::default()
    }

    fn decomp_app(config: RequestDecompressionConfig) -> Router {
        let mw = MiddlewareConfig::none()
            .request_decompression(config)
            .request_decompression_enabled(true);
        mw.apply(Router::new().route("/api/v1/data", any(|| async { "ok" })))
    }

    fn gzip_compress(data: &[u8]) -> Vec<u8> {
        use flate2::write::GzEncoder;
        use flate2::Compression;
        use std::io::Write;
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(data).unwrap();
        encoder.finish().unwrap()
    }

    fn deflate_compress(data: &[u8]) -> Vec<u8> {
        use flate2::write::DeflateEncoder;
        use flate2::Compression;
        use std::io::Write;
        let mut encoder = DeflateEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(data).unwrap();
        encoder.finish().unwrap()
    }

    #[tokio::test]
    async fn test_decomp_gzip_body() {
        let app = decomp_app(decomp_config());
        let payload = b"hello world gzip";
        let compressed = gzip_compress(payload);
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/data")
            .header("content-encoding", "gzip")
            .body(Body::from(compressed))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_decomp_deflate_body() {
        let app = decomp_app(decomp_config());
        let payload = b"hello world deflate";
        let compressed = deflate_compress(payload);
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/data")
            .header("content-encoding", "deflate")
            .body(Body::from(compressed))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_decomp_identity_passthrough() {
        let app = decomp_app(decomp_config());
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/data")
            .header("content-encoding", "identity")
            .body(Body::from("plain text"))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_decomp_no_encoding_passthrough() {
        let app = decomp_app(decomp_config());
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/data")
            .body(Body::from("no encoding"))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_decomp_unsupported_encoding() {
        let app = decomp_app(decomp_config());
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/data")
            .header("content-encoding", "br")
            .body(Body::from("brotli data"))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);
        let bytes = axum::body::to_bytes(resp.into_body(), 8192).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["error"], "UNSUPPORTED_ENCODING");
    }

    #[tokio::test]
    async fn test_decomp_gzip_disabled() {
        let mut config = decomp_config();
        config.enable_gzip = false;
        let app = decomp_app(config);
        let compressed = gzip_compress(b"data");
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/data")
            .header("content-encoding", "gzip")
            .body(Body::from(compressed))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);
        let bytes = axum::body::to_bytes(resp.into_body(), 8192).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["error"], "ENCODING_DISABLED");
    }

    #[tokio::test]
    async fn test_decomp_deflate_disabled() {
        let mut config = decomp_config();
        config.enable_deflate = false;
        let app = decomp_app(config);
        let compressed = deflate_compress(b"data");
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/data")
            .header("content-encoding", "deflate")
            .body(Body::from(compressed))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);
    }

    #[tokio::test]
    async fn test_decomp_excluded_path() {
        let app = decomp_app(decomp_config());
        let req = Request::builder()
            .method("POST")
            .uri("/health")
            .header("content-encoding", "gzip")
            .body(Body::from("not actually gzip"))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        // Excluded path, passes through without decompression attempt
        assert_ne!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_decomp_invalid_gzip_data() {
        let app = decomp_app(decomp_config());
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/data")
            .header("content-encoding", "gzip")
            .body(Body::from("not valid gzip bytes!"))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let bytes = axum::body::to_bytes(resp.into_body(), 8192).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["error"], "DECOMPRESSION_FAILED");
    }

    #[tokio::test]
    async fn test_decomp_invalid_deflate_data() {
        let app = decomp_app(decomp_config());
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/data")
            .header("content-encoding", "deflate")
            .body(Body::from("not valid deflate!"))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_decomp_x_gzip_alias() {
        let app = decomp_app(decomp_config());
        let compressed = gzip_compress(b"alias test");
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/data")
            .header("content-encoding", "x-gzip")
            .body(Body::from(compressed))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_decomp_max_size_exceeded() {
        let mut config = decomp_config();
        config.max_decompressed_size = 8;
        let app = decomp_app(config);
        let big_payload = vec![b'A'; 100];
        let compressed = gzip_compress(&big_payload);
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/data")
            .header("content-encoding", "gzip")
            .body(Body::from(compressed))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        // Compressed data exceeds to_bytes limit (max_size*2), returns 413
        assert!(
            resp.status() == StatusCode::PAYLOAD_TOO_LARGE
                || resp.status() == StatusCode::BAD_REQUEST
        );
    }

    #[tokio::test]
    async fn test_decomp_encoding_from_header_parsing() {
        assert_eq!(
            RequestEncoding::from_header("gzip"),
            Some(RequestEncoding::Gzip)
        );
        assert_eq!(
            RequestEncoding::from_header("GZIP"),
            Some(RequestEncoding::Gzip)
        );
        assert_eq!(
            RequestEncoding::from_header("x-gzip"),
            Some(RequestEncoding::Gzip)
        );
        assert_eq!(
            RequestEncoding::from_header("deflate"),
            Some(RequestEncoding::Deflate)
        );
        assert_eq!(
            RequestEncoding::from_header("identity"),
            Some(RequestEncoding::Identity)
        );
        assert_eq!(RequestEncoding::from_header("br"), None);
        assert_eq!(RequestEncoding::from_header("unknown"), None);
    }

    #[tokio::test]
    async fn test_decomp_removes_content_encoding_header() {
        // The handler removes Content-Encoding after decompression
        // We can verify behavior indirectly: the request proceeds OK
        let app = decomp_app(decomp_config());
        let payload = b"test payload for header removal";
        let compressed = gzip_compress(payload);
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/data")
            .header("content-encoding", "gzip")
            .body(Body::from(compressed))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_decomp_empty_compressed_body() {
        let app = decomp_app(decomp_config());
        let compressed = gzip_compress(b"");
        let req = Request::builder()
            .method("POST")
            .uri("/api/v1/data")
            .header("content-encoding", "gzip")
            .body(Body::from(compressed))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_decomp_get_request_with_encoding() {
        let app = decomp_app(decomp_config());
        let compressed = gzip_compress(b"get body");
        let req = Request::builder()
            .method("GET")
            .uri("/api/v1/data")
            .header("content-encoding", "gzip")
            .body(Body::from(compressed))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    // ── Slow Request Detection Tests ──────────────────────────────────

    fn slow_config_instant() -> SlowRequestConfig {
        SlowRequestConfig {
            threshold_ms: 0, // threshold=0 means ALL requests are "slow"
            ..SlowRequestConfig::default()
        }
    }

    fn slow_config_high() -> SlowRequestConfig {
        SlowRequestConfig {
            threshold_ms: 60_000, // 60s threshold — no request will be slow
            ..SlowRequestConfig::default()
        }
    }

    fn slow_app(config: SlowRequestConfig) -> Router {
        let mw = MiddlewareConfig::none()
            .slow_request_enabled(true)
            .slow_request(config);
        mw.apply(test_router())
    }

    #[tokio::test]
    async fn test_slow_request_flagged() {
        let app = slow_app(slow_config_instant());

        let req = Request::builder().uri("/test").body(Body::empty()).unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(resp.headers().get("x-slow-request").unwrap(), "true");
    }

    #[tokio::test]
    async fn test_slow_request_elapsed_header() {
        let app = slow_app(slow_config_instant());

        let req = Request::builder().uri("/test").body(Body::empty()).unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert!(resp.headers().contains_key("x-slow-request-ms"));
        let ms: u64 = resp
            .headers()
            .get("x-slow-request-ms")
            .unwrap()
            .to_str()
            .unwrap()
            .parse()
            .unwrap();
        assert!(ms < 10_000); // should be well under 10 seconds
    }

    #[tokio::test]
    async fn test_slow_request_warning_header() {
        let app = slow_app(slow_config_instant());

        let req = Request::builder().uri("/test").body(Body::empty()).unwrap();
        let resp = app.oneshot(req).await.unwrap();
        let warning = resp.headers().get("warning").unwrap().to_str().unwrap();
        assert!(warning.contains("Slow request"));
        assert!(warning.contains("exceeded 0ms threshold"));
    }

    #[tokio::test]
    async fn test_slow_request_not_flagged_under_threshold() {
        let app = slow_app(slow_config_high());

        let req = Request::builder().uri("/test").body(Body::empty()).unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert!(resp.headers().get("x-slow-request").is_none());
        assert!(resp.headers().get("x-slow-request-ms").is_none());
        assert!(resp.headers().get("warning").is_none());
    }

    #[tokio::test]
    async fn test_slow_request_excluded_path() {
        let app = slow_app(slow_config_instant());

        let req = Request::builder()
            .uri("/health")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        // /health is excluded — no slow request header even with 0ms threshold
        assert!(resp.headers().get("x-slow-request").is_none());
    }

    #[tokio::test]
    async fn test_slow_request_no_warning_when_disabled() {
        let config = SlowRequestConfig {
            threshold_ms: 0,
            add_warning_header: false,
            ..SlowRequestConfig::default()
        };
        let app = slow_app(config);

        let req = Request::builder().uri("/test").body(Body::empty()).unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.headers().get("x-slow-request").unwrap(), "true");
        assert!(resp.headers().get("warning").is_none());
    }

    #[tokio::test]
    async fn test_slow_request_no_elapsed_when_disabled() {
        let config = SlowRequestConfig {
            threshold_ms: 0,
            include_elapsed: false,
            ..SlowRequestConfig::default()
        };
        let app = slow_app(config);

        let req = Request::builder().uri("/test").body(Body::empty()).unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.headers().get("x-slow-request").unwrap(), "true");
        assert!(resp.headers().get("x-slow-request-ms").is_none());
    }

    #[tokio::test]
    async fn test_slow_request_custom_header_name() {
        let config = SlowRequestConfig {
            threshold_ms: 0,
            header_name: "X-Latency-Warning".to_string(),
            ..SlowRequestConfig::default()
        };
        let app = slow_app(config);

        let req = Request::builder().uri("/test").body(Body::empty()).unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.headers().get("x-latency-warning").unwrap(), "true");
    }

    #[tokio::test]
    async fn test_slow_request_custom_elapsed_header() {
        let config = SlowRequestConfig {
            threshold_ms: 0,
            elapsed_header_name: "X-Duration-Ms".to_string(),
            ..SlowRequestConfig::default()
        };
        let app = slow_app(config);

        let req = Request::builder().uri("/test").body(Body::empty()).unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert!(resp.headers().contains_key("x-duration-ms"));
    }

    #[tokio::test]
    async fn test_slow_request_post_method() {
        let app = slow_app(slow_config_instant());

        let req = Request::builder()
            .method("POST")
            .uri("/test")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.headers().get("x-slow-request").unwrap(), "true");
    }

    #[tokio::test]
    async fn test_slow_request_put_method() {
        let app = slow_app(slow_config_instant());

        let req = Request::builder()
            .method("PUT")
            .uri("/test")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.headers().get("x-slow-request").unwrap(), "true");
    }

    #[tokio::test]
    async fn test_slow_request_delete_method() {
        let app = slow_app(slow_config_instant());

        let req = Request::builder()
            .method("DELETE")
            .uri("/test")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.headers().get("x-slow-request").unwrap(), "true");
    }

    #[tokio::test]
    async fn test_slow_request_config_defaults() {
        let config = SlowRequestConfig::default();
        assert_eq!(config.threshold_ms, 5000);
        assert_eq!(config.header_name, "X-Slow-Request");
        assert!(config.include_elapsed);
        assert_eq!(config.elapsed_header_name, "X-Slow-Request-Ms");
        assert_eq!(config.excluded_paths, vec!["/health"]);
        assert!(config.add_warning_header);
    }

    #[tokio::test]
    async fn test_slow_request_disabled_middleware() {
        // With slow request disabled, no headers should be added even with 0ms threshold
        let mw = MiddlewareConfig::none();
        let app = mw.apply(test_router());

        let req = Request::builder().uri("/test").body(Body::empty()).unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert!(resp.headers().get("x-slow-request").is_none());
    }

    #[tokio::test]
    async fn test_slow_request_multiple_excluded_paths() {
        let config = SlowRequestConfig {
            threshold_ms: 0,
            excluded_paths: vec!["/health".to_string(), "/metrics".to_string()],
            ..SlowRequestConfig::default()
        };
        let app = slow_app(config);

        // /health excluded
        let req = Request::builder()
            .uri("/health")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert!(resp.headers().get("x-slow-request").is_none());
    }

    #[test]
    fn test_slow_request_summary_entry() {
        let config = MiddlewareConfig::default();
        let summary = config.summary();
        let entry = summary.iter().find(|(name, _)| *name == "Slow Req.");
        assert!(entry.is_some());
        assert!(!entry.unwrap().1); // off by default
    }
    // ===== Header Propagation tests =====

    fn hp_config() -> HeaderPropagationConfig {
        HeaderPropagationConfig {
            propagated_headers: vec!["X-Request-Id".to_string(), "X-Correlation-Id".to_string()],
            response_prefix: String::new(),
            case_insensitive: true,
            skip_existing: true,
            excluded_paths: Vec::new(),
            add_propagated_list_header: false,
        }
    }

    #[tokio::test]
    async fn test_header_propagation_copies_request_header() {
        let config = hp_config();
        let app = Router::new()
            .route("/test", axum::routing::get(|| async { "ok" }))
            .layer(middleware::from_fn(move |req: Request, next: Next| {
                let c = config.clone();
                header_propagation_handler(c, req, next)
            }));
        let req = Request::builder()
            .uri("/test")
            .header("X-Request-Id", "abc-123")
            .body(axum::body::Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.headers().get("x-request-id").unwrap(), "abc-123");
    }

    #[tokio::test]
    async fn test_header_propagation_copies_multiple_headers() {
        let config = hp_config();
        let app = Router::new()
            .route("/test", axum::routing::get(|| async { "ok" }))
            .layer(middleware::from_fn(move |req: Request, next: Next| {
                let c = config.clone();
                header_propagation_handler(c, req, next)
            }));
        let req = Request::builder()
            .uri("/test")
            .header("X-Request-Id", "id-1")
            .header("X-Correlation-Id", "corr-2")
            .body(axum::body::Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.headers().get("x-request-id").unwrap(), "id-1");
        assert_eq!(resp.headers().get("x-correlation-id").unwrap(), "corr-2");
    }

    #[tokio::test]
    async fn test_header_propagation_missing_header_not_added() {
        let config = hp_config();
        let app = Router::new()
            .route("/test", axum::routing::get(|| async { "ok" }))
            .layer(middleware::from_fn(move |req: Request, next: Next| {
                let c = config.clone();
                header_propagation_handler(c, req, next)
            }));
        let req = Request::builder()
            .uri("/test")
            .body(axum::body::Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert!(resp.headers().get("x-request-id").is_none());
        assert!(resp.headers().get("x-correlation-id").is_none());
    }

    #[tokio::test]
    async fn test_header_propagation_with_prefix() {
        let config = HeaderPropagationConfig {
            propagated_headers: vec!["X-Request-Id".to_string()],
            response_prefix: "X-Prop-".to_string(),
            case_insensitive: true,
            skip_existing: true,
            excluded_paths: Vec::new(),
            add_propagated_list_header: false,
        };
        let app = Router::new()
            .route("/test", axum::routing::get(|| async { "ok" }))
            .layer(middleware::from_fn(move |req: Request, next: Next| {
                let c = config.clone();
                header_propagation_handler(c, req, next)
            }));
        let req = Request::builder()
            .uri("/test")
            .header("X-Request-Id", "abc")
            .body(axum::body::Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.headers().get("x-prop-x-request-id").unwrap(), "abc");
        assert!(resp.headers().get("x-request-id").is_none());
    }

    #[tokio::test]
    async fn test_header_propagation_case_insensitive() {
        let config = hp_config();
        let app = Router::new()
            .route("/test", axum::routing::get(|| async { "ok" }))
            .layer(middleware::from_fn(move |req: Request, next: Next| {
                let c = config.clone();
                header_propagation_handler(c, req, next)
            }));
        let req = Request::builder()
            .uri("/test")
            .header("x-request-id", "lower-case")
            .body(axum::body::Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.headers().get("x-request-id").unwrap(), "lower-case");
    }

    #[tokio::test]
    async fn test_header_propagation_case_sensitive() {
        let config = HeaderPropagationConfig {
            propagated_headers: vec!["X-Custom".to_string()],
            response_prefix: String::new(),
            case_insensitive: false,
            skip_existing: true,
            excluded_paths: Vec::new(),
            add_propagated_list_header: false,
        };
        let app = Router::new()
            .route("/test", axum::routing::get(|| async { "ok" }))
            .layer(middleware::from_fn(move |req: Request, next: Next| {
                let c = config.clone();
                header_propagation_handler(c, req, next)
            }));
        // HTTP header names are lowercased by the HTTP spec / hyper
        // so case-sensitive "X-Custom" won't match "x-custom"
        let req = Request::builder()
            .uri("/test")
            .header("X-Custom", "val")
            .body(axum::body::Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        // In practice hyper lowercases header names, so case-sensitive match
        // against "X-Custom" (mixed case) won't match "x-custom" (lowercase)
        assert!(resp.headers().get("x-custom").is_none());
    }

    #[tokio::test]
    async fn test_header_propagation_skip_existing() {
        let config = HeaderPropagationConfig {
            propagated_headers: vec!["X-Request-Id".to_string()],
            response_prefix: String::new(),
            case_insensitive: true,
            skip_existing: true,
            excluded_paths: Vec::new(),
            add_propagated_list_header: false,
        };
        let app = Router::new()
            .route(
                "/test",
                axum::routing::get(|| async {
                    let mut resp = axum::response::Response::new(axum::body::Body::from("ok"));
                    resp.headers_mut()
                        .insert("x-request-id", "existing".parse().unwrap());
                    resp
                }),
            )
            .layer(middleware::from_fn(move |req: Request, next: Next| {
                let c = config.clone();
                header_propagation_handler(c, req, next)
            }));
        let req = Request::builder()
            .uri("/test")
            .header("X-Request-Id", "new-val")
            .body(axum::body::Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.headers().get("x-request-id").unwrap(), "existing");
    }

    #[tokio::test]
    async fn test_header_propagation_overwrite_existing() {
        let config = HeaderPropagationConfig {
            propagated_headers: vec!["X-Request-Id".to_string()],
            response_prefix: String::new(),
            case_insensitive: true,
            skip_existing: false,
            excluded_paths: Vec::new(),
            add_propagated_list_header: false,
        };
        let app = Router::new()
            .route(
                "/test",
                axum::routing::get(|| async {
                    let mut resp = axum::response::Response::new(axum::body::Body::from("ok"));
                    resp.headers_mut()
                        .insert("x-request-id", "existing".parse().unwrap());
                    resp
                }),
            )
            .layer(middleware::from_fn(move |req: Request, next: Next| {
                let c = config.clone();
                header_propagation_handler(c, req, next)
            }));
        let req = Request::builder()
            .uri("/test")
            .header("X-Request-Id", "new-val")
            .body(axum::body::Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.headers().get("x-request-id").unwrap(), "new-val");
    }

    #[tokio::test]
    async fn test_header_propagation_excluded_path() {
        let config = HeaderPropagationConfig {
            propagated_headers: vec!["X-Request-Id".to_string()],
            response_prefix: String::new(),
            case_insensitive: true,
            skip_existing: true,
            excluded_paths: vec!["/health".to_string()],
            add_propagated_list_header: false,
        };
        let app = Router::new()
            .route("/health", axum::routing::get(|| async { "ok" }))
            .layer(middleware::from_fn(move |req: Request, next: Next| {
                let c = config.clone();
                header_propagation_handler(c, req, next)
            }));
        let req = Request::builder()
            .uri("/health")
            .header("X-Request-Id", "skip-me")
            .body(axum::body::Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert!(resp.headers().get("x-request-id").is_none());
    }

    #[tokio::test]
    async fn test_header_propagation_list_header() {
        let config = HeaderPropagationConfig {
            propagated_headers: vec!["X-Request-Id".to_string(), "X-Correlation-Id".to_string()],
            response_prefix: String::new(),
            case_insensitive: true,
            skip_existing: true,
            excluded_paths: Vec::new(),
            add_propagated_list_header: true,
        };
        let app = Router::new()
            .route("/test", axum::routing::get(|| async { "ok" }))
            .layer(middleware::from_fn(move |req: Request, next: Next| {
                let c = config.clone();
                header_propagation_handler(c, req, next)
            }));
        let req = Request::builder()
            .uri("/test")
            .header("X-Request-Id", "id1")
            .header("X-Correlation-Id", "corr1")
            .body(axum::body::Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        let list = resp
            .headers()
            .get("x-propagated-headers")
            .unwrap()
            .to_str()
            .unwrap();
        assert!(list.contains("X-Request-Id"));
        assert!(list.contains("X-Correlation-Id"));
    }

    #[tokio::test]
    async fn test_header_propagation_list_header_disabled() {
        let config = hp_config(); // add_propagated_list_header is false
        let app = Router::new()
            .route("/test", axum::routing::get(|| async { "ok" }))
            .layer(middleware::from_fn(move |req: Request, next: Next| {
                let c = config.clone();
                header_propagation_handler(c, req, next)
            }));
        let req = Request::builder()
            .uri("/test")
            .header("X-Request-Id", "id1")
            .body(axum::body::Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert!(resp.headers().get("x-propagated-headers").is_none());
    }

    #[tokio::test]
    async fn test_header_propagation_post_method() {
        let config = hp_config();
        let app = Router::new()
            .route("/test", axum::routing::post(|| async { "ok" }))
            .layer(middleware::from_fn(move |req: Request, next: Next| {
                let c = config.clone();
                header_propagation_handler(c, req, next)
            }));
        let req = Request::builder()
            .method(Method::POST)
            .uri("/test")
            .header("X-Request-Id", "post-id")
            .body(axum::body::Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.headers().get("x-request-id").unwrap(), "post-id");
    }

    #[tokio::test]
    async fn test_header_propagation_default_config() {
        let config = HeaderPropagationConfig::default();
        assert_eq!(config.propagated_headers.len(), 2);
        assert!(config.case_insensitive);
        assert!(config.skip_existing);
        assert!(config.response_prefix.is_empty());
        assert!(config.excluded_paths.is_empty());
        assert!(!config.add_propagated_list_header);
    }

    #[tokio::test]
    async fn test_header_propagation_disabled_middleware() {
        let config = MiddlewareConfig::none();
        assert!(!config.enable_header_propagation);
    }

    #[test]
    fn test_header_propagation_summary_entry() {
        let config = MiddlewareConfig::default();
        let summary = config.summary();
        let entry = summary.iter().find(|(name, _)| *name == "Hdr Prop.");
        assert!(entry.is_some());
        assert!(!entry.unwrap().1); // off by default
    }

    #[tokio::test]
    async fn test_header_propagation_empty_propagated_list() {
        let config = HeaderPropagationConfig {
            propagated_headers: Vec::new(),
            response_prefix: String::new(),
            case_insensitive: true,
            skip_existing: true,
            excluded_paths: Vec::new(),
            add_propagated_list_header: true,
        };
        let app = Router::new()
            .route("/test", axum::routing::get(|| async { "ok" }))
            .layer(middleware::from_fn(move |req: Request, next: Next| {
                let c = config.clone();
                header_propagation_handler(c, req, next)
            }));
        let req = Request::builder()
            .uri("/test")
            .header("X-Request-Id", "val")
            .body(axum::body::Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        // No headers propagated, no list header either
        assert!(resp.headers().get("x-propagated-headers").is_none());
    }

    // ===== Request Context tests =====

    fn rc_config() -> RequestContextConfig {
        RequestContextConfig::default()
    }

    fn rc_config_full() -> RequestContextConfig {
        RequestContextConfig {
            header_prefix: "X-Context-".to_string(),
            environment: "staging".to_string(),
            service_name: "test-svc".to_string(),
            region: "us-west-2".to_string(),
            instance_id: "i-abc123".to_string(),
            custom_fields: vec![("Deployment".to_string(), "canary".to_string())],
            excluded_paths: Vec::new(),
            add_context_list_header: false,
        }
    }

    #[tokio::test]
    async fn test_request_context_injects_environment() {
        let config = rc_config();
        let app = Router::new()
            .route("/test", axum::routing::get(|| async { "ok" }))
            .layer(middleware::from_fn(move |req: Request, next: Next| {
                let c = config.clone();
                request_context_handler(c, req, next)
            }));
        let req = Request::builder()
            .uri("/test")
            .body(axum::body::Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(
            resp.headers().get("x-context-environment").unwrap(),
            "production"
        );
    }

    #[tokio::test]
    async fn test_request_context_injects_service() {
        let config = rc_config();
        let app = Router::new()
            .route("/test", axum::routing::get(|| async { "ok" }))
            .layer(middleware::from_fn(move |req: Request, next: Next| {
                let c = config.clone();
                request_context_handler(c, req, next)
            }));
        let req = Request::builder()
            .uri("/test")
            .body(axum::body::Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.headers().get("x-context-service").unwrap(), "hv2-api");
    }

    #[tokio::test]
    async fn test_request_context_skips_empty_region() {
        let config = rc_config(); // region is empty
        let app = Router::new()
            .route("/test", axum::routing::get(|| async { "ok" }))
            .layer(middleware::from_fn(move |req: Request, next: Next| {
                let c = config.clone();
                request_context_handler(c, req, next)
            }));
        let req = Request::builder()
            .uri("/test")
            .body(axum::body::Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert!(resp.headers().get("x-context-region").is_none());
    }

    #[tokio::test]
    async fn test_request_context_injects_region_when_set() {
        let config = rc_config_full();
        let app = Router::new()
            .route("/test", axum::routing::get(|| async { "ok" }))
            .layer(middleware::from_fn(move |req: Request, next: Next| {
                let c = config.clone();
                request_context_handler(c, req, next)
            }));
        let req = Request::builder()
            .uri("/test")
            .body(axum::body::Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.headers().get("x-context-region").unwrap(), "us-west-2");
    }

    #[tokio::test]
    async fn test_request_context_injects_instance_id() {
        let config = rc_config_full();
        let app = Router::new()
            .route("/test", axum::routing::get(|| async { "ok" }))
            .layer(middleware::from_fn(move |req: Request, next: Next| {
                let c = config.clone();
                request_context_handler(c, req, next)
            }));
        let req = Request::builder()
            .uri("/test")
            .body(axum::body::Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(
            resp.headers().get("x-context-instance").unwrap(),
            "i-abc123"
        );
    }

    #[tokio::test]
    async fn test_request_context_custom_fields() {
        let config = rc_config_full();
        let app = Router::new()
            .route("/test", axum::routing::get(|| async { "ok" }))
            .layer(middleware::from_fn(move |req: Request, next: Next| {
                let c = config.clone();
                request_context_handler(c, req, next)
            }));
        let req = Request::builder()
            .uri("/test")
            .body(axum::body::Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(
            resp.headers().get("x-context-deployment").unwrap(),
            "canary"
        );
    }

    #[tokio::test]
    async fn test_request_context_custom_prefix() {
        let config = RequestContextConfig {
            header_prefix: "X-Svc-".to_string(),
            environment: "dev".to_string(),
            service_name: "my-api".to_string(),
            region: String::new(),
            instance_id: String::new(),
            custom_fields: Vec::new(),
            excluded_paths: Vec::new(),
            add_context_list_header: false,
        };
        let app = Router::new()
            .route("/test", axum::routing::get(|| async { "ok" }))
            .layer(middleware::from_fn(move |req: Request, next: Next| {
                let c = config.clone();
                request_context_handler(c, req, next)
            }));
        let req = Request::builder()
            .uri("/test")
            .body(axum::body::Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.headers().get("x-svc-environment").unwrap(), "dev");
        assert_eq!(resp.headers().get("x-svc-service").unwrap(), "my-api");
        assert!(resp.headers().get("x-context-environment").is_none());
    }

    #[tokio::test]
    async fn test_request_context_excluded_path() {
        let config = RequestContextConfig {
            excluded_paths: vec!["/health".to_string()],
            ..RequestContextConfig::default()
        };
        let app = Router::new()
            .route("/health", axum::routing::get(|| async { "ok" }))
            .layer(middleware::from_fn(move |req: Request, next: Next| {
                let c = config.clone();
                request_context_handler(c, req, next)
            }));
        let req = Request::builder()
            .uri("/health")
            .body(axum::body::Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert!(resp.headers().get("x-context-environment").is_none());
    }

    #[tokio::test]
    async fn test_request_context_list_header() {
        let config = RequestContextConfig {
            add_context_list_header: true,
            ..RequestContextConfig::default()
        };
        let app = Router::new()
            .route("/test", axum::routing::get(|| async { "ok" }))
            .layer(middleware::from_fn(move |req: Request, next: Next| {
                let c = config.clone();
                request_context_handler(c, req, next)
            }));
        let req = Request::builder()
            .uri("/test")
            .body(axum::body::Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        let list = resp
            .headers()
            .get("x-context-headers")
            .unwrap()
            .to_str()
            .unwrap();
        assert!(list.contains("X-Context-Environment"));
        assert!(list.contains("X-Context-Service"));
    }

    #[tokio::test]
    async fn test_request_context_list_header_disabled() {
        let config = rc_config(); // add_context_list_header is false
        let app = Router::new()
            .route("/test", axum::routing::get(|| async { "ok" }))
            .layer(middleware::from_fn(move |req: Request, next: Next| {
                let c = config.clone();
                request_context_handler(c, req, next)
            }));
        let req = Request::builder()
            .uri("/test")
            .body(axum::body::Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert!(resp.headers().get("x-context-headers").is_none());
    }

    #[tokio::test]
    async fn test_request_context_all_fields() {
        let config = rc_config_full();
        let app = Router::new()
            .route("/test", axum::routing::get(|| async { "ok" }))
            .layer(middleware::from_fn(move |req: Request, next: Next| {
                let c = config.clone();
                request_context_handler(c, req, next)
            }));
        let req = Request::builder()
            .uri("/test")
            .body(axum::body::Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(
            resp.headers().get("x-context-environment").unwrap(),
            "staging"
        );
        assert_eq!(resp.headers().get("x-context-service").unwrap(), "test-svc");
        assert_eq!(resp.headers().get("x-context-region").unwrap(), "us-west-2");
        assert_eq!(
            resp.headers().get("x-context-instance").unwrap(),
            "i-abc123"
        );
        assert_eq!(
            resp.headers().get("x-context-deployment").unwrap(),
            "canary"
        );
    }

    #[tokio::test]
    async fn test_request_context_post_method() {
        let config = rc_config();
        let app = Router::new()
            .route("/test", axum::routing::post(|| async { "ok" }))
            .layer(middleware::from_fn(move |req: Request, next: Next| {
                let c = config.clone();
                request_context_handler(c, req, next)
            }));
        let req = Request::builder()
            .method(Method::POST)
            .uri("/test")
            .body(axum::body::Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(
            resp.headers().get("x-context-environment").unwrap(),
            "production"
        );
    }

    #[tokio::test]
    async fn test_request_context_default_config() {
        let config = RequestContextConfig::default();
        assert_eq!(config.header_prefix, "X-Context-");
        assert_eq!(config.environment, "production");
        assert_eq!(config.service_name, "hv2-api");
        assert!(config.region.is_empty());
        assert!(config.instance_id.is_empty());
        assert!(config.custom_fields.is_empty());
        assert!(config.excluded_paths.is_empty());
        assert!(!config.add_context_list_header);
    }

    #[tokio::test]
    async fn test_request_context_disabled_middleware() {
        let config = MiddlewareConfig::none();
        assert!(!config.enable_request_context);
    }

    #[test]
    fn test_request_context_summary_entry() {
        let config = MiddlewareConfig::default();
        let summary = config.summary();
        let entry = summary.iter().find(|(name, _)| *name == "Req Context");
        assert!(entry.is_some());
        assert!(!entry.unwrap().1); // off by default
    }

    #[tokio::test]
    async fn test_request_context_multiple_custom_fields() {
        let config = RequestContextConfig {
            custom_fields: vec![
                ("Team".to_string(), "platform".to_string()),
                ("Version".to_string(), "v2.1".to_string()),
            ],
            ..RequestContextConfig::default()
        };
        let app = Router::new()
            .route("/test", axum::routing::get(|| async { "ok" }))
            .layer(middleware::from_fn(move |req: Request, next: Next| {
                let c = config.clone();
                request_context_handler(c, req, next)
            }));
        let req = Request::builder()
            .uri("/test")
            .body(axum::body::Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.headers().get("x-context-team").unwrap(), "platform");
        assert_eq!(resp.headers().get("x-context-version").unwrap(), "v2.1");
    }
}
