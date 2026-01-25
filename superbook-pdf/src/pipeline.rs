//! Pipeline processing module
//!
//! Provides a clean API for PDF processing pipeline, separating
//! business logic from CLI handling.
//!
//! ## Processing Steps
//!
//! 1. PDF読み込み・メタデータ抽出
//! 2. 画像抽出 (pdftoppm/ImageMagick)
//! 3. 傾き補正 (Deskew)
//! 4. マージントリミング
//! 5. AI超解像 (RealESRGAN)
//! 6. 内部解像度正規化
//! 7. 色統計分析・グローバル色補正
//! 8. Tukey fenceグループクロップ
//! 9. ページ番号オフセット計算
//! 10. 最終出力リサイズ
//! 11. 縦書き検出
//! 12. YomiToku OCR
//! 13. PDF生成

use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Instant;
use thiserror::Error;

use crate::cli::ConvertArgs;

// ============================================================
// Memory Management Utilities (Phase 3 optimization)
// ============================================================

/// Estimated memory per image at 300 DPI (A4 size, RGBA)
const ESTIMATED_IMAGE_MEMORY_MB: usize = 100;

/// Minimum chunk size for parallel processing
const MIN_CHUNK_SIZE: usize = 4;

/// Default memory limit if not specified (4GB)
const DEFAULT_MEMORY_LIMIT_MB: usize = 4096;

/// Calculate optimal chunk size based on memory constraints
///
/// # Arguments
/// * `total_items` - Total number of items to process
/// * `max_memory_mb` - Maximum memory to use (0 = use default limit)
/// * `threads` - Number of parallel threads
///
/// # Returns
/// Optimal chunk size that fits within memory constraints
pub fn calculate_optimal_chunk_size(
    total_items: usize,
    max_memory_mb: usize,
    threads: usize,
) -> usize {
    let memory_limit = if max_memory_mb == 0 {
        get_available_memory_mb().unwrap_or(DEFAULT_MEMORY_LIMIT_MB)
    } else {
        max_memory_mb
    };

    // Reserve 50% of available memory for OS and other processes
    let usable_memory = memory_limit / 2;

    // Calculate how many images can be processed concurrently
    // Consider thread count to avoid over-committing memory
    let max_concurrent = threads.max(num_cpus::get());
    let per_thread_capacity = usable_memory / ESTIMATED_IMAGE_MEMORY_MB;
    let concurrent_capacity = per_thread_capacity.min(max_concurrent);

    // Chunk size should be at least MIN_CHUNK_SIZE but not more than concurrent capacity
    let chunk_size = concurrent_capacity.max(MIN_CHUNK_SIZE);

    // Don't exceed total items
    chunk_size.min(total_items).max(1)
}

/// Get available system memory in MB
#[cfg(target_os = "linux")]
fn get_available_memory_mb() -> Option<usize> {
    use std::fs;

    let meminfo = fs::read_to_string("/proc/meminfo").ok()?;
    for line in meminfo.lines() {
        if line.starts_with("MemAvailable:") {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 2 {
                let kb: usize = parts[1].parse().ok()?;
                return Some(kb / 1024);
            }
        }
    }
    None
}

#[cfg(not(target_os = "linux"))]
fn get_available_memory_mb() -> Option<usize> {
    // For non-Linux systems, use a conservative default
    Some(DEFAULT_MEMORY_LIMIT_MB)
}

/// Process items in chunks for memory-controlled parallel execution
///
/// This function processes items in batches to prevent memory exhaustion.
/// Each chunk is processed in parallel using rayon, but chunks are processed
/// sequentially to limit peak memory usage.
///
/// # Arguments
/// * `items` - Items to process
/// * `chunk_size` - Size of each processing chunk (0 = process all at once)
/// * `processor` - Function to apply to each item
/// * `progress` - Optional progress callback (current, total)
///
/// # Returns
/// Vector of results in the same order as input items
pub fn process_in_chunks<T, R, F, P>(
    items: &[T],
    chunk_size: usize,
    processor: F,
    progress: Option<&P>,
) -> Vec<R>
where
    T: Sync,
    R: Send,
    F: Fn(&T) -> R + Sync,
    P: Fn(usize, usize) + Sync,
{
    let total = items.len();
    if total == 0 {
        return vec![];
    }

    let effective_chunk_size = if chunk_size == 0 { total } else { chunk_size };
    let completed = AtomicUsize::new(0);

    // Collect all results with their indices, then sort
    let mut indexed_results: Vec<(usize, R)> = Vec::with_capacity(total);

    for chunk_start in (0..total).step_by(effective_chunk_size) {
        let chunk_end = (chunk_start + effective_chunk_size).min(total);
        let chunk: Vec<(usize, &T)> = (chunk_start..chunk_end)
            .map(|i| (i, &items[i]))
            .collect();

        let chunk_results: Vec<(usize, R)> = chunk
            .par_iter()
            .map(|(idx, item)| {
                let result = processor(item);
                let done = completed.fetch_add(1, Ordering::Relaxed) + 1;
                if let Some(cb) = progress {
                    cb(done, total);
                }
                (*idx, result)
            })
            .collect();

        indexed_results.extend(chunk_results);
    }

    // Sort by index to maintain original order
    indexed_results.sort_by_key(|(idx, _)| *idx);
    indexed_results.into_iter().map(|(_, r)| r).collect()
}

/// Progress callback for pipeline steps
pub trait ProgressCallback: Send + Sync {
    /// Called when a new step starts
    fn on_step_start(&self, step: &str);
    /// Called to report progress within a step
    fn on_step_progress(&self, current: usize, total: usize);
    /// Called when a step completes
    fn on_step_complete(&self, step: &str, message: &str);
    /// Called for debug/verbose messages
    fn on_debug(&self, message: &str);
}

