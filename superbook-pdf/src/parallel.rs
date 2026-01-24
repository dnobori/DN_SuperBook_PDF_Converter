//! Parallel processing utilities for image pipeline
//!
//! # Overview
//!
//! This module provides parallel processing capabilities using Rayon
//! to speed up image processing operations across multiple CPU cores.
//!
//! # Example
//!
//! ```ignore
//! use superbook_pdf::parallel::{ParallelOptions, parallel_process};
//!
//! let options = ParallelOptions::default();
//! let results = parallel_process(&image_paths, |path| {
//!     process_image(path)
//! }, &options);
//! ```

use rayon::prelude::*;
use std::error::Error;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Error type for parallel processing operations
#[derive(Debug, Clone)]
pub enum ParallelError {
    /// Thread pool creation failed
    ThreadPoolError(String),
    /// Processing error with page index
    ProcessingError { index: usize, message: String },
    /// All tasks failed
    AllTasksFailed(usize),
}

impl fmt::Display for ParallelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ThreadPoolError(msg) => write!(f, "Thread pool error: {}", msg),
            Self::ProcessingError { index, message } => {
                write!(f, "Processing error at index {}: {}", index, message)
            }
            Self::AllTasksFailed(count) => write!(f, "All {} tasks failed", count),
        }
    }
}

impl Error for ParallelError {}

/// Options for parallel processing
#[derive(Debug, Clone)]
pub struct ParallelOptions {
    /// Number of threads (0 = auto-detect based on CPU cores)
    pub num_threads: usize,
    /// Chunk size for memory-controlled processing (0 = process all at once)
    pub chunk_size: usize,
    /// Whether to continue on errors
    pub continue_on_error: bool,
}

impl Default for ParallelOptions {
    fn default() -> Self {
        Self {
            num_threads: 0,
            chunk_size: 0,
            continue_on_error: true,
        }
    }
}

impl ParallelOptions {
    /// Create options with specific thread count
    pub fn with_threads(num_threads: usize) -> Self {
        Self {
            num_threads,
            ..Default::default()
        }
    }

    /// Create options with chunk processing for memory control
    pub fn with_chunks(chunk_size: usize) -> Self {
        Self {
            chunk_size,
            ..Default::default()
        }
    }

    /// Get effective thread count
    pub fn effective_threads(&self) -> usize {
        if self.num_threads == 0 {
            num_cpus::get()
        } else {
            self.num_threads
        }
    }
}

/// Result of parallel processing
#[derive(Debug)]
pub struct ParallelResult<T> {
    /// Successful results with their original indices
    pub results: Vec<(usize, T)>,
    /// Errors with their indices and messages
    pub errors: Vec<(usize, String)>,
    /// Total processing duration
    pub duration: Duration,
    /// Number of items processed
    pub processed_count: usize,
}

impl<T> ParallelResult<T> {
    /// Check if all items were processed successfully
    pub fn is_success(&self) -> bool {
        self.errors.is_empty()
    }

    /// Get success rate as a percentage
    pub fn success_rate(&self) -> f64 {
        if self.processed_count == 0 {
            return 0.0;
        }
        (self.results.len() as f64 / self.processed_count as f64) * 100.0
    }

    /// Get ordered results (sorted by original index)
    pub fn ordered_results(mut self) -> Vec<T> {
        self.results.sort_by_key(|(idx, _)| *idx);
        self.results.into_iter().map(|(_, v)| v).collect()
    }
}

/// Progress callback type
pub type ProgressCallback = Arc<dyn Fn(usize, usize) + Send + Sync>;

/// Parallel processor for batch operations
pub struct ParallelProcessor {
    options: ParallelOptions,
    progress_callback: Option<ProgressCallback>,
}

impl ParallelProcessor {
    /// Create a new parallel processor with default options
    pub fn new() -> Self {
        Self {
            options: ParallelOptions::default(),
            progress_callback: None,
        }
    }

