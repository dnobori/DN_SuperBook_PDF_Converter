//! Job management for the web server
//!
//! Handles job creation, status tracking, and queue management.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use uuid::Uuid;

/// Job status enumeration
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum JobStatus {
    /// Waiting in queue
    Queued,
    /// Currently processing
    Processing,
    /// Successfully completed
    Completed,
    /// Failed with error
    Failed,
    /// Cancelled by user
    Cancelled,
}

impl std::fmt::Display for JobStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            JobStatus::Queued => write!(f, "queued"),
            JobStatus::Processing => write!(f, "processing"),
            JobStatus::Completed => write!(f, "completed"),
            JobStatus::Failed => write!(f, "failed"),
            JobStatus::Cancelled => write!(f, "cancelled"),
        }
    }
}

/// Progress information for a processing job
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Progress {
    /// Current step number (1-based)
    pub current_step: u32,
    /// Total number of steps
    pub total_steps: u32,
    /// Human-readable step name
    pub step_name: String,
    /// Percentage complete (0-100)
    pub percent: u8,
}

impl Progress {
    /// Create a new progress instance
    pub fn new(current_step: u32, total_steps: u32, step_name: impl Into<String>) -> Self {
        let percent = if total_steps > 0 {
            ((current_step as f64 / total_steps as f64) * 100.0) as u8
        } else {
            0
        };
        Self {
            current_step,
            total_steps,
            step_name: step_name.into(),
            percent,
        }
    }
}

/// Conversion options from the client
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConvertOptions {
    /// Output DPI
    #[serde(default = "default_dpi")]
    pub dpi: u32,
    /// Enable deskew correction
    #[serde(default = "default_true")]
    pub deskew: bool,
    /// Enable AI upscaling
    #[serde(default = "default_true")]
    pub upscale: bool,
    /// Enable OCR
    #[serde(default)]
    pub ocr: bool,
    /// Enable all advanced features
    #[serde(default)]
    pub advanced: bool,
}

fn default_dpi() -> u32 {
    300
}

fn default_true() -> bool {
    true
}

impl Default for ConvertOptions {
    fn default() -> Self {
        Self {
            dpi: 300,
            deskew: true,
            upscale: true,
            ocr: false,
            advanced: false,
        }
    }
}

/// A conversion job
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Job {
    /// Unique job identifier
    pub id: Uuid,
    /// Current job status
    pub status: JobStatus,
    /// Conversion options
    pub options: ConvertOptions,
    /// Progress information (when processing)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub progress: Option<Progress>,
    /// Original input filename
    pub input_filename: String,
    /// Path to output file (when completed)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_path: Option<PathBuf>,
    /// Job creation timestamp
    pub created_at: DateTime<Utc>,
    /// Processing start timestamp
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at: Option<DateTime<Utc>>,
    /// Completion timestamp
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<DateTime<Utc>>,
    /// Error message (when failed)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl Job {
    /// Create a new job
    pub fn new(input_filename: impl Into<String>, options: ConvertOptions) -> Self {
        Self {
            id: Uuid::new_v4(),
            status: JobStatus::Queued,
            options,
            progress: None,
            input_filename: input_filename.into(),
            output_path: None,
            created_at: Utc::now(),
            started_at: None,
            completed_at: None,
            error: None,
        }
    }

    /// Mark job as processing
    pub fn start(&mut self) {
        self.status = JobStatus::Processing;
        self.started_at = Some(Utc::now());
    }

    /// Update progress
    pub fn update_progress(&mut self, progress: Progress) {
        self.progress = Some(progress);
    }

    /// Mark job as completed
    pub fn complete(&mut self, output_path: PathBuf) {
        self.status = JobStatus::Completed;
        self.output_path = Some(output_path);
        self.completed_at = Some(Utc::now());
        self.progress = Some(Progress::new(12, 12, "Complete"));
    }

    /// Mark job as failed
    pub fn fail(&mut self, error: impl Into<String>) {
        self.status = JobStatus::Failed;
        self.error = Some(error.into());
        self.completed_at = Some(Utc::now());
    }

    /// Mark job as cancelled
    pub fn cancel(&mut self) {
        self.status = JobStatus::Cancelled;
        self.completed_at = Some(Utc::now());
    }

    /// Check if job is in terminal state
    pub fn is_terminal(&self) -> bool {
        matches!(
            self.status,
            JobStatus::Completed | JobStatus::Failed | JobStatus::Cancelled
        )
    }
}

