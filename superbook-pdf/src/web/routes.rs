//! REST API routes for the web server
//!
//! Provides endpoints for PDF conversion, job management, and health checks.

use axum::{
    extract::{Multipart, Path, State},
    http::{header, StatusCode},
    response::{IntoResponse, Json},
    routing::{delete, get, post},
    Router,
};
use rust_embed::RustEmbed;
use serde::Serialize;
use std::sync::Arc;
use uuid::Uuid;

use super::auth::{AuthConfig, AuthManager, AuthResult, AuthStatusResponse, extract_api_key};
use super::batch::{BatchJob, BatchProgress, BatchQueue, Priority};
use super::job::{ConvertOptions, Job, JobQueue, JobStatus};
use super::metrics::{MetricsCollector, StatsResponse, SystemMetrics};
use super::persistence::{HistoryQuery, HistoryResponse, JsonJobStore, JobStore, PersistenceConfig, RetryResponse};
use super::rate_limit::{RateLimitConfig, RateLimitError, RateLimitResult, RateLimiter, RateLimitStatus};
use super::websocket::{ws_job_handler, WsBroadcaster};
use super::worker::WorkerPool;

use std::net::IpAddr;
use std::path::PathBuf;

/// Embedded static files
#[derive(RustEmbed)]
#[folder = "src/web/static"]
struct Assets;

/// Application state shared across handlers
pub struct AppState {
    pub queue: JobQueue,
    pub batch_queue: BatchQueue,
    pub version: String,
    pub worker_pool: WorkerPool,
    pub upload_dir: PathBuf,
    pub broadcaster: Arc<WsBroadcaster>,
    pub metrics: Arc<MetricsCollector>,
    pub rate_limiter: Arc<RateLimiter>,
    pub auth_manager: Arc<AuthManager>,
    pub job_store: Option<Arc<dyn JobStore>>,
    #[allow(dead_code)]
    pub persistence_config: PersistenceConfig,
}

impl AppState {
    pub fn new(work_dir: PathBuf, worker_count: usize) -> Self {
        Self::new_with_config(work_dir, worker_count, RateLimitConfig::default(), AuthConfig::default())
    }

    /// Create AppState with custom rate limit config (convenience method)
    #[allow(dead_code)]
    pub fn new_with_rate_limit(work_dir: PathBuf, worker_count: usize, rate_limit_config: RateLimitConfig) -> Self {
        Self::new_with_config(work_dir, worker_count, rate_limit_config, AuthConfig::default())
    }

    pub fn new_with_config(
        work_dir: PathBuf,
        worker_count: usize,
        rate_limit_config: RateLimitConfig,
        auth_config: AuthConfig,
    ) -> Self {
        Self::new_with_persistence(
            work_dir,
            worker_count,
            rate_limit_config,
            auth_config,
            PersistenceConfig::default(),
        )
    }

    /// Create AppState with full configuration including persistence
    pub fn new_with_persistence(
        work_dir: PathBuf,
        worker_count: usize,
        rate_limit_config: RateLimitConfig,
        auth_config: AuthConfig,
        persistence_config: PersistenceConfig,
    ) -> Self {
        let queue = JobQueue::new();
        let batch_queue = BatchQueue::new(queue.clone());
        let upload_dir = work_dir.join("uploads");
        std::fs::create_dir_all(&upload_dir).ok();

        let broadcaster = Arc::new(WsBroadcaster::new());
        let metrics = Arc::new(MetricsCollector::new());
        let rate_limiter = Arc::new(RateLimiter::new(rate_limit_config));
        let auth_manager = Arc::new(AuthManager::new(auth_config));
        let worker_pool =
            WorkerPool::new(queue.clone(), work_dir.clone(), worker_count, broadcaster.clone());

        // Initialize job store if persistence is enabled
        let job_store: Option<Arc<dyn JobStore>> = if persistence_config.enabled {
            let store_path = persistence_config.storage_path.join("jobs.json");
            match JsonJobStore::new(store_path) {
                Ok(store) => Some(Arc::new(store)),
                Err(e) => {
                    eprintln!("Warning: Failed to initialize job store: {}", e);
                    None
                }
            }
        } else {
            None
        };

        Self {
            queue,
            batch_queue,
            version: env!("CARGO_PKG_VERSION").to_string(),
            worker_pool,
            upload_dir,
            broadcaster,
            metrics,
            rate_limiter,
            auth_manager,
            job_store,
            persistence_config,
        }
    }
}

