//! Web server implementation
//!
//! Provides the main server struct and configuration.

use axum::Router;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use tower_http::limit::RequestBodyLimitLayer;

use super::cors::CorsConfig;
use super::routes::{api_routes, web_routes, ws_routes, AppState};
use super::shutdown::{ShutdownConfig, wait_for_shutdown_signal};
use super::{DEFAULT_BIND, DEFAULT_PORT, DEFAULT_UPLOAD_LIMIT};

/// Server configuration
#[derive(Debug, Clone)]
pub struct ServerConfig {
    /// Port to listen on
    pub port: u16,
    /// Address to bind to
    pub bind: String,
    /// Number of worker threads
    pub workers: usize,
    /// Maximum upload size in bytes
    pub upload_limit: usize,
    /// Job timeout in seconds
    pub job_timeout: u64,
    /// Working directory for uploads and outputs
    pub work_dir: PathBuf,
    /// CORS configuration
    pub cors: CorsConfig,
    /// Graceful shutdown configuration
    pub shutdown: ShutdownConfig,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            port: DEFAULT_PORT,
            bind: DEFAULT_BIND.to_string(),
            workers: num_cpus::get(),
            upload_limit: DEFAULT_UPLOAD_LIMIT,
            job_timeout: super::DEFAULT_JOB_TIMEOUT,
            work_dir: std::env::temp_dir().join("superbook-pdf"),
            cors: CorsConfig::default(),
            shutdown: ShutdownConfig::default(),
        }
    }
}

impl ServerConfig {
    /// Create a new server config with the given port
    pub fn with_port(mut self, port: u16) -> Self {
        self.port = port;
        self
    }

    /// Create a new server config with the given bind address
    pub fn with_bind(mut self, bind: impl Into<String>) -> Self {
        self.bind = bind.into();
        self
    }

    /// Create a new server config with the given upload limit
    pub fn with_upload_limit(mut self, limit: usize) -> Self {
        self.upload_limit = limit;
        self
    }

    /// Set CORS configuration
    pub fn with_cors(mut self, cors: CorsConfig) -> Self {
        self.cors = cors;
        self
    }

    /// Set permissive CORS (for development)
    pub fn with_cors_permissive(mut self) -> Self {
        self.cors = CorsConfig::permissive();
        self
    }

    /// Set strict CORS with specific origins (for production)
    pub fn with_cors_origins(mut self, origins: Vec<String>) -> Self {
        self.cors = CorsConfig::strict(origins);
        self
    }

    /// Disable CORS
    pub fn with_cors_disabled(mut self) -> Self {
        self.cors = CorsConfig::disabled();
        self
    }

    /// Get the socket address
    pub fn socket_addr(&self) -> Result<SocketAddr, std::net::AddrParseError> {
        format!("{}:{}", self.bind, self.port).parse()
    }
}

/// Web server instance
pub struct WebServer {
    config: ServerConfig,
    state: Arc<AppState>,
}

impl WebServer {
    /// Create a new web server with default configuration
    pub fn new() -> Self {
        let config = ServerConfig::default();
        std::fs::create_dir_all(&config.work_dir).ok();
        let state = Arc::new(AppState::new(config.work_dir.clone(), config.workers));
        Self { config, state }
    }

    /// Create a new web server with the given configuration
    pub fn with_config(config: ServerConfig) -> Self {
        std::fs::create_dir_all(&config.work_dir).ok();
        let state = Arc::new(AppState::new(config.work_dir.clone(), config.workers));
        Self { config, state }
    }

    /// Get the server configuration
    pub fn config(&self) -> &ServerConfig {
        &self.config
    }

    /// Build the router
    fn build_router(&self) -> Router {
        Router::new()
            .merge(web_routes())
            .nest("/api", api_routes())
            .nest("/ws", ws_routes())
            .layer(self.config.cors.clone().into_layer())
            .layer(RequestBodyLimitLayer::new(self.config.upload_limit))
            .with_state(self.state.clone())
    }