/// No-op progress callback (silent mode)
pub struct SilentProgress;

impl ProgressCallback for SilentProgress {
    fn on_step_start(&self, _step: &str) {}
    fn on_step_progress(&self, _current: usize, _total: usize) {}
    fn on_step_complete(&self, _step: &str, _message: &str) {}
    fn on_debug(&self, _message: &str) {}
}

/// Pipeline processing error
#[derive(Error, Debug)]
pub enum PipelineError {
    #[error("Input file not found: {0}")]
    InputNotFound(PathBuf),

    #[error("Output directory not writable: {0}")]
    OutputNotWritable(PathBuf),

    #[error("PDF extraction failed: {0}")]
    ExtractionFailed(String),

    #[error("Image processing failed: {0}")]
    ImageProcessingFailed(String),

    #[error("PDF generation failed: {0}")]
    PdfGenerationFailed(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

/// Pipeline configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineConfig {
    /// Output DPI
    pub dpi: u32,
    /// Enable deskew
    pub deskew: bool,
    /// Margin trim percentage
    pub margin_trim: f64,
    /// Enable AI upscaling
    pub upscale: bool,
    /// Enable GPU
    pub gpu: bool,
    /// Enable internal resolution normalization
    pub internal_resolution: bool,
    /// Enable color correction
    pub color_correction: bool,
    /// Enable offset alignment
    pub offset_alignment: bool,
    /// Output height
    pub output_height: u32,
    /// Enable OCR
    pub ocr: bool,
    /// Max pages for debug
    pub max_pages: Option<usize>,
    /// Save debug images
    pub save_debug: bool,
    /// JPEG quality (0-100)
    pub jpeg_quality: u8,
    /// Thread count (None = auto)
    pub threads: Option<usize>,
    /// Maximum memory usage in MB (0 = unlimited)
    #[serde(default)]
    pub max_memory_mb: usize,
    /// Chunk size for batch processing (0 = auto based on memory)
    #[serde(default)]
    pub chunk_size: usize,
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            dpi: 300,
            deskew: true,
            margin_trim: 0.5,
            upscale: true,
            gpu: true,
            internal_resolution: false,
            color_correction: false,
            offset_alignment: false,
            output_height: 3508,
            ocr: false,
            max_pages: None,
            save_debug: false,
            jpeg_quality: 90,
            threads: None,
            max_memory_mb: 0,  // 0 = unlimited
            chunk_size: 0,    // 0 = auto
        }
    }
}

impl PipelineConfig {
    /// Create configuration from CLI convert arguments
    pub fn from_convert_args(args: &ConvertArgs) -> Self {
        let advanced = args.advanced;
        Self {
            dpi: args.dpi,
            deskew: args.effective_deskew(),
            margin_trim: args.margin_trim as f64,
            upscale: args.effective_upscale(),
            gpu: args.effective_gpu(),
            internal_resolution: args.internal_resolution || advanced,
            color_correction: args.color_correction || advanced,
            offset_alignment: args.offset_alignment || advanced,
            output_height: args.output_height,
            ocr: args.ocr,
            max_pages: args.max_pages,
            save_debug: args.save_debug,
            jpeg_quality: args.jpeg_quality,
            threads: args.threads,
            max_memory_mb: 0,  // Auto-detect based on available memory
            chunk_size: 0,    // Auto-calculate based on memory limit
        }
    }

    /// Convert to JSON string for cache digest
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| "{}".to_string())
    }

    /// Builder pattern: set DPI
    pub fn with_dpi(mut self, dpi: u32) -> Self {
        self.dpi = dpi;
        self
    }

    /// Builder pattern: set deskew
    pub fn with_deskew(mut self, enabled: bool) -> Self {
        self.deskew = enabled;
        self
    }

    /// Builder pattern: set margin trim
    pub fn with_margin_trim(mut self, percent: f64) -> Self {
        self.margin_trim = percent;
        self
    }

    /// Builder pattern: set upscale
    pub fn with_upscale(mut self, enabled: bool) -> Self {
        self.upscale = enabled;
        self
    }

    /// Builder pattern: set GPU
    pub fn with_gpu(mut self, enabled: bool) -> Self {
        self.gpu = enabled;
        self
    }

    /// Builder pattern: set OCR
    pub fn with_ocr(mut self, enabled: bool) -> Self {
        self.ocr = enabled;
        self
    }

    /// Builder pattern: set max pages
    pub fn with_max_pages(mut self, max: Option<usize>) -> Self {
        self.max_pages = max;
        self
    }

    /// Enable all advanced features
    pub fn with_advanced(mut self) -> Self {
        self.internal_resolution = true;
        self.color_correction = true;
        self.offset_alignment = true;
        self
    }
}

/// Result of pipeline processing
#[derive(Debug, Clone)]
pub struct PipelineResult {
    /// Number of pages processed
    pub page_count: usize,
    /// Detected page number shift
    pub page_number_shift: Option<i32>,
    /// Whether vertical text was detected
    pub is_vertical: bool,
    /// Processing time in seconds
    pub elapsed_seconds: f64,
    /// Output file path
    pub output_path: PathBuf,
    /// Output file size in bytes
    pub output_size: u64,
}

impl PipelineResult {
    /// Create a new pipeline result
    pub fn new(
        page_count: usize,
        page_number_shift: Option<i32>,
        is_vertical: bool,
        elapsed_seconds: f64,
        output_path: PathBuf,
        output_size: u64,
    ) -> Self {
        Self {
            page_count,
            page_number_shift,
            is_vertical,
            elapsed_seconds,
            output_path,
            output_size,
        }
    }

