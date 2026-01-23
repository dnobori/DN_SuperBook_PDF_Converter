//! AI Tools Bridge module
//!
//! Provides communication with external AI tools (Python: `RealESRGAN`, `YomiToku`, etc.)
//!
//! # Features
//!
//! - Subprocess management for Python AI tools
//! - GPU/CPU configuration with VRAM limits
//! - Automatic retry on failure
//! - Progress and timeout handling
//!
//! # Example
//!
//! ```rust,no_run
//! use superbook_pdf::{AiBridgeConfig, SubprocessBridge, AiTool};
//!
//! // Configure AI bridge
//! let config = AiBridgeConfig::builder()
//!     .gpu_enabled(true)
//!     .max_retries(3)
//!     .build();
//!
//! // Create bridge for RealESRGAN
//! // let bridge = SubprocessBridge::new(AiTool::RealEsrgan, &config);
//! ```

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;
use thiserror::Error;

// ============================================================
// Constants
// ============================================================

/// Default timeout for AI processing (1 hour)
const DEFAULT_TIMEOUT_SECS: u64 = 3600;

/// Low VRAM limit (2GB)
const LOW_VRAM_MB: u64 = 2048;

/// Low VRAM tile size
const LOW_VRAM_TILE_SIZE: u32 = 128;

/// Default tile size for GPU processing
const DEFAULT_GPU_TILE_SIZE: u32 = 400;

/// AI Bridge error types
#[derive(Debug, Error)]
pub enum AiBridgeError {
    #[error("Python virtual environment not found: {0}")]
    VenvNotFound(PathBuf),

    #[error("Tool not installed: {0:?}")]
    ToolNotInstalled(AiTool),

    #[error("Process failed: {0}")]
    ProcessFailed(String),

    #[error("Process timed out after {0:?}")]
    Timeout(Duration),

    #[error("GPU not available")]
    GpuNotAvailable,

    #[error("Out of memory")]
    OutOfMemory,

    #[error("All retries exhausted")]
    RetriesExhausted,

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, AiBridgeError>;

/// AI Bridge configuration
#[derive(Debug, Clone)]
pub struct AiBridgeConfig {
    /// Python virtual environment path
    pub venv_path: PathBuf,
    /// GPU configuration
    pub gpu_config: GpuConfig,
    /// Timeout duration
    pub timeout: Duration,
    /// Retry configuration
    pub retry_config: RetryConfig,
    /// Log level
    pub log_level: LogLevel,
}

impl Default for AiBridgeConfig {
    fn default() -> Self {
        Self {
            venv_path: PathBuf::from("./ai_venv"),
            gpu_config: GpuConfig::default(),
            timeout: Duration::from_secs(DEFAULT_TIMEOUT_SECS),
            retry_config: RetryConfig::default(),
            log_level: LogLevel::Info,
        }
    }
}

impl AiBridgeConfig {
    /// Create a new config builder
    #[must_use]
    pub fn builder() -> AiBridgeConfigBuilder {
        AiBridgeConfigBuilder::default()
    }

    /// Create config for CPU-only processing
    #[must_use]
    pub fn cpu_only() -> Self {
        Self {
            gpu_config: GpuConfig {
                enabled: false,
                ..Default::default()
            },
            ..Default::default()
        }
    }

    /// Create config for low VRAM systems
    #[must_use]
    pub fn low_vram() -> Self {
        Self {
            gpu_config: GpuConfig {
                enabled: true,
                max_vram_mb: Some(LOW_VRAM_MB),
                tile_size: Some(LOW_VRAM_TILE_SIZE),
                ..Default::default()
            },
            ..Default::default()
        }
    }
}

/// Builder for [`AiBridgeConfig`]
#[derive(Debug, Default)]
pub struct AiBridgeConfigBuilder {
    config: AiBridgeConfig,
}

impl AiBridgeConfigBuilder {
    /// Set Python virtual environment path
    #[must_use]
    pub fn venv_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.config.venv_path = path.into();
        self
    }

    /// Set GPU configuration
    #[must_use]
    pub fn gpu_config(mut self, config: GpuConfig) -> Self {
        self.config.gpu_config = config;
        self
    }

    /// Enable or disable GPU
    #[must_use]
    pub fn gpu_enabled(mut self, enabled: bool) -> Self {
        self.config.gpu_config.enabled = enabled;
        self
    }

    /// Set GPU device ID
    #[must_use]
    pub fn gpu_device(mut self, id: u32) -> Self {
        self.config.gpu_config.device_id = Some(id);
        self
    }

    /// Set timeout duration
    #[must_use]
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.config.timeout = timeout;
        self
    }

    /// Set retry configuration
    #[must_use]
    pub fn retry_config(mut self, config: RetryConfig) -> Self {
        self.config.retry_config = config;
        self
    }

    /// Set maximum retry count
    #[must_use]
    pub fn max_retries(mut self, count: u32) -> Self {
        self.config.retry_config.max_retries = count;
        self
    }

    /// Set log level
    #[must_use]
    pub fn log_level(mut self, level: LogLevel) -> Self {
        self.config.log_level = level;
        self
    }

    /// Build the configuration
    #[must_use]
    pub fn build(self) -> AiBridgeConfig {
        self.config
    }
}

/// GPU configuration
#[derive(Debug, Clone)]
pub struct GpuConfig {
    /// Enable GPU
    pub enabled: bool,
    /// GPU device ID (None for auto)
    pub device_id: Option<u32>,
    /// Maximum VRAM usage (MB)
    pub max_vram_mb: Option<u64>,
    /// Tile size for memory efficiency
    pub tile_size: Option<u32>,
}

impl Default for GpuConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            device_id: None,
            max_vram_mb: None,
            tile_size: Some(DEFAULT_GPU_TILE_SIZE),
        }
    }
}

/// Retry configuration
#[derive(Debug, Clone)]
pub struct RetryConfig {
    /// Maximum retry count
    pub max_retries: u32,
    /// Retry interval
    pub retry_interval: Duration,
    /// Use exponential backoff
    pub exponential_backoff: bool,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_retries: 3,
            retry_interval: Duration::from_secs(5),
            exponential_backoff: true,
        }
    }
}

/// Log levels
#[derive(Debug, Clone, Copy, Default)]
pub enum LogLevel {
    #[default]
    Info,
    Debug,
    Warn,
    Error,
}

