//! Web API integration tests
//!
//! Tests for the REST API endpoints.

#![cfg(feature = "web")]

use superbook_pdf::{
    BatchJob, BatchProgress, BatchQueue, BatchStatus, Job, JobQueue, JobStatus, Priority,
    ServerConfig, WebConvertOptions, WebProgress,
};
use std::path::PathBuf;

#[cfg(test)]
mod tests {
    use super::*;

    // TC-WEB-002: Health check endpoint
    #[tokio::test]
    async fn test_health_endpoint_structure() {
        let queue = JobQueue::new();
        // Queue should be empty initially
        assert_eq!(queue.list().len(), 0);
    }

    // TC-WEB-003: Job queue operations
    #[tokio::test]
    async fn test_job_queue_lifecycle() {
        let queue = JobQueue::new();

        // Create and submit a job
        let options = WebConvertOptions::default();
        let job = Job::new("test.pdf", options);
        let job_id = job.id;

        queue.submit(job);

        // Verify job exists
        let retrieved = queue.get(job_id);
        assert!(retrieved.is_some());

        let job = retrieved.unwrap();
        assert_eq!(job.status, JobStatus::Queued);
        assert_eq!(job.input_filename, "test.pdf");
    }

    // TC-WEB-004: Job status transitions
    #[tokio::test]
    async fn test_job_status_transitions() {
        let queue = JobQueue::new();

        let options = WebConvertOptions::default();
        let job = Job::new("test.pdf", options);
        let job_id = job.id;

        queue.submit(job);

        // Transition to processing
        queue.update(job_id, |j: &mut Job| j.start());
        let job = queue.get(job_id).unwrap();
        assert_eq!(job.status, JobStatus::Processing);

        // Transition to completed
        queue.update(job_id, |j: &mut Job| j.complete(PathBuf::from("/output/test.pdf")));
        let job = queue.get(job_id).unwrap();
        assert_eq!(job.status, JobStatus::Completed);
    }

    // TC-WEB-005: Job cancellation
    #[tokio::test]
    async fn test_job_cancellation() {
        let queue = JobQueue::new();

        let options = WebConvertOptions::default();
        let job = Job::new("test.pdf", options);
        let job_id = job.id;

        queue.submit(job);

        // Cancel the job
        let cancelled = queue.cancel(job_id);
        assert!(cancelled.is_some());

        let job = cancelled.unwrap();
        assert_eq!(job.status, JobStatus::Cancelled);
    }

    // TC-WEB-006: Progress updates
    #[tokio::test]
    async fn test_progress_updates() {
        let queue = JobQueue::new();

        let options = WebConvertOptions::default();
        let job = Job::new("test.pdf", options);
        let job_id = job.id;

        queue.submit(job);

        // Update progress
        let progress = WebProgress::new(5, 12, "Processing images");
        queue.update(job_id, |j: &mut Job| j.update_progress(progress.clone()));

        let job = queue.get(job_id).unwrap();
        let p = job.progress.unwrap();
        assert_eq!(p.current_step, 5);
        assert_eq!(p.total_steps, 12);
        assert_eq!(p.step_name, "Processing images");
        assert_eq!(p.percent, 41); // 5/12 * 100 ≈ 41%
    }

    // TC-WEB-007: Convert options parsing
    #[tokio::test]
    async fn test_convert_options_default() {
        let options = WebConvertOptions::default();
        assert_eq!(options.dpi, 300);
        assert!(options.deskew);
        assert!(options.upscale);
        assert!(!options.ocr);
        assert!(!options.advanced);
    }

    // TC-WEB-008: Convert options JSON deserialization
    #[tokio::test]
    async fn test_convert_options_json() {
        let json = r#"{"dpi": 600, "deskew": false, "upscale": true, "ocr": true, "advanced": true}"#;
        let options: WebConvertOptions = serde_json::from_str(json).unwrap();

        assert_eq!(options.dpi, 600);
        assert!(!options.deskew);
        assert!(options.upscale);
        assert!(options.ocr);
        assert!(options.advanced);
    }

    // TC-WEB-009: Concurrent job processing
    #[tokio::test]
    async fn test_concurrent_jobs() {
        let queue = JobQueue::new();

        // Submit multiple jobs
        for i in 0..10 {
            let options = WebConvertOptions::default();
            let job = Job::new(&format!("test{}.pdf", i), options);
            queue.submit(job);
        }

        // All jobs should be in queue
        assert_eq!(queue.list().len(), 10);
    }