    /// Run the server
    pub async fn run(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let addr = self.config.socket_addr()?;
        let router = self.build_router();

        println!("Starting server on http://{}", addr);
        println!("API endpoints:");
        println!("  POST /api/convert     - Upload and convert PDF");
        println!("  GET  /api/jobs/:id    - Get job status");
        println!("  DELETE /api/jobs/:id  - Cancel job");
        println!("  GET  /api/jobs/:id/download - Download result");
        println!("  GET  /api/health      - Health check");
        println!("WebSocket endpoints:");
        println!("  WS   /ws/jobs/:id     - Real-time job progress");
        println!("Press Ctrl+C to shutdown gracefully");

        let listener = tokio::net::TcpListener::bind(addr).await?;

        // Run server with graceful shutdown
        axum::serve(listener, router)
            .with_graceful_shutdown(wait_for_shutdown_signal())
            .await?;

        println!("Server shutdown complete");
        Ok(())
    }
}

impl Default for WebServer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_server_config_default() {
        let config = ServerConfig::default();
        assert_eq!(config.port, 8080);
        assert_eq!(config.bind, "127.0.0.1");
        assert_eq!(config.upload_limit, 500 * 1024 * 1024);
        assert!(config.workers > 0);
    }

    #[test]
    fn test_server_config_builder() {
        let config = ServerConfig::default()
            .with_port(3000)
            .with_bind("0.0.0.0")
            .with_upload_limit(100 * 1024 * 1024);

        assert_eq!(config.port, 3000);
        assert_eq!(config.bind, "0.0.0.0");
        assert_eq!(config.upload_limit, 100 * 1024 * 1024);
    }

    #[test]
    fn test_server_config_socket_addr() {
        let config = ServerConfig::default();
        let addr = config.socket_addr().unwrap();
        assert_eq!(addr.port(), 8080);
        assert_eq!(addr.ip().to_string(), "127.0.0.1");
    }

    #[tokio::test]
    async fn test_web_server_new() {
        let server = WebServer::new();
        assert_eq!(server.config().port, 8080);
    }

    #[tokio::test]
    async fn test_web_server_with_config() {
        let config = ServerConfig::default().with_port(9000);
        let server = WebServer::with_config(config);
        assert_eq!(server.config().port, 9000);
    }

    // CORS integration tests
    #[test]
    fn test_server_config_default_cors() {
        let config = ServerConfig::default();
        assert!(config.cors.enabled);
        assert!(config.cors.allowed_origins.is_none()); // All origins allowed by default
    }

    #[test]
    fn test_server_config_with_cors() {
        let cors = CorsConfig::strict(vec!["https://example.com".to_string()]);
        let config = ServerConfig::default().with_cors(cors);
        assert!(config.cors.allowed_origins.is_some());
        assert_eq!(
            config.cors.allowed_origins.as_ref().unwrap()[0],
            "https://example.com"
        );
    }

    #[test]
    fn test_server_config_with_cors_permissive() {
        let config = ServerConfig::default().with_cors_permissive();
        assert!(config.cors.enabled);
        assert!(config.cors.allow_credentials);
        assert!(config.cors.allowed_headers.contains(&"*".to_string()));
    }

    #[test]
    fn test_server_config_with_cors_origins() {
        let config = ServerConfig::default().with_cors_origins(vec![
            "https://app1.example.com".to_string(),
            "https://app2.example.com".to_string(),
        ]);
        assert!(config.cors.enabled);
        let origins = config.cors.allowed_origins.as_ref().unwrap();
        assert_eq!(origins.len(), 2);
        assert!(origins.contains(&"https://app1.example.com".to_string()));
        assert!(origins.contains(&"https://app2.example.com".to_string()));
    }

    #[test]
    fn test_server_config_with_cors_disabled() {
        let config = ServerConfig::default().with_cors_disabled();
        assert!(!config.cors.enabled);
    }

    #[tokio::test]
    async fn test_web_server_with_cors_config() {
        let config = ServerConfig::default()
            .with_port(9001)
            .with_cors_permissive();
        let server = WebServer::with_config(config);
        assert!(server.config().cors.enabled);
        assert!(server.config().cors.allow_credentials);
    }

    #[tokio::test]
    async fn test_web_server_build_router_with_cors() {
        let config = ServerConfig::default()
            .with_cors_origins(vec!["https://test.example.com".to_string()]);
        let server = WebServer::with_config(config);
        // Router should build successfully with CORS layer
        let _router = server.build_router();
    }
}