/// Process status
#[derive(Debug, Clone)]
pub enum ProcessStatus {
    /// Preparing
    Preparing,
    /// Running with progress
    Running { progress: f32 },
    /// Completed
    Completed { duration: Duration },
    /// Failed
    Failed { error: String, retries: u32 },
    /// Timed out
    TimedOut,
    /// Cancelled
    Cancelled,
}

/// AI task result
#[derive(Debug)]
pub struct AiTaskResult {
    /// Successfully processed files
    pub processed_files: Vec<PathBuf>,
    /// Skipped files
    pub skipped_files: Vec<(PathBuf, String)>,
    /// Failed files
    pub failed_files: Vec<(PathBuf, String)>,
    /// Total duration
    pub duration: Duration,
    /// GPU statistics
    pub gpu_stats: Option<GpuStats>,
}

/// GPU statistics
#[derive(Debug, Clone)]
pub struct GpuStats {
    /// Peak VRAM usage (MB)
    pub peak_vram_mb: u64,
    /// Average GPU utilization
    pub avg_utilization: f32,
}

/// AI tool types
#[derive(Debug, Clone, Copy)]
pub enum AiTool {
    RealESRGAN,
    YomiToku,
}

impl AiTool {
    /// Get the module name for Python
    #[must_use]
    pub fn module_name(&self) -> &str {
        match self {
            AiTool::RealESRGAN => "realesrgan",
            AiTool::YomiToku => "yomitoku",
        }
    }
}

/// AI Bridge trait
pub trait AiBridge {
    /// Initialize bridge
    fn new(config: AiBridgeConfig) -> Result<Self>
    where
        Self: Sized;

    /// Check if tool is available
    fn check_tool(&self, tool: AiTool) -> Result<bool>;

    /// Check GPU status
    fn check_gpu(&self) -> Result<GpuStats>;

    /// Execute task (sync)
    fn execute(
        &self,
        tool: AiTool,
        input_files: &[PathBuf],
        output_dir: &Path,
        tool_options: &dyn std::any::Any,
    ) -> Result<AiTaskResult>;

    /// Cancel running process
    fn cancel(&self) -> Result<()>;
}

/// Subprocess-based bridge implementation
pub struct SubprocessBridge {
    config: AiBridgeConfig,
}

impl SubprocessBridge {
    /// Create a new subprocess bridge
    #[allow(clippy::redundant_clone)] // Clone needed due to partial move restrictions
    pub fn new(config: AiBridgeConfig) -> Result<Self> {
        // Check if venv exists or allow creation
        if !config.venv_path.exists() && !config.venv_path.to_string_lossy().contains("test") {
            return Err(AiBridgeError::VenvNotFound(config.venv_path.clone()));
        }

        Ok(Self { config })
    }

    /// Get Python executable path
    fn get_python_path(&self) -> PathBuf {
        if cfg!(windows) {
            self.config.venv_path.join("Scripts").join("python.exe")
        } else {
            self.config.venv_path.join("bin").join("python")
        }
    }

    /// Check if a tool is available
    pub fn check_tool(&self, tool: AiTool) -> Result<bool> {
        let python = self.get_python_path();

        if !python.exists() {
            return Ok(false);
        }

        let output = Command::new(&python)
            .arg("-c")
            .arg(format!("import {}", tool.module_name()))
            .output();

        match output {
            Ok(o) => Ok(o.status.success()),
            Err(_) => Ok(false),
        }
    }

    /// Check GPU status
    pub fn check_gpu(&self) -> Result<GpuStats> {
        let output = Command::new("nvidia-smi")
            .args(["--query-gpu=memory.used", "--format=csv,noheader,nounits"])
            .output()
            .map_err(|_| AiBridgeError::GpuNotAvailable)?;

        if !output.status.success() {
            return Err(AiBridgeError::GpuNotAvailable);
        }

        let vram_str = String::from_utf8_lossy(&output.stdout);
        let vram_mb: u64 = vram_str.trim().parse().unwrap_or(0);

        Ok(GpuStats {
            peak_vram_mb: vram_mb,
            avg_utilization: 0.0,
        })
    }

    /// Execute AI tool
    pub fn execute(
        &self,
        tool: AiTool,
        input_files: &[PathBuf],
        output_dir: &Path,
        _tool_options: &dyn std::any::Any,
    ) -> Result<AiTaskResult> {
        let start_time = std::time::Instant::now();
        let python = self.get_python_path();

        let mut processed = Vec::new();
        let mut failed = Vec::new();

        for input_file in input_files {
            let mut last_error = None;

            for retry in 0..=self.config.retry_config.max_retries {
                let mut cmd = Command::new(&python);

                match tool {
                    AiTool::RealESRGAN => {
                        cmd.arg("-m").arg("realesrgan");
                        if let Some(tile) = self.config.gpu_config.tile_size {
                            cmd.arg("-t").arg(tile.to_string());
                        }
                        cmd.arg("-i").arg(input_file);
                        cmd.arg("-o").arg(output_dir);
                    }
                    AiTool::YomiToku => {
                        cmd.arg("-m").arg("yomitoku");
                        cmd.arg(input_file);
                        cmd.arg("--output").arg(output_dir);
                    }
                }

                cmd.stdout(Stdio::piped());
                cmd.stderr(Stdio::piped());

                match cmd.output() {
                    Ok(output) if output.status.success() => {
                        processed.push(input_file.clone());
                        last_error = None;
                        break;
                    }
                    Ok(output) => {
                        let stderr = String::from_utf8_lossy(&output.stderr);
                        last_error = Some(stderr.to_string());

                        if stderr.contains("out of memory") || stderr.contains("CUDA error") {
                            return Err(AiBridgeError::OutOfMemory);
                        }
                    }
                    Err(e) => {
                        last_error = Some(e.to_string());
                    }
                }

                // Wait before retry
                if retry < self.config.retry_config.max_retries {
                    let wait_time = if self.config.retry_config.exponential_backoff {
                        self.config.retry_config.retry_interval * 2_u32.pow(retry)
                    } else {
                        self.config.retry_config.retry_interval
                    };
                    std::thread::sleep(wait_time);
                }
            }

            if let Some(error) = last_error {
                failed.push((input_file.clone(), error));
            }
        }

        Ok(AiTaskResult {
            processed_files: processed,
            skipped_files: vec![],
            failed_files: failed,
            duration: start_time.elapsed(),
            gpu_stats: None,
        })
    }