/// Build the API router
pub fn api_routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/convert", post(upload_and_convert))
        .route("/jobs/{id}", get(get_job))
        .route("/jobs/{id}", delete(cancel_job))
        .route("/jobs/{id}/download", get(download_result))
        .route("/jobs/{id}/retry", post(retry_job))
        .route("/jobs/history", get(get_job_history))
        .route("/batch", post(create_batch))
        .route("/batch/{id}", get(get_batch))
        .route("/batch/{id}", delete(cancel_batch))
        .route("/batch/{id}/jobs", get(get_batch_jobs))
        .route("/health", get(health_check))
        .route("/metrics", get(get_metrics))
        .route("/stats", get(get_stats))
        .route("/rate-limit/status", get(get_rate_limit_status))
        .route("/auth/status", get(get_auth_status))
}

/// Build the web UI router
pub fn web_routes() -> Router<Arc<AppState>> {
    Router::new().route("/", get(index_page))
}

/// Build the WebSocket router
pub fn ws_routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/jobs/{id}", get(ws_handler))
        .route("/batch/{id}", get(ws_batch_handler))
}

/// WebSocket handler wrapper that extracts broadcaster from AppState
async fn ws_handler(
    ws: axum::extract::ws::WebSocketUpgrade,
    Path(job_id): Path<Uuid>,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    ws_job_handler(ws, Path(job_id), axum::extract::State(state.broadcaster.clone())).await
}

/// WebSocket handler for batch progress updates
async fn ws_batch_handler(
    ws: axum::extract::ws::WebSocketUpgrade,
    Path(batch_id): Path<Uuid>,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    // Reuse the same WebSocket handler - batch and job use the same broadcaster
    ws_job_handler(ws, Path(batch_id), axum::extract::State(state.broadcaster.clone())).await
}

/// Serve the index page
async fn index_page() -> impl IntoResponse {
    match Assets::get("index.html") {
        Some(content) => {
            let body = content.data.into_owned();
            (
                StatusCode::OK,
                [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
                body,
            )
                .into_response()
        }
        None => (StatusCode::NOT_FOUND, "Not Found").into_response(),
    }
}

/// Health check response
#[derive(Debug, Serialize)]
pub struct HealthResponse {
    pub status: String,
    pub version: String,
    pub tools: ToolStatus,
}

#[derive(Debug, Serialize)]
pub struct ToolStatus {
    pub poppler: bool,
    pub tesseract: bool,
    pub realesrgan: bool,
    pub yomitoku: bool,
}

/// Health check endpoint
async fn health_check(State(state): State<Arc<AppState>>) -> Json<HealthResponse> {
    let tools = ToolStatus {
        poppler: which::which("pdftoppm").is_ok(),
        tesseract: which::which("tesseract").is_ok(),
        realesrgan: check_python_module("realesrgan"),
        yomitoku: check_python_module("yomitoku"),
    };

    Json(HealthResponse {
        status: "healthy".to_string(),
        version: state.version.clone(),
        tools,
    })
}

/// Check if a Python module is available
fn check_python_module(module: &str) -> bool {
    // Check if python3 exists
    let python = which::which("python3").or_else(|_| which::which("python"));
    if python.is_err() {
        return false;
    }

    // Check if module is importable
    let import_cmd = format!("import {}", module);
    let output = std::process::Command::new(python.unwrap())
        .args(["-c", &import_cmd])
        .output();

    matches!(output, Ok(o) if o.status.success())
}

/// Get metrics in Prometheus format
async fn get_metrics(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let queued = state.queue.pending_count() as u64;
    let ws_connections = state.broadcaster.channel_count().await;
    let worker_count = state.worker_pool.worker_count();

    let body = state.metrics.format_prometheus(queued, ws_connections, worker_count);

    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
        body,
    )
}

/// Get statistics in JSON format
async fn get_stats(State(state): State<Arc<AppState>>) -> Json<StatsResponse> {
    let queued = state.queue.pending_count() as u64;
    let ws_connections = state.broadcaster.channel_count().await;
    let worker_count = state.worker_pool.worker_count();

    let response = StatsResponse {
        server: state.metrics.get_server_info(),
        jobs: state.metrics.get_job_statistics(queued),
        batches: state.metrics.get_batch_statistics(),
        system: SystemMetrics {
            memory_used_mb: get_memory_usage_mb(),
            worker_count,
            websocket_connections: ws_connections,
        },
    };

    Json(response)
}

/// Get current process memory usage in MB
fn get_memory_usage_mb() -> u64 {
    // Try to read from /proc/self/statm on Linux
    if let Ok(content) = std::fs::read_to_string("/proc/self/statm") {
        if let Some(rss) = content.split_whitespace().nth(1) {
            if let Ok(pages) = rss.parse::<u64>() {
                // Each page is typically 4KB
                return pages * 4 / 1024;
            }
        }
    }
    0
}