    /// Convert to cache ProcessingResult
    pub fn to_cache_result(&self) -> crate::cache::ProcessingResult {
        crate::cache::ProcessingResult::new(
            self.page_count,
            self.page_number_shift,
            self.is_vertical,
            self.elapsed_seconds,
            self.output_size,
        )
    }
}

/// Processing context for intermediate data
#[derive(Debug)]
pub struct ProcessingContext {
    /// Working directory for this PDF
    pub work_dir: PathBuf,
    /// PDF document info
    pub pdf_info: crate::PdfDocument,
    /// Extracted page images
    pub current_images: Vec<PathBuf>,
    /// Detected vertical writing
    pub is_vertical: bool,
    /// Page number shift
    pub page_number_shift: Option<i32>,
}

/// PDF processing pipeline
pub struct PdfPipeline {
    config: PipelineConfig,
}

impl PdfPipeline {
    /// Create a new pipeline with the given configuration
    pub fn new(config: PipelineConfig) -> Self {
        Self { config }
    }

    /// Get the pipeline configuration
    pub fn config(&self) -> &PipelineConfig {
        &self.config
    }

    /// Get the output PDF path for a given input PDF
    pub fn get_output_path(&self, input: &Path, output_dir: &Path) -> PathBuf {
        let pdf_name = input.file_stem().unwrap_or_default().to_string_lossy();
        output_dir.join(format!("{}_converted.pdf", pdf_name))
    }

    /// Get the working directory for a PDF
    pub fn get_work_dir(&self, input: &Path, output_dir: &Path) -> PathBuf {
        let pdf_name = input.file_stem().unwrap_or_default().to_string_lossy();
        output_dir.join(format!(".work_{}", pdf_name))
    }

    /// Process a single PDF file (silent mode)
    pub fn process(&self, input: &Path, output_dir: &Path) -> Result<PipelineResult, PipelineError> {
        self.process_with_progress(input, output_dir, &SilentProgress)
    }

    /// Process a single PDF file with progress callback
    ///
    /// This is the main entry point for PDF processing.
    pub fn process_with_progress<P: ProgressCallback>(
        &self,
        input: &Path,
        output_dir: &Path,
        progress: &P,
    ) -> Result<PipelineResult, PipelineError> {
        let start_time = Instant::now();

        // Validate input
        if !input.exists() {
            return Err(PipelineError::InputNotFound(input.to_path_buf()));
        }

        // Create output directory
        std::fs::create_dir_all(output_dir)?;

        let output_path = self.get_output_path(input, output_dir);
        let work_dir = self.get_work_dir(input, output_dir);
        std::fs::create_dir_all(&work_dir)?;

        // Step 1: Read PDF metadata
        progress.on_step_start("Reading PDF...");
        let reader = crate::LopdfReader::new(input)
            .map_err(|e| PipelineError::ExtractionFailed(e.to_string()))?;
        let total_pages = reader.info.page_count;
        progress.on_step_complete("Reading PDF", &format!("{} pages", total_pages));

        // Step 2: Extract images
        progress.on_step_start(&format!("Extracting images (DPI: {})...", self.config.dpi));
        let extract_options = crate::ExtractOptions::builder()
            .dpi(self.config.dpi)
            .build();
        let extracted_dir = work_dir.join("extracted");
        std::fs::create_dir_all(&extracted_dir)?;

        let mut extracted_pages = crate::LopdfExtractor::extract_auto(input, &extracted_dir, &extract_options)
            .map_err(|e| PipelineError::ExtractionFailed(e.to_string()))?;

        // Apply max_pages limit
        if let Some(max_pages) = self.config.max_pages {
            if extracted_pages.len() > max_pages {
                progress.on_debug(&format!("Limiting to {} pages (--max-pages)", max_pages));
                extracted_pages.truncate(max_pages);
            }
        }
        let page_count = extracted_pages.len();
        progress.on_step_complete("Extracting images", &format!("{} pages", page_count));

        // Convert to PathBuf list
        let mut current_images: Vec<PathBuf> = extracted_pages.iter().map(|p| p.path.clone()).collect();

        // ================================================================
        // C#版互換処理順序:
        // 1. PDF抽出 (上で完了)
        // 2. マージントリム (0.5%)
        // 3. AI超解像 (RealESRGAN 2x)
        // 4. 内部解像度正規化 (4960x7016)
        // 5. 傾き補正 (Deskew) ← C#はここで実行
        // 6. 色補正
        // ================================================================

        // Step 2: Margin Trimming (C# does this first)
        // Note: margin_trim is a percentage, skip if 0
        if self.config.margin_trim > 0.0 {
            current_images = self.step_margin_trim(&work_dir, &current_images, progress)?;
        }

        // Step 3: AI Upscaling (if enabled)
        if self.config.upscale {
            current_images = self.step_upscale(&work_dir, &current_images, progress)?;
        }

        // Step 4: Internal Resolution Normalization (if enabled)
        // C#: Fit to 4960x7016 with Lanczos3, padding with paper color
        if self.config.internal_resolution {
            current_images = self.step_normalize(&work_dir, &current_images, progress)?;
        }

        // Step 5: Deskew (if enabled) - C# does deskew AFTER normalization
        if self.config.deskew {
            current_images = self.step_deskew(&work_dir, &current_images, progress)?;
        }

        // Step 6: Color Correction (if enabled)
        if self.config.color_correction {
            current_images = self.step_color_correction(&work_dir, &current_images, progress)?;
        }

        // Step 8: Tukey Fence Group Crop (if offset_alignment enabled)
        if self.config.offset_alignment {
            current_images = self.step_group_crop(&work_dir, &current_images, progress)?;
        }

        // Step 9: Page Number Offset Calculation
        let page_number_shift = if self.config.offset_alignment {
            self.step_page_number_detection(&current_images, progress)?
        } else {
            None
        };

        // Step 10: Final Output (resize)
        if self.config.output_height != 0 && self.config.output_height != 7016 {
            current_images = self.step_finalize(&work_dir, &current_images, progress)?;
        }

        // Step 11: Vertical Text Detection
        let is_vertical = self.step_vertical_detection(&current_images, progress)?;

        // Step 12: OCR with YomiToku (if enabled)
        let ocr_results = if self.config.ocr {
            self.step_ocr(&current_images, progress)?
        } else {
            vec![]
        };

        // Step 13: Generate PDF
        progress.on_step_start("Generating output PDF...");
        self.step_generate_pdf(&current_images, &output_path, &reader.info, &ocr_results, progress)?;

        // Get output file size
        let output_size = std::fs::metadata(&output_path)
            .map(|m| m.len())
            .unwrap_or(0);
        progress.on_step_complete("Generating PDF", &format!("{} bytes", output_size));

        // Cleanup work directory (unless save_debug)
        if !self.config.save_debug {
            std::fs::remove_dir_all(&work_dir).ok();
        }

        let elapsed = start_time.elapsed().as_secs_f64();

        Ok(PipelineResult::new(
            page_count,
            page_number_shift,
            is_vertical,
            elapsed,
            output_path,
            output_size,
        ))
    }