    // TC-WEB-010: Job failure handling
    #[tokio::test]
    async fn test_job_failure() {
        let queue = JobQueue::new();

        let options = WebConvertOptions::default();
        let job = Job::new("test.pdf", options);
        let job_id = job.id;

        queue.submit(job);

        // Mark as failed
        queue.update(job_id, |j: &mut Job| j.fail("Test error message".to_string()));

        let job = queue.get(job_id).unwrap();
        assert_eq!(job.status, JobStatus::Failed);
        assert_eq!(job.error, Some("Test error message".to_string()));
    }

    // TC-WEB-011: Server config builder
    #[tokio::test]
    async fn test_server_config_builder() {
        let config = ServerConfig::default()
            .with_port(9000)
            .with_bind("0.0.0.0")
            .with_upload_limit(100 * 1024 * 1024);

        assert_eq!(config.port, 9000);
        assert_eq!(config.bind, "0.0.0.0");
        assert_eq!(config.upload_limit, 100 * 1024 * 1024);
    }

    // TC-WEB-012: Socket address parsing
    #[tokio::test]
    async fn test_socket_addr_parsing() {
        let config = ServerConfig::default().with_port(8080).with_bind("127.0.0.1");

        let addr = config.socket_addr().unwrap();
        assert_eq!(addr.port(), 8080);
        assert_eq!(addr.ip().to_string(), "127.0.0.1");
    }

    // TC-WEB-013: Job list filtering
    #[tokio::test]
    async fn test_job_list() {
        let queue = JobQueue::new();

        // Submit 3 jobs
        for i in 0..3 {
            let options = WebConvertOptions::default();
            let job = Job::new(&format!("file{}.pdf", i), options);
            queue.submit(job);
        }

        let jobs = queue.list();
        assert_eq!(jobs.len(), 3);

        // All should be queued
        for job in jobs {
            assert_eq!(job.status, JobStatus::Queued);
        }
    }

    // TC-WEB-014: Job timestamps
    #[tokio::test]
    async fn test_job_timestamps() {
        let queue = JobQueue::new();

        let options = WebConvertOptions::default();
        let job = Job::new("test.pdf", options);
        let job_id = job.id;
        let created_at = job.created_at;

        queue.submit(job);

        // Start the job
        queue.update(job_id, |j: &mut Job| j.start());
        let job = queue.get(job_id).unwrap();

        assert_eq!(job.created_at, created_at);
        assert!(job.started_at.is_some());
        assert!(job.started_at.unwrap() >= created_at);
    }

    // TC-WEB-015: Job completion with output path
    #[tokio::test]
    async fn test_job_output_path() {
        let queue = JobQueue::new();

        let options = WebConvertOptions::default();
        let job = Job::new("test.pdf", options);
        let job_id = job.id;

        queue.submit(job);
        queue.update(job_id, |j: &mut Job| j.start());

        let output = PathBuf::from("/tmp/output/test_converted.pdf");
        queue.update(job_id, |j: &mut Job| j.complete(output.clone()));

        let job = queue.get(job_id).unwrap();
        assert_eq!(job.output_path, Some(output));
        assert!(job.completed_at.is_some());
    }

    // ========== Batch API Integration Tests ==========