/// Get rate limit status
async fn get_rate_limit_status(
    State(state): State<Arc<AppState>>,
    axum::extract::ConnectInfo(addr): axum::extract::ConnectInfo<std::net::SocketAddr>,
) -> Json<RateLimitStatus> {
    let ip = addr.ip();

    // Check current status for this IP
    let (remaining, reset_at) = match state.rate_limiter.check(ip) {
        RateLimitResult::Allowed { remaining, reset_at } => (remaining, reset_at),
        RateLimitResult::Limited { retry_after } => {
            use std::time::{SystemTime, UNIX_EPOCH};
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            (0, now + retry_after)
        }
    };

    Json(RateLimitStatus {
        enabled: state.rate_limiter.is_enabled(),
        requests_per_minute: state.rate_limiter.requests_per_minute(),
        burst_size: state.rate_limiter.burst_size(),
        your_remaining: remaining,
        reset_at,
    })
}

/// Rate limit response type alias (used by middleware integration)
#[allow(dead_code)]
type RateLimitResponse = (StatusCode, [(header::HeaderName, String); 4], Json<RateLimitError>);

/// Check rate limit for a request. Returns None if allowed, or an error response if limited.
/// This function is designed to be used in middleware for rate limiting all API endpoints.
#[allow(dead_code)]
pub fn check_rate_limit(
    rate_limiter: &RateLimiter,
    ip: IpAddr,
) -> Option<RateLimitResponse> {
    match rate_limiter.check(ip) {
        RateLimitResult::Allowed { remaining, reset_at } => {
            // Request allowed - headers will be added by middleware
            let _ = (remaining, reset_at);
            None
        }
        RateLimitResult::Limited { retry_after } => {
            // Request limited
            let error = RateLimitError::new(retry_after);
            Some((
                StatusCode::TOO_MANY_REQUESTS,
                [
                    (header::HeaderName::from_static("x-ratelimit-limit"), "0".to_string()),
                    (header::HeaderName::from_static("x-ratelimit-remaining"), "0".to_string()),
                    (header::HeaderName::from_static("x-ratelimit-reset"), "0".to_string()),
                    (header::HeaderName::from_static("retry-after"), retry_after.to_string()),
                ],
                Json(error),
            ))
        }
    }
}

/// Get authentication status
async fn get_auth_status(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
) -> Json<AuthStatusResponse> {
    // If auth is disabled, return disabled status
    if !state.auth_manager.is_enabled() {
        return Json(AuthStatusResponse::disabled());
    }

    // Extract API key from headers
    let authorization = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok());
    let x_api_key = headers
        .get("x-api-key")
        .and_then(|v| v.to_str().ok());

    let api_key = extract_api_key(authorization, x_api_key);

    match api_key {
        Some(key) => match state.auth_manager.validate(&key) {
            AuthResult::Authenticated { key_name, scopes } => {
                Json(AuthStatusResponse::authenticated(key_name, scopes))
            }
            AuthResult::Disabled => Json(AuthStatusResponse::disabled()),
            AuthResult::Expired => Json(AuthStatusResponse::unauthenticated(true)),
            AuthResult::InvalidKey => Json(AuthStatusResponse::unauthenticated(true)),
            AuthResult::Missing => Json(AuthStatusResponse::unauthenticated(true)),
        },
        None => Json(AuthStatusResponse::unauthenticated(true)),
    }
}

/// Get job history
async fn get_job_history(
    State(state): State<Arc<AppState>>,
    axum::extract::Query(query): axum::extract::Query<HistoryQuery>,
) -> Result<Json<HistoryResponse>, AppError> {
    // Get jobs from job store if persistence is enabled
    let all_jobs = if let Some(store) = &state.job_store {
        store.list().map_err(|e| {
            AppError::Internal(format!("Failed to get job history: {}", e))
        })?
    } else {
        // Fall back to in-memory queue
        state.queue.list()
    };

    // Filter by status if provided
    let filtered: Vec<Job> = if let Some(status) = &query.status {
        all_jobs.into_iter().filter(|j| &j.status == status).collect()
    } else {
        all_jobs
    };

    let total = filtered.len();

    // Apply pagination
    let jobs: Vec<Job> = filtered
        .into_iter()
        .skip(query.offset)
        .take(query.limit)
        .collect();

    Ok(Json(HistoryResponse {
        jobs,
        total,
        limit: query.limit,
        offset: query.offset,
    }))
}

