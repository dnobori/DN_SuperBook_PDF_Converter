//! REST API routes for the web server
//!
//! Provides endpoints for PDF conversion, job management, and health checks.

use axum::{
    extract::{Multipart, Path, State},
    http::StatusCode,
    response::{IntoResponse, Json},
    routing::{delete, get, post},
    Router,
};
use serde::Serialize;
use std::sync::Arc;
use uuid::Uuid;

use super::job::{ConvertOptions, Job, JobQueue};
use super::worker::WorkerPool;

use std::path::PathBuf;

/// Application state shared across handlers
pub struct AppState {
    pub queue: JobQueue,
    pub version: String,
    pub worker_pool: WorkerPool,
    pub upload_dir: PathBuf,
}

impl AppState {
    pub fn new(work_dir: PathBuf, worker_count: usize) -> Self {
        let queue = JobQueue::new();
        let upload_dir = work_dir.join("uploads");
        std::fs::create_dir_all(&upload_dir).ok();

        let worker_pool = WorkerPool::new(queue.clone(), work_dir, worker_count);

        Self {
            queue,
            version: env!("CARGO_PKG_VERSION").to_string(),
            worker_pool,
            upload_dir,
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
        .route("/health", get(health_check))
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
}

/// Health check endpoint
async fn health_check(State(state): State<Arc<AppState>>) -> Json<HealthResponse> {
    let tools = ToolStatus {
        poppler: which::which("pdftoppm").is_ok(),
        tesseract: which::which("tesseract").is_ok(),
        realesrgan: false, // TODO: Check Python availability
    };

    Json(HealthResponse {
        status: "healthy".to_string(),
        version: state.version.clone(),
        tools,
    })
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
    let job = Job::new(&filename, options);
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
    if let Err(e) = state.worker_pool.submit(job_id, input_path).await {
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

/// API error type
#[derive(Debug)]
pub enum AppError {
    BadRequest(String),
    NotFound(String),
    Conflict(String),
    Internal(String),
}

impl IntoResponse for AppError {
    fn into_response(self) -> axum::response::Response {
        #[derive(Serialize)]
        struct ErrorResponse {
            error: String,
        }

        let (status, message) = match self {
            AppError::BadRequest(msg) => (StatusCode::BAD_REQUEST, msg),
            AppError::NotFound(msg) => (StatusCode::NOT_FOUND, msg),
            AppError::Conflict(msg) => (StatusCode::CONFLICT, msg),
            AppError::Internal(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg),
        };

        (status, Json(ErrorResponse { error: message })).into_response()
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
        };
        let json = serde_json::to_string(&status).unwrap();
        assert!(json.contains("\"poppler\":true"));
        assert!(json.contains("\"tesseract\":false"));
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
}
