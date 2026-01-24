//! Background worker for processing PDF conversion jobs
//!
//! Handles the actual PDF conversion in a background task.

use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::mpsc;
use uuid::Uuid;

use super::job::{JobQueue, JobStatus, Progress};

/// Worker message types
#[derive(Debug)]
pub enum WorkerMessage {
    /// Process a job with the given ID and input file path
    Process {
        job_id: Uuid,
        input_path: PathBuf,
    },
    /// Shutdown the worker
    Shutdown,
}

/// Background worker for job processing
pub struct JobWorker {
    queue: JobQueue,
    receiver: mpsc::Receiver<WorkerMessage>,
    work_dir: PathBuf,
}

impl JobWorker {
    /// Create a new worker
    pub fn new(
        queue: JobQueue,
        receiver: mpsc::Receiver<WorkerMessage>,
        work_dir: PathBuf,
    ) -> Self {
        Self {
            queue,
            receiver,
            work_dir,
        }
    }

    /// Run the worker loop
    pub async fn run(mut self) {
        while let Some(msg) = self.receiver.recv().await {
            match msg {
                WorkerMessage::Process { job_id, input_path } => {
                    self.process_job(job_id, input_path).await;
                }
                WorkerMessage::Shutdown => {
                    break;
                }
            }
        }
    }

    /// Process a single job
    async fn process_job(&self, job_id: Uuid, _input_path: PathBuf) {
        // Mark job as processing
        self.queue.update(job_id, |job| {
            job.start();
            job.update_progress(Progress::new(1, 12, "Starting"));
        });

        // Simulate processing steps (TODO: integrate with actual pipeline)
        let steps = [
            "PDF Reading",
            "Image Extraction",
            "Deskew Detection",
            "Margin Trimming",
            "AI Upscaling",
            "Normalization",
            "Color Correction",
            "Group Crop",
            "Page Offset",
            "Finalize",
            "PDF Generation",
            "Complete",
        ];

        for (i, step) in steps.iter().enumerate() {
            // Check if job was cancelled
            if let Some(job) = self.queue.get(job_id) {
                if job.status == JobStatus::Cancelled {
                    return;
                }
            }

            // Update progress
            self.queue.update(job_id, |job| {
                job.update_progress(Progress::new((i + 1) as u32, 12, *step));
            });

            // Simulate processing time
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        }

        // Mark as complete (TODO: actual output path)
        let output_path = self.work_dir.join(format!("{}_converted.pdf", job_id));

        // For now, just create an empty file as placeholder
        if let Err(e) = std::fs::write(&output_path, b"placeholder") {
            self.queue.update(job_id, |job| {
                job.fail(format!("Failed to write output: {}", e));
            });
            return;
        }

        self.queue.update(job_id, |job| {
            job.complete(output_path);
        });
    }
}

/// Worker pool for managing multiple workers
pub struct WorkerPool {
    sender: mpsc::Sender<WorkerMessage>,
    work_dir: PathBuf,
}

impl WorkerPool {
    /// Create a new worker pool
    pub fn new(queue: JobQueue, work_dir: PathBuf, worker_count: usize) -> Self {
        let (sender, receiver) = mpsc::channel::<WorkerMessage>(100);

        // Spawn workers
        let receiver = Arc::new(tokio::sync::Mutex::new(receiver));

        for _ in 0..worker_count {
            let queue = queue.clone();
            let work_dir = work_dir.clone();
            let receiver = receiver.clone();

            tokio::spawn(async move {
                loop {
                    let msg = {
                        let mut rx = receiver.lock().await;
                        rx.recv().await
                    };

                    match msg {
                        Some(WorkerMessage::Process { job_id, input_path }) => {
                            // Create a temporary worker for this job
                            let (_, dummy_rx) = mpsc::channel(1);
                            let worker = JobWorker::new(queue.clone(), dummy_rx, work_dir.clone());
                            worker.process_job(job_id, input_path).await;
                        }
                        Some(WorkerMessage::Shutdown) | None => {
                            break;
                        }
                    }
                }
            });
        }

        Self { sender, work_dir }
    }

    /// Submit a job for processing
    pub async fn submit(&self, job_id: Uuid, input_path: PathBuf) -> Result<(), String> {
        self.sender
            .send(WorkerMessage::Process { job_id, input_path })
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::web::job::{ConvertOptions, Job};

    #[tokio::test]
    async fn test_worker_message_debug() {
        let msg = WorkerMessage::Process {
            job_id: Uuid::new_v4(),
            input_path: PathBuf::from("/test.pdf"),
        };
        let debug = format!("{:?}", msg);
        assert!(debug.contains("Process"));
    }

    #[tokio::test]
    async fn test_worker_pool_creation() {
        let queue = JobQueue::new();
        let work_dir = std::env::temp_dir();
        let _pool = WorkerPool::new(queue, work_dir, 2);
        // Pool created successfully
    }

    #[tokio::test]
    async fn test_job_processing() {
        let queue = JobQueue::new();
        let work_dir = std::env::temp_dir().join("superbook_test");
        std::fs::create_dir_all(&work_dir).ok();

        let pool = WorkerPool::new(queue.clone(), work_dir.clone(), 1);

        // Create a job
        let job = Job::new("test.pdf", ConvertOptions::default());
        let job_id = job.id;
        queue.submit(job);

        // Submit for processing
        let input_path = work_dir.join("test_input.pdf");
        std::fs::write(&input_path, b"test pdf content").ok();

        pool.submit(job_id, input_path).await.unwrap();

        // Wait a bit for processing
        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

        // Check job status
        let job = queue.get(job_id).unwrap();
        assert!(
            job.status == JobStatus::Completed || job.status == JobStatus::Processing,
            "Job should be completed or processing, got {:?}",
            job.status
        );

        // Cleanup
        std::fs::remove_dir_all(&work_dir).ok();
    }
}