/// Thread-safe job queue
#[derive(Debug, Clone)]
pub struct JobQueue {
    jobs: Arc<RwLock<HashMap<Uuid, Job>>>,
}

impl Default for JobQueue {
    fn default() -> Self {
        Self::new()
    }
}

impl JobQueue {
    /// Create a new job queue
    pub fn new() -> Self {
        Self {
            jobs: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Submit a new job
    pub fn submit(&self, job: Job) -> Uuid {
        let id = job.id;
        let mut jobs = self.jobs.write().expect("lock poisoned");
        jobs.insert(id, job);
        id
    }

    /// Get a job by ID
    pub fn get(&self, id: Uuid) -> Option<Job> {
        let jobs = self.jobs.read().expect("lock poisoned");
        jobs.get(&id).cloned()
    }

    /// Update a job
    pub fn update<F>(&self, id: Uuid, f: F) -> Option<Job>
    where
        F: FnOnce(&mut Job),
    {
        let mut jobs = self.jobs.write().expect("lock poisoned");
        if let Some(job) = jobs.get_mut(&id) {
            f(job);
            Some(job.clone())
        } else {
            None
        }
    }

    /// Cancel a job
    pub fn cancel(&self, id: Uuid) -> Option<Job> {
        self.update(id, |job| job.cancel())
    }

    /// List all jobs
    pub fn list(&self) -> Vec<Job> {
        let jobs = self.jobs.read().expect("lock poisoned");
        jobs.values().cloned().collect()
    }

    /// Get pending jobs (queued or processing)
    pub fn pending(&self) -> Vec<Job> {
        let jobs = self.jobs.read().expect("lock poisoned");
        jobs.values()
            .filter(|j| matches!(j.status, JobStatus::Queued | JobStatus::Processing))
            .cloned()
            .collect()
    }

    /// Get count of pending jobs (queued or processing)
    pub fn pending_count(&self) -> usize {
        let jobs = self.jobs.read().expect("lock poisoned");
        jobs.values()
            .filter(|j| matches!(j.status, JobStatus::Queued | JobStatus::Processing))
            .count()
    }

    /// Remove completed/failed jobs older than the given duration
    pub fn cleanup(&self, max_age: std::time::Duration) {
        let mut jobs = self.jobs.write().expect("lock poisoned");
        let now = Utc::now();
        jobs.retain(|_, job| {
            if job.is_terminal() {
                if let Some(completed_at) = job.completed_at {
                    let age = now.signed_duration_since(completed_at);
                    return age < chrono::Duration::from_std(max_age).unwrap_or(chrono::TimeDelta::MAX);
                }
            }
            true
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_job_status_display() {
        assert_eq!(format!("{}", JobStatus::Queued), "queued");
        assert_eq!(format!("{}", JobStatus::Processing), "processing");
        assert_eq!(format!("{}", JobStatus::Completed), "completed");
        assert_eq!(format!("{}", JobStatus::Failed), "failed");
        assert_eq!(format!("{}", JobStatus::Cancelled), "cancelled");
    }

    #[test]
    fn test_progress_new() {
        let progress = Progress::new(3, 12, "Deskew");
        assert_eq!(progress.current_step, 3);
        assert_eq!(progress.total_steps, 12);
        assert_eq!(progress.step_name, "Deskew");
        assert_eq!(progress.percent, 25);
    }

    #[test]
    fn test_progress_zero_total() {
        let progress = Progress::new(0, 0, "Empty");
        assert_eq!(progress.percent, 0);
    }

    #[test]
    fn test_convert_options_default() {
        let opts = ConvertOptions::default();
        assert_eq!(opts.dpi, 300);
        assert!(opts.deskew);
        assert!(opts.upscale);
        assert!(!opts.ocr);
        assert!(!opts.advanced);
    }

    #[test]
    fn test_job_new() {
        let job = Job::new("test.pdf", ConvertOptions::default());
        assert_eq!(job.status, JobStatus::Queued);
        assert_eq!(job.input_filename, "test.pdf");
        assert!(job.progress.is_none());
        assert!(job.output_path.is_none());
        assert!(job.started_at.is_none());
        assert!(job.completed_at.is_none());
        assert!(job.error.is_none());
    }

    #[test]
    fn test_job_lifecycle() {
        let mut job = Job::new("test.pdf", ConvertOptions::default());

        // Start
        job.start();
        assert_eq!(job.status, JobStatus::Processing);
        assert!(job.started_at.is_some());

        // Update progress
        job.update_progress(Progress::new(5, 12, "Processing"));
        assert!(job.progress.is_some());
        assert_eq!(job.progress.as_ref().unwrap().current_step, 5);

        // Complete
        job.complete(PathBuf::from("/tmp/output.pdf"));
        assert_eq!(job.status, JobStatus::Completed);
        assert!(job.output_path.is_some());
        assert!(job.completed_at.is_some());
        assert!(job.is_terminal());
    }

    #[test]
    fn test_job_fail() {
        let mut job = Job::new("test.pdf", ConvertOptions::default());
        job.start();
        job.fail("Something went wrong");

        assert_eq!(job.status, JobStatus::Failed);
        assert_eq!(job.error.as_deref(), Some("Something went wrong"));
        assert!(job.is_terminal());
    }

    #[test]
    fn test_job_cancel() {
        let mut job = Job::new("test.pdf", ConvertOptions::default());
        job.cancel();

        assert_eq!(job.status, JobStatus::Cancelled);
        assert!(job.is_terminal());
    }

    #[test]
    fn test_job_queue_submit_and_get() {
        let queue = JobQueue::new();
        let job = Job::new("test.pdf", ConvertOptions::default());
        let id = job.id;

        queue.submit(job);

        let retrieved = queue.get(id);
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().id, id);
    }

    #[test]
    fn test_job_queue_get_nonexistent() {
        let queue = JobQueue::new();
        let id = Uuid::new_v4();

        assert!(queue.get(id).is_none());
    }

    #[test]
    fn test_job_queue_update() {
        let queue = JobQueue::new();
        let job = Job::new("test.pdf", ConvertOptions::default());
        let id = job.id;

        queue.submit(job);

        let updated = queue.update(id, |j| j.start());
        assert!(updated.is_some());
        assert_eq!(updated.unwrap().status, JobStatus::Processing);
    }

    #[test]
    fn test_job_queue_cancel() {
        let queue = JobQueue::new();
        let job = Job::new("test.pdf", ConvertOptions::default());
        let id = job.id;

        queue.submit(job);

        let cancelled = queue.cancel(id);
        assert!(cancelled.is_some());
        assert_eq!(cancelled.unwrap().status, JobStatus::Cancelled);
    }

    #[test]
    fn test_job_queue_list() {
        let queue = JobQueue::new();

        queue.submit(Job::new("test1.pdf", ConvertOptions::default()));
        queue.submit(Job::new("test2.pdf", ConvertOptions::default()));

        let jobs = queue.list();
        assert_eq!(jobs.len(), 2);
    }

    #[test]
    fn test_job_queue_pending() {
        let queue = JobQueue::new();

        let mut job1 = Job::new("test1.pdf", ConvertOptions::default());
        let job2 = Job::new("test2.pdf", ConvertOptions::default());
        let mut job3 = Job::new("test3.pdf", ConvertOptions::default());

        job1.complete(PathBuf::from("/tmp/out1.pdf"));
        job3.start();

        queue.submit(job1);
        queue.submit(job2);
        queue.submit(job3);

        let pending = queue.pending();
        assert_eq!(pending.len(), 2); // job2 (queued) + job3 (processing)
    }

    #[test]
    fn test_job_queue_pending_count() {
        let queue = JobQueue::new();

        let mut job1 = Job::new("test1.pdf", ConvertOptions::default());
        let job2 = Job::new("test2.pdf", ConvertOptions::default());
        let mut job3 = Job::new("test3.pdf", ConvertOptions::default());

        job1.complete(PathBuf::from("/tmp/out1.pdf"));
        job3.start();

        queue.submit(job1);
        queue.submit(job2);
        queue.submit(job3);

        // pending_count should be 2 (job2 queued + job3 processing)
        assert_eq!(queue.pending_count(), 2);
    }

    #[test]
    fn test_convert_options_serde() {
        let json = r#"{"dpi": 600, "ocr": true}"#;
        let opts: ConvertOptions = serde_json::from_str(json).unwrap();

        assert_eq!(opts.dpi, 600);
        assert!(opts.deskew); // default
        assert!(opts.upscale); // default
        assert!(opts.ocr);
        assert!(!opts.advanced); // default
    }
}