    // ============ Processing Step Implementations ============

    /// Step 3: Deskew correction
    fn step_deskew<P: ProgressCallback>(
        &self,
        work_dir: &Path,
        images: &[PathBuf],
        progress: &P,
    ) -> Result<Vec<PathBuf>, PipelineError> {
        progress.on_step_start("Applying deskew correction...");
        let deskewed_dir = work_dir.join("deskewed");
        std::fs::create_dir_all(&deskewed_dir)?;

        // Use PageEdge algorithm for scanned book pages (detects page boundary skew)
        let deskew_options = crate::DeskewOptions::builder()
            .algorithm(crate::DeskewAlgorithm::PageEdge)
            .build();
        let output_paths: Vec<PathBuf> = images
            .iter()
            .enumerate()
            .map(|(idx, img)| {
                let name = img
                    .file_name()
                    .map(|n| n.to_os_string())
                    .unwrap_or_else(|| std::ffi::OsString::from(format!("page_{:04}.png", idx)));
                deskewed_dir.join(name)
            })
            .collect();

        let results: Vec<PathBuf> = images
            .par_iter()
            .zip(output_paths.par_iter())
            .map(|(img_path, output_path)| {
                match crate::ImageProcDeskewer::correct_skew(img_path, output_path, &deskew_options) {
                    Ok(_) => {}
                    Err(_) => {
                        std::fs::copy(img_path, output_path).ok();
                    }
                }
                output_path.clone()
            })
            .collect();

        progress.on_step_complete("Deskew", &format!("{} images", results.len()));
        Ok(results)
    }

    /// Step 4: Margin trimming
    fn step_margin_trim<P: ProgressCallback>(
        &self,
        work_dir: &Path,
        images: &[PathBuf],
        progress: &P,
    ) -> Result<Vec<PathBuf>, PipelineError> {
        progress.on_step_start(&format!("Trimming margins ({}%)...", self.config.margin_trim));
        let trimmed_dir = work_dir.join("trimmed");
        std::fs::create_dir_all(&trimmed_dir)?;

        let margin_options = crate::MarginOptions::builder()
            .default_trim_percent(self.config.margin_trim as f32)
            .build();

        // Detect unified margins
        let unified = match crate::ImageMarginDetector::detect_unified(images, &margin_options) {
            Ok(u) => u,
            Err(_) => {
                progress.on_debug("Margin detection failed, skipping trim");
                return Ok(images.to_vec());
            }
        };

        let output_paths: Vec<PathBuf> = images
            .iter()
            .enumerate()
            .map(|(idx, img)| {
                let name = img
                    .file_name()
                    .map(|n| n.to_os_string())
                    .unwrap_or_else(|| std::ffi::OsString::from(format!("page_{:04}.png", idx)));
                trimmed_dir.join(name)
            })
            .collect();

        let results: Vec<PathBuf> = images
            .par_iter()
            .zip(output_paths.par_iter())
            .map(|(img_path, output_path)| {
                match crate::ImageMarginDetector::trim(img_path, output_path, &unified.margins) {
                    Ok(_) => {}
                    Err(_) => {
                        std::fs::copy(img_path, output_path).ok();
                    }
                }
                output_path.clone()
            })
            .collect();

        progress.on_step_complete("Margin trim", &format!("{} images", results.len()));
        Ok(results)
    }

    /// Step 5: AI Upscaling
    fn step_upscale<P: ProgressCallback>(
        &self,
        work_dir: &Path,
        images: &[PathBuf],
        progress: &P,
    ) -> Result<Vec<PathBuf>, PipelineError> {
        progress.on_step_start("AI Upscaling (RealESRGAN)...");
        let upscaled_dir = work_dir.join("upscaled");
        std::fs::create_dir_all(&upscaled_dir)?;

        // Try to initialize RealESRGAN
        let venv_path = std::env::var("SUPERBOOK_VENV")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("./venv"));

        let bridge_config = crate::AiBridgeConfig::builder()
            .venv_path(venv_path)
            .build();

        let bridge = match crate::SubprocessBridge::new(bridge_config) {
            Ok(b) => b,
            Err(e) => {
                progress.on_debug(&format!("RealESRGAN not available: {}", e));
                return Ok(images.to_vec());
            }
        };