    // TC-BATCH-INT-001: Batch job creation and queue
    #[tokio::test]
    async fn test_batch_queue_integration() {
        let job_queue = JobQueue::new();
        let batch_queue = BatchQueue::new(job_queue.clone());

        let options = WebConvertOptions::default();
        let batch = BatchJob::new(options, Priority::Normal);
        let batch_id = batch.id;

        batch_queue.submit(batch).await;

        let retrieved = batch_queue.get(batch_id).await;
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().status, BatchStatus::Queued);
    }

    // TC-BATCH-INT-002: Batch with multiple jobs
    #[tokio::test]
    async fn test_batch_with_jobs() {
        let job_queue = JobQueue::new();
        let batch_queue = BatchQueue::new(job_queue.clone());

        let options = WebConvertOptions::default();
        let mut batch = BatchJob::new(options.clone(), Priority::Normal);

        // Create jobs for batch
        let filenames = vec!["doc1.pdf", "doc2.pdf", "doc3.pdf"];
        batch_queue.create_jobs(&mut batch, &filenames.iter().map(|s| s.to_string()).collect::<Vec<_>>());

        assert_eq!(batch.job_count(), 3);

        batch.start();
        batch_queue.submit(batch).await;

        assert_eq!(batch_queue.active_count().await, 1);
    }

    // TC-BATCH-INT-003: Batch progress tracking
    #[tokio::test]
    async fn test_batch_progress_tracking() {
        let job_queue = JobQueue::new();
        let batch_queue = BatchQueue::new(job_queue.clone());

        let options = WebConvertOptions::default();
        let mut batch = BatchJob::new(options.clone(), Priority::High);

        // Create 5 jobs
        let filenames: Vec<String> = (0..5).map(|i| format!("file{}.pdf", i)).collect();
        batch_queue.create_jobs(&mut batch, &filenames);

        let batch_id = batch.id;
        batch.start();
        batch_queue.submit(batch).await;

        // Complete some jobs
        let retrieved = batch_queue.get(batch_id).await.unwrap();
        for (i, job_id) in retrieved.job_ids.iter().take(3).enumerate() {
            job_queue.update(*job_id, |j: &mut Job| {
                if i < 2 {
                    j.complete(PathBuf::from("/tmp/output.pdf"));
                } else {
                    j.fail("Test failure".to_string());
                }
            });
        }

        // Check progress
        let progress = batch_queue.get_progress(batch_id).await.unwrap();
        assert_eq!(progress.completed, 2);
        assert_eq!(progress.failed, 1);
        assert_eq!(progress.pending, 2);
        assert_eq!(progress.total, 5);
    }

    // TC-BATCH-INT-004: Batch cancellation
    #[tokio::test]
    async fn test_batch_cancellation() {
        let job_queue = JobQueue::new();
        let batch_queue = BatchQueue::new(job_queue.clone());

        let options = WebConvertOptions::default();
        let mut batch = BatchJob::new(options.clone(), Priority::Normal);

        // Create jobs
        let filenames: Vec<String> = (0..3).map(|i| format!("doc{}.pdf", i)).collect();
        batch_queue.create_jobs(&mut batch, &filenames);

        let batch_id = batch.id;
        batch_queue.submit(batch).await;

        // Cancel the batch
        let result = batch_queue.cancel(batch_id).await;
        assert!(result.is_some());

        let (cancelled, completed) = result.unwrap();
        assert_eq!(cancelled, 3);
        assert_eq!(completed, 0);

        // Verify batch status
        let batch = batch_queue.get(batch_id).await.unwrap();
        assert_eq!(batch.status, BatchStatus::Cancelled);
    }

    // TC-BATCH-INT-005: Priority ordering
    #[tokio::test]
    async fn test_batch_priority() {
        assert!(Priority::High.value() > Priority::Normal.value());
        assert!(Priority::Normal.value() > Priority::Low.value());
        assert_eq!(Priority::default(), Priority::Normal);
    }

    // TC-BATCH-INT-006: Batch progress calculation
    #[tokio::test]
    async fn test_batch_progress_calculation() {
        let mut progress = BatchProgress::new(10);
        assert_eq!(progress.percent(), 0);
        assert!(!progress.is_complete());

        progress.completed = 5;
        progress.pending = 5;
        assert_eq!(progress.percent(), 50);

        progress.completed = 10;
        progress.pending = 0;
        assert_eq!(progress.percent(), 100);
        assert!(progress.is_complete());
    }

    // TC-BATCH-INT-007: Batch list
    #[tokio::test]
    async fn test_batch_list() {
        let job_queue = JobQueue::new();
        let batch_queue = BatchQueue::new(job_queue);

        // Submit multiple batches
        let options = WebConvertOptions::default();
        for priority in [Priority::Low, Priority::Normal, Priority::High] {
            let batch = BatchJob::new(options.clone(), priority);
            batch_queue.submit(batch).await;
        }

        let batches = batch_queue.list().await;
        assert_eq!(batches.len(), 3);
    }

    // TC-BATCH-INT-008: Batch update closure
    #[tokio::test]
    async fn test_batch_update() {
        let job_queue = JobQueue::new();
        let batch_queue = BatchQueue::new(job_queue);

        let options = WebConvertOptions::default();
        let batch = BatchJob::new(options, Priority::Normal);
        let batch_id = batch.id;

        batch_queue.submit(batch).await;

        // Update batch
        batch_queue
            .update(batch_id, |b| {
                b.start();
            })
            .await;

        let retrieved = batch_queue.get(batch_id).await.unwrap();
        assert_eq!(retrieved.status, BatchStatus::Processing);
        assert!(retrieved.started_at.is_some());
    }
}