/// Retry a failed job
async fn retry_job(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
) -> Result<Json<RetryResponse>, AppError> {
    // Get the job
    let job = state
        .queue
        .get(id)
        .ok_or_else(|| AppError::NotFound(format!("Job {} not found", id)))?;

    // Check if job can be retried (only failed jobs)
    if job.status != JobStatus::Failed {
        return Ok(Json(RetryResponse::error(
            id,
            format!("Cannot retry job with status: {}", job.status),
        )));
    }

    // Create a new job based on the failed one
    let new_job = Job::new(&job.input_filename, job.options.clone());
    let new_job_id = new_job.id;

    // Submit the new job
    state.queue.submit(new_job);

    // Try to find the original input file and resubmit
    let input_path = state.upload_dir.join(format!("{}_{}", id, job.input_filename));
    if input_path.exists() {
        // Copy to new job path
        let new_input_path = state.upload_dir.join(format!("{}_{}", new_job_id, job.input_filename));
        if std::fs::copy(&input_path, &new_input_path).is_ok() {
            if let Err(e) = state.worker_pool.submit(new_job_id, new_input_path, job.options.clone()).await {
                state.queue.update(new_job_id, |job| {
                    job.fail(format!("Failed to start processing: {}", e));
                });
            }
        }
    }

    Ok(Json(RetryResponse::success(new_job_id)))
}

/// Upload request response
#[derive(Debug, Serialize)]
pub struct UploadResponse {
    pub job_id: Uuid,
    pub status: String,
    pub created_at: String,
}

/// Upload and convert a PDF
async fn upload_and_convert(
    State(state): State<Arc<AppState>>,
    mut multipart: Multipart,
) -> Result<(StatusCode, Json<UploadResponse>), AppError> {
    let mut filename = String::new();
    let mut options = ConvertOptions::default();
    let mut file_data: Option<Vec<u8>> = None;

    while let Ok(Some(field)) = multipart.next_field().await {
        let name = field.name().unwrap_or("").to_string();

        match name.as_str() {
            "file" => {
                filename = field
                    .file_name()
                    .unwrap_or("upload.pdf")
                    .to_string();
                if let Ok(data) = field.bytes().await {
                    file_data = Some(data.to_vec());
                }
            }
            "options" => {
                if let Ok(text) = field.text().await {
                    if let Ok(parsed) = serde_json::from_str(&text) {
                        options = parsed;
                    }
                }
            }
            _ => {}
        }
    }

    if filename.is_empty() {
        return Err(AppError::BadRequest("No file uploaded".to_string()));
    }

    let file_data = file_data.ok_or_else(|| AppError::BadRequest("No file data".to_string()))?;

    // Create job
    let job = Job::new(&filename, options.clone());
    let job_id = job.id;
    let created_at = job.created_at.to_rfc3339();

    // Save uploaded file
    let input_path = state.upload_dir.join(format!("{}_{}", job_id, filename));
    std::fs::write(&input_path, &file_data).map_err(|e| {
        AppError::Internal(format!("Failed to save uploaded file: {}", e))
    })?;

    // Submit job to queue
    state.queue.submit(job);

    // Trigger background processing
    if let Err(e) = state.worker_pool.submit(job_id, input_path, options).await {
        // Update job as failed if we couldn't submit
        state.queue.update(job_id, |job| {
            job.fail(format!("Failed to start processing: {}", e));
        });
    }

    Ok((
        StatusCode::ACCEPTED,
        Json(UploadResponse {
            job_id,
            status: "queued".to_string(),
            created_at,
        }),
    ))
}

/// Get job status
async fn get_job(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
) -> Result<Json<Job>, AppError> {
    state
        .queue
        .get(id)
        .map(Json)
        .ok_or(AppError::NotFound(format!("Job {} not found", id)))
}

/// Cancel a job
async fn cancel_job(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
) -> Result<Json<Job>, AppError> {
    state
        .queue
        .cancel(id)
        .map(Json)
        .ok_or(AppError::NotFound(format!("Job {} not found", id)))
}

/// Download result response struct
#[derive(Debug)]
pub struct PdfDownload {
    data: Vec<u8>,
    filename: String,
}

impl IntoResponse for PdfDownload {
    fn into_response(self) -> axum::response::Response {
        (
            StatusCode::OK,
            [
                ("Content-Type", "application/pdf"),
                (
                    "Content-Disposition",
                    format!("attachment; filename=\"{}\"", self.filename).as_str(),
                ),
            ],
            self.data,
        )
            .into_response()
    }
}

/// Download conversion result
async fn download_result(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    let job = state
        .queue
        .get(id)
        .ok_or(AppError::NotFound(format!("Job {} not found", id)))?;

    match job.status {
        super::job::JobStatus::Completed => {
            if let Some(path) = &job.output_path {
                let data = std::fs::read(path).map_err(|e| {
                    AppError::Internal(format!("Failed to read output file: {}", e))
                })?;

                let filename = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("output.pdf")
                    .to_string();

                Ok(PdfDownload { data, filename })
            } else {
                Err(AppError::Internal("Output file not found".to_string()))
            }
        }
        super::job::JobStatus::Queued | super::job::JobStatus::Processing => {
            Err(AppError::Conflict(format!(
                "Job {} is still {}",
                id, job.status
            )))
        }
        super::job::JobStatus::Failed => Err(AppError::Conflict(format!(
            "Job {} failed: {}",
            id,
            job.error.as_deref().unwrap_or("Unknown error")
        ))),
        super::job::JobStatus::Cancelled => {
            Err(AppError::Conflict(format!("Job {} was cancelled", id)))
        }
    }
}