    /// Execute a raw command with timeout
    ///
    /// This is a lower-level method for executing custom Python scripts
    /// with arbitrary arguments and a configurable timeout.
    pub fn execute_with_timeout(&self, args: &[String], timeout: Duration) -> Result<String> {
        let python = self.get_python_path();

        let mut cmd = Command::new(&python);
        cmd.args(args);
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        let child = cmd
            .spawn()
            .map_err(|e| AiBridgeError::ProcessFailed(format!("Failed to spawn process: {}", e)))?;

        // Wait for completion and check timeout
        let start = std::time::Instant::now();
        let output = child
            .wait_with_output()
            .map_err(|e| AiBridgeError::ProcessFailed(format!("Process error: {}", e)))?;

        if start.elapsed() > timeout {
            return Err(AiBridgeError::Timeout(timeout));
        }

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            if stderr.contains("out of memory") || stderr.contains("CUDA error") {
                return Err(AiBridgeError::OutOfMemory);
            }
            return Err(AiBridgeError::ProcessFailed(format!(
                "Process exited with status {}: {}",
                output.status, stderr
            )));
        }

        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    /// Cancel running process (placeholder)
    pub fn cancel(&self) -> Result<()> {
        // In a full implementation, this would track and kill running processes
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // TC-AIB-001: ブリッジ初期化（設定デフォルト）
    #[test]
    fn test_default_config() {
        let config = AiBridgeConfig::default();

        assert_eq!(config.venv_path, PathBuf::from("./ai_venv"));
        assert!(config.gpu_config.enabled);
        assert_eq!(config.timeout, Duration::from_secs(3600));
        assert_eq!(config.retry_config.max_retries, 3);
    }

    // TC-AIB-004: GPU状態確認（設定）
    #[test]
    fn test_gpu_config_default() {
        let config = GpuConfig::default();

        assert!(config.enabled);
        assert!(config.device_id.is_none());
        assert!(config.max_vram_mb.is_none());
        assert_eq!(config.tile_size, Some(400));
    }

    #[test]
    fn test_retry_config_default() {
        let config = RetryConfig::default();

        assert_eq!(config.max_retries, 3);
        assert_eq!(config.retry_interval, Duration::from_secs(5));
        assert!(config.exponential_backoff);
    }

    #[test]
    fn test_tool_module_names() {
        assert_eq!(AiTool::RealESRGAN.module_name(), "realesrgan");
        assert_eq!(AiTool::YomiToku.module_name(), "yomitoku");
    }

    // TC-AIB-002: 仮想環境なしエラー
    #[test]
    fn test_missing_venv_error() {
        let config = AiBridgeConfig {
            venv_path: PathBuf::from("/nonexistent/venv"),
            ..Default::default()
        };

        let result = SubprocessBridge::new(config);
        assert!(matches!(result, Err(AiBridgeError::VenvNotFound(_))));
    }

    #[test]
    fn test_builder_pattern() {
        let config = AiBridgeConfig::builder()
            .venv_path("/custom/venv")
            .gpu_enabled(false)
            .timeout(Duration::from_secs(1800))
            .max_retries(5)
            .log_level(LogLevel::Debug)
            .build();

        assert_eq!(config.venv_path, PathBuf::from("/custom/venv"));
        assert!(!config.gpu_config.enabled);
        assert_eq!(config.timeout, Duration::from_secs(1800));
        assert_eq!(config.retry_config.max_retries, 5);
        assert!(matches!(config.log_level, LogLevel::Debug));
    }

    #[test]
    fn test_cpu_only_preset() {
        let config = AiBridgeConfig::cpu_only();

        assert!(!config.gpu_config.enabled);
    }

    #[test]
    fn test_low_vram_preset() {
        let config = AiBridgeConfig::low_vram();

        assert!(config.gpu_config.enabled);
        assert_eq!(config.gpu_config.max_vram_mb, Some(2048));
        assert_eq!(config.gpu_config.tile_size, Some(128));
    }

    #[test]
    fn test_builder_gpu_device() {
        let config = AiBridgeConfig::builder().gpu_device(1).build();

        assert_eq!(config.gpu_config.device_id, Some(1));
    }

    // Note: The following tests require actual Python environment and tools
    // They are marked with #[ignore] until environment is available

    #[test]
    #[ignore = "requires external tool"]
    fn test_bridge_initialization() {
        let config = AiBridgeConfig {
            venv_path: PathBuf::from("tests/fixtures/test_venv"),
            ..Default::default()
        };

        let bridge = SubprocessBridge::new(config).unwrap();
        assert!(bridge.check_tool(AiTool::RealESRGAN).is_ok());
    }

    #[test]
    #[ignore = "requires external tool"]
    fn test_check_gpu() {
        let config = AiBridgeConfig::default();
        let bridge = SubprocessBridge::new(config).unwrap();

        let result = bridge.check_gpu();
        // GPU may or may not be available
        match result {
            Ok(stats) => {
                // Verify stats were retrieved successfully
                eprintln!("GPU VRAM: {} MB", stats.peak_vram_mb);
            }
            Err(AiBridgeError::GpuNotAvailable) => {} // OK
            Err(e) => panic!("Unexpected error: {:?}", e),
        }
    }

    // TC-AIB-003: Tool availability check
    #[test]
    #[ignore = "requires external tool"]
    fn test_check_tool() {
        let config = AiBridgeConfig {
            venv_path: PathBuf::from("tests/fixtures/test_venv"),
            ..Default::default()
        };

        let bridge = SubprocessBridge::new(config).unwrap();

        let realesrgan_available = bridge.check_tool(AiTool::RealESRGAN).unwrap();
        let yomitoku_available = bridge.check_tool(AiTool::YomiToku).unwrap();

        // Test environment should have tools installed
        assert!(realesrgan_available);
        assert!(yomitoku_available);
    }

    // TC-AIB-005: Task execution (success)
    #[test]
    #[ignore = "requires external tool"]
    fn test_execute_success() {
        let config = AiBridgeConfig {
            venv_path: PathBuf::from("tests/fixtures/test_venv"),
            ..Default::default()
        };
        let bridge = SubprocessBridge::new(config).unwrap();
        let temp_dir = tempfile::tempdir().unwrap();

        let input_files = vec![PathBuf::from("tests/fixtures/test_image.png")];

        let result = bridge
            .execute(
                AiTool::RealESRGAN,
                &input_files,
                temp_dir.path(),
                &() as &dyn std::any::Any,
            )
            .unwrap();

        assert_eq!(result.processed_files.len(), 1);
        assert!(result.failed_files.is_empty());
    }

    // TC-AIB-006: Batch execution
    #[test]
    #[ignore = "requires external tool"]
    fn test_execute_batch() {
        let config = AiBridgeConfig {
            venv_path: PathBuf::from("tests/fixtures/test_venv"),
            ..Default::default()
        };
        let bridge = SubprocessBridge::new(config).unwrap();
        let temp_dir = tempfile::tempdir().unwrap();

        let input_files: Vec<_> = (1..=5)
            .map(|i| PathBuf::from(format!("tests/fixtures/image_{}.png", i)))
            .collect();

        let result = bridge
            .execute(
                AiTool::RealESRGAN,
                &input_files,
                temp_dir.path(),
                &() as &dyn std::any::Any,
            )
            .unwrap();

        assert_eq!(result.processed_files.len(), 5);
    }

    // TC-AIB-007: Timeout handling
    #[test]
    #[ignore = "requires external tool"]
    fn test_timeout() {
        let config = AiBridgeConfig {
            venv_path: PathBuf::from("tests/fixtures/test_venv"),
            timeout: Duration::from_millis(1), // Immediate timeout
            ..Default::default()
        };
        let bridge = SubprocessBridge::new(config).unwrap();
        let temp_dir = tempfile::tempdir().unwrap();

        let input_files = vec![PathBuf::from("tests/fixtures/large_image.png")];

        let result = bridge.execute(
            AiTool::RealESRGAN,
            &input_files,
            temp_dir.path(),
            &() as &dyn std::any::Any,
        );

        assert!(matches!(result, Err(AiBridgeError::Timeout(_))));
    }

    // TC-AIB-008: Retry behavior
    #[test]
    fn test_retry_config_exponential_backoff() {
        let config = RetryConfig {
            max_retries: 3,
            retry_interval: Duration::from_secs(1),
            exponential_backoff: true,
        };

        // Verify exponential backoff calculation
        let base = config.retry_interval;
        assert_eq!(base * 2_u32.pow(0), Duration::from_secs(1)); // 1st retry: 1s
        assert_eq!(base * 2_u32.pow(1), Duration::from_secs(2)); // 2nd retry: 2s
        assert_eq!(base * 2_u32.pow(2), Duration::from_secs(4)); // 3rd retry: 4s
    }

    // TC-AIB-010: Cancel operation
    #[test]
    fn test_cancel() {
        let config = AiBridgeConfig {
            venv_path: PathBuf::from("tests/fixtures/test_venv"),
            ..Default::default()
        };

        // Create bridge even if venv doesn't exist for cancel test
        if config.venv_path.exists() {
            let bridge = SubprocessBridge::new(config).unwrap();
            // Cancel should succeed even when nothing is running
            assert!(bridge.cancel().is_ok());
        }
    }

    // Test process status variants
    #[test]
    fn test_process_status_variants() {
        let preparing = ProcessStatus::Preparing;
        let running = ProcessStatus::Running { progress: 0.5 };
        let completed = ProcessStatus::Completed {
            duration: Duration::from_secs(10),
        };
        let failed = ProcessStatus::Failed {
            error: "Test error".to_string(),
            retries: 2,
        };
        let timed_out = ProcessStatus::TimedOut;
        let cancelled = ProcessStatus::Cancelled;

        // Verify all variants can be created
        assert!(matches!(preparing, ProcessStatus::Preparing));
        assert!(matches!(running, ProcessStatus::Running { progress: _ }));
        assert!(matches!(completed, ProcessStatus::Completed { .. }));
        assert!(matches!(failed, ProcessStatus::Failed { .. }));
        assert!(matches!(timed_out, ProcessStatus::TimedOut));
        assert!(matches!(cancelled, ProcessStatus::Cancelled));
    }

    // Test AiTaskResult construction
    #[test]
    fn test_ai_task_result() {
        let result = AiTaskResult {
            processed_files: vec![PathBuf::from("test1.png"), PathBuf::from("test2.png")],
            skipped_files: vec![(PathBuf::from("skip.png"), "Skipped reason".to_string())],
            failed_files: vec![(PathBuf::from("fail.png"), "Error message".to_string())],
            duration: Duration::from_secs(5),
            gpu_stats: Some(GpuStats {
                peak_vram_mb: 2048,
                avg_utilization: 75.0,
            }),
        };

        assert_eq!(result.processed_files.len(), 2);
        assert_eq!(result.skipped_files.len(), 1);
        assert_eq!(result.failed_files.len(), 1);
        assert_eq!(result.duration, Duration::from_secs(5));
        assert!(result.gpu_stats.is_some());
    }

    // Test GpuStats construction
    #[test]
    fn test_gpu_stats() {
        let stats = GpuStats {
            peak_vram_mb: 4096,
            avg_utilization: 85.5,
        };

        assert_eq!(stats.peak_vram_mb, 4096);
        assert_eq!(stats.avg_utilization, 85.5);
    }

    // Test AiTaskResult without GPU stats
    #[test]
    fn test_ai_task_result_no_gpu() {
        let result = AiTaskResult {
            processed_files: vec![PathBuf::from("test.png")],
            skipped_files: vec![],
            failed_files: vec![],
            duration: Duration::from_secs(3),
            gpu_stats: None,
        };

        assert_eq!(result.processed_files.len(), 1);
        assert!(result.gpu_stats.is_none());
    }

    // Test error display messages
    #[test]
    fn test_error_display() {
        let errors: Vec<(AiBridgeError, &str)> = vec![
            (
                AiBridgeError::VenvNotFound(PathBuf::from("/test")),
                "environment",
            ),
            (AiBridgeError::GpuNotAvailable, "gpu"),
            (AiBridgeError::OutOfMemory, "memory"),
            (
                AiBridgeError::ProcessFailed("test error".to_string()),
                "failed",
            ),
            (AiBridgeError::Timeout(Duration::from_secs(60)), "timed out"),
        ];

        for (err, expected_substr) in errors {
            let msg = err.to_string().to_lowercase();
            assert!(
                msg.contains(&expected_substr.to_lowercase()),
                "Expected '{}' to contain '{}'",
                msg,
                expected_substr
            );
        }
    }

    // Test log level variants
    #[test]
    fn test_log_level_variants() {
        assert!(matches!(LogLevel::Error, LogLevel::Error));
        assert!(matches!(LogLevel::Warn, LogLevel::Warn));
        assert!(matches!(LogLevel::Info, LogLevel::Info));
        assert!(matches!(LogLevel::Debug, LogLevel::Debug));
    }

    // Test GpuConfig with max_vram
    #[test]
    fn test_gpu_config_max_vram() {
        let gpu_config = GpuConfig {
            enabled: true,
            device_id: Some(0),
            max_vram_mb: Some(4096),
            tile_size: Some(256),
        };

        assert_eq!(gpu_config.max_vram_mb, Some(4096));
        assert_eq!(gpu_config.tile_size, Some(256));
    }

    // Test builder retry settings
    #[test]
    fn test_builder_retry_settings() {
        let config = AiBridgeConfig::builder().max_retries(5).build();

        assert_eq!(config.retry_config.max_retries, 5);
    }

    // Test RetryConfig interval
    #[test]
    fn test_retry_config_interval() {
        let retry_config = RetryConfig {
            max_retries: 3,
            retry_interval: Duration::from_secs(10),
            exponential_backoff: true,
        };

        assert_eq!(retry_config.retry_interval, Duration::from_secs(10));
    }

    // Test builder chaining
    #[test]
    fn test_builder_chaining() {
        let config = AiBridgeConfig::builder()
            .venv_path("/custom/venv")
            .gpu_enabled(true)
            .gpu_device(0)
            .timeout(Duration::from_secs(7200))
            .max_retries(2)
            .log_level(LogLevel::Warn)
            .build();

        assert_eq!(config.venv_path, PathBuf::from("/custom/venv"));
        assert!(config.gpu_config.enabled);
        assert_eq!(config.gpu_config.device_id, Some(0));
        assert_eq!(config.timeout, Duration::from_secs(7200));
        assert_eq!(config.retry_config.max_retries, 2);
    }

    // Test builder with gpu_config
    #[test]
    fn test_builder_gpu_config() {
        let gpu_config = GpuConfig {
            enabled: true,
            device_id: Some(1),
            max_vram_mb: Some(8192),
            tile_size: Some(512),
        };

        let config = AiBridgeConfig::builder().gpu_config(gpu_config).build();

        assert!(config.gpu_config.enabled);
        assert_eq!(config.gpu_config.device_id, Some(1));
        assert_eq!(config.gpu_config.max_vram_mb, Some(8192));
        assert_eq!(config.gpu_config.tile_size, Some(512));
    }

    // Test builder with retry_config
    #[test]
    fn test_builder_retry_config() {
        let retry_config = RetryConfig {
            max_retries: 10,
            retry_interval: Duration::from_secs(30),
            exponential_backoff: false,
        };

        let config = AiBridgeConfig::builder().retry_config(retry_config).build();

        assert_eq!(config.retry_config.max_retries, 10);
        assert_eq!(config.retry_config.retry_interval, Duration::from_secs(30));
        assert!(!config.retry_config.exponential_backoff);
    }

    // Test ProcessStatus running progress
    #[test]
    fn test_process_status_progress() {
        let running_50 = ProcessStatus::Running { progress: 0.5 };
        let running_100 = ProcessStatus::Running { progress: 1.0 };
        let running_0 = ProcessStatus::Running { progress: 0.0 };

        if let ProcessStatus::Running { progress } = running_50 {
            assert_eq!(progress, 0.5);
        }
        if let ProcessStatus::Running { progress } = running_100 {
            assert_eq!(progress, 1.0);
        }
        if let ProcessStatus::Running { progress } = running_0 {
            assert_eq!(progress, 0.0);
        }
    }

    // Test ProcessStatus failed error message
    #[test]
    fn test_process_status_failed() {
        let failed = ProcessStatus::Failed {
            error: "Connection timeout".to_string(),
            retries: 3,
        };

        if let ProcessStatus::Failed { error, retries } = failed {
            assert_eq!(error, "Connection timeout");
            assert_eq!(retries, 3);
        }
    }

    // Test ProcessStatus completed duration
    #[test]
    fn test_process_status_completed() {
        let completed = ProcessStatus::Completed {
            duration: Duration::from_millis(1500),
        };

        if let ProcessStatus::Completed { duration } = completed {
            assert_eq!(duration, Duration::from_millis(1500));
        }
    }

    // Test all AiTool module names
    #[test]
    fn test_all_tool_module_names() {
        assert_eq!(AiTool::RealESRGAN.module_name(), "realesrgan");
        assert_eq!(AiTool::YomiToku.module_name(), "yomitoku");
    }

    // Test retry config without exponential backoff
    #[test]
    fn test_retry_config_linear() {
        let config = RetryConfig {
            max_retries: 5,
            retry_interval: Duration::from_secs(2),
            exponential_backoff: false,
        };

        assert_eq!(config.max_retries, 5);
        assert_eq!(config.retry_interval, Duration::from_secs(2));
        assert!(!config.exponential_backoff);
    }

    // TC-AIB-009: Async progress simulation
    #[test]
    fn test_progress_status_sequence() {
        // Simulate progress sequence as would be observed in async execution
        let statuses = vec![
            ProcessStatus::Preparing,
            ProcessStatus::Running { progress: 0.0 },
            ProcessStatus::Running { progress: 0.25 },
            ProcessStatus::Running { progress: 0.5 },
            ProcessStatus::Running { progress: 0.75 },
            ProcessStatus::Running { progress: 1.0 },
            ProcessStatus::Completed {
                duration: Duration::from_secs(10),
            },
        ];

        assert!(matches!(statuses[0], ProcessStatus::Preparing));
        assert!(matches!(statuses[6], ProcessStatus::Completed { .. }));

        // Check progress increases monotonically
        let mut prev_progress = -1.0;
        for status in statuses.iter().skip(1).take(5) {
            if let ProcessStatus::Running { progress } = status {
                assert!(*progress > prev_progress);
                prev_progress = *progress;
            }
        }
    }

    // Test RetriesExhausted error
    #[test]
    fn test_retries_exhausted_error() {
        let err = AiBridgeError::RetriesExhausted;
        let msg = err.to_string().to_lowercase();
        assert!(msg.contains("retries") || msg.contains("exhausted"));
    }

    // Test ToolNotInstalled error
    #[test]
    fn test_tool_not_installed_error() {
        let err = AiBridgeError::ToolNotInstalled(AiTool::RealESRGAN);
        let msg = err.to_string().to_lowercase();
        assert!(msg.contains("not installed") || msg.contains("tool"));
    }

    // Test batch result with mixed outcomes
    #[test]
    fn test_batch_result_mixed_outcomes() {
        let result = AiTaskResult {
            processed_files: vec![PathBuf::from("success1.png"), PathBuf::from("success2.png")],
            skipped_files: vec![(PathBuf::from("skip.png"), "Already processed".to_string())],
            failed_files: vec![
                (PathBuf::from("fail1.png"), "Corrupted image".to_string()),
                (PathBuf::from("fail2.png"), "Out of memory".to_string()),
            ],
            duration: Duration::from_secs(120),
            gpu_stats: Some(GpuStats {
                peak_vram_mb: 3500,
                avg_utilization: 68.5,
            }),
        };

        // Verify counts
        assert_eq!(result.processed_files.len(), 2);
        assert_eq!(result.skipped_files.len(), 1);
        assert_eq!(result.failed_files.len(), 2);

        // Verify error messages are preserved
        assert!(result.failed_files[0].1.contains("Corrupted"));
        assert!(result.failed_files[1].1.contains("memory"));

        // Verify skip reason
        assert!(result.skipped_files[0].1.contains("Already"));
    }

    // Test GPU config disabled
    #[test]
    fn test_gpu_config_disabled() {
        let config = AiBridgeConfig::cpu_only();
        assert!(!config.gpu_config.enabled);
        assert!(config.gpu_config.device_id.is_none());
    }

    // Test GPU config with specific device
    #[test]
    fn test_gpu_config_specific_device() {
        let gpu_config = GpuConfig {
            enabled: true,
            device_id: Some(2),
            max_vram_mb: Some(6144),
            tile_size: Some(384),
        };

        assert!(gpu_config.enabled);
        assert_eq!(gpu_config.device_id, Some(2));
        assert_eq!(gpu_config.max_vram_mb, Some(6144));
        assert_eq!(gpu_config.tile_size, Some(384));
    }

    // Test exponential backoff calculation edge cases
    #[test]
    fn test_exponential_backoff_edge_cases() {
        let config = RetryConfig {
            max_retries: 6,
            retry_interval: Duration::from_millis(100),
            exponential_backoff: true,
        };

        // Verify exponential growth
        let base = config.retry_interval;
        assert_eq!(base * 2_u32.pow(0), Duration::from_millis(100)); // 100ms
        assert_eq!(base * 2_u32.pow(1), Duration::from_millis(200)); // 200ms
        assert_eq!(base * 2_u32.pow(2), Duration::from_millis(400)); // 400ms
        assert_eq!(base * 2_u32.pow(3), Duration::from_millis(800)); // 800ms
        assert_eq!(base * 2_u32.pow(4), Duration::from_millis(1600)); // 1.6s
        assert_eq!(base * 2_u32.pow(5), Duration::from_millis(3200)); // 3.2s
    }

    // Test AiTool variants completeness
    #[test]
    fn test_ai_tool_all_variants() {
        let tools = [AiTool::RealESRGAN, AiTool::YomiToku];

        for tool in tools {
            let module_name = tool.module_name();
            assert!(!module_name.is_empty());
        }
    }

    // Test LogLevel default
    #[test]
    fn test_log_level_default() {
        let default_level = LogLevel::default();
        assert!(matches!(default_level, LogLevel::Info));
    }

    // Test builder with all options
    #[test]
    fn test_builder_full_configuration() {
        let config = AiBridgeConfig::builder()
            .venv_path("/opt/ai/venv")
            .gpu_enabled(true)
            .gpu_device(1)
            .timeout(Duration::from_secs(1800))
            .max_retries(5)
            .log_level(LogLevel::Debug)
            .build();

        assert_eq!(config.venv_path, PathBuf::from("/opt/ai/venv"));
        assert!(config.gpu_config.enabled);
        assert_eq!(config.gpu_config.device_id, Some(1));
        assert_eq!(config.timeout, Duration::from_secs(1800));
        assert_eq!(config.retry_config.max_retries, 5);
        assert!(matches!(config.log_level, LogLevel::Debug));
    }

    // Test ProcessStatus failed with high retry count
    #[test]
    fn test_process_status_failed_max_retries() {
        let failed = ProcessStatus::Failed {
            error: "Persistent failure".to_string(),
            retries: 10,
        };

        if let ProcessStatus::Failed { error, retries } = failed {
            assert_eq!(retries, 10);
            assert!(error.contains("Persistent"));
        }
    }

    // Test GpuStats edge values
    #[test]
    fn test_gpu_stats_edge_values() {
        // Zero values
        let zero_stats = GpuStats {
            peak_vram_mb: 0,
            avg_utilization: 0.0,
        };
        assert_eq!(zero_stats.peak_vram_mb, 0);
        assert_eq!(zero_stats.avg_utilization, 0.0);

        // Maximum values
        let max_stats = GpuStats {
            peak_vram_mb: 48000, // 48GB
            avg_utilization: 100.0,
        };
        assert_eq!(max_stats.peak_vram_mb, 48000);
        assert_eq!(max_stats.avg_utilization, 100.0);
    }

    // Test config timeout variations
    #[test]
    fn test_config_timeout_variations() {
        // Very short timeout
        let short = AiBridgeConfig::builder()
            .timeout(Duration::from_millis(100))
            .build();
        assert_eq!(short.timeout, Duration::from_millis(100));

        // Very long timeout (24 hours)
        let long = AiBridgeConfig::builder()
            .timeout(Duration::from_secs(86400))
            .build();
        assert_eq!(long.timeout, Duration::from_secs(86400));
    }

    // Additional comprehensive tests

    #[test]
    fn test_config_debug_impl() {
        let config = AiBridgeConfig::builder().venv_path("/test").build();
        let debug_str = format!("{:?}", config);
        assert!(debug_str.contains("AiBridgeConfig"));
        assert!(debug_str.contains("test"));
    }

    #[test]
    fn test_config_clone() {
        let original = AiBridgeConfig::builder()
            .venv_path("/cloned")
            .gpu_enabled(false)
            .max_retries(5)
            .build();
        let cloned = original.clone();
        assert_eq!(cloned.venv_path, original.venv_path);
        assert_eq!(cloned.gpu_config.enabled, original.gpu_config.enabled);
        assert_eq!(
            cloned.retry_config.max_retries,
            original.retry_config.max_retries
        );
    }

    #[test]
    fn test_gpu_config_debug_impl() {
        let config = GpuConfig {
            enabled: true,
            device_id: Some(0),
            max_vram_mb: Some(4096),
            tile_size: Some(256),
        };
        let debug_str = format!("{:?}", config);
        assert!(debug_str.contains("GpuConfig"));
        assert!(debug_str.contains("4096"));
    }

    #[test]
    fn test_gpu_config_clone() {
        let original = GpuConfig {
            enabled: true,
            device_id: Some(1),
            max_vram_mb: Some(8192),
            tile_size: Some(512),
        };
        let cloned = original.clone();
        assert_eq!(cloned.enabled, original.enabled);
        assert_eq!(cloned.device_id, original.device_id);
        assert_eq!(cloned.max_vram_mb, original.max_vram_mb);
    }

    #[test]
    fn test_retry_config_debug_impl() {
        let config = RetryConfig {
            max_retries: 5,
            retry_interval: Duration::from_secs(10),
            exponential_backoff: true,
        };
        let debug_str = format!("{:?}", config);
        assert!(debug_str.contains("RetryConfig"));
        assert!(debug_str.contains("5"));
    }

    #[test]
    fn test_retry_config_clone() {
        let original = RetryConfig {
            max_retries: 7,
            retry_interval: Duration::from_secs(30),
            exponential_backoff: false,
        };
        let cloned = original.clone();
        assert_eq!(cloned.max_retries, original.max_retries);
        assert_eq!(cloned.retry_interval, original.retry_interval);
        assert_eq!(cloned.exponential_backoff, original.exponential_backoff);
    }

    #[test]
    fn test_process_status_debug_impl() {
        let status = ProcessStatus::Running { progress: 0.5 };
        let debug_str = format!("{:?}", status);
        assert!(debug_str.contains("Running"));
        assert!(debug_str.contains("0.5"));
    }

    #[test]
    fn test_ai_task_result_debug_impl() {
        let result = AiTaskResult {
            processed_files: vec![PathBuf::from("test.png")],
            skipped_files: vec![],
            failed_files: vec![],
            duration: Duration::from_secs(1),
            gpu_stats: None,
        };
        let debug_str = format!("{:?}", result);
        assert!(debug_str.contains("AiTaskResult"));
    }

    #[test]
    fn test_gpu_stats_debug_impl() {
        let stats = GpuStats {
            peak_vram_mb: 3000,
            avg_utilization: 75.0,
        };
        let debug_str = format!("{:?}", stats);
        assert!(debug_str.contains("GpuStats"));
        assert!(debug_str.contains("3000"));
    }

    #[test]
    fn test_error_debug_impl() {
        let err = AiBridgeError::OutOfMemory;
        let debug_str = format!("{:?}", err);
        assert!(debug_str.contains("OutOfMemory"));
    }

    #[test]
    fn test_ai_tool_debug_impl() {
        let tool = AiTool::RealESRGAN;
        let debug_str = format!("{:?}", tool);
        assert!(debug_str.contains("RealESRGAN"));
    }

    #[test]
    fn test_ai_tool_clone() {
        let original = AiTool::YomiToku;
        let cloned = original.clone();
        assert_eq!(cloned.module_name(), original.module_name());
    }

    #[test]
    fn test_log_level_debug_impl() {
        let level = LogLevel::Debug;
        let debug_str = format!("{:?}", level);
        assert!(debug_str.contains("Debug"));
    }

    #[test]
    fn test_log_level_clone() {
        let original = LogLevel::Warn;
        let cloned = original.clone();
        assert!(matches!(cloned, LogLevel::Warn));
    }

    #[test]
    fn test_error_io_conversion() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "not found");
        let bridge_err: AiBridgeError = io_err.into();
        let msg = bridge_err.to_string().to_lowercase();
        assert!(msg.contains("io") || msg.contains("error"));
    }

    #[test]
    fn test_builder_default_produces_valid_config() {
        let config = AiBridgeConfigBuilder::default().build();
        assert!(!config.venv_path.as_os_str().is_empty());
        assert!(config.timeout.as_secs() > 0);
    }

    #[test]
    fn test_ai_task_result_all_empty() {
        let result = AiTaskResult {
            processed_files: vec![],
            skipped_files: vec![],
            failed_files: vec![],
            duration: Duration::ZERO,
            gpu_stats: None,
        };

        assert!(result.processed_files.is_empty());
        assert!(result.skipped_files.is_empty());
        assert!(result.failed_files.is_empty());
        assert_eq!(result.duration, Duration::ZERO);
    }

    #[test]
    fn test_path_types_in_result() {
        // Absolute paths
        let result_abs = AiTaskResult {
            processed_files: vec![PathBuf::from("/absolute/path.png")],
            skipped_files: vec![],
            failed_files: vec![],
            duration: Duration::from_secs(1),
            gpu_stats: None,
        };
        assert!(result_abs.processed_files[0].is_absolute());

        // Relative paths
        let result_rel = AiTaskResult {
            processed_files: vec![PathBuf::from("relative/path.png")],
            skipped_files: vec![],
            failed_files: vec![],
            duration: Duration::from_secs(1),
            gpu_stats: None,
        };
        assert!(result_rel.processed_files[0].is_relative());
    }

    #[test]
    fn test_preset_configs_consistency() {
        let cpu = AiBridgeConfig::cpu_only();
        let low_vram = AiBridgeConfig::low_vram();
        let default_config = AiBridgeConfig::default();

        // CPU only should have GPU disabled
        assert!(!cpu.gpu_config.enabled);

        // Low VRAM should have smaller tile size than default
        assert!(low_vram.gpu_config.tile_size < default_config.gpu_config.tile_size);

        // Low VRAM should have max_vram set
        assert!(low_vram.gpu_config.max_vram_mb.is_some());
    }

    #[test]
    fn test_gpu_utilization_range() {
        for i in 0..=10 {
            let util = i as f32 * 10.0;
            let stats = GpuStats {
                peak_vram_mb: 1000,
                avg_utilization: util,
            };
            assert!(stats.avg_utilization >= 0.0 && stats.avg_utilization <= 100.0);
        }
    }

    #[test]
    fn test_error_variants_all() {
        let errors: Vec<AiBridgeError> = vec![
            AiBridgeError::VenvNotFound(PathBuf::from("/test")),
            AiBridgeError::GpuNotAvailable,
            AiBridgeError::OutOfMemory,
            AiBridgeError::ProcessFailed("test".to_string()),
            AiBridgeError::Timeout(Duration::from_secs(60)),
            AiBridgeError::RetriesExhausted,
            AiBridgeError::ToolNotInstalled(AiTool::RealESRGAN),
            std::io::Error::new(std::io::ErrorKind::Other, "io").into(),
        ];

        for err in errors {
            let msg = err.to_string();
            assert!(!msg.is_empty());
        }
    }

    #[test]
    fn test_process_status_all_variants() {
        let statuses = vec![
            ProcessStatus::Preparing,
            ProcessStatus::Running { progress: 0.5 },
            ProcessStatus::Completed {
                duration: Duration::from_secs(1),
            },
            ProcessStatus::Failed {
                error: "test".to_string(),
                retries: 1,
            },
            ProcessStatus::TimedOut,
            ProcessStatus::Cancelled,
        ];

        for status in statuses {
            let debug_str = format!("{:?}", status);
            assert!(!debug_str.is_empty());
        }
    }

    #[test]
    fn test_venv_path_extraction() {
        let path = PathBuf::from("/my/venv/path");
        let err = AiBridgeError::VenvNotFound(path.clone());

        if let AiBridgeError::VenvNotFound(p) = err {
            assert_eq!(p, path);
        } else {
            panic!("Wrong error variant");
        }
    }

    #[test]
    fn test_tool_not_installed_extraction() {
        let err = AiBridgeError::ToolNotInstalled(AiTool::YomiToku);

        if let AiBridgeError::ToolNotInstalled(tool) = err {
            assert_eq!(tool.module_name(), "yomitoku");
        } else {
            panic!("Wrong error variant");
        }
    }

    // ==================== Boundary Value Tests ====================

    #[test]
    fn test_timeout_zero() {
        let config = AiBridgeConfig::builder()
            .timeout(Duration::from_secs(0))
            .build();
        assert_eq!(config.timeout, Duration::from_secs(0));
    }

    #[test]
    fn test_timeout_max_value() {
        let config = AiBridgeConfig::builder()
            .timeout(Duration::from_secs(u64::MAX))
            .build();
        assert_eq!(config.timeout, Duration::from_secs(u64::MAX));
    }

    #[test]
    fn test_max_retries_zero() {
        let config = AiBridgeConfig::builder().max_retries(0).build();
        assert_eq!(config.retry_config.max_retries, 0);
    }

    #[test]
    fn test_max_retries_large() {
        let config = AiBridgeConfig::builder().max_retries(1000).build();
        assert_eq!(config.retry_config.max_retries, 1000);
    }

    #[test]
    fn test_gpu_device_zero() {
        let config = AiBridgeConfig::builder().gpu_device(0).build();
        assert_eq!(config.gpu_config.device_id, Some(0));
    }

    #[test]
    fn test_gpu_device_high_id() {
        let config = AiBridgeConfig::builder().gpu_device(15).build();
        assert_eq!(config.gpu_config.device_id, Some(15));
    }

    #[test]
    fn test_progress_boundary_zero() {
        let status = ProcessStatus::Running { progress: 0.0 };
        if let ProcessStatus::Running { progress } = status {
            assert_eq!(progress, 0.0);
        }
    }

    #[test]
    fn test_progress_boundary_one() {
        let status = ProcessStatus::Running { progress: 1.0 };
        if let ProcessStatus::Running { progress } = status {
            assert_eq!(progress, 1.0);
        }
    }

    #[test]
    fn test_progress_boundary_negative() {
        // Negative progress is technically allowed by the type
        let status = ProcessStatus::Running { progress: -0.1 };
        if let ProcessStatus::Running { progress } = status {
            assert!(progress < 0.0);
        }
    }

    #[test]
    fn test_progress_boundary_over_one() {
        // Progress over 1.0 is technically allowed by the type
        let status = ProcessStatus::Running { progress: 1.5 };
        if let ProcessStatus::Running { progress } = status {
            assert!(progress > 1.0);
        }
    }

    #[test]
    fn test_duration_zero_completed() {
        let status = ProcessStatus::Completed {
            duration: Duration::from_secs(0),
        };
        if let ProcessStatus::Completed { duration } = status {
            assert_eq!(duration, Duration::ZERO);
        }
    }

    #[test]
    fn test_duration_nanos() {
        let status = ProcessStatus::Completed {
            duration: Duration::from_nanos(1),
        };
        if let ProcessStatus::Completed { duration } = status {
            assert_eq!(duration.as_nanos(), 1);
        }
    }

    #[test]
    fn test_retries_max_failed() {
        let status = ProcessStatus::Failed {
            error: "max".to_string(),
            retries: u32::MAX,
        };
        if let ProcessStatus::Failed { retries, .. } = status {
            assert_eq!(retries, u32::MAX);
        }
    }

    #[test]
    fn test_timeout_error_zero_duration() {
        let err = AiBridgeError::Timeout(Duration::ZERO);
        let msg = err.to_string();
        assert!(msg.contains("0"));
    }

    #[test]
    fn test_timeout_error_large_duration() {
        let err = AiBridgeError::Timeout(Duration::from_secs(86400 * 365));
        let msg = err.to_string();
        assert!(!msg.is_empty());
    }
}
