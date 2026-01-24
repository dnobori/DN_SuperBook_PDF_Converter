//! Web server module for superbook-pdf
//!
//! Provides a REST API and simple Web UI for PDF conversion.
//!
//! # Features
//!
//! - PDF upload and conversion via REST API
//! - Real-time job status tracking
//! - Result download
//! - Simple Web UI for browser access
//!
//! # Usage
//!
//! Enable the `web` feature and use the `serve` subcommand:
//!
//! ```bash
//! cargo build --features web
//! superbook-pdf serve --port 8080
//! ```
//!
//! Spec Reference: specs/20-web.spec.md, specs/21-websocket.spec.md, specs/22-batch.spec.md

mod auth;
mod batch;
mod cors;
mod job;
mod metrics;
mod persistence;
mod rate_limit;
mod routes;
mod server;
mod shutdown;
mod websocket;
mod worker;

pub use auth::{ApiKey, AuthConfig, AuthError, AuthManager, AuthResult, AuthStatusResponse, Scope, extract_api_key};
pub use batch::{BatchJob, BatchProgress, BatchQueue, BatchStatus, Priority};
pub use cors::CorsConfig;
pub use job::{ConvertOptions, Job, JobQueue, JobStatus, Progress};
pub use metrics::{BatchStatistics, JobStatistics, MetricsCollector, ServerInfo, StatsResponse, SystemMetrics};
pub use persistence::{HistoryQuery, HistoryResponse, JsonJobStore, JobStore, PersistenceConfig, RecoveryManager, RecoveryResult, RetryResponse, StorageBackend, StoreError};
pub use rate_limit::{RateLimitConfig, RateLimitError, RateLimitResult, RateLimiter, RateLimitStatus};
pub use server::{ServerConfig, WebServer};
pub use shutdown::{ShutdownConfig, ShutdownCoordinator, ShutdownResult, ShutdownSignal, graceful_shutdown, wait_for_shutdown_signal};
pub use websocket::{
    generate_preview_base64, preview_stage, WsBroadcaster, WsMessage, PREVIEW_WIDTH,
};
pub use worker::{JobWorker, WorkerPool};

/// Default server port
pub const DEFAULT_PORT: u16 = 8080;

/// Default bind address
pub const DEFAULT_BIND: &str = "127.0.0.1";

/// Default upload limit in bytes (500 MB)
pub const DEFAULT_UPLOAD_LIMIT: usize = 500 * 1024 * 1024;

/// Default job timeout in seconds (1 hour)
pub const DEFAULT_JOB_TIMEOUT: u64 = 3600;

#[cfg(test)]
mod tests {
    use super::*;

    // TC-WEB-001: Server config defaults
    #[test]
    fn test_default_constants() {
        assert_eq!(DEFAULT_PORT, 8080);
        assert_eq!(DEFAULT_BIND, "127.0.0.1");
        assert_eq!(DEFAULT_UPLOAD_LIMIT, 500 * 1024 * 1024);
        assert_eq!(DEFAULT_JOB_TIMEOUT, 3600);
    }
}