// ========== Batch API Handlers ==========

/// Batch creation request
#[derive(Debug, serde::Deserialize)]
pub struct BatchRequest {
    #[serde(default)]
    pub options: ConvertOptions,
    #[serde(default)]
    pub priority: Priority,
}

/// Batch creation response
#[derive(Debug, Serialize)]
pub struct BatchResponse {
    pub batch_id: Uuid,
    pub status: String,
    pub job_count: usize,
    pub created_at: String,
}

/// Batch status response
#[derive(Debug, Serialize)]
pub struct BatchStatusResponse {
    pub batch_id: Uuid,
    pub status: String,
    pub progress: BatchProgress,
    pub created_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<String>,
}

/// Batch jobs response
#[derive(Debug, Serialize)]
pub struct BatchJobsResponse {
    pub batch_id: Uuid,
    pub jobs: Vec<BatchJobInfo>,
}

/// Individual job info in batch
#[derive(Debug, Serialize)]
pub struct BatchJobInfo {
    pub job_id: Uuid,
    pub filename: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub download_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub progress: Option<JobProgressInfo>,
}

/// Job progress info
#[derive(Debug, Serialize)]
pub struct JobProgressInfo {
    pub percent: u8,
    pub step_name: String,
}

/// Batch cancel response
#[derive(Debug, Serialize)]
pub struct BatchCancelResponse {
    pub batch_id: Uuid,
    pub status: String,
    pub cancelled_jobs: usize,
    pub completed_jobs: usize,
}

/// Create a new batch job
async fn create_batch(
    State(state): State<Arc<AppState>>,
    mut multipart: Multipart,
) -> Result<(StatusCode, Json<BatchResponse>), AppError> {
    let mut filenames: Vec<String> = Vec::new();
    let mut file_data_list: Vec<(String, Vec<u8>)> = Vec::new();
    let mut options = ConvertOptions::default();
    let mut priority = Priority::default();

    while let Ok(Some(field)) = multipart.next_field().await {
        let name = field.name().unwrap_or("").to_string();

        match name.as_str() {
            "files[]" | "files" => {
                let filename = field
                    .file_name()
                    .unwrap_or("upload.pdf")
                    .to_string();
                if let Ok(data) = field.bytes().await {
                    file_data_list.push((filename.clone(), data.to_vec()));
                    filenames.push(filename);
                }
            }
            "options" => {
                if let Ok(text) = field.text().await {
                    if let Ok(parsed) = serde_json::from_str::<BatchRequest>(&text) {
                        options = parsed.options;
                        priority = parsed.priority;
                    } else if let Ok(parsed) = serde_json::from_str::<ConvertOptions>(&text) {
                        options = parsed;
                    }
                }
            }
            _ => {}
        }
    }

    if filenames.is_empty() {
        return Err(AppError::BadRequest("No files uploaded".to_string()));
    }

    // Create batch job
    let mut batch = BatchJob::new(options.clone(), priority);
    let batch_id = batch.id;
    let created_at = batch.created_at.to_rfc3339();

    // Create individual jobs and save files
    for (filename, data) in file_data_list {
        let job = Job::new(&filename, options.clone());
        let job_id = job.id;

        // Save uploaded file
        let input_path = state.upload_dir.join(format!("{}_{}", job_id, filename));
        std::fs::write(&input_path, &data).map_err(|e| {
            AppError::Internal(format!("Failed to save uploaded file: {}", e))
        })?;

        // Submit job
        state.queue.submit(job);
        batch.add_job(job_id);

        // Start processing
        if let Err(e) = state.worker_pool.submit(job_id, input_path, options.clone()).await {
            state.queue.update(job_id, |job| {
                job.fail(format!("Failed to start processing: {}", e));
            });
        }
    }

    let job_count = batch.job_count();
    batch.start();

    // Submit batch to queue
    state.batch_queue.submit(batch).await;

    Ok((
        StatusCode::ACCEPTED,
        Json(BatchResponse {
            batch_id,
            status: "processing".to_string(),
            job_count,
            created_at,
        }),
    ))
}

/// Get batch status
async fn get_batch(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
) -> Result<Json<BatchStatusResponse>, AppError> {
    let batch = state
        .batch_queue
        .get(id)
        .await
        .ok_or_else(|| AppError::NotFound(format!("Batch {} not found", id)))?;

    let progress = state
        .batch_queue
        .get_progress(id)
        .await
        .unwrap_or_else(|| BatchProgress::new(0));

    Ok(Json(BatchStatusResponse {
        batch_id: batch.id,
        status: batch.status.to_string(),
        progress,
        created_at: batch.created_at.to_rfc3339(),
        started_at: batch.started_at.map(|t| t.to_rfc3339()),
        completed_at: batch.completed_at.map(|t| t.to_rfc3339()),
    }))
}