        let esrgan = crate::RealEsrgan::new(bridge);
        let mut options = crate::RealEsrganOptions::builder().scale(2);
        if self.config.gpu {
            options = options.gpu_id(0);
        }
        let options = options.build();

        match esrgan.upscale_batch(images, &upscaled_dir, &options, None) {
            Ok(result) => {
                progress.on_step_complete("Upscaling", &format!("{} images", result.successful.len()));
                Ok(result.successful.iter().map(|r| r.output_path.clone()).collect())
            }
            Err(e) => {
                progress.on_debug(&format!("Upscaling failed: {}", e));
                Ok(images.to_vec())
            }
        }
    }

    /// Step 6: Internal resolution normalization
    fn step_normalize<P: ProgressCallback>(
        &self,
        work_dir: &Path,
        images: &[PathBuf],
        progress: &P,
    ) -> Result<Vec<PathBuf>, PipelineError> {
        progress.on_step_start("Normalizing to internal resolution (4960x7016)...");
        let normalized_dir = work_dir.join("normalized");
        std::fs::create_dir_all(&normalized_dir)?;

        let normalize_options = crate::NormalizeOptions::builder()
            .target_width(4960)
            .target_height(7016)
            .build();

        let output_paths: Vec<PathBuf> = (0..images.len())
            .map(|i| normalized_dir.join(format!("page_{:04}.png", i)))
            .collect();

        let completed = Arc::new(AtomicUsize::new(0));
        let _total = images.len();

        let results: Vec<PathBuf> = images
            .par_iter()
            .zip(output_paths.par_iter())
            .map(|(img_path, output_path)| {
                match crate::ImageNormalizer::normalize(img_path, output_path, &normalize_options) {
                    Ok(_) => {}
                    Err(_) => {
                        std::fs::copy(img_path, output_path).ok();
                    }
                }
                completed.fetch_add(1, Ordering::Relaxed);
                output_path.clone()
            })
            .collect();

        progress.on_step_complete("Normalization", &format!("{} images", results.len()));
        Ok(results)
    }

    /// Step 7: Color correction
    fn step_color_correction<P: ProgressCallback>(
        &self,
        work_dir: &Path,
        images: &[PathBuf],
        progress: &P,
    ) -> Result<Vec<PathBuf>, PipelineError> {
        progress.on_step_start("Analyzing color statistics...");
        let color_corrected_dir = work_dir.join("color_corrected");
        std::fs::create_dir_all(&color_corrected_dir)?;

        // Collect color statistics
        let stats_results: Vec<_> = images
            .par_iter()
            .map(|img_path| crate::ColorAnalyzer::calculate_stats(img_path))
            .collect();

        let all_stats: Vec<_> = stats_results
            .into_iter()
            .filter_map(|r| r.ok())
            .collect();

        if all_stats.is_empty() {
            progress.on_debug("Color analysis failed, skipping correction");
            return Ok(images.to_vec());
        }

        let global_param = crate::ColorAnalyzer::decide_global_adjustment(&all_stats);

        let output_paths: Vec<PathBuf> = (0..images.len())
            .map(|i| color_corrected_dir.join(format!("page_{:04}.png", i)))
            .collect();

        let results: Vec<PathBuf> = images
            .par_iter()
            .zip(output_paths.par_iter())
            .map(|(img_path, output_path)| {
                if let Ok(img) = image::open(img_path) {
                    let mut rgb_img = img.to_rgb8();
                    crate::ColorAnalyzer::apply_adjustment(&mut rgb_img, &global_param);
                    rgb_img.save(output_path).ok();
                } else {
                    std::fs::copy(img_path, output_path).ok();
                }
                output_path.clone()
            })
            .collect();

        progress.on_step_complete("Color correction", &format!("{} images", results.len()));
        Ok(results)
    }

    /// Step 8: Tukey fence group crop
    fn step_group_crop<P: ProgressCallback>(
        &self,
        work_dir: &Path,
        images: &[PathBuf],
        progress: &P,
    ) -> Result<Vec<PathBuf>, PipelineError> {
        progress.on_step_start("Detecting text bounding boxes...");
        let cropped_dir = work_dir.join("cropped");
        std::fs::create_dir_all(&cropped_dir)?;

        let bounding_boxes = crate::GroupCropAnalyzer::detect_all_bounding_boxes(images, 240);

        if bounding_boxes.is_empty() {
            progress.on_debug("No bounding boxes detected, skipping crop");
            return Ok(images.to_vec());
        }

        let unified = crate::GroupCropAnalyzer::unify_and_expand_regions(
            &bounding_boxes,
            5,    // 5% margin expansion
            4960, // internal width limit
            7016, // internal height limit
        );

        let output_paths: Vec<PathBuf> = (0..images.len())
            .map(|i| cropped_dir.join(format!("page_{:04}.png", i)))
            .collect();

        let results: Vec<PathBuf> = images
            .par_iter()
            .zip(output_paths.par_iter())
            .enumerate()
            .map(|(i, (img_path, output_path))| {
                let region = if i % 2 == 0 { &unified.odd_region } else { &unified.even_region };

                if let Ok(img) = image::open(img_path) {
                    let cropped = img.crop_imm(
                        region.left,
                        region.top,
                        region.width.min(img.width() - region.left),
                        region.height.min(img.height() - region.top),
                    );
                    cropped.save(output_path).ok();
                } else {
                    std::fs::copy(img_path, output_path).ok();
                }
                output_path.clone()
            })
            .collect();

        progress.on_step_complete("Group crop", &format!("{} images", results.len()));
        Ok(results)
    }

    /// Step 9: Page number detection
    fn step_page_number_detection<P: ProgressCallback>(
        &self,
        images: &[PathBuf],
        progress: &P,
    ) -> Result<Option<i32>, PipelineError> {
        progress.on_step_start("Detecting page numbers...");

        let page_options = crate::PageNumberOptions::default();
        let mut page_detections = Vec::new();

        for (i, img_path) in images.iter().enumerate() {
            if let Ok(detection) = crate::TesseractPageDetector::detect_single(img_path, i, &page_options) {
                page_detections.push(detection);
            }
        }

        if page_detections.is_empty() {
            progress.on_step_complete("Page number detection", "no pages detected");
            return Ok(None);
        }

        let first_img = image::open(&images[0]).ok();
        let img_height = first_img.as_ref().map(|img| img.height()).unwrap_or(7016);

        let analysis = crate::PageOffsetAnalyzer::analyze_offsets(&page_detections, img_height);
        progress.on_step_complete("Page number detection", &format!("shift: {}", analysis.page_number_shift));

        Ok(Some(analysis.page_number_shift))
    }

    /// Step 10: Finalize output
    fn step_finalize<P: ProgressCallback>(
        &self,
        work_dir: &Path,
        images: &[PathBuf],
        progress: &P,
    ) -> Result<Vec<PathBuf>, PipelineError> {
        progress.on_step_start(&format!("Finalizing output (height: {})...", self.config.output_height));
        let finalized_dir = work_dir.join("finalized");
        std::fs::create_dir_all(&finalized_dir)?;

        let finalize_options = crate::FinalizeOptions::builder()
            .target_height(self.config.output_height)
            .build();

        let output_paths: Vec<PathBuf> = (0..images.len())
            .map(|i| finalized_dir.join(format!("page_{:04}.png", i)))
            .collect();

        let results: Vec<PathBuf> = images
            .par_iter()
            .zip(output_paths.par_iter())
            .map(|(img_path, output_path)| {
                match crate::PageFinalizer::finalize(img_path, output_path, &finalize_options, None, 0, 0) {
                    Ok(_) => {}
                    Err(_) => {
                        std::fs::copy(img_path, output_path).ok();
                    }
                }
                output_path.clone()
            })
            .collect();

        progress.on_step_complete("Finalize", &format!("{} images", results.len()));
        Ok(results)
    }

    /// Step 11: Vertical text detection
    fn step_vertical_detection<P: ProgressCallback>(
        &self,
        images: &[PathBuf],
        progress: &P,
    ) -> Result<bool, PipelineError> {
        if images.is_empty() {
            return Ok(false);
        }

        progress.on_step_start("Detecting text direction...");

        let mut gray_images = Vec::new();
        for img_path in images.iter().take(20) {
            if let Ok(img) = image::open(img_path) {
                gray_images.push(img.to_luma8());
            }
        }

        if gray_images.is_empty() {
            progress.on_step_complete("Vertical detection", "no images to analyze");
            return Ok(false);
        }

        let vd_options = crate::VerticalDetectOptions::default();
        match crate::detect_book_vertical_writing(&gray_images, &vd_options) {
            Ok(result) => {
                let direction = if result.is_vertical { "vertical" } else { "horizontal" };
                progress.on_step_complete("Vertical detection", direction);
                Ok(result.is_vertical)
            }
            Err(_) => {
                progress.on_step_complete("Vertical detection", "failed");
                Ok(false)
            }
        }
    }

    /// Step 12: OCR with YomiToku
    fn step_ocr<P: ProgressCallback>(
        &self,
        images: &[PathBuf],
        progress: &P,
    ) -> Result<Vec<Option<crate::OcrResult>>, PipelineError> {
        progress.on_step_start("Running OCR (YomiToku)...");

        let venv_path = std::env::var("SUPERBOOK_VENV")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("./venv"));

        let bridge_config = crate::AiBridgeConfig::builder()
            .venv_path(venv_path)
            .build();

        let bridge = match crate::SubprocessBridge::new(bridge_config) {
            Ok(b) => b,
            Err(e) => {
                progress.on_debug(&format!("YomiToku not available: {}", e));
                return Ok(vec![]);
            }
        };

        let yomitoku = crate::YomiToku::new(bridge);
        let mut ocr_opts = crate::YomiTokuOptions::builder();
        if self.config.gpu {
            ocr_opts = ocr_opts.use_gpu(true).gpu_id(0);
        }
        let ocr_opts = ocr_opts.build();

        let mut results = Vec::new();
        for img_path in images {
            match yomitoku.ocr(img_path, &ocr_opts) {
                Ok(result) => results.push(Some(result)),
                Err(_) => results.push(None),
            }
        }

        let success_count = results.iter().filter(|r| r.is_some()).count();
        progress.on_step_complete("OCR", &format!("{}/{} pages", success_count, results.len()));
        Ok(results)
    }

    /// Step 13: Generate PDF
    fn step_generate_pdf<P: ProgressCallback>(
        &self,
        images: &[PathBuf],
        output_path: &Path,
        pdf_info: &crate::PdfDocument,
        ocr_results: &[Option<crate::OcrResult>],
        _progress: &P,
    ) -> Result<(), PipelineError> {
        use crate::pdf_writer::{OcrLayer, OcrPageText, TextBlock};

        // Convert OCR results to OcrLayer
        let ocr_layer = if !ocr_results.is_empty() {
            let pages: Vec<OcrPageText> = ocr_results
                .iter()
                .enumerate()
                .filter_map(|(idx, result): (usize, &Option<crate::OcrResult>)| {
                    result.as_ref().map(|r| OcrPageText {
                        page_index: idx,
                        blocks: r
                            .text_blocks
                            .iter()
                            .map(|b| TextBlock {
                                x: b.bbox.0 as f64,
                                y: b.bbox.1 as f64,
                                width: (b.bbox.2 - b.bbox.0) as f64,
                                height: (b.bbox.3 - b.bbox.1) as f64,
                                text: b.text.clone(),
                                font_size: b.font_size.unwrap_or(12.0) as f64,
                                vertical: matches!(b.direction, crate::TextDirection::Vertical),
                            })
                            .collect(),
                    })
                })
                .collect();

            if pages.is_empty() { None } else { Some(OcrLayer { pages }) }
        } else {
            None
        };

        let mut pdf_builder = crate::PdfWriterOptions::builder()
            .dpi(self.config.dpi)
            .jpeg_quality(self.config.jpeg_quality)
            .metadata(pdf_info.metadata.clone());

        if let Some(layer) = ocr_layer {
            pdf_builder = pdf_builder.ocr_layer(layer);
        }

        let pdf_options = pdf_builder.build();

        crate::PrintPdfWriter::create_from_images(images, output_path, &pdf_options)
            .map_err(|e| PipelineError::PdfGenerationFailed(e.to_string()))?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ============ PipelineConfig Tests ============

    #[test]
    fn test_pipeline_config_default() {
        // TC: PIPE-003
        let config = PipelineConfig::default();

        assert_eq!(config.dpi, 300);
        assert!(config.deskew);
        assert_eq!(config.margin_trim, 0.5);
        assert!(config.upscale);
        assert!(config.gpu);
        assert!(!config.internal_resolution);
        assert!(!config.color_correction);
        assert!(!config.offset_alignment);
        assert_eq!(config.output_height, 3508);
        assert!(!config.ocr);
        assert!(config.max_pages.is_none());
        assert!(!config.save_debug);
        assert_eq!(config.jpeg_quality, 90);
        assert!(config.threads.is_none());
        // Phase 3: Memory management fields
        assert_eq!(config.max_memory_mb, 0);
        assert_eq!(config.chunk_size, 0);
    }

    #[test]
    fn test_pipeline_config_to_json() {
        // TC: PIPE-002
        let config = PipelineConfig::default();
        let json = config.to_json();

        assert!(json.contains("\"dpi\":300"));
        assert!(json.contains("\"deskew\":true"));
        assert!(json.contains("\"upscale\":true"));
    }

    #[test]
    fn test_pipeline_config_builder() {
        let config = PipelineConfig::default()
            .with_dpi(600)
            .with_deskew(false)
            .with_upscale(false)
            .with_ocr(true);

        assert_eq!(config.dpi, 600);
        assert!(!config.deskew);
        assert!(!config.upscale);
        assert!(config.ocr);
    }

    #[test]
    fn test_pipeline_config_with_advanced() {
        let config = PipelineConfig::default().with_advanced();

        assert!(config.internal_resolution);
        assert!(config.color_correction);
        assert!(config.offset_alignment);
    }

    #[test]
    fn test_pipeline_config_with_max_pages() {
        let config = PipelineConfig::default().with_max_pages(Some(10));

        assert_eq!(config.max_pages, Some(10));
    }

    #[test]
    fn test_pipeline_config_with_margin_trim() {
        let config = PipelineConfig::default().with_margin_trim(1.0);

        assert_eq!(config.margin_trim, 1.0);
    }

    #[test]
    fn test_pipeline_config_with_gpu() {
        let config = PipelineConfig::default().with_gpu(false);

        assert!(!config.gpu);
    }

    // ============ PipelineResult Tests ============

    #[test]
    fn test_pipeline_result_new() {
        // TC: PIPE-004
        let result = PipelineResult::new(
            100,
            Some(2),
            true,
            45.5,
            PathBuf::from("/output/file.pdf"),
            12345678,
        );

        assert_eq!(result.page_count, 100);
        assert_eq!(result.page_number_shift, Some(2));
        assert!(result.is_vertical);
        assert_eq!(result.elapsed_seconds, 45.5);
        assert_eq!(result.output_path, PathBuf::from("/output/file.pdf"));
        assert_eq!(result.output_size, 12345678);
    }

    #[test]
    fn test_pipeline_result_no_shift() {
        let result = PipelineResult::new(
            50,
            None,
            false,
            10.0,
            PathBuf::from("/output/test.pdf"),
            5000000,
        );

        assert_eq!(result.page_count, 50);
        assert!(result.page_number_shift.is_none());
        assert!(!result.is_vertical);
    }

    #[test]
    fn test_pipeline_result_to_cache() {
        let result = PipelineResult::new(100, Some(2), true, 45.5, PathBuf::from("/out.pdf"), 1000);

        let cache_result = result.to_cache_result();

        assert_eq!(cache_result.page_count, 100);
        assert_eq!(cache_result.page_number_shift, Some(2));
        assert!(cache_result.is_vertical);
        assert_eq!(cache_result.elapsed_seconds, 45.5);
        assert_eq!(cache_result.output_size, 1000);
    }

    // ============ PdfPipeline Tests ============

    #[test]
    fn test_pdf_pipeline_new() {
        // TC: PIPE-005
        let config = PipelineConfig::default();
        let pipeline = PdfPipeline::new(config);

        assert_eq!(pipeline.config().dpi, 300);
    }

    #[test]
    fn test_pdf_pipeline_get_output_path() {
        let config = PipelineConfig::default();
        let pipeline = PdfPipeline::new(config);

        let input = Path::new("/input/document.pdf");
        let output_dir = Path::new("/output");

        let output_path = pipeline.get_output_path(input, output_dir);

        assert_eq!(output_path, PathBuf::from("/output/document_converted.pdf"));
    }

    #[test]
    fn test_pdf_pipeline_get_output_path_no_extension() {
        let config = PipelineConfig::default();
        let pipeline = PdfPipeline::new(config);

        let input = Path::new("/input/document");
        let output_dir = Path::new("/output");

        let output_path = pipeline.get_output_path(input, output_dir);

        assert_eq!(output_path, PathBuf::from("/output/document_converted.pdf"));
    }

    #[test]
    fn test_pdf_pipeline_process_input_not_found() {
        let config = PipelineConfig::default();
        let pipeline = PdfPipeline::new(config);

        let result = pipeline.process(Path::new("/nonexistent/file.pdf"), Path::new("/output"));

        assert!(matches!(result, Err(PipelineError::InputNotFound(_))));
    }

    // ============ PipelineError Tests ============

    #[test]
    fn test_pipeline_error_display() {
        let err = PipelineError::InputNotFound(PathBuf::from("/test.pdf"));
        assert!(err.to_string().contains("/test.pdf"));

        let err = PipelineError::ExtractionFailed("test error".to_string());
        assert!(err.to_string().contains("test error"));
    }

    #[test]
    fn test_pipeline_error_io() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
        let err: PipelineError = io_err.into();

        assert!(matches!(err, PipelineError::Io(_)));
    }

    #[test]
    fn test_pipeline_error_variants() {
        let errors = vec![
            PipelineError::InputNotFound(PathBuf::from("/test.pdf")),
            PipelineError::OutputNotWritable(PathBuf::from("/out")),
            PipelineError::ExtractionFailed("test".to_string()),
            PipelineError::ImageProcessingFailed("test".to_string()),
            PipelineError::PdfGenerationFailed("test".to_string()),
        ];

        for err in errors {
            assert!(!err.to_string().is_empty());
        }
    }

    // ============ Memory Management Tests (Phase 3) ============

    #[test]
    fn test_calculate_optimal_chunk_size_basic() {
        // With 4GB memory limit and 100MB per image, should allow ~20 concurrent
        let chunk = calculate_optimal_chunk_size(100, 4096, 8);
        assert!(chunk >= MIN_CHUNK_SIZE);
        assert!(chunk <= 100);
    }

    #[test]
    fn test_calculate_optimal_chunk_size_small_batch() {
        // When total items is small, chunk size should not exceed it
        let chunk = calculate_optimal_chunk_size(3, 4096, 8);
        assert!(chunk >= 1);
        assert!(chunk <= 3);
    }

    #[test]
    fn test_calculate_optimal_chunk_size_zero_items() {
        let chunk = calculate_optimal_chunk_size(0, 4096, 8);
        assert_eq!(chunk, 1); // At least 1 to prevent division by zero
    }

    #[test]
    fn test_calculate_optimal_chunk_size_limited_memory() {
        // With very limited memory, chunk size should be small
        let chunk = calculate_optimal_chunk_size(100, 200, 8);
        assert!(chunk >= MIN_CHUNK_SIZE);
        // With only 200MB, usable = 100MB, capacity = 1, so MIN_CHUNK_SIZE
    }

    #[test]
    fn test_calculate_optimal_chunk_size_auto_memory() {
        // 0 memory means auto-detect
        let chunk = calculate_optimal_chunk_size(100, 0, 8);
        assert!(chunk >= 1);
    }

    #[test]
    fn test_process_in_chunks_empty() {
        let items: Vec<i32> = vec![];
        let results: Vec<i32> = process_in_chunks(&items, 4, |x| *x * 2, None::<&fn(usize, usize)>);
        assert!(results.is_empty());
    }

    #[test]
    fn test_process_in_chunks_single_item() {
        let items = vec![5];
        let results: Vec<i32> = process_in_chunks(&items, 4, |x| *x * 2, None::<&fn(usize, usize)>);
        assert_eq!(results, vec![10]);
    }

    #[test]
    fn test_process_in_chunks_maintains_order() {
        let items: Vec<i32> = (0..20).collect();
        let results: Vec<i32> = process_in_chunks(&items, 4, |x| *x * 2, None::<&fn(usize, usize)>);

        let expected: Vec<i32> = (0..20).map(|x| x * 2).collect();
        assert_eq!(results, expected);
    }

    #[test]
    fn test_process_in_chunks_with_progress() {
        use std::sync::atomic::AtomicUsize;

        let items: Vec<i32> = (0..10).collect();
        let progress_count = Arc::new(AtomicUsize::new(0));
        let progress_count_clone = progress_count.clone();

        let progress_fn = move |_current: usize, _total: usize| {
            progress_count_clone.fetch_add(1, Ordering::Relaxed);
        };

        let results: Vec<i32> = process_in_chunks(&items, 4, |x| *x * 2, Some(&progress_fn));

        assert_eq!(results.len(), 10);
        assert_eq!(progress_count.load(Ordering::Relaxed), 10);
    }

    #[test]
    fn test_process_in_chunks_chunk_size_zero() {
        // chunk_size 0 means process all at once
        let items: Vec<i32> = (0..10).collect();
        let results: Vec<i32> = process_in_chunks(&items, 0, |x| *x + 1, None::<&fn(usize, usize)>);

        let expected: Vec<i32> = (1..11).collect();
        assert_eq!(results, expected);
    }

    #[test]
    fn test_pipeline_config_memory_fields() {
        let config = PipelineConfig::default();
        assert_eq!(config.max_memory_mb, 0);
        assert_eq!(config.chunk_size, 0);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn test_get_available_memory_linux() {
        let mem = get_available_memory_mb();
        // On Linux, this should return Some value
        assert!(mem.is_some());
        assert!(mem.unwrap() > 0);
    }
}