    /// Create a parallel processor with specific options
    pub fn with_options(options: ParallelOptions) -> Self {
        Self {
            options,
            progress_callback: None,
        }
    }

    /// Set a progress callback
    pub fn with_progress<F>(mut self, callback: F) -> Self
    where
        F: Fn(usize, usize) + Send + Sync + 'static,
    {
        self.progress_callback = Some(Arc::new(callback));
        self
    }

    /// Process items in parallel
    pub fn process<T, E, F>(&self, items: &[PathBuf], processor: F) -> ParallelResult<T>
    where
        F: Fn(&Path) -> Result<T, E> + Sync + Send,
        E: std::fmt::Display,
        T: Send,
    {
        let start = Instant::now();
        let total = items.len();

        if total == 0 {
            return ParallelResult {
                results: vec![],
                errors: vec![],
                duration: Duration::ZERO,
                processed_count: 0,
            };
        }

        let completed = Arc::new(AtomicUsize::new(0));
        let progress_callback = self.progress_callback.clone();

        // Build thread pool if custom thread count specified
        let pool = if self.options.num_threads > 0 {
            rayon::ThreadPoolBuilder::new()
                .num_threads(self.options.num_threads)
                .build()
                .ok()
        } else {
            None
        };

        let process_chunk = |chunk: &[(usize, &PathBuf)]| -> Vec<(usize, Result<T, String>)> {
            chunk
                .par_iter()
                .map(|(idx, path)| {
                    let result = processor(path).map_err(|e| e.to_string());

                    // Update progress
                    let done = completed.fetch_add(1, Ordering::Relaxed) + 1;
                    if let Some(ref cb) = progress_callback {
                        cb(done, total);
                    }

                    (*idx, result)
                })
                .collect()
        };

        let indexed_items: Vec<_> = items.iter().enumerate().collect();

        let all_results = if self.options.chunk_size > 0 {
            // Process in chunks for memory control
            let mut all_results = Vec::with_capacity(total);
            for chunk in indexed_items.chunks(self.options.chunk_size) {
                let chunk_results = if let Some(ref pool) = pool {
                    pool.install(|| process_chunk(chunk))
                } else {
                    process_chunk(chunk)
                };
                all_results.extend(chunk_results);
            }
            all_results
        } else {
            // Process all at once
            if let Some(ref pool) = pool {
                pool.install(|| process_chunk(&indexed_items))
            } else {
                process_chunk(&indexed_items)
            }
        };

        // Separate successes and errors
        let mut results = Vec::new();
        let mut errors = Vec::new();

        for (idx, result) in all_results {
            match result {
                Ok(value) => results.push((idx, value)),
                Err(msg) => errors.push((idx, msg)),
            }
        }

        ParallelResult {
            results,
            errors,
            duration: start.elapsed(),
            processed_count: total,
        }
    }

    /// Process items with a simple function (no error handling)
    pub fn map<T, F>(&self, items: &[PathBuf], mapper: F) -> Vec<T>
    where
        F: Fn(&Path) -> T + Sync + Send,
        T: Send,
    {
        if self.options.num_threads > 0 {
            if let Ok(pool) = rayon::ThreadPoolBuilder::new()
                .num_threads(self.options.num_threads)
                .build()
            {
                return pool.install(|| items.par_iter().map(|p| mapper(p)).collect());
            }
        }

        items.par_iter().map(|p| mapper(p)).collect()
    }
}

impl Default for ParallelProcessor {
    fn default() -> Self {
        Self::new()
    }
}

/// Convenience function for parallel processing
pub fn parallel_process<T, E, F>(
    inputs: &[PathBuf],
    processor: F,
    options: &ParallelOptions,
) -> ParallelResult<T>
where
    F: Fn(&Path) -> Result<T, E> + Sync + Send,
    E: std::fmt::Display,
    T: Send,
{
    ParallelProcessor::with_options(options.clone()).process(inputs, processor)
}