/// Get batch jobs list
async fn get_batch_jobs(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
) -> Result<Json<BatchJobsResponse>, AppError> {
    let batch = state
        .batch_queue
        .get(id)
        .await
        .ok_or_else(|| AppError::NotFound(format!("Batch {} not found", id)))?;

    let mut jobs = Vec::new();
    for job_id in &batch.job_ids {
        if let Some(job) = state.queue.get(*job_id) {
            let download_url = if job.status == JobStatus::Completed {
                Some(format!("/api/jobs/{}/download", job_id))
            } else {
                None
            };

            let progress = if job.status == JobStatus::Processing {
                job.progress.as_ref().map(|p| JobProgressInfo {
                    percent: p.percent,
                    step_name: p.step_name.clone(),
                })
            } else {
                None
            };

            jobs.push(BatchJobInfo {
                job_id: *job_id,
                filename: job.input_filename.clone(),
                status: job.status.to_string(),
                download_url,
                progress,
            });
        }
    }

    Ok(Json(BatchJobsResponse {
        batch_id: batch.id,
        jobs,
    }))
}

/// Cancel a batch
async fn cancel_batch(
    State(state): State<Arc<AppState>>,
    Path(id): Path<Uuid>,
) -> Result<Json<BatchCancelResponse>, AppError> {
    let result = state
        .batch_queue
        .cancel(id)
        .await
        .ok_or_else(|| AppError::NotFound(format!("Batch {} not found", id)))?;

    Ok(Json(BatchCancelResponse {
        batch_id: id,
        status: "cancelled".to_string(),
        cancelled_jobs: result.0,
        completed_jobs: result.1,
    }))
}

/// API error type
#[derive(Debug)]
pub enum AppError {
    BadRequest(String),
    NotFound(String),
    Conflict(String),
    Internal(String),
    /// Rate limit exceeded (used by rate limiting middleware)
    #[allow(dead_code)]
    TooManyRequests { retry_after: u64 },
}

impl IntoResponse for AppError {
    fn into_response(self) -> axum::response::Response {
        #[derive(Serialize)]
        struct ErrorResponse {
            error: String,
            #[serde(skip_serializing_if = "Option::is_none")]
            retry_after: Option<u64>,
        }

        let (status, message, retry_after) = match &self {
            AppError::BadRequest(msg) => (StatusCode::BAD_REQUEST, msg.clone(), None),
            AppError::NotFound(msg) => (StatusCode::NOT_FOUND, msg.clone(), None),
            AppError::Conflict(msg) => (StatusCode::CONFLICT, msg.clone(), None),
            AppError::Internal(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg.clone(), None),
            AppError::TooManyRequests { retry_after } => (
                StatusCode::TOO_MANY_REQUESTS,
                "Rate limit exceeded".to_string(),
                Some(*retry_after),
            ),
        };

        let mut response = (status, Json(ErrorResponse { error: message, retry_after })).into_response();

        if let AppError::TooManyRequests { retry_after } = self {
            if let Ok(value) = header::HeaderValue::from_str(&retry_after.to_string()) {
                response.headers_mut().insert(header::RETRY_AFTER, value);
            }
        }

        response
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_app_state_new() {
        let work_dir = std::env::temp_dir().join("superbook_test_routes");
        let state = AppState::new(work_dir.clone(), 1);
        assert!(!state.version.is_empty());
        std::fs::remove_dir_all(&work_dir).ok();
    }

    #[test]
    fn test_tool_status_serialize() {
        let status = ToolStatus {
            poppler: true,
            tesseract: false,
            realesrgan: false,
            yomitoku: false,
        };
        let json = serde_json::to_string(&status).unwrap();
        assert!(json.contains("\"poppler\":true"));
        assert!(json.contains("\"tesseract\":false"));
        assert!(json.contains("\"yomitoku\":false"));
    }

    #[test]
    fn test_health_response_serialize() {
        let response = HealthResponse {
            status: "healthy".to_string(),
            version: "0.4.0".to_string(),
            tools: ToolStatus {
                poppler: true,
                tesseract: false,
                realesrgan: false,
                yomitoku: false,
            },
        };
        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("\"status\":\"healthy\""));
        assert!(json.contains("\"version\":\"0.4.0\""));
    }

