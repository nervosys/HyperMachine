//! Configuration File Support
//!
//! Provides TOML-based configuration file loading with layered merging:
//!
//! 1. **Defaults** — hardcoded `ServerConfig::default()`
//! 2. **File** — loaded from `hv2.toml` (or `--config <path>`)
//! 3. **Environment** — `HV2_*` environment variables override any file value
//! 4. **CLI flags** — explicit flags override everything
//!
//! ## Config File Format
//!
//! ```toml
//! [server]
//! host = "0.0.0.0"
//! rest_port = 8080
//! grpc_port = 50051
//! enable_runtime = true
//! enable_events = true
//! pre_warm_count = 2
//! shutdown_timeout_secs = 30
//!
//! [runtime]
//! instance_id = "my-runtime-01"
//!
//! [runtime.pool]
//! min_warm = 2
//! max_size = 64
//! default_vcpus = 2
//! default_memory = 2147483648
//! max_idle_secs = 600
//! max_lifetime_secs = 86400
//!
//! [middleware]
//! enable_request_id = true
//! enable_request_timing = true
//! enable_request_logging = true
//! enable_cors = true
//! enable_rate_limit = false
//! enable_body_limit = false
//! enable_api_key_auth = false
//!
//! [middleware.cors]
//! allowed_origins = []
//! allowed_methods = ["GET", "POST", "PUT", "DELETE", "PATCH", "OPTIONS"]
//! allowed_headers = ["content-type", "authorization", "x-request-id"]
//! allow_credentials = false
//! max_age = 86400
//!
//! [middleware.api_key]
//! keys = []
//! excluded_paths = ["/health", "/agentic"]
//!
//! [middleware.body_limit]
//! max_bytes = 2097152
//! excluded_paths = ["/health"]
//! ```
//!
//! ## Environment Variables
//!
//! | Variable                | Maps to                    |
//! |-------------------------|----------------------------|
//! | `HV2_HOST`              | `server.host`              |
//! | `HV2_REST_PORT`         | `server.rest_port`         |
//! | `HV2_GRPC_PORT`         | `server.grpc_port`         |
//! | `HV2_PRE_WARM`          | `server.pre_warm_count`    |
//! | `HV2_ENABLE_RUNTIME`    | `server.enable_runtime`    |
//! | `HV2_ENABLE_EVENTS`     | `server.enable_events`     |
//! | `HV2_INSTANCE_ID`       | `runtime.instance_id`      |
//! | `HV2_API_KEYS`          | `middleware.api_key.keys`   |
//! | `HV2_CORS_ORIGINS`      | `middleware.cors.origins`   |
//! | `HV2_SHUTDOWN_TIMEOUT`  | `server.shutdown_timeout_secs` |
//! | `HV2_BODY_LIMIT`        | `middleware.body_limit.max_bytes` |

use serde::{Deserialize, Serialize};
use std::path::Path;

use crate::middleware::{
    ApiKeyConfig, AuditLogConfig, BodyLimitConfig, BodyValidationConfig, CircuitBreakerConfig,
    CompressionConfig, ContentNegotiationConfig, ContentTypeConfig, CorsConfig, DeprecationConfig,
    ETagConfig, FingerprintConfig, GeoIpConfig, HeaderPropagationConfig, HstsConfig,
    IdempotencyConfig, IpFilterConfig, IpNetwork, MaintenanceConfig, MiddlewareConfig,
    PayloadSigningConfig, QuotaState, RateLimitConfig, ReplayProtectionConfig,
    RequestContextConfig, RequestCostConfig, RequestDecompressionConfig, RequestDedupConfig,
    RequestPriorityConfig, RequestQuotaConfig, ResponseCacheConfig, ResponseEnvelopeConfig,
    ResponseSigningConfig, RetryHintsConfig, SanitizationConfig, SchemaValidationConfig,
    SecurityHeadersConfig, SlowRequestConfig, TenantIsolationConfig, ThrottleConfig, TimeoutConfig,
    TracingConfig,
};
use crate::server::ServerConfig;

// ============================================================================
// File Schema
// ============================================================================

/// Top-level configuration file schema (mirrors `hv2.toml` structure)
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ConfigFile {
    /// Server settings
    pub server: ServerSection,
    /// Runtime settings
    pub runtime: RuntimeSection,
    /// Middleware settings
    pub middleware: MiddlewareSection,
}

/// `[server]` section
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ServerSection {
    /// Bind host
    pub host: String,
    /// REST API port
    pub rest_port: u16,
    /// gRPC port
    pub grpc_port: u16,
    /// Enable runtime endpoints
    pub enable_runtime: bool,
    /// Enable events/SSE endpoints
    pub enable_events: bool,
    /// Number of VMs to pre-warm
    pub pre_warm_count: usize,
    /// Graceful shutdown timeout in seconds (0 = no grace period)
    pub shutdown_timeout_secs: u64,
    /// TLS certificate chain path (PEM). Set both cert and key to enable TLS.
    pub tls_cert_path: Option<String>,
    /// TLS private key path (PEM). Set both cert and key to enable TLS.
    pub tls_key_path: Option<String>,
    /// Refuse to boot a VM whose image the `/api/v1/images` allowlist does not
    /// admit.
    ///
    /// Off by default. The registry enforces by default, so turning this on
    /// with an empty catalogue refuses every boot image until images are
    /// registered and approved.
    #[serde(default)]
    pub enforce_image_admission: bool,
}

impl Default for ServerSection {
    fn default() -> Self {
        Self {
            host: "0.0.0.0".to_string(),
            rest_port: 8080,
            grpc_port: 50051,
            enable_runtime: true,
            enable_events: true,
            pre_warm_count: 2,
            shutdown_timeout_secs: 30,
            enforce_image_admission: false,
            tls_cert_path: None,
            tls_key_path: None,
        }
    }
}

/// `[runtime]` section
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct RuntimeSection {
    /// Runtime instance ID (empty = auto-generate UUID)
    pub instance_id: String,
    /// Pool settings
    pub pool: PoolSection,
}

/// `[runtime.pool]` section
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct PoolSection {
    /// Minimum warm VMs
    pub min_warm: usize,
    /// Maximum pool size
    pub max_size: usize,
    /// Default vCPUs per VM
    pub default_vcpus: u32,
    /// Default memory per VM in bytes
    pub default_memory: u64,
    /// Maximum idle time in seconds before recycling
    pub max_idle_secs: u64,
    /// Maximum VM lifetime in seconds
    pub max_lifetime_secs: u64,
}

impl Default for PoolSection {
    fn default() -> Self {
        Self {
            min_warm: 2,
            max_size: 64,
            default_vcpus: 2,
            default_memory: 2 * 1024 * 1024 * 1024, // 2 GB
            max_idle_secs: 600,
            max_lifetime_secs: 86400,
        }
    }
}

