//! Background worker for processing PDF conversion jobs
//!
//! Handles the actual PDF conversion in a background task.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::sync::mpsc;
use uuid::Uuid;

use super::job::{ConvertOptions, JobQueue, JobStatus, Progress};
use super::websocket::WsBroadcaster;
use crate::pipeline::{PdfPipeline, PipelineConfig, ProgressCallback};

/// Worker message types
#[derive(Debug)]
pub enum WorkerMessage {
    /// Process a job with the given ID and input file path
    Process {
        job_id: Uuid,
        input_path: PathBuf,
        options: ConvertOptions,
    },
    /// Shutdown the worker
    Shutdown,
}

/// Progress callback that updates the job queue
struct WebProgressCallback {
    job_id: Uuid,
    queue: JobQueue,
    current_step: AtomicU32,
    total_steps: AtomicU32,
    step_progress: AtomicUsize,
    step_total: AtomicUsize,
}

impl WebProgressCallback {
    fn new(job_id: Uuid, queue: JobQueue) -> Self {
        Self {
            job_id,
            queue,
            current_step: AtomicU32::new(1),
            total_steps: AtomicU32::new(13),
            step_progress: AtomicUsize::new(0),
            step_total: AtomicUsize::new(0),
        }
    }
}

impl ProgressCallback for WebProgressCallback {
    fn on_step_start(&self, step: &str) {
        let current = self.current_step.fetch_add(1, Ordering::Relaxed);
        let total = self.total_steps.load(Ordering::Relaxed);
        self.step_progress.store(0, Ordering::Relaxed);
        self.step_total.store(0, Ordering::Relaxed);

        let progress = Progress::new(current, total, step);
        self.queue.update(self.job_id, |job| {
            job.update_progress(progress);
        });
    }

    fn on_step_progress(&self, current: usize, total: usize) {
        self.step_progress.store(current, Ordering::Relaxed);
        self.step_total.store(total, Ordering::Relaxed);
    }

    fn on_step_complete(&self, _step: &str, _message: &str) {
        // Step completion is handled by on_step_start of next step
    }

    fn on_debug(&self, _message: &str) {
        // Debug messages not shown in web UI
    }
}

/// Convert web ConvertOptions to pipeline PipelineConfig
fn to_pipeline_config(options: &ConvertOptions) -> PipelineConfig {
    let advanced = options.advanced;
    PipelineConfig {
        dpi: options.dpi,
        deskew: options.deskew,
        margin_trim: 0.5,
        upscale: options.upscale,
        gpu: true,
        internal_resolution: advanced,
        color_correction: advanced,
        offset_alignment: advanced,
        output_height: 3508,
        ocr: options.ocr,
        max_pages: None,
        save_debug: false,
        jpeg_quality: 90,
        threads: None,
        max_memory_mb: 0,  // Auto-detect
        chunk_size: 0,    // Auto-calculate
    }
}

/// Background worker for job processing
pub struct JobWorker {
    queue: JobQueue,
    receiver: mpsc::Receiver<WorkerMessage>,
    work_dir: PathBuf,
    broadcaster: Arc<WsBroadcaster>,
}

impl JobWorker {
    /// Create a new worker
    pub fn new(
        queue: JobQueue,
        receiver: mpsc::Receiver<WorkerMessage>,
        work_dir: PathBuf,
        broadcaster: Arc<WsBroadcaster>,
    ) -> Self {
        Self {
            queue,
            receiver,
            work_dir,
            broadcaster,
        }
    }

    /// Run the worker loop
    pub async fn run(mut self) {
        while let Some(msg) = self.receiver.recv().await {
            match msg {
                WorkerMessage::Process {
                    job_id,
                    input_path,
                    options,
                } => {
                    self.process_job(job_id, input_path, options).await;
                }
                WorkerMessage::Shutdown => {
                    break;
                }
            }
        }
    }

    /// Process a single job with actual pipeline
    pub async fn process_job(&self, job_id: Uuid, input_path: PathBuf, options: ConvertOptions) {
        // Mark job as processing
        self.queue.update(job_id, |job| {
            job.start();
            job.update_progress(Progress::new(1, 13, "Starting"));
        });

        // Broadcast status change via WebSocket
        self.broadcaster
            .broadcast_status_change(job_id, JobStatus::Queued, JobStatus::Processing)
            .await;
        self.broadcaster
            .broadcast_progress(job_id, 1, 13, "Starting")
            .await;

        // Check if job was cancelled
        if let Some(job) = self.queue.get(job_id) {
            if job.status == JobStatus::Cancelled {
                return;
            }
        }

        // Create output directory
        let output_dir = self.work_dir.join("output");
        if let Err(e) = std::fs::create_dir_all(&output_dir) {
            let error_msg = format!("Failed to create output directory: {}", e);
            self.queue.update(job_id, |job| {
                job.fail(error_msg.clone());
            });
            self.broadcaster.broadcast_error(job_id, &error_msg).await;
            return;
        }

        // Convert options to pipeline config
        let config = to_pipeline_config(&options);
        let pipeline = PdfPipeline::new(config);

        // Create progress callback
        let progress = WebProgressCallback::new(job_id, self.queue.clone());

        // Run pipeline in blocking task (pipeline uses rayon internally)
        let queue = self.queue.clone();
        let broadcaster = self.broadcaster.clone();
        let result = tokio::task::spawn_blocking(move || {
            pipeline.process_with_progress(&input_path, &output_dir, &progress)
        })
        .await;

        match result {
            Ok(Ok(pipeline_result)) => {
                // Pipeline succeeded
                let page_count = pipeline_result.page_count;
                let elapsed = pipeline_result.elapsed_seconds;
                self.queue.update(job_id, |job| {
                    job.complete(pipeline_result.output_path);
                });
                // Broadcast completion via WebSocket
                self.broadcaster
                    .broadcast_completed(job_id, elapsed, page_count)
                    .await;
            }
            Ok(Err(e)) => {
                // Pipeline error
                let error_msg = format!("Pipeline error: {}", e);
                queue.update(job_id, |job| {
                    job.fail(error_msg.clone());
                });
                broadcaster.broadcast_error(job_id, &error_msg).await;
            }
            Err(e) => {
                // Task panic
                let error_msg = format!("Task panic: {}", e);
                queue.update(job_id, |job| {
                    job.fail(error_msg.clone());
                });
                broadcaster.broadcast_error(job_id, &error_msg).await;
            }
        }
    }
}