    #[test]
    fn test_upload_response_serialize() {
        let id = Uuid::new_v4();
        let response = UploadResponse {
            job_id: id,
            status: "queued".to_string(),
            created_at: "2024-01-01T00:00:00Z".to_string(),
        };
        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains(&id.to_string()));
        assert!(json.contains("\"status\":\"queued\""));
    }

    #[test]
    fn test_check_python_module_nonexistent() {
        // Should return false for nonexistent module
        let result = check_python_module("nonexistent_module_xyz_12345");
        assert!(!result);
    }

    #[test]
    fn test_check_python_module_builtin() {
        // If python is available, should return true for built-in modules
        if which::which("python3").is_ok() || which::which("python").is_ok() {
            let result = check_python_module("sys");
            assert!(result);
        }
    }

    // TC-BATCH-API-001: Batch response serialization
    #[test]
    fn test_batch_response_serialize() {
        let id = Uuid::new_v4();
        let response = BatchResponse {
            batch_id: id,
            status: "processing".to_string(),
            job_count: 5,
            created_at: "2024-01-01T00:00:00Z".to_string(),
        };
        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains(&id.to_string()));
        assert!(json.contains("\"status\":\"processing\""));
        assert!(json.contains("\"job_count\":5"));
    }

    // TC-BATCH-API-002: Batch status response serialization
    #[test]
    fn test_batch_status_response_serialize() {
        let id = Uuid::new_v4();
        let response = BatchStatusResponse {
            batch_id: id,
            status: "processing".to_string(),
            progress: BatchProgress {
                completed: 3,
                processing: 1,
                pending: 1,
                failed: 0,
                total: 5,
            },
            created_at: "2024-01-01T00:00:00Z".to_string(),
            started_at: Some("2024-01-01T00:00:10Z".to_string()),
            completed_at: None,
        };
        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("\"completed\":3"));
        assert!(json.contains("\"total\":5"));
        assert!(json.contains("\"started_at\""));
        assert!(!json.contains("\"completed_at\""));
    }

    // TC-BATCH-API-003: Batch jobs response serialization
    #[test]
    fn test_batch_jobs_response_serialize() {
        let batch_id = Uuid::new_v4();
        let job_id = Uuid::new_v4();
        let response = BatchJobsResponse {
            batch_id,
            jobs: vec![BatchJobInfo {
                job_id,
                filename: "test.pdf".to_string(),
                status: "completed".to_string(),
                download_url: Some("/api/jobs/123/download".to_string()),
                progress: None,
            }],
        };
        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains(&job_id.to_string()));
        assert!(json.contains("\"filename\":\"test.pdf\""));
        assert!(json.contains("\"download_url\""));
    }

    // TC-BATCH-API-004: Batch cancel response serialization
    #[test]
    fn test_batch_cancel_response_serialize() {
        let id = Uuid::new_v4();
        let response = BatchCancelResponse {
            batch_id: id,
            status: "cancelled".to_string(),
            cancelled_jobs: 2,
            completed_jobs: 3,
        };
        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("\"status\":\"cancelled\""));
        assert!(json.contains("\"cancelled_jobs\":2"));
        assert!(json.contains("\"completed_jobs\":3"));
    }

    // TC-BATCH-API-005: Job progress info serialization
    #[test]
    fn test_job_progress_info_serialize() {
        let info = JobProgressInfo {
            percent: 45,
            step_name: "Deskew".to_string(),
        };
        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("\"percent\":45"));
        assert!(json.contains("\"step_name\":\"Deskew\""));
    }

    // TC-BATCH-API-006: Batch request deserialization
    #[test]
    fn test_batch_request_deserialize() {
        let json = r#"{"options":{"dpi":300},"priority":"high"}"#;
        let request: BatchRequest = serde_json::from_str(json).unwrap();
        assert_eq!(request.options.dpi, 300);
        assert_eq!(request.priority, Priority::High);
    }

    // TC-BATCH-API-007: Batch request with defaults
    #[test]
    fn test_batch_request_defaults() {
        let json = r#"{}"#;
        let request: BatchRequest = serde_json::from_str(json).unwrap();
        assert_eq!(request.priority, Priority::Normal);
    }

    // TC-BATCH-API-008: AppState includes batch queue
    #[tokio::test]
    async fn test_app_state_has_batch_queue() {
        let work_dir = std::env::temp_dir().join("superbook_test_batch_state");
        let state = AppState::new(work_dir.clone(), 1);
        assert_eq!(state.batch_queue.active_count().await, 0);
        std::fs::remove_dir_all(&work_dir).ok();
    }

    // TC-RATE-001: AppState includes rate limiter
    #[tokio::test]
    async fn test_app_state_has_rate_limiter() {
        let work_dir = std::env::temp_dir().join("superbook_test_rate_state");
        let state = AppState::new(work_dir.clone(), 1);
        assert!(state.rate_limiter.is_enabled());
        std::fs::remove_dir_all(&work_dir).ok();
    }

    // TC-RATE-002: AppState with custom rate limit config
    #[tokio::test]
    async fn test_app_state_with_rate_limit() {
        let work_dir = std::env::temp_dir().join("superbook_test_rate_custom");
        let config = RateLimitConfig {
            requests_per_minute: 120,
            burst_size: 20,
            enabled: true,
            ..Default::default()
        };
        let state = AppState::new_with_rate_limit(work_dir.clone(), 1, config);
        assert_eq!(state.rate_limiter.requests_per_minute(), 120);
        assert_eq!(state.rate_limiter.burst_size(), 20);
        std::fs::remove_dir_all(&work_dir).ok();
    }

    // TC-RATE-003: Rate limit status serialization
    #[test]
    fn test_rate_limit_status_serialize() {
        let status = RateLimitStatus {
            enabled: true,
            requests_per_minute: 60,
            burst_size: 10,
            your_remaining: 55,
            reset_at: 1704067200,
        };
        let json = serde_json::to_string(&status).unwrap();
        assert!(json.contains("\"enabled\":true"));
        assert!(json.contains("\"requests_per_minute\":60"));
        assert!(json.contains("\"your_remaining\":55"));
    }

    // TC-RATE-004: Rate limit error serialization
    #[test]
    fn test_rate_limit_error_serialize() {
        let error = RateLimitError::new(60);
        let json = serde_json::to_string(&error).unwrap();
        assert!(json.contains("\"error\":\"rate_limit_exceeded\""));
        assert!(json.contains("\"retry_after\":60"));
    }

    // TC-RATE-005: Check rate limit allowed
    #[test]
    fn test_check_rate_limit_allowed() {
        let config = RateLimitConfig::default();
        let limiter = RateLimiter::new(config);
        let ip: IpAddr = "192.168.1.1".parse().unwrap();

        let result = check_rate_limit(&limiter, ip);
        assert!(result.is_none()); // Request allowed
    }

    // TC-RATE-006: Check rate limit exceeded
    #[test]
    fn test_check_rate_limit_exceeded() {
        let config = RateLimitConfig {
            burst_size: 1,
            requests_per_minute: 1,
            ..Default::default()
        };
        let limiter = RateLimiter::new(config);
        let ip: IpAddr = "192.168.1.1".parse().unwrap();

        // First request allowed
        let _ = check_rate_limit(&limiter, ip);

        // Second request limited
        let result = check_rate_limit(&limiter, ip);
        assert!(result.is_some());

        let (status, _headers, _body) = result.unwrap();
        assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    }

    // TC-RATE-007: AppError TooManyRequests
    #[test]
    fn test_app_error_too_many_requests() {
        let error = AppError::TooManyRequests { retry_after: 60 };
        let response = error.into_response();
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    }

    // TC-RATE-008: Disabled rate limiter always allows
    #[test]
    fn test_rate_limit_disabled() {
        let config = RateLimitConfig {
            enabled: false,
            burst_size: 1,
            ..Default::default()
        };
        let limiter = RateLimiter::new(config);
        let ip: IpAddr = "192.168.1.1".parse().unwrap();

        // All requests allowed when disabled
        for _ in 0..100 {
            let result = check_rate_limit(&limiter, ip);
            assert!(result.is_none());
        }
    }

    // TC-AUTH-001: AppState includes auth manager
    #[tokio::test]
    async fn test_app_state_has_auth_manager() {
        let work_dir = std::env::temp_dir().join("superbook_test_auth_state");
        let state = AppState::new(work_dir.clone(), 1);
        assert!(!state.auth_manager.is_enabled()); // Default is disabled
        std::fs::remove_dir_all(&work_dir).ok();
    }

    // TC-AUTH-002: AppState with custom auth config
    #[tokio::test]
    async fn test_app_state_with_auth_config() {
        use crate::web::auth::ApiKey;

        let work_dir = std::env::temp_dir().join("superbook_test_auth_custom");
        let keys = vec![ApiKey::new("test-key", "Test")];
        let auth_config = AuthConfig::enabled_with_keys(keys);
        let state = AppState::new_with_config(
            work_dir.clone(),
            1,
            RateLimitConfig::default(),
            auth_config,
        );
        assert!(state.auth_manager.is_enabled());
        assert_eq!(state.auth_manager.key_count(), 1);
        std::fs::remove_dir_all(&work_dir).ok();
    }

    // TC-AUTH-003: Auth status response serialization
    #[test]
    fn test_auth_status_response_serialize() {
        use crate::web::auth::Scope;
        let response = AuthStatusResponse::authenticated(
            "my-key".to_string(),
            vec![Scope::Read, Scope::Write],
        );
        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("\"authenticated\":true"));
        assert!(json.contains("\"key_name\":\"my-key\""));
    }
}