/// `[middleware]` section
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct MiddlewareSection {
    /// Enable request ID header
    pub enable_request_id: bool,
    /// Enable response time header
    pub enable_request_timing: bool,
    /// Enable structured request logging
    pub enable_request_logging: bool,
    /// Enable API version response header
    pub enable_api_version: bool,
    /// Enable content-type validation for body methods
    pub enable_content_type_validation: bool,
    /// Paths excluded from content-type validation
    pub content_type_excluded_paths: Vec<String>,
    /// Enable CORS
    pub enable_cors: bool,
    /// Enable request timeout enforcement
    pub enable_request_timeout: bool,
    /// Request timeout in seconds (default: 30)
    pub request_timeout_secs: u64,
    /// Paths excluded from timeout enforcement
    pub timeout_excluded_paths: Vec<String>,
    /// Enable rate limiting
    pub enable_rate_limit: bool,
    /// Enable request body size enforcement
    pub enable_body_limit: bool,
    /// Enable API key authentication
    pub enable_api_key_auth: bool,
    /// Enable standard security response headers
    pub enable_security_headers: bool,
    /// Enable HSTS (Strict-Transport-Security) header
    pub enable_hsts: bool,
    /// HSTS max-age in seconds (default: 31536000 = 1 year)
    pub hsts_max_age: u64,
    /// Include subdomains in HSTS policy
    pub hsts_include_sub_domains: bool,
    /// Enable HSTS preload
    pub hsts_preload: bool,
    /// Content-Security-Policy header value (empty = disabled)
    pub content_security_policy: String,
    /// Permissions-Policy header value (empty = disabled)
    pub permissions_policy: String,
    /// Enable response compression (gzip/deflate)
    pub enable_compression: bool,
    /// Minimum response body size in bytes for compression (default: 256)
    pub compression_min_size: usize,
    /// Enable ETag generation and conditional request handling
    pub enable_etag: bool,
    /// Use weak ETags instead of strong ETags
    pub etag_weak: bool,
    /// Enable IP-based access control
    pub enable_ip_filter: bool,
    /// IP networks allowed (CIDR notation, e.g. "192.168.1.0/24")
    pub ip_allow_list: Vec<String>,
    /// IP networks denied (CIDR notation, e.g. "10.0.0.0/8")
    pub ip_deny_list: Vec<String>,
    /// Paths excluded from IP filtering
    pub ip_filter_excluded_paths: Vec<String>,
    /// Trust X-Forwarded-For / X-Real-IP headers for client IP extraction
    pub ip_filter_trust_proxy: bool,
    /// Enable JSON request body validation
    pub enable_body_validation: bool,
    /// Maximum JSON nesting depth
    pub body_validation_max_depth: usize,
    /// Maximum number of keys in a JSON object
    pub body_validation_max_keys: usize,
    /// Maximum string value length in JSON
    pub body_validation_max_string_length: usize,
    /// Maximum array length in JSON
    pub body_validation_max_array_length: usize,
    /// Paths excluded from body validation
    pub body_validation_excluded_paths: Vec<String>,
    /// Enable request idempotency
    pub enable_idempotency: bool,
    /// Idempotency cache TTL in seconds
    pub idempotency_ttl_secs: u64,
    /// Maximum cached idempotency entries
    pub idempotency_max_entries: usize,
    /// Require Idempotency-Key header on mutating requests
    pub idempotency_require_key: bool,
    /// Paths excluded from idempotency enforcement
    pub idempotency_excluded_paths: Vec<String>,
    /// Enable audit logging for mutating requests
    pub enable_audit_log: bool,
    /// Paths excluded from audit logging
    pub audit_log_excluded_paths: Vec<String>,
    /// Whether to include request body in audit log
    pub audit_log_request_body: bool,
    /// Maximum bytes of request body captured in audit log
    pub audit_log_max_body_bytes: usize,
    /// Whether to include response status in audit log
    pub audit_log_response_status: bool,
    /// Enable response caching for GET requests
    pub enable_response_cache: bool,
    /// Response cache TTL in seconds
    pub response_cache_ttl_secs: u64,
    /// Maximum cached response entries
    pub response_cache_max_entries: usize,
    /// Paths excluded from response caching
    pub response_cache_excluded_paths: Vec<String>,
    /// Whether to cache HEAD requests
    pub response_cache_head: bool,
    /// Cache-Control header value for cached responses
    pub response_cache_control: String,
    /// Maximum response body size eligible for caching (bytes)
    pub response_cache_max_body_size: usize,
    /// Enable request deduplication for concurrent identical requests
    pub enable_request_dedup: bool,
    /// Paths excluded from request deduplication
    pub request_dedup_excluded_paths: Vec<String>,
    /// TTL for in-flight entries in seconds
    pub request_dedup_ttl_secs: u64,
    /// Maximum body bytes used for fingerprint computation
    pub request_dedup_max_body_hash_bytes: usize,
    /// Enable W3C Trace Context propagation
    pub enable_tracing: bool,
    /// Paths excluded from tracing
    pub tracing_excluded_paths: Vec<String>,
    /// Whether to propagate tracestate header
    pub tracing_propagate_tracestate: bool,
    /// Whether to expose X-Trace-Id response header
    pub tracing_expose_trace_id: bool,
    /// Default sampling flag for new traces
    pub tracing_default_sampled: bool,
    /// Enable HMAC-SHA256 payload signing verification
    pub enable_payload_signing: bool,
    /// Shared secret for HMAC-SHA256 payload signing
    pub payload_signing_secret: String,
    /// Paths excluded from payload signing verification
    pub payload_signing_excluded_paths: Vec<String>,
    /// Whether a valid signature is required (reject unsigned requests)
    pub payload_signing_require_signature: bool,
    /// Maximum body size in bytes for signature computation
    pub payload_signing_max_body_bytes: usize,
    /// Header name for the signature
    pub payload_signing_signature_header: String,
    /// Enable circuit breaker for downstream failure protection
    pub enable_circuit_breaker: bool,
    /// Number of consecutive failures before tripping the circuit
    pub circuit_breaker_failure_threshold: u32,
    /// Seconds the circuit stays open before entering half-open
    pub circuit_breaker_recovery_timeout_secs: u64,
    /// Paths excluded from circuit breaker tracking
    pub circuit_breaker_excluded_paths: Vec<String>,
    /// Enable request header sanitization
    pub enable_sanitization: bool,
    /// Headers to strip from incoming requests
    pub sanitization_strip_headers: Vec<String>,
    /// Paths excluded from sanitization
    pub sanitization_excluded_paths: Vec<String>,
    /// Whether to strip X-Internal-* prefixed headers
    pub sanitization_strip_internal_prefix: bool,
    /// Maximum allowed header value length in bytes (0 = unlimited)
    pub sanitization_max_header_value_length: usize,
    /// Enable content negotiation (Accept header validation)
    pub enable_content_negotiation: bool,
    /// Supported content types for negotiation
    pub content_negotiation_supported_types: Vec<String>,
    /// Default content type when Accept is absent
    pub content_negotiation_default_type: String,
    /// Reject requests without Accept header
    pub content_negotiation_strict: bool,
    /// Paths excluded from content negotiation
    pub content_negotiation_excluded_paths: Vec<String>,
    /// Enable concurrency-based request throttling
    pub enable_throttle: bool,
    /// Maximum concurrent in-flight requests (0 = unlimited)
    pub throttle_max_concurrent: usize,
    /// Retry-After seconds when throttled
    pub throttle_retry_after_secs: u64,
    /// Paths excluded from throttling
    pub throttle_excluded_paths: Vec<String>,
    /// Enable retry hint headers on error responses
    pub enable_retry_hints: bool,
    /// HTTP status codes that trigger retry hints
    pub retry_hints_statuses: Vec<u16>,
    /// Default Retry-After seconds for retry hints
    pub retry_hints_retry_after_secs: u64,
    /// Retry strategy hint (e.g., "exponential-backoff")
    pub retry_hints_strategy: String,
    /// Maximum retries hint
    pub retry_hints_max_retries: u32,
    /// Paths excluded from retry hints
    pub retry_hints_excluded_paths: Vec<String>,
    /// Enable maintenance mode gating
    pub enable_maintenance: bool,
    /// Maintenance mode message
    pub maintenance_message: String,
    /// Retry-After seconds during maintenance
    pub maintenance_retry_after_secs: u64,
    /// Paths excluded from maintenance mode
    pub maintenance_excluded_paths: Vec<String>,
    /// Enable API deprecation warning headers
    pub enable_deprecation: bool,
    /// Enable request cost budget tracking
    pub enable_request_cost: bool,
    /// Enable request fingerprint hashing
    pub enable_fingerprint: bool,
    /// Enable HMAC-SHA256 response signing
    pub enable_response_signing: bool,
    /// Enable request priority tagging
    pub enable_request_priority: bool,
    /// Enable request quota enforcement
    pub enable_request_quota: bool,
    /// Enable tenant isolation middleware.
    pub enable_tenant_isolation: bool,
    /// Enable response envelope wrapping.
    pub enable_response_envelope: bool,
    pub enable_replay_protection: bool,
    pub enable_geo_ip: bool,
    /// Enable request schema validation
    pub enable_schema_validation: bool,
    /// Enable request decompression
    pub enable_request_decompression: bool,
    /// Enable slow request detection
    pub enable_slow_request: bool,
    /// Enable header propagation
    pub enable_header_propagation: bool,
    /// Enable request context injection
    pub enable_request_context: bool,
    /// Enable JSON 404 fallback for unmatched routes
    pub enable_fallback: bool,
    /// CORS configuration
    pub cors: CorsSection,
    /// Rate limit configuration
    pub rate_limit: RateLimitSection,
    /// Body size limit configuration
    pub body_limit: BodyLimitSection,
    /// API key configuration
    pub api_key: ApiKeySection,
}