/// Worker pool for managing multiple workers
pub struct WorkerPool {
    sender: mpsc::Sender<WorkerMessage>,
    work_dir: PathBuf,
    worker_count: usize,
}

impl WorkerPool {
    /// Create a new worker pool
    pub fn new(
        queue: JobQueue,
        work_dir: PathBuf,
        worker_count: usize,
        broadcaster: Arc<WsBroadcaster>,
    ) -> Self {
        let (sender, receiver) = mpsc::channel::<WorkerMessage>(100);

        // Spawn workers
        let receiver = Arc::new(tokio::sync::Mutex::new(receiver));

        for _ in 0..worker_count {
            let queue = queue.clone();
            let work_dir = work_dir.clone();
            let receiver = receiver.clone();
            let broadcaster = broadcaster.clone();

            tokio::spawn(async move {
                loop {
                    let msg = {
                        let mut rx = receiver.lock().await;
                        rx.recv().await
                    };

                    match msg {
                        Some(WorkerMessage::Process {
                            job_id,
                            input_path,
                            options,
                        }) => {
                            // Create a temporary worker for this job
                            let (_, dummy_rx) = mpsc::channel(1);
                            let worker = JobWorker::new(
                                queue.clone(),
                                dummy_rx,
                                work_dir.clone(),
                                broadcaster.clone(),
                            );
                            worker.process_job(job_id, input_path, options).await;
                        }
                        Some(WorkerMessage::Shutdown) | None => {
                            break;
                        }
                    }
                }
            });
        }

        Self { sender, work_dir, worker_count }
    }

    /// Submit a job for processing
    pub async fn submit(
        &self,
        job_id: Uuid,
        input_path: PathBuf,
        options: ConvertOptions,
    ) -> Result<(), String> {
        self.sender
            .send(WorkerMessage::Process {
                job_id,
                input_path,
                options,
            })
            .await
            .map_err(|e| format!("Failed to submit job: {}", e))
    }

    /// Get the work directory
    pub fn work_dir(&self) -> &PathBuf {
        &self.work_dir
    }

    /// Shutdown all workers
    pub async fn shutdown(&self) {
        // Send shutdown message (workers will exit after current job)
        let _ = self.sender.send(WorkerMessage::Shutdown).await;
    }

    /// Get the number of workers
    pub fn worker_count(&self) -> usize {
        self.worker_count
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::web::job::Job;

    #[tokio::test]
    async fn test_worker_message_debug() {
        let msg = WorkerMessage::Process {
            job_id: Uuid::new_v4(),
            input_path: PathBuf::from("/test.pdf"),
            options: ConvertOptions::default(),
        };
        let debug = format!("{:?}", msg);
        assert!(debug.contains("Process"));
    }

    #[tokio::test]
    async fn test_worker_pool_creation() {
        let queue = JobQueue::new();
        let work_dir = std::env::temp_dir();
        let broadcaster = Arc::new(WsBroadcaster::new());
        let _pool = WorkerPool::new(queue, work_dir, 2, broadcaster);
        // Pool created successfully
    }

    #[tokio::test]
    async fn test_convert_options_to_pipeline_config() {
        let options = ConvertOptions::default();
        let config = to_pipeline_config(&options);

        assert_eq!(config.dpi, 300);
        assert!(config.deskew);
        assert!(config.upscale);
        assert!(!config.ocr);
        assert!(!config.internal_resolution);
    }

    #[tokio::test]
    async fn test_convert_options_advanced() {
        let options = ConvertOptions {
            dpi: 600,
            deskew: true,
            upscale: true,
            ocr: true,
            advanced: true,
        };
        let config = to_pipeline_config(&options);

        assert_eq!(config.dpi, 600);
        assert!(config.internal_resolution);
        assert!(config.color_correction);
        assert!(config.offset_alignment);
        assert!(config.ocr);
    }

    #[tokio::test]
    async fn test_job_processing_with_invalid_pdf() {
        let queue = JobQueue::new();
        let work_dir = std::env::temp_dir().join("superbook_test_worker");
        std::fs::create_dir_all(&work_dir).ok();

        let broadcaster = Arc::new(WsBroadcaster::new());
        let pool = WorkerPool::new(queue.clone(), work_dir.clone(), 1, broadcaster);

        // Create a job
        let options = ConvertOptions::default();
        let job = Job::new("test.pdf", options.clone());
        let job_id = job.id;
        queue.submit(job);

        // Submit for processing with invalid PDF
        let input_path = work_dir.join("invalid.pdf");
        std::fs::write(&input_path, b"not a valid pdf").ok();

        pool.submit(job_id, input_path, options).await.unwrap();

        // Wait for processing
        tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;

        // Job should fail because input is not a valid PDF
        let job = queue.get(job_id).unwrap();
        assert!(
            job.status == JobStatus::Failed || job.status == JobStatus::Processing,
            "Job should be failed or still processing, got {:?}",
            job.status
        );

        // Cleanup
        std::fs::remove_dir_all(&work_dir).ok();
    }
}
