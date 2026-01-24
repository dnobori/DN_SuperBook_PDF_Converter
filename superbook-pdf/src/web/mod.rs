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
//! Spec Reference: specs/20-web.spec.md

mod job;
mod routes;
mod server;
mod worker;

pub use job::{ConvertOptions, Job, JobQueue, JobStatus, Progress};
pub use server::{ServerConfig, WebServer};
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