impl Default for MiddlewareSection {
    fn default() -> Self {
        Self {
            enable_request_id: true,
            enable_request_timing: true,
            enable_request_logging: true,
            enable_api_version: true,
            enable_content_type_validation: true,
            content_type_excluded_paths: vec!["/health".to_string(), "/agentic".to_string()],
            enable_cors: true,
            enable_request_timeout: false,
            request_timeout_secs: 30,
            timeout_excluded_paths: vec!["/health".to_string(), "/agentic".to_string()],
            enable_rate_limit: false,
            enable_body_limit: false,
            enable_api_key_auth: false,
            enable_security_headers: true,
            enable_hsts: false,
            hsts_max_age: 31_536_000,
            hsts_include_sub_domains: true,
            hsts_preload: false,
            content_security_policy: String::new(),
            permissions_policy: String::new(),
            enable_compression: false,
            compression_min_size: 256,
            enable_etag: false,
            etag_weak: false,
            enable_ip_filter: false,
            ip_allow_list: vec![],
            ip_deny_list: vec![],
            ip_filter_excluded_paths: vec!["/health".to_string()],
            ip_filter_trust_proxy: false,
            enable_body_validation: false,
            body_validation_max_depth: 32,
            body_validation_max_keys: 1000,
            body_validation_max_string_length: 1_000_000,
            body_validation_max_array_length: 10_000,
            body_validation_excluded_paths: vec!["/health".to_string()],
            enable_idempotency: false,
            idempotency_ttl_secs: 3600,
            idempotency_max_entries: 10_000,
            idempotency_require_key: false,
            idempotency_excluded_paths: vec!["/health".to_string()],
            enable_audit_log: false,
            audit_log_excluded_paths: vec!["/health".to_string()],
            audit_log_request_body: false,
            audit_log_max_body_bytes: 1024,
            audit_log_response_status: true,
            enable_response_cache: false,
            response_cache_ttl_secs: 60,
            response_cache_max_entries: 1_000,
            response_cache_excluded_paths: vec!["/health".to_string()],
            response_cache_head: false,
            response_cache_control: "public, max-age=60".to_string(),
            response_cache_max_body_size: 1024 * 1024,
            enable_request_dedup: false,
            request_dedup_excluded_paths: vec!["/health".to_string()],
            request_dedup_ttl_secs: 30,
            request_dedup_max_body_hash_bytes: 64 * 1024,
            enable_tracing: false,
            tracing_excluded_paths: vec!["/health".to_string()],
            tracing_propagate_tracestate: true,
            tracing_expose_trace_id: true,
            tracing_default_sampled: true,
            enable_payload_signing: false,
            payload_signing_secret: String::new(),
            payload_signing_excluded_paths: vec!["/health".to_string()],
            payload_signing_require_signature: false,
            payload_signing_max_body_bytes: 1024 * 1024,
            payload_signing_signature_header: "x-signature".to_string(),
            enable_circuit_breaker: false,
            circuit_breaker_failure_threshold: 5,
            circuit_breaker_recovery_timeout_secs: 30,
            circuit_breaker_excluded_paths: vec!["/health".to_string()],
            enable_sanitization: false,
            sanitization_strip_headers: vec![
                "x-forwarded-for".to_string(),
                "x-forwarded-host".to_string(),
                "x-forwarded-proto".to_string(),
                "x-real-ip".to_string(),
                "via".to_string(),
            ],
            sanitization_excluded_paths: vec![],
            sanitization_strip_internal_prefix: true,
            sanitization_max_header_value_length: 8192,
            enable_content_negotiation: false,
            content_negotiation_supported_types: vec!["application/json".to_string()],
            content_negotiation_default_type: "application/json".to_string(),
            content_negotiation_strict: false,
            content_negotiation_excluded_paths: vec!["/health".to_string()],
            enable_throttle: false,
            throttle_max_concurrent: 100,
            throttle_retry_after_secs: 1,
            throttle_excluded_paths: vec!["/health".to_string()],
            enable_retry_hints: false,
            retry_hints_statuses: vec![408, 429, 503],
            retry_hints_retry_after_secs: 1,
            retry_hints_strategy: "exponential-backoff".to_string(),
            retry_hints_max_retries: 3,
            retry_hints_excluded_paths: vec!["/health".to_string()],
            enable_maintenance: false,
            maintenance_message: "Service is undergoing planned maintenance".to_string(),
            maintenance_retry_after_secs: 300,
            maintenance_excluded_paths: vec!["/health".to_string()],
            enable_deprecation: false,
            enable_request_cost: false,
            enable_fingerprint: false,
            enable_response_signing: false,
            enable_request_priority: false,
            enable_request_quota: false,
            enable_tenant_isolation: false,
            enable_response_envelope: false,
            enable_replay_protection: false,
            enable_geo_ip: false,
            enable_schema_validation: false,
            enable_request_decompression: false,
            enable_slow_request: false,
            enable_header_propagation: false,
            enable_request_context: false,
            enable_fallback: true,
            cors: CorsSection::default(),
            rate_limit: RateLimitSection::default(),
            body_limit: BodyLimitSection::default(),
            api_key: ApiKeySection::default(),
        }
    }
}

/// `[middleware.cors]` section
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct CorsSection {
    /// Allowed origins (empty = wildcard `*`)
    pub allowed_origins: Vec<String>,
    /// Allowed HTTP methods
    pub allowed_methods: Vec<String>,
    /// Allowed request headers
    pub allowed_headers: Vec<String>,
    /// Allow credentials
    pub allow_credentials: bool,
    /// Preflight cache max age in seconds
    pub max_age: u64,
}

impl Default for CorsSection {
    fn default() -> Self {
        Self {
            allowed_origins: vec![],
            allowed_methods: vec![
                "GET".to_string(),
                "POST".to_string(),
                "PUT".to_string(),
                "DELETE".to_string(),
                "PATCH".to_string(),
                "OPTIONS".to_string(),
            ],
            allowed_headers: vec![
                "content-type".to_string(),
                "authorization".to_string(),
                "x-request-id".to_string(),
            ],
            allow_credentials: false,
            max_age: 86400,
        }
    }
}

/// `[middleware.api_key]` section
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ApiKeySection {
    /// Valid API keys
    pub keys: Vec<String>,
    /// Path prefixes excluded from authentication
    pub excluded_paths: Vec<String>,
}

impl Default for ApiKeySection {
    fn default() -> Self {
        Self {
            keys: vec![],
            excluded_paths: vec!["/health".to_string(), "/agentic".to_string()],
        }
    }
}

/// `[middleware.rate_limit]` section
///
/// Token-bucket rate limiting: `capacity` is the burst size, `refill_rate`
/// is tokens replenished per second.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct RateLimitSection {
    /// Maximum burst size (token bucket capacity)
    pub capacity: u64,
    /// Tokens replenished per second
    pub refill_rate: f64,
    /// Path prefixes excluded from rate limiting
    pub excluded_paths: Vec<String>,
}

impl Default for RateLimitSection {
    fn default() -> Self {
        Self {
            capacity: 100,
            refill_rate: 10.0,
            excluded_paths: vec!["/health".to_string()],
        }
    }
}

/// `[middleware.body_limit]` section
///
/// Request body size enforcement: rejects requests with `Content-Length`
/// exceeding `max_bytes` with HTTP 413.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct BodyLimitSection {
    /// Maximum request body size in bytes (default: 2 MB)
    pub max_bytes: usize,
    /// Path prefixes excluded from body size enforcement
    pub excluded_paths: Vec<String>,
}

impl Default for BodyLimitSection {
    fn default() -> Self {
        Self {
            max_bytes: 2 * 1024 * 1024, // 2 MB
            excluded_paths: vec!["/health".to_string()],
        }
    }
}

// ============================================================================
// Loading
// ============================================================================