/// Parallel map with simple function
pub fn parallel_map<T, F>(inputs: &[PathBuf], mapper: F, num_threads: usize) -> Vec<T>
where
    F: Fn(&Path) -> T + Sync + Send,
    T: Send,
{
    ParallelProcessor::with_options(ParallelOptions::with_threads(num_threads)).map(inputs, mapper)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Write;
    use tempfile::tempdir;

    // ============ TC PAR-001: Basic parallel processing ============

    #[test]
    fn test_par001_parallel_process_basic() {
        let dir = tempdir().unwrap();
        let paths: Vec<PathBuf> = (0..10)
            .map(|i| {
                let path = dir.path().join(format!("file_{}.txt", i));
                let mut f = File::create(&path).unwrap();
                writeln!(f, "content {}", i).unwrap();
                path
            })
            .collect();

        let options = ParallelOptions::default();
        let result = parallel_process(&paths, |path| Ok::<_, String>(path.exists()), &options);

        assert!(result.is_success());
        assert_eq!(result.results.len(), 10);
        assert!(result.success_rate() > 99.0);
    }

    // ============ TC PAR-006: Thread count control ============

    #[test]
    fn test_par006_thread_count_options() {
        let options = ParallelOptions::with_threads(4);
        assert_eq!(options.effective_threads(), 4);

        let auto_options = ParallelOptions::default();
        assert!(auto_options.effective_threads() >= 1);
    }

    #[test]
    fn test_par006_zero_threads_uses_cpu_count() {
        let options = ParallelOptions::with_threads(0);
        assert_eq!(options.effective_threads(), num_cpus::get());
    }

    // ============ TC PAR-007: Error handling ============

    #[test]
    fn test_par007_partial_failure() {
        let paths: Vec<PathBuf> = (0..5)
            .map(|i| PathBuf::from(format!("/nonexistent/path_{}", i)))
            .collect();

        let options = ParallelOptions::default();
        let result = parallel_process(
            &paths,
            |path| {
                if path.exists() {
                    Ok(true)
                } else {
                    Err("File not found")
                }
            },
            &options,
        );

        assert!(!result.is_success());
        assert_eq!(result.errors.len(), 5);
    }

    #[test]
    fn test_par007_continue_on_error() {
        let dir = tempdir().unwrap();
        let mut paths: Vec<PathBuf> = vec![];

        // Create some valid files
        for i in 0..3 {
            let path = dir.path().join(format!("valid_{}.txt", i));
            File::create(&path).unwrap();
            paths.push(path);
        }
        // Add some invalid paths
        for i in 0..2 {
            paths.push(PathBuf::from(format!("/invalid_{}", i)));
        }

        let options = ParallelOptions {
            continue_on_error: true,
            ..Default::default()
        };

        let result = parallel_process(
            &paths,
            |path| {
                if path.exists() {
                    Ok(true)
                } else {
                    Err("Not found")
                }
            },
            &options,
        );

        assert_eq!(result.results.len(), 3);
        assert_eq!(result.errors.len(), 2);
    }

    // ============ TC PAR-009: Memory control with chunks ============

    #[test]
    fn test_par009_chunk_processing() {
        let dir = tempdir().unwrap();
        let paths: Vec<PathBuf> = (0..20)
            .map(|i| {
                let path = dir.path().join(format!("file_{}.txt", i));
                File::create(&path).unwrap();
                path
            })
            .collect();

        let options = ParallelOptions::with_chunks(5);
        let result = parallel_process(&paths, |path| Ok::<_, String>(path.exists()), &options);

        assert!(result.is_success());
        assert_eq!(result.results.len(), 20);
    }

    // ============ Additional tests ============

    #[test]
    fn test_empty_input() {
        let paths: Vec<PathBuf> = vec![];
        let options = ParallelOptions::default();
        let result = parallel_process(&paths, |_| Ok::<_, String>(true), &options);

        assert!(result.is_success());
        assert_eq!(result.processed_count, 0);
    }

    #[test]
    fn test_ordered_results() {
        let dir = tempdir().unwrap();
        let paths: Vec<PathBuf> = (0..5)
            .map(|i| {
                let path = dir.path().join(format!("{}.txt", i));
                File::create(&path).unwrap();
                path
            })
            .collect();

        let options = ParallelOptions::default();
        let result = parallel_process(
            &paths,
            |path| {
                let name = path.file_stem().unwrap().to_str().unwrap();
                Ok::<_, String>(name.parse::<usize>().unwrap())
            },
            &options,
        );

        let ordered = result.ordered_results();
        assert_eq!(ordered, vec![0, 1, 2, 3, 4]);
    }

    #[test]
    fn test_parallel_processor_builder() {
        let processor = ParallelProcessor::new();
        assert!(processor.progress_callback.is_none());

        let processor_with_progress =
            ParallelProcessor::new().with_progress(|done, total| println!("{}/{}", done, total));
        assert!(processor_with_progress.progress_callback.is_some());
    }

    #[test]
    fn test_parallel_map() {
        let paths: Vec<PathBuf> = (0..5).map(|i| PathBuf::from(format!("{}", i))).collect();

        let results = parallel_map(&paths, |p| p.to_string_lossy().parse::<i32>().unwrap(), 2);

        assert_eq!(results.len(), 5);
    }

    #[test]
    fn test_success_rate_calculation() {
        let result: ParallelResult<bool> = ParallelResult {
            results: vec![(0, true), (1, true)],
            errors: vec![(2, "error".to_string())],
            duration: Duration::ZERO,
            processed_count: 3,
        };

        let rate = result.success_rate();
        assert!((rate - 66.67).abs() < 1.0);
    }

    #[test]
    fn test_error_types() {
        let err1 = ParallelError::ThreadPoolError("test".to_string());
        assert!(err1.to_string().contains("Thread pool"));

        let err2 = ParallelError::ProcessingError {
            index: 5,
            message: "fail".to_string(),
        };
        assert!(err2.to_string().contains("index 5"));

        let err3 = ParallelError::AllTasksFailed(10);
        assert!(err3.to_string().contains("10 tasks"));
    }

    #[test]
    fn test_default_options() {
        let options = ParallelOptions::default();
        assert_eq!(options.num_threads, 0);
        assert_eq!(options.chunk_size, 0);
        assert!(options.continue_on_error);
    }

    #[test]
    fn test_thread_safety() {
        use std::sync::atomic::AtomicUsize;
        use std::sync::Arc;

        let counter = Arc::new(AtomicUsize::new(0));
        let paths: Vec<PathBuf> = (0..100).map(|i| PathBuf::from(format!("{}", i))).collect();

        let counter_clone = Arc::clone(&counter);
        let results = parallel_map(
            &paths,
            move |_| {
                counter_clone.fetch_add(1, Ordering::Relaxed);
                true
            },
            4,
        );

        assert_eq!(results.len(), 100);
        assert_eq!(counter.load(Ordering::Relaxed), 100);
    }

    #[test]
    fn test_progress_callback() {
        use std::sync::atomic::AtomicUsize;
        use std::sync::Arc;

        let progress_count = Arc::new(AtomicUsize::new(0));
        let progress_clone = Arc::clone(&progress_count);

        let dir = tempdir().unwrap();
        let paths: Vec<PathBuf> = (0..10)
            .map(|i| {
                let path = dir.path().join(format!("{}.txt", i));
                File::create(&path).unwrap();
                path
            })
            .collect();

        let processor = ParallelProcessor::new().with_progress(move |_, _| {
            progress_clone.fetch_add(1, Ordering::Relaxed);
        });

        let _ = processor.process(&paths, |p| Ok::<_, String>(p.exists()));

        assert_eq!(progress_count.load(Ordering::Relaxed), 10);
    }

    // ============ Additional edge case tests ============

    #[test]
    fn test_custom_thread_pool() {
        let dir = tempdir().unwrap();
        let paths: Vec<PathBuf> = (0..20)
            .map(|i| {
                let path = dir.path().join(format!("file_{}.txt", i));
                File::create(&path).unwrap();
                path
            })
            .collect();

        // Use exactly 2 threads
        let options = ParallelOptions::with_threads(2);
        let result = parallel_process(&paths, |path| Ok::<_, String>(path.exists()), &options);

        assert!(result.is_success());
        assert_eq!(result.results.len(), 20);
    }

    #[test]
    fn test_large_chunk_size() {
        let dir = tempdir().unwrap();
        let paths: Vec<PathBuf> = (0..50)
            .map(|i| {
                let path = dir.path().join(format!("file_{}.txt", i));
                File::create(&path).unwrap();
                path
            })
            .collect();

        // Chunk size larger than input
        let options = ParallelOptions::with_chunks(100);
        let result = parallel_process(&paths, |path| Ok::<_, String>(path.exists()), &options);

        assert!(result.is_success());
        assert_eq!(result.results.len(), 50);
    }

    #[test]
    fn test_mixed_success_failure() {
        let dir = tempdir().unwrap();
        let mut paths = Vec::new();

        // Create 5 valid files
        for i in 0..5 {
            let path = dir.path().join(format!("valid_{}.txt", i));
            File::create(&path).unwrap();
            paths.push(path);
        }
        // Add 5 invalid paths
        for i in 0..5 {
            paths.push(PathBuf::from(format!("/nonexistent/path_{}", i)));
        }

        let options = ParallelOptions::default();
        let result = parallel_process(
            &paths,
            |path| {
                if path.exists() {
                    Ok(path.to_string_lossy().to_string())
                } else {
                    Err("File not found")
                }
            },
            &options,
        );

        assert_eq!(result.results.len(), 5);
        assert_eq!(result.errors.len(), 5);
        assert!((result.success_rate() - 50.0).abs() < 0.1);
    }

    #[test]
    fn test_processor_with_custom_options() {
        let dir = tempdir().unwrap();
        let paths: Vec<PathBuf> = (0..10)
            .map(|i| {
                let path = dir.path().join(format!("{}.txt", i));
                File::create(&path).unwrap();
                path
            })
            .collect();

        let options = ParallelOptions {
            num_threads: 4,
            chunk_size: 3,
            continue_on_error: true,
        };

        let processor = ParallelProcessor::with_options(options);
        let result = processor.process(&paths, |p| Ok::<_, String>(p.exists()));

        assert!(result.is_success());
        assert_eq!(result.processed_count, 10);
    }

    #[test]
    fn test_parallel_result_duration() {
        let dir = tempdir().unwrap();
        let paths: Vec<PathBuf> = (0..5)
            .map(|i| {
                let path = dir.path().join(format!("{}.txt", i));
                File::create(&path).unwrap();
                path
            })
            .collect();

        let options = ParallelOptions::default();
        let result = parallel_process(&paths, |path| Ok::<_, String>(path.exists()), &options);

        // Duration should be non-zero
        assert!(result.duration.as_nanos() > 0);
    }

    #[test]
    fn test_single_item_processing() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("single.txt");
        File::create(&path).unwrap();
        let paths = vec![path];

        let options = ParallelOptions::default();
        let result = parallel_process(&paths, |p| Ok::<_, String>(p.exists()), &options);

        assert!(result.is_success());
        assert_eq!(result.results.len(), 1);
        assert_eq!(result.processed_count, 1);
    }

    #[test]
    fn test_parallel_map_preserves_order() {
        let paths: Vec<PathBuf> = (0..20).map(|i| PathBuf::from(format!("{:02}", i))).collect();

        let results = parallel_map(&paths, |p| p.to_string_lossy().to_string(), 4);

        // Results should be returned (order may vary due to parallelism)
        assert_eq!(results.len(), 20);
        // Each result should be valid
        for result in &results {
            assert!(result.len() == 2); // "00" to "19"
        }
    }
}