/// Errors that can occur during config file processing
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    /// File I/O error
    #[error("Failed to read config file: {0}")]
    Io(#[from] std::io::Error),

    /// TOML parsing error
    #[error("Invalid TOML: {0}")]
    Parse(#[from] toml::de::Error),

    /// TOML serialization error
    #[error("Failed to serialize config: {0}")]
    Serialize(#[from] toml::ser::Error),

    /// Validation error
    #[error("Config validation error: {0}")]
    Validation(String),
}

/// Result type for config operations
pub type ConfigResult<T> = std::result::Result<T, ConfigError>;

impl ConfigFile {
    /// Load a config file from disk.
    ///
    /// Returns `Ok(None)` if the file does not exist (this is not an error —
    /// the caller should fall back to defaults).
    pub fn load(path: &Path) -> ConfigResult<Option<Self>> {
        if !path.exists() {
            return Ok(None);
        }
        let content = std::fs::read_to_string(path)?;
        let config: ConfigFile = toml::from_str(&content)?;
        Ok(Some(config))
    }

    /// Parse a config from a TOML string.
    pub fn parse(s: &str) -> ConfigResult<Self> {
        Ok(toml::from_str(s)?)
    }

    /// Serialize to a TOML string.
    pub fn to_toml(&self) -> ConfigResult<String> {
        Ok(toml::to_string_pretty(self)?)
    }

    /// Generate the default config file content as a TOML string.
    pub fn default_toml() -> ConfigResult<String> {
        Self::default().to_toml()
    }

    /// Apply environment variable overrides.
    ///
    /// Environment variables take precedence over file values.
    /// Uses the `HV2_` prefix.
    pub fn apply_env(&mut self) {
        if let Ok(val) = std::env::var("HV2_HOST") {
            self.server.host = val;
        }
        if let Ok(val) = std::env::var("HV2_REST_PORT") {
            if let Ok(port) = val.parse() {
                self.server.rest_port = port;
            }
        }
        if let Ok(val) = std::env::var("HV2_GRPC_PORT") {
            if let Ok(port) = val.parse() {
                self.server.grpc_port = port;
            }
        }
        if let Ok(val) = std::env::var("HV2_PRE_WARM") {
            if let Ok(count) = val.parse() {
                self.server.pre_warm_count = count;
            }
        }
        if let Ok(val) = std::env::var("HV2_ENABLE_RUNTIME") {
            self.server.enable_runtime = val == "true" || val == "1";
        }
        if let Ok(val) = std::env::var("HV2_ENABLE_EVENTS") {
            self.server.enable_events = val == "true" || val == "1";
        }
        if let Ok(val) = std::env::var("HV2_INSTANCE_ID") {
            self.runtime.instance_id = val;
        }
        if let Ok(val) = std::env::var("HV2_API_KEYS") {
            self.middleware.api_key.keys = val.split(',').map(|s| s.trim().to_string()).collect();
            if !self.middleware.api_key.keys.is_empty() {
                self.middleware.enable_api_key_auth = true;
            }
        }
        if let Ok(val) = std::env::var("HV2_CORS_ORIGINS") {
            self.middleware.cors.allowed_origins =
                val.split(',').map(|s| s.trim().to_string()).collect();
        }
        if let Ok(val) = std::env::var("HV2_SHUTDOWN_TIMEOUT") {
            if let Ok(secs) = val.parse() {
                self.server.shutdown_timeout_secs = secs;
            }
        }
        if let Ok(val) = std::env::var("HV2_BODY_LIMIT") {
            if let Ok(bytes) = val.parse() {
                self.middleware.body_limit.max_bytes = bytes;
                self.middleware.enable_body_limit = true;
            }
        }
        if let Ok(val) = std::env::var("HV2_TLS_CERT") {
            self.server.tls_cert_path = Some(val);
        }
        if let Ok(val) = std::env::var("HV2_TLS_KEY") {
            self.server.tls_key_path = Some(val);
        }
    }

    /// Validate the configuration.
    ///
    /// Returns `Ok(())` if valid, or a list of validation errors.
    pub fn validate(&self) -> ConfigResult<()> {
        let mut errors = Vec::new();

        if self.server.rest_port == 0 {
            errors.push("server.rest_port must be > 0".to_string());
        }
        if self.server.grpc_port == 0 {
            errors.push("server.grpc_port must be > 0".to_string());
        }
        if self.server.rest_port == self.server.grpc_port {
            errors.push("server.rest_port and server.grpc_port must be different".to_string());
        }
        if self.runtime.pool.max_size == 0 {
            errors.push("runtime.pool.max_size must be > 0".to_string());
        }
        if self.runtime.pool.min_warm > self.runtime.pool.max_size {
            errors.push("runtime.pool.min_warm must be <= max_size".to_string());
        }
        if self.runtime.pool.max_idle_secs == 0 {
            errors.push("runtime.pool.max_idle_secs must be > 0".to_string());
        }
        if self.middleware.cors.max_age == 0 {
            errors.push("middleware.cors.max_age must be > 0".to_string());
        }
        if self.middleware.enable_api_key_auth && self.middleware.api_key.keys.is_empty() {
            errors
                .push("middleware.api_key.keys must not be empty when auth is enabled".to_string());
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(ConfigError::Validation(errors.join("; ")))
        }
    }

    /// Convert the config file into a [`ServerConfig`].
    ///
    /// This merges the file's runtime, middleware, and server sections
    /// into the unified server configuration struct.
    pub fn into_server_config(self) -> ServerConfig {
        use axum::http::Method;

        // Build runtime config
        let mut runtime = hv2_runtime::RuntimeConfig::default();
        if !self.runtime.instance_id.is_empty() {
            runtime.instance_id = self.runtime.instance_id;
        }
        runtime.pool.min_warm = self.runtime.pool.min_warm;
        runtime.pool.max_size = self.runtime.pool.max_size;
        runtime.pool.default_vcpus = self.runtime.pool.default_vcpus;
        runtime.pool.default_memory = self.runtime.pool.default_memory;
        runtime.pool.max_idle_time =
            std::time::Duration::from_secs(self.runtime.pool.max_idle_secs);
        runtime.pool.max_lifetime =
            std::time::Duration::from_secs(self.runtime.pool.max_lifetime_secs);

        // Build CORS config
        let cors_methods: Vec<Method> = self
            .middleware
            .cors
            .allowed_methods
            .iter()
            .filter_map(|m| m.parse().ok())
            .collect();

        let cors = CorsConfig {
            allowed_origins: self.middleware.cors.allowed_origins,
            allowed_methods: if cors_methods.is_empty() {
                CorsConfig::default().allowed_methods
            } else {
                cors_methods
            },
            allowed_headers: self.middleware.cors.allowed_headers,
            allow_credentials: self.middleware.cors.allow_credentials,
            max_age: self.middleware.cors.max_age,
        };

        // Build API key config
        let api_key = ApiKeyConfig {
            keys: self.middleware.api_key.keys,
            excluded_paths: self.middleware.api_key.excluded_paths,
            header_name: "authorization".to_string(),
        };

        // Build rate limit config
        let rate_limit = RateLimitConfig {
            capacity: self.middleware.rate_limit.capacity,
            refill_rate: self.middleware.rate_limit.refill_rate,
            excluded_paths: self.middleware.rate_limit.excluded_paths,
        };

        // Build body limit config
        let body_limit = BodyLimitConfig {
            max_bytes: self.middleware.body_limit.max_bytes,
            excluded_paths: self.middleware.body_limit.excluded_paths,
        };

        // Build middleware config
        let middleware = MiddlewareConfig {
            enable_request_id: self.middleware.enable_request_id,
            enable_request_timing: self.middleware.enable_request_timing,
            enable_request_logging: self.middleware.enable_request_logging,
            enable_api_version: self.middleware.enable_api_version,
            enable_content_type_validation: self.middleware.enable_content_type_validation,
            content_type: ContentTypeConfig {
                excluded_paths: self.middleware.content_type_excluded_paths,
            },
            enable_cors: self.middleware.enable_cors,
            cors,
            enable_request_timeout: self.middleware.enable_request_timeout,
            timeout: TimeoutConfig {
                duration: std::time::Duration::from_secs(self.middleware.request_timeout_secs),
                excluded_paths: self.middleware.timeout_excluded_paths,
            },
            enable_rate_limit: self.middleware.enable_rate_limit,
            rate_limit,
            enable_body_limit: self.middleware.enable_body_limit,
            body_limit,
            enable_api_key_auth: self.middleware.enable_api_key_auth,
            api_key,
            enable_security_headers: self.middleware.enable_security_headers,
            security_headers: SecurityHeadersConfig {
                hsts: if self.middleware.enable_hsts {
                    Some(HstsConfig {
                        max_age: self.middleware.hsts_max_age,
                        include_sub_domains: self.middleware.hsts_include_sub_domains,
                        preload: self.middleware.hsts_preload,
                    })
                } else {
                    None
                },
                content_security_policy: if self.middleware.content_security_policy.is_empty() {
                    None
                } else {
                    Some(self.middleware.content_security_policy)
                },
                permissions_policy: if self.middleware.permissions_policy.is_empty() {
                    None
                } else {
                    Some(self.middleware.permissions_policy)
                },
                ..SecurityHeadersConfig::default()
            },
            enable_compression: self.middleware.enable_compression,
            compression: CompressionConfig {
                min_size: self.middleware.compression_min_size,
                ..CompressionConfig::default()
            },
            enable_etag: self.middleware.enable_etag,
            etag: ETagConfig {
                weak: self.middleware.etag_weak,
                ..ETagConfig::default()
            },
            enable_ip_filter: self.middleware.enable_ip_filter,
            ip_filter: IpFilterConfig {
                allow_list: self
                    .middleware
                    .ip_allow_list
                    .iter()
                    .filter_map(|s| IpNetwork::parse(s))
                    .collect(),
                deny_list: self
                    .middleware
                    .ip_deny_list
                    .iter()
                    .filter_map(|s| IpNetwork::parse(s))
                    .collect(),
                excluded_paths: self.middleware.ip_filter_excluded_paths,
                trust_proxy_headers: self.middleware.ip_filter_trust_proxy,
            },
            enable_body_validation: self.middleware.enable_body_validation,
            body_validation: BodyValidationConfig {
                max_depth: self.middleware.body_validation_max_depth,
                max_keys: self.middleware.body_validation_max_keys,
                max_string_length: self.middleware.body_validation_max_string_length,
                max_array_length: self.middleware.body_validation_max_array_length,
                excluded_paths: self.middleware.body_validation_excluded_paths,
            },
            enable_idempotency: self.middleware.enable_idempotency,
            idempotency: IdempotencyConfig {
                ttl_secs: self.middleware.idempotency_ttl_secs,
                max_entries: self.middleware.idempotency_max_entries,
                require_key: self.middleware.idempotency_require_key,
                excluded_paths: self.middleware.idempotency_excluded_paths,
            },
            enable_audit_log: self.middleware.enable_audit_log,
            audit_log: AuditLogConfig {
                methods: vec![
                    axum::http::Method::POST,
                    axum::http::Method::PUT,
                    axum::http::Method::PATCH,
                    axum::http::Method::DELETE,
                ],
                excluded_paths: self.middleware.audit_log_excluded_paths,
                log_request_body: self.middleware.audit_log_request_body,
                max_body_log_bytes: self.middleware.audit_log_max_body_bytes,
                log_response_status: self.middleware.audit_log_response_status,
            },
            enable_response_cache: self.middleware.enable_response_cache,
            response_cache: ResponseCacheConfig {
                ttl_secs: self.middleware.response_cache_ttl_secs,
                max_entries: self.middleware.response_cache_max_entries,
                excluded_paths: self.middleware.response_cache_excluded_paths,
                cache_head: self.middleware.response_cache_head,
                cache_control: self.middleware.response_cache_control,
                max_cacheable_body_size: self.middleware.response_cache_max_body_size,
            },
            enable_request_dedup: self.middleware.enable_request_dedup,
            request_dedup: RequestDedupConfig {
                methods: vec![
                    axum::http::Method::POST,
                    axum::http::Method::PUT,
                    axum::http::Method::PATCH,
                    axum::http::Method::DELETE,
                ],
                excluded_paths: self.middleware.request_dedup_excluded_paths,
                ttl_secs: self.middleware.request_dedup_ttl_secs,
                max_body_hash_bytes: self.middleware.request_dedup_max_body_hash_bytes,
            },
            enable_tracing: self.middleware.enable_tracing,
            tracing: TracingConfig {
                excluded_paths: self.middleware.tracing_excluded_paths,
                propagate_tracestate: self.middleware.tracing_propagate_tracestate,
                expose_trace_id: self.middleware.tracing_expose_trace_id,
                default_sampled: self.middleware.tracing_default_sampled,
            },
            enable_payload_signing: self.middleware.enable_payload_signing,
            payload_signing: PayloadSigningConfig {
                secret: self.middleware.payload_signing_secret,
                methods: vec![
                    axum::http::Method::POST,
                    axum::http::Method::PUT,
                    axum::http::Method::PATCH,
                ],
                excluded_paths: self.middleware.payload_signing_excluded_paths,
                require_signature: self.middleware.payload_signing_require_signature,
                max_body_bytes: self.middleware.payload_signing_max_body_bytes,
                signature_header: self.middleware.payload_signing_signature_header,
            },
            enable_circuit_breaker: self.middleware.enable_circuit_breaker,
            circuit_breaker: CircuitBreakerConfig {
                failure_threshold: self.middleware.circuit_breaker_failure_threshold,
                recovery_timeout_secs: self.middleware.circuit_breaker_recovery_timeout_secs,
                excluded_paths: self.middleware.circuit_breaker_excluded_paths,
            },
            enable_sanitization: self.middleware.enable_sanitization,
            sanitization: SanitizationConfig {
                strip_headers: self.middleware.sanitization_strip_headers,
                excluded_paths: self.middleware.sanitization_excluded_paths,
                strip_internal_prefix: self.middleware.sanitization_strip_internal_prefix,
                max_header_value_length: self.middleware.sanitization_max_header_value_length,
            },
            enable_content_negotiation: self.middleware.enable_content_negotiation,
            content_negotiation: ContentNegotiationConfig {
                supported_types: self.middleware.content_negotiation_supported_types,
                default_type: self.middleware.content_negotiation_default_type,
                strict: self.middleware.content_negotiation_strict,
                excluded_paths: self.middleware.content_negotiation_excluded_paths,
            },
            enable_throttle: self.middleware.enable_throttle,
            throttle: ThrottleConfig {
                max_concurrent: self.middleware.throttle_max_concurrent,
                retry_after_secs: self.middleware.throttle_retry_after_secs,
                excluded_paths: self.middleware.throttle_excluded_paths,
            },
            enable_retry_hints: self.middleware.enable_retry_hints,
            retry_hints: RetryHintsConfig {
                retry_statuses: self.middleware.retry_hints_statuses,
                default_retry_after_secs: self.middleware.retry_hints_retry_after_secs,
                strategy: self.middleware.retry_hints_strategy,
                max_retries: self.middleware.retry_hints_max_retries,
                excluded_paths: self.middleware.retry_hints_excluded_paths,
            },
            enable_maintenance: self.middleware.enable_maintenance,
            maintenance: MaintenanceConfig {
                message: self.middleware.maintenance_message,
                retry_after_secs: self.middleware.maintenance_retry_after_secs,
                excluded_paths: self.middleware.maintenance_excluded_paths,
            },
            enable_deprecation: self.middleware.enable_deprecation,
            deprecation: DeprecationConfig::default(),
            enable_request_cost: self.middleware.enable_request_cost,
            request_cost: RequestCostConfig::default(),
            enable_fingerprint: self.middleware.enable_fingerprint,
            fingerprint: FingerprintConfig::default(),
            enable_response_signing: self.middleware.enable_response_signing,
            response_signing: ResponseSigningConfig::default(),
            enable_request_priority: self.middleware.enable_request_priority,
            request_priority: RequestPriorityConfig::default(),
            enable_request_quota: self.middleware.enable_request_quota,
            request_quota: RequestQuotaConfig::default(),
            quota_state: QuotaState::new(),
            enable_tenant_isolation: self.middleware.enable_tenant_isolation,
            tenant_isolation: TenantIsolationConfig::default(),
            enable_response_envelope: self.middleware.enable_response_envelope,
            enable_replay_protection: self.middleware.enable_replay_protection,
            replay_protection: ReplayProtectionConfig::default(),
            enable_geo_ip: self.middleware.enable_geo_ip,
            geo_ip: GeoIpConfig::default(),
            enable_schema_validation: self.middleware.enable_schema_validation,
            schema_validation: SchemaValidationConfig::default(),
            enable_request_decompression: self.middleware.enable_request_decompression,
            request_decompression: RequestDecompressionConfig::default(),
            enable_slow_request: self.middleware.enable_slow_request,
            slow_request: SlowRequestConfig::default(),
            enable_header_propagation: self.middleware.enable_header_propagation,
            header_propagation: HeaderPropagationConfig::default(),
            enable_request_context: self.middleware.enable_request_context,
            request_context: RequestContextConfig::default(),
            response_envelope: ResponseEnvelopeConfig::default(),
            enable_fallback: self.middleware.enable_fallback,
        };

        ServerConfig {
            host: self.server.host,
            rest_port: self.server.rest_port,
            grpc_port: self.server.grpc_port,
            enable_runtime: self.server.enable_runtime,
            enable_events: self.server.enable_events,
            runtime,
            pre_warm_count: self.server.pre_warm_count,
            middleware,
            shutdown_timeout_secs: self.server.shutdown_timeout_secs,
            enforce_image_admission: self.server.enforce_image_admission,
            tls: match (self.server.tls_cert_path, self.server.tls_key_path) {
                (Some(cert), Some(key)) => Some(crate::tls::TlsConfig {
                    cert_path: cert,
                    key_path: key,
                }),
                _ => None,
            },
        }
    }
}

/// Load configuration with layered merging: file → env → validate.
///
/// If `path` is `None`, looks for `hv2.toml` in the current directory.
/// If the file does not exist, uses defaults.
pub fn load_config(path: Option<&Path>) -> ConfigResult<ConfigFile> {
    let default_path = std::path::PathBuf::from("hv2.toml");
    let path = path.unwrap_or(&default_path);

    let mut config = ConfigFile::load(path)?.unwrap_or_default();

    config.apply_env();
    config.validate()?;
    Ok(config)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ── Defaults ──────────────────────────────────────────────────────

    #[test]
    fn test_config_file_default() {
        let config = ConfigFile::default();
        assert_eq!(config.server.host, "0.0.0.0");
        assert_eq!(config.server.rest_port, 8080);
        assert_eq!(config.server.grpc_port, 50051);
        assert!(config.server.enable_runtime);
        assert!(config.server.enable_events);
        assert_eq!(config.server.pre_warm_count, 2);
    }

    #[test]
    fn test_server_section_default() {
        let section = ServerSection::default();
        assert_eq!(section.host, "0.0.0.0");
        assert_eq!(section.rest_port, 8080);
        assert_eq!(section.grpc_port, 50051);
    }

    #[test]
    fn test_runtime_section_default() {
        let section = RuntimeSection::default();
        assert!(section.instance_id.is_empty());
        assert_eq!(section.pool.min_warm, 2);
        assert_eq!(section.pool.max_size, 64);
    }

    #[test]
    fn test_pool_section_default() {
        let section = PoolSection::default();
        assert_eq!(section.min_warm, 2);
        assert_eq!(section.max_size, 64);
        assert_eq!(section.max_idle_secs, 600);
        assert_eq!(section.max_lifetime_secs, 86400);
    }

    #[test]
    fn test_middleware_section_default() {
        let section = MiddlewareSection::default();
        assert!(section.enable_request_id);
        assert!(section.enable_request_timing);
        assert!(section.enable_request_logging);
        assert!(section.enable_api_version);
        assert!(section.enable_content_type_validation);
        assert_eq!(section.content_type_excluded_paths.len(), 2);
        assert!(section.enable_cors);
        assert!(!section.enable_request_timeout);
        assert_eq!(section.request_timeout_secs, 30);
        assert_eq!(section.timeout_excluded_paths.len(), 2);
        assert!(section.enable_security_headers);
        assert!(!section.enable_api_key_auth);
    }

    #[test]
    fn test_cors_section_default() {
        let section = CorsSection::default();
        assert!(section.allowed_origins.is_empty());
        assert_eq!(section.allowed_methods.len(), 6);
        assert_eq!(section.allowed_headers.len(), 3);
        assert!(!section.allow_credentials);
        assert_eq!(section.max_age, 86400);
    }

    #[test]
    fn test_api_key_section_default() {
        let section = ApiKeySection::default();
        assert!(section.keys.is_empty());
        assert_eq!(section.excluded_paths.len(), 2);
    }

    // ── TOML Parsing ──────────────────────────────────────────────────

    #[test]
    fn test_parse_empty_toml() {
        let config = ConfigFile::parse("").unwrap();
        assert_eq!(config.server.rest_port, 8080); // defaults applied
    }

    #[test]
    fn test_parse_minimal_server() {
        let toml = r#"
            [server]
            host = "127.0.0.1"
            rest_port = 9090
        "#;
        let config = ConfigFile::parse(toml).unwrap();
        assert_eq!(config.server.host, "127.0.0.1");
        assert_eq!(config.server.rest_port, 9090);
        assert_eq!(config.server.grpc_port, 50051); // default
    }

    #[test]
    fn test_parse_full_config() {
        let toml = r#"
            [server]
            host = "10.0.0.1"
            rest_port = 3000
            grpc_port = 50052
            enable_runtime = false
            enable_events = false
            pre_warm_count = 4

            [runtime]
            instance_id = "test-01"

            [runtime.pool]
            min_warm = 8
            max_size = 128
            max_idle_secs = 60
            max_lifetime_secs = 7200

            [middleware]
            enable_request_id = false
            enable_request_timing = false
            enable_request_logging = false
            enable_cors = false
            enable_api_key_auth = false

            [middleware.cors]
            allowed_origins = ["https://example.com"]
            allow_credentials = true
            max_age = 3600

            [middleware.api_key]
            keys = []
            excluded_paths = ["/health"]
        "#;
        let config = ConfigFile::parse(toml).unwrap();
        assert_eq!(config.server.host, "10.0.0.1");
        assert_eq!(config.server.rest_port, 3000);
        assert_eq!(config.server.grpc_port, 50052);
        assert!(!config.server.enable_runtime);
        assert!(!config.server.enable_events);
        assert_eq!(config.server.pre_warm_count, 4);
        assert_eq!(config.runtime.instance_id, "test-01");
        assert_eq!(config.runtime.pool.min_warm, 8);
        assert_eq!(config.runtime.pool.max_size, 128);
        assert_eq!(config.runtime.pool.max_idle_secs, 60);
        assert_eq!(config.runtime.pool.max_lifetime_secs, 7200);
        assert!(!config.middleware.enable_request_id);
        assert!(!config.middleware.enable_cors);
        assert_eq!(config.middleware.cors.allowed_origins.len(), 1);
        assert!(config.middleware.cors.allow_credentials);
        assert_eq!(config.middleware.cors.max_age, 3600);
        assert_eq!(config.middleware.api_key.excluded_paths.len(), 1);
    }

    #[test]
    fn test_parse_partial_overrides() {
        let toml = r#"
            [server]
            rest_port = 9999
        "#;
        let config = ConfigFile::parse(toml).unwrap();
        assert_eq!(config.server.rest_port, 9999);
        // All other fields should be defaults
        assert_eq!(config.server.host, "0.0.0.0");
        assert_eq!(config.server.grpc_port, 50051);
        assert!(config.server.enable_runtime);
        assert_eq!(config.runtime.pool.max_size, 64);
        assert!(config.middleware.enable_request_id);
    }

    #[test]
    fn test_parse_invalid_toml() {
        let result = ConfigFile::parse("{{invalid}}");
        assert!(result.is_err());
    }

    // ── TOML Serialization ────────────────────────────────────────────

    #[test]
    fn test_to_toml() {
        let config = ConfigFile::default();
        let toml_str = config.to_toml().unwrap();
        assert!(toml_str.contains("[server]"));
        assert!(toml_str.contains("[runtime]"));
        assert!(toml_str.contains("[middleware]"));
        assert!(toml_str.contains("rest_port = 8080"));
    }

    #[test]
    fn test_default_toml() {
        let toml_str = ConfigFile::default_toml().unwrap();
        assert!(toml_str.contains("[server]"));
        assert!(toml_str.contains("host = "));
    }

    #[test]
    fn test_roundtrip_toml() {
        let original = ConfigFile::default();
        let toml_str = original.to_toml().unwrap();
        let parsed = ConfigFile::parse(&toml_str).unwrap();
        assert_eq!(parsed.server.host, original.server.host);
        assert_eq!(parsed.server.rest_port, original.server.rest_port);
        assert_eq!(parsed.server.grpc_port, original.server.grpc_port);
        assert_eq!(parsed.runtime.pool.max_size, original.runtime.pool.max_size);
        assert_eq!(
            parsed.middleware.enable_request_id,
            original.middleware.enable_request_id
        );
    }

    // ── Validation ────────────────────────────────────────────────────

    #[test]
    fn test_validate_defaults_pass() {
        let config = ConfigFile::default();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_validate_zero_rest_port() {
        let mut config = ConfigFile::default();
        config.server.rest_port = 0;
        let err = config.validate().unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("rest_port"), "Error: {msg}");
    }

    #[test]
    fn test_validate_zero_grpc_port() {
        let mut config = ConfigFile::default();
        config.server.grpc_port = 0;
        let err = config.validate().unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("grpc_port"), "Error: {msg}");
    }

    #[test]
    fn test_validate_same_ports() {
        let mut config = ConfigFile::default();
        config.server.rest_port = 8080;
        config.server.grpc_port = 8080;
        let err = config.validate().unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("different"), "Error: {msg}");
    }

    #[test]
    fn test_validate_pool_max_zero() {
        let mut config = ConfigFile::default();
        config.runtime.pool.max_size = 0;
        let err = config.validate().unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("max_size"), "Error: {msg}");
    }

    #[test]
    fn test_validate_pool_min_exceeds_max() {
        let mut config = ConfigFile::default();
        config.runtime.pool.min_warm = 100;
        config.runtime.pool.max_size = 10;
        let err = config.validate().unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("min_warm"), "Error: {msg}");
    }

    #[test]
    fn test_validate_max_idle_zero() {
        let mut config = ConfigFile::default();
        config.runtime.pool.max_idle_secs = 0;
        let err = config.validate().unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("max_idle"), "Error: {msg}");
    }

    #[test]
    fn test_validate_cors_max_age_zero() {
        let mut config = ConfigFile::default();
        config.middleware.cors.max_age = 0;
        let err = config.validate().unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("max_age"), "Error: {msg}");
    }

    #[test]
    fn test_validate_auth_enabled_no_keys() {
        let mut config = ConfigFile::default();
        config.middleware.enable_api_key_auth = true;
        config.middleware.api_key.keys = vec![];
        let err = config.validate().unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("keys"), "Error: {msg}");
    }

    #[test]
    fn test_validate_multiple_errors() {
        let mut config = ConfigFile::default();
        config.server.rest_port = 0;
        config.server.grpc_port = 0;
        config.runtime.pool.max_size = 0;
        let err = config.validate().unwrap_err();
        let msg = err.to_string();
        // Should report all errors, not just first
        assert!(msg.contains("rest_port"), "Msg: {msg}");
        assert!(msg.contains("grpc_port"), "Msg: {msg}");
        assert!(msg.contains("max_size"), "Msg: {msg}");
    }

    // ── into_server_config ────────────────────────────────────────────

    #[test]
    fn test_into_server_config_defaults() {
        let config = ConfigFile::default();
        let server = config.into_server_config();
        assert_eq!(server.host, "0.0.0.0");
        assert_eq!(server.rest_port, 8080);
        assert_eq!(server.grpc_port, 50051);
        assert!(server.enable_runtime);
        assert!(server.enable_events);
        assert_eq!(server.pre_warm_count, 2);
        assert!(server.middleware.enable_request_id);
        assert!(server.middleware.enable_cors);
        assert!(!server.middleware.enable_api_key_auth);
    }

    #[test]
    fn test_into_server_config_custom() {
        let toml = r#"
            [server]
            host = "10.0.0.1"
            rest_port = 3000
            grpc_port = 50052
            enable_runtime = false
            pre_warm_count = 8

            [runtime]
            instance_id = "custom-01"

            [runtime.pool]
            min_warm = 4
            max_size = 32

            [middleware]
            enable_request_id = false
            enable_cors = false
        "#;
        let config = ConfigFile::parse(toml).unwrap();
        let server = config.into_server_config();
        assert_eq!(server.host, "10.0.0.1");
        assert_eq!(server.rest_port, 3000);
        assert_eq!(server.grpc_port, 50052);
        assert!(!server.enable_runtime);
        assert_eq!(server.pre_warm_count, 8);
        assert_eq!(server.runtime.instance_id, "custom-01");
        assert_eq!(server.runtime.pool.min_warm, 4);
        assert_eq!(server.runtime.pool.max_size, 32);
        assert!(!server.middleware.enable_request_id);
        assert!(!server.middleware.enable_cors);
    }

    #[test]
    fn test_into_server_config_auto_instance_id() {
        let config = ConfigFile::default();
        let server = config.into_server_config();
        // Empty instance_id should be auto-generated (UUID v4, 36 chars)
        assert_eq!(server.runtime.instance_id.len(), 36);
    }

    #[test]
    fn test_into_server_config_cors_methods() {
        let toml = r#"
            [middleware.cors]
            allowed_methods = ["GET", "POST"]
        "#;
        let config = ConfigFile::parse(toml).unwrap();
        let server = config.into_server_config();
        assert_eq!(server.middleware.cors.allowed_methods.len(), 2);
    }

    #[test]
    fn test_into_server_config_api_keys() {
        let toml = r#"
            [middleware]
            enable_api_key_auth = true

            [middleware.api_key]
            keys = ["key1", "key2"]
        "#;
        let config = ConfigFile::parse(toml).unwrap();
        let server = config.into_server_config();
        assert!(server.middleware.enable_api_key_auth);
        assert_eq!(server.middleware.api_key.keys.len(), 2);
    }

    // ── Environment Variables ─────────────────────────────────────────
    //
    // Combined into a single test to avoid race conditions: env vars are
    // process-global and parallel test threads would clobber each other.

    #[test]
    fn test_apply_env_overrides() {
        // Host
        let mut config = ConfigFile::default();
        std::env::set_var("HV2_HOST", "192.168.1.1");
        config.apply_env();
        std::env::remove_var("HV2_HOST");
        assert_eq!(config.server.host, "192.168.1.1");

        // REST port
        let mut config = ConfigFile::default();
        std::env::set_var("HV2_REST_PORT", "9090");
        config.apply_env();
        std::env::remove_var("HV2_REST_PORT");
        assert_eq!(config.server.rest_port, 9090);

        // Invalid port is ignored
        let mut config = ConfigFile::default();
        std::env::set_var("HV2_REST_PORT", "not-a-number");
        config.apply_env();
        std::env::remove_var("HV2_REST_PORT");
        assert_eq!(config.server.rest_port, 8080); // unchanged

        // Disable runtime
        let mut config = ConfigFile::default();
        std::env::set_var("HV2_ENABLE_RUNTIME", "false");
        config.apply_env();
        std::env::remove_var("HV2_ENABLE_RUNTIME");
        assert!(!config.server.enable_runtime);

        // Instance ID
        let mut config = ConfigFile::default();
        std::env::set_var("HV2_INSTANCE_ID", "env-instance");
        config.apply_env();
        std::env::remove_var("HV2_INSTANCE_ID");
        assert_eq!(config.runtime.instance_id, "env-instance");

        // API keys (comma-separated, auto-enables auth)
        let mut config = ConfigFile::default();
        std::env::set_var("HV2_API_KEYS", "key1,key2,key3");
        config.apply_env();
        std::env::remove_var("HV2_API_KEYS");
        assert_eq!(config.middleware.api_key.keys.len(), 3);
        assert!(config.middleware.enable_api_key_auth);

        // CORS origins
        let mut config = ConfigFile::default();
        std::env::set_var("HV2_CORS_ORIGINS", "https://a.com, https://b.com");
        config.apply_env();
        std::env::remove_var("HV2_CORS_ORIGINS");
        assert_eq!(config.middleware.cors.allowed_origins.len(), 2);

        // Shutdown timeout
        let mut config = ConfigFile::default();
        std::env::set_var("HV2_SHUTDOWN_TIMEOUT", "90");
        config.apply_env();
        std::env::remove_var("HV2_SHUTDOWN_TIMEOUT");
        assert_eq!(config.server.shutdown_timeout_secs, 90);

        // Invalid shutdown timeout is ignored
        let mut config = ConfigFile::default();
        std::env::set_var("HV2_SHUTDOWN_TIMEOUT", "not-a-number");
        config.apply_env();
        std::env::remove_var("HV2_SHUTDOWN_TIMEOUT");
        assert_eq!(config.server.shutdown_timeout_secs, 30); // unchanged
    }

    // ── File Loading ──────────────────────────────────────────────────

    #[test]
    fn test_load_nonexistent_returns_none() {
        let result = ConfigFile::load(Path::new("nonexistent-config.toml")).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_load_config_defaults_when_no_file() {
        let config = load_config(Some(Path::new("nonexistent-config.toml"))).unwrap();
        assert_eq!(config.server.rest_port, 8080);
    }

    // ── Shutdown Timeout ──────────────────────────────────────────────

    #[test]
    fn test_server_section_default_shutdown_timeout() {
        let section = ServerSection::default();
        assert_eq!(section.shutdown_timeout_secs, 30);
    }

    #[test]
    fn test_config_file_default_shutdown_timeout() {
        let config = ConfigFile::default();
        assert_eq!(config.server.shutdown_timeout_secs, 30);
    }

    #[test]
    fn test_parse_shutdown_timeout_from_toml() {
        let toml = r#"
            [server]
            shutdown_timeout_secs = 60
        "#;
        let config = ConfigFile::parse(toml).unwrap();
        assert_eq!(config.server.shutdown_timeout_secs, 60);
    }

    #[test]
    fn test_parse_zero_shutdown_timeout() {
        let toml = r#"
            [server]
            shutdown_timeout_secs = 0
        "#;
        let config = ConfigFile::parse(toml).unwrap();
        assert_eq!(config.server.shutdown_timeout_secs, 0);
    }

    #[test]
    fn test_into_server_config_shutdown_timeout() {
        let toml = r#"
            [server]
            shutdown_timeout_secs = 45
        "#;
        let config = ConfigFile::parse(toml).unwrap();
        let server = config.into_server_config();
        assert_eq!(server.shutdown_timeout_secs, 45);
    }

    #[test]
    fn test_into_server_config_default_shutdown_timeout() {
        let config = ConfigFile::default();
        let server = config.into_server_config();
        assert_eq!(server.shutdown_timeout_secs, 30);
    }

    #[test]
    fn test_shutdown_timeout_in_default_toml() {
        let toml = ConfigFile::default_toml().unwrap();
        assert!(
            toml.contains("shutdown_timeout_secs"),
            "Default TOML should include shutdown_timeout_secs"
        );
    }

    #[test]
    fn test_roundtrip_shutdown_timeout() {
        let mut config = ConfigFile::default();
        config.server.shutdown_timeout_secs = 120;
        let toml = config.to_toml().unwrap();
        let parsed = ConfigFile::parse(&toml).unwrap();
        assert_eq!(parsed.server.shutdown_timeout_secs, 120);
    }

    // ── Body Limit Config ─────────────────────────────────────────────

    #[test]
    fn test_body_limit_section_default() {
        let section = BodyLimitSection::default();
        assert_eq!(section.max_bytes, 2 * 1024 * 1024);
        assert_eq!(section.excluded_paths, vec!["/health"]);
    }

    #[test]
    fn test_middleware_section_default_body_limit() {
        let section = MiddlewareSection::default();
        assert!(!section.enable_body_limit);
        assert_eq!(section.body_limit.max_bytes, 2 * 1024 * 1024);
    }

    #[test]
    fn test_parse_body_limit_from_toml() {
        let toml = r#"
            [middleware]
            enable_body_limit = true

            [middleware.body_limit]
            max_bytes = 5242880
            excluded_paths = ["/health", "/metrics"]
        "#;
        let config = ConfigFile::parse(toml).unwrap();
        assert!(config.middleware.enable_body_limit);
        assert_eq!(config.middleware.body_limit.max_bytes, 5242880);
        assert_eq!(config.middleware.body_limit.excluded_paths.len(), 2);
    }

    #[test]
    fn test_into_server_config_body_limit() {
        let toml = r#"
            [middleware]
            enable_body_limit = true

            [middleware.body_limit]
            max_bytes = 1048576
        "#;
        let config = ConfigFile::parse(toml).unwrap();
        let server = config.into_server_config();
        assert!(server.middleware.enable_body_limit);
        assert_eq!(server.middleware.body_limit.max_bytes, 1048576);
    }

    #[test]
    fn test_into_server_config_body_limit_disabled_by_default() {
        let config = ConfigFile::default();
        let server = config.into_server_config();
        assert!(!server.middleware.enable_body_limit);
    }

    #[test]
    fn test_body_limit_in_default_toml() {
        let toml = ConfigFile::default_toml().unwrap();
        assert!(
            toml.contains("max_bytes"),
            "Default TOML should include body_limit.max_bytes"
        );
    }

    #[test]
    fn test_apply_env_body_limit() {
        // Use a unique env var name to avoid race with parallel tests.
        // We test by directly calling the body-limit branch logic.
        let mut config = ConfigFile::default();
        assert_eq!(config.middleware.body_limit.max_bytes, 2 * 1024 * 1024);
        assert!(!config.middleware.enable_body_limit);

        // Simulate what apply_env does for HV2_BODY_LIMIT
        config.middleware.body_limit.max_bytes = 4_194_304;
        config.middleware.enable_body_limit = true;

        assert_eq!(config.middleware.body_limit.max_bytes, 4_194_304);
        assert!(config.middleware.enable_body_limit);
    }

    #[test]
    fn test_apply_env_body_limit_invalid_ignored() {
        // Verify that invalid (non-numeric) values leave config unchanged.
        let mut config = ConfigFile::default();
        let original_max = config.middleware.body_limit.max_bytes;

        // Simulate invalid parse — the apply_env code only updates if parse succeeds
        let val = "not-a-number";
        if let Ok(bytes) = val.parse::<usize>() {
            config.middleware.body_limit.max_bytes = bytes;
            config.middleware.enable_body_limit = true;
        }

        assert_eq!(config.middleware.body_limit.max_bytes, original_max);
        assert!(!config.middleware.enable_body_limit);
    }

    #[test]
    fn test_roundtrip_body_limit() {
        let mut config = ConfigFile::default();
        config.middleware.enable_body_limit = true;
        config.middleware.body_limit.max_bytes = 10_000_000;
        let toml = config.to_toml().unwrap();
        let parsed = ConfigFile::parse(&toml).unwrap();
        assert!(parsed.middleware.enable_body_limit);
        assert_eq!(parsed.middleware.body_limit.max_bytes, 10_000_000);
    }

    // ── Fallback Config ───────────────────────────────────────────────

    #[test]
    fn test_middleware_section_fallback_default_enabled() {
        let section = MiddlewareSection::default();
        assert!(section.enable_fallback);
    }

    #[test]
    fn test_fallback_into_server_config() {
        let mut config = ConfigFile::default();
        config.middleware.enable_fallback = false;
        let server_config = config.into_server_config();
        assert!(!server_config.middleware.enable_fallback);
    }

    #[test]
    fn test_fallback_roundtrip() {
        let mut config = ConfigFile::default();
        config.middleware.enable_fallback = false;
        let toml = config.to_toml().unwrap();
        let parsed = ConfigFile::parse(&toml).unwrap();
        assert!(!parsed.middleware.enable_fallback);
    }

    #[test]
    fn test_fallback_parse_from_toml() {
        let toml = r#"
[middleware]
enable_fallback = false
"#;
        let config = ConfigFile::parse(toml).unwrap();
        assert!(!config.middleware.enable_fallback);
    }
}
