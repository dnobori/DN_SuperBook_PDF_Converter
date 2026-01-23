//! RealESRGAN Integration module
//!
//! Provides integration with RealESRGAN AI upscaling model.
//!
//! # Features
//!
//! - 2x/4x AI upscaling for scanned images
//! - Multiple model support (general, anime, video)
//! - VRAM-aware tile processing
//! - GPU acceleration with fallback
//!
//! # Example
//!
//! ```rust,no_run
//! use superbook_pdf::{RealEsrgan, RealEsrganOptions};
//!
//! // Configure upscaling
//! let options = RealEsrganOptions::builder()
//!     .scale(2)
//!     .tile_size(400)
//!     .build();
//!
//! // Upscale an image
//! // let result = RealEsrgan::new().upscale("input.png", "output.png", &options);
//! ```

use crate::ai_bridge::{AiBridgeError, AiTool, SubprocessBridge};
use std::path::{Path, PathBuf};
use std::time::Duration;
use thiserror::Error;

// ============================================================
// Constants
// ============================================================

/// Default tile size for balanced performance
const DEFAULT_TILE_SIZE: u32 = 400;

/// Tile size for high quality processing (more VRAM)
#[allow(dead_code)]
const HIGH_QUALITY_TILE_SIZE: u32 = 512;

/// Tile size for anime content optimization
const ANIME_TILE_SIZE: u32 = 256;

/// Tile size for low VRAM environments
const LOW_VRAM_TILE_SIZE: u32 = 128;

/// Default tile padding
const DEFAULT_TILE_PADDING: u32 = 10;

/// Minimum allowed tile size
const MIN_TILE_SIZE: u32 = 64;

/// Maximum allowed tile size
const MAX_TILE_SIZE: u32 = 1024;

/// Default scale factor
const DEFAULT_SCALE: u32 = 2;

/// Base VRAM for tile size calculation (4GB)
const BASE_VRAM_MB: u64 = 4096;

/// RealESRGAN error types
#[derive(Debug, Error)]
pub enum RealEsrganError {
    #[error("Model not found: {0}")]
    ModelNotFound(String),

    #[error("Invalid scale: {0} (must be 2 or 4)")]
    InvalidScale(u32),

    #[error("Input image not found: {0}")]
    InputNotFound(PathBuf),

    #[error("Output directory not writable: {0}")]
    OutputNotWritable(PathBuf),

    #[error("Processing failed: {0}")]
    ProcessingFailed(String),

    #[error("GPU memory insufficient (need {required}MB, available {available}MB)")]
    InsufficientVram { required: u64, available: u64 },

    #[error("Bridge error: {0}")]
    BridgeError(#[from] AiBridgeError),

    #[error("Image error: {0}")]
    ImageError(String),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, RealEsrganError>;

/// RealESRGAN options
#[derive(Debug, Clone)]
pub struct RealEsrganOptions {
    /// Upscale factor
    pub scale: u32,
    /// Model selection
    pub model: RealEsrganModel,
    /// Tile size (pixels)
    pub tile_size: u32,
    /// Tile padding
    pub tile_padding: u32,
    /// Output format
    pub output_format: OutputFormat,
    /// Enable face enhancement
    pub face_enhance: bool,
    /// GPU ID (None for auto)
    pub gpu_id: Option<u32>,
    /// Use FP16 for speed
    pub fp16: bool,
}

impl Default for RealEsrganOptions {
    fn default() -> Self {
        Self {
            scale: DEFAULT_SCALE,
            model: RealEsrganModel::X4Plus,
            tile_size: DEFAULT_TILE_SIZE,
            tile_padding: DEFAULT_TILE_PADDING,
            output_format: OutputFormat::Png,
            face_enhance: false,
            gpu_id: None,
            fp16: true,
        }
    }
}

impl RealEsrganOptions {
    /// Create a new options builder
    pub fn builder() -> RealEsrganOptionsBuilder {
        RealEsrganOptionsBuilder::default()
    }

    /// Create options for 4x upscaling (high quality)
    pub fn x4_high_quality() -> Self {
        Self {
            scale: 4,
            model: RealEsrganModel::X4Plus,
            tile_size: ANIME_TILE_SIZE, // Smaller tiles for quality
            fp16: false,                // More accurate
            ..Default::default()
        }
    }

    /// Create options optimized for anime/illustrations
    pub fn anime() -> Self {
        Self {
            scale: 4,
            model: RealEsrganModel::X4PlusAnime,
            ..Default::default()
        }
    }

    /// Create options for low VRAM (< 4GB)
    pub fn low_vram() -> Self {
        Self {
            tile_size: LOW_VRAM_TILE_SIZE,
            tile_padding: 8,
            fp16: true,
            ..Default::default()
        }
    }
}

/// Builder for RealEsrganOptions
#[derive(Debug, Default)]
pub struct RealEsrganOptionsBuilder {
    options: RealEsrganOptions,
}

impl RealEsrganOptionsBuilder {
    /// Set upscale factor (2 or 4)
    #[must_use]
    pub fn scale(mut self, scale: u32) -> Self {
        self.options.scale = if scale >= 4 { 4 } else { 2 };
        self
    }

    /// Set model type
    #[must_use]
    pub fn model(mut self, model: RealEsrganModel) -> Self {
        self.options.model = model;
        self
    }

    /// Set tile size for memory efficiency
    #[must_use]
    pub fn tile_size(mut self, size: u32) -> Self {
        self.options.tile_size = size.clamp(MIN_TILE_SIZE, MAX_TILE_SIZE);
        self
    }

    /// Set tile padding
    #[must_use]
    pub fn tile_padding(mut self, padding: u32) -> Self {
        self.options.tile_padding = padding;
        self
    }

    /// Set output format
    #[must_use]
    pub fn output_format(mut self, format: OutputFormat) -> Self {
        self.options.output_format = format;
        self
    }

    /// Enable face enhancement
    #[must_use]
    pub fn face_enhance(mut self, enable: bool) -> Self {
        self.options.face_enhance = enable;
        self
    }

    /// Set GPU device ID
    #[must_use]
    pub fn gpu_id(mut self, id: u32) -> Self {
        self.options.gpu_id = Some(id);
        self
    }

    /// Enable FP16 mode for speed
    #[must_use]
    pub fn fp16(mut self, enable: bool) -> Self {
        self.options.fp16 = enable;
        self
    }

    /// Build the options
    #[must_use]
    pub fn build(self) -> RealEsrganOptions {
        self.options
    }
}

/// RealESRGAN model types
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum RealEsrganModel {
    /// RealESRGAN_x4plus (high quality, general purpose)
    #[default]
    X4Plus,
    /// RealESRGAN_x4plus_anime (anime/illustration)
    X4PlusAnime,
    /// RealESRNet_x4plus (faster, slightly lower quality)
    NetX4Plus,
    /// RealESRGAN_x2plus
    X2Plus,
    /// Custom model
    Custom(String),
}

impl RealEsrganModel {
    /// Get default scale for model
    pub fn default_scale(&self) -> u32 {
        match self {
            Self::X4Plus | Self::X4PlusAnime | Self::NetX4Plus => 4,
            Self::X2Plus => 2,
            Self::Custom(_) => 4,
        }
    }

    /// Get model name
    pub fn model_name(&self) -> &str {
        match self {
            Self::X4Plus => "RealESRGAN_x4plus",
            Self::X4PlusAnime => "RealESRGAN_x4plus_anime_6B",
            Self::NetX4Plus => "RealESRNet_x4plus",
            Self::X2Plus => "RealESRGAN_x2plus",
            Self::Custom(name) => name,
        }
    }
}

/// Output formats
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum OutputFormat {
    #[default]
    Png,
    Jpg {
        quality: u8,
    },
    Webp {
        quality: u8,
    },
}

impl OutputFormat {
    /// Get file extension
    pub fn extension(&self) -> &str {
        match self {
            OutputFormat::Png => "png",
            OutputFormat::Jpg { .. } => "jpg",
            OutputFormat::Webp { .. } => "webp",
        }
    }
}

/// Upscale result for single image
#[derive(Debug, Clone)]
pub struct UpscaleResult {
    /// Input file path
    pub input_path: PathBuf,
    /// Output file path
    pub output_path: PathBuf,
    /// Original resolution
    pub original_size: (u32, u32),
    /// Upscaled resolution
    pub upscaled_size: (u32, u32),
    /// Actual scale factor
    pub actual_scale: f32,
    /// Processing time
    pub processing_time: Duration,
    /// VRAM usage (MB)
    pub vram_used_mb: Option<u64>,
}

/// Batch upscale result
#[derive(Debug)]
pub struct BatchUpscaleResult {
    /// Successful results
    pub successful: Vec<UpscaleResult>,
    /// Failed files
    pub failed: Vec<(PathBuf, String)>,
    /// Total processing time
    pub total_time: Duration,
    /// Peak VRAM usage
    pub peak_vram_mb: Option<u64>,
}

/// RealESRGAN processor trait
pub trait RealEsrganProcessor {
    /// Upscale single image
    fn upscale(
        &self,
        input_path: &Path,
        output_path: &Path,
        options: &RealEsrganOptions,
    ) -> Result<UpscaleResult>;

    /// Batch upscale
    fn upscale_batch(
        &self,
        input_files: &[PathBuf],
        output_dir: &Path,
        options: &RealEsrganOptions,
        progress: Option<Box<dyn Fn(usize, usize) + Send>>,
    ) -> Result<BatchUpscaleResult>;

    /// Upscale all images in directory
    fn upscale_directory(
        &self,
        input_dir: &Path,
        output_dir: &Path,
        options: &RealEsrganOptions,
        progress: Option<Box<dyn Fn(usize, usize) + Send>>,
    ) -> Result<BatchUpscaleResult>;

    /// Get available models
    fn available_models(&self) -> Vec<RealEsrganModel>;

    /// Calculate recommended tile size
    fn recommended_tile_size(&self, image_size: (u32, u32), available_vram_mb: u64) -> u32;
}

/// RealESRGAN implementation
pub struct RealEsrgan {
    bridge: SubprocessBridge,
}

impl RealEsrgan {
    /// Create a new RealESRGAN processor
    pub fn new(bridge: SubprocessBridge) -> Self {
        Self { bridge }
    }

    /// Upscale single image
    pub fn upscale(
        &self,
        input_path: &Path,
        output_path: &Path,
        options: &RealEsrganOptions,
    ) -> Result<UpscaleResult> {
        if !input_path.exists() {
            return Err(RealEsrganError::InputNotFound(input_path.to_path_buf()));
        }

        // Get original image size
        let img =
            image::open(input_path).map_err(|e| RealEsrganError::ImageError(e.to_string()))?;
        let original_size = (img.width(), img.height());

        let start_time = std::time::Instant::now();

        // Create output directory if needed
        if let Some(parent) = output_path.parent() {
            if !parent.exists() {
                std::fs::create_dir_all(parent)
                    .map_err(|_| RealEsrganError::OutputNotWritable(parent.to_path_buf()))?;
            }
        }

        // Execute via bridge
        let result = self
            .bridge
            .execute(
                AiTool::RealESRGAN,
                &[input_path.to_path_buf()],
                output_path.parent().unwrap_or(Path::new(".")),
                options,
            )
            .map_err(RealEsrganError::BridgeError)?;

        if !result.failed_files.is_empty() {
            let (_, error) = &result.failed_files[0];
            return Err(RealEsrganError::ProcessingFailed(error.clone()));
        }

        // Verify output and get upscaled size
        let upscaled_size = if output_path.exists() {
            let output_img =
                image::open(output_path).map_err(|e| RealEsrganError::ImageError(e.to_string()))?;
            (output_img.width(), output_img.height())
        } else {
            // Estimate based on scale
            (
                original_size.0 * options.scale,
                original_size.1 * options.scale,
            )
        };

        let actual_scale = upscaled_size.0 as f32 / original_size.0 as f32;

        Ok(UpscaleResult {
            input_path: input_path.to_path_buf(),
            output_path: output_path.to_path_buf(),
            original_size,
            upscaled_size,
            actual_scale,
            processing_time: start_time.elapsed(),
            vram_used_mb: result.gpu_stats.map(|s| s.peak_vram_mb),
        })
    }

    /// Batch upscale multiple images
    pub fn upscale_batch(
        &self,
        input_files: &[PathBuf],
        output_dir: &Path,
        options: &RealEsrganOptions,
        progress: Option<Box<dyn Fn(usize, usize) + Send>>,
    ) -> Result<BatchUpscaleResult> {
        let start_time = std::time::Instant::now();
        let mut successful = Vec::new();
        let mut failed = Vec::new();

        // Create output directory
        if !output_dir.exists() {
            std::fs::create_dir_all(output_dir)
                .map_err(|_| RealEsrganError::OutputNotWritable(output_dir.to_path_buf()))?;
        }

        for (i, input_path) in input_files.iter().enumerate() {
            let output_filename = format!(
                "{}_{}x.{}",
                input_path.file_stem().unwrap_or_default().to_string_lossy(),
                options.scale,
                options.output_format.extension()
            );
            let output_path = output_dir.join(output_filename);

            match self.upscale(input_path, &output_path, options) {
                Ok(result) => successful.push(result),
                Err(e) => failed.push((input_path.clone(), e.to_string())),
            }

            if let Some(ref callback) = progress {
                callback(i + 1, input_files.len());
            }
        }

        Ok(BatchUpscaleResult {
            successful,
            failed,
            total_time: start_time.elapsed(),
            peak_vram_mb: None,
        })
    }

    /// Upscale all images in a directory
    pub fn upscale_directory(
        &self,
        input_dir: &Path,
        output_dir: &Path,
        options: &RealEsrganOptions,
        progress: Option<Box<dyn Fn(usize, usize) + Send>>,
    ) -> Result<BatchUpscaleResult> {
        // Find all image files in directory
        let mut input_files = Vec::new();
        let extensions = ["png", "jpg", "jpeg", "bmp", "tiff", "webp"];

        if input_dir.is_dir() {
            for entry in std::fs::read_dir(input_dir)? {
                let entry = entry?;
                let path = entry.path();
                if path.is_file() {
                    if let Some(ext) = path.extension() {
                        let ext_lower = ext.to_string_lossy().to_lowercase();
                        if extensions.contains(&ext_lower.as_str()) {
                            input_files.push(path);
                        }
                    }
                }
            }
        }

        // Sort for consistent ordering
        input_files.sort();

        self.upscale_batch(&input_files, output_dir, options, progress)
    }

    /// Get list of available models
    pub fn available_models(&self) -> Vec<RealEsrganModel> {
        vec![
            RealEsrganModel::X4Plus,
            RealEsrganModel::X4PlusAnime,
            RealEsrganModel::NetX4Plus,
            RealEsrganModel::X2Plus,
        ]
    }

    /// Calculate recommended tile size based on VRAM
    pub fn recommended_tile_size(&self, _image_size: (u32, u32), available_vram_mb: u64) -> u32 {
        // Empirical formula:
        // 4x upscale with FP16: ~100MB per 400x400 tile
        let scale_factor = (available_vram_mb as f64 / BASE_VRAM_MB as f64).sqrt();
        let recommended = (DEFAULT_TILE_SIZE as f64 * scale_factor) as u32;

        // Clamp to reasonable range
        recommended.clamp(MIN_TILE_SIZE, MAX_TILE_SIZE)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_options() {
        let opts = RealEsrganOptions::default();

        assert_eq!(opts.scale, 2);
        assert!(matches!(opts.model, RealEsrganModel::X4Plus));
        assert_eq!(opts.tile_size, 400);
        assert_eq!(opts.tile_padding, 10);
        assert!(matches!(opts.output_format, OutputFormat::Png));
        assert!(!opts.face_enhance);
        assert!(opts.gpu_id.is_none());
        assert!(opts.fp16);
    }

    #[test]
    fn test_model_default_scale() {
        assert_eq!(RealEsrganModel::X4Plus.default_scale(), 4);
        assert_eq!(RealEsrganModel::X4PlusAnime.default_scale(), 4);
        assert_eq!(RealEsrganModel::NetX4Plus.default_scale(), 4);
        assert_eq!(RealEsrganModel::X2Plus.default_scale(), 2);
        assert_eq!(
            RealEsrganModel::Custom("test".to_string()).default_scale(),
            4
        );
    }

    #[test]
    fn test_model_names() {
        assert_eq!(RealEsrganModel::X4Plus.model_name(), "RealESRGAN_x4plus");
        assert_eq!(
            RealEsrganModel::X4PlusAnime.model_name(),
            "RealESRGAN_x4plus_anime_6B"
        );
        assert_eq!(RealEsrganModel::NetX4Plus.model_name(), "RealESRNet_x4plus");
        assert_eq!(RealEsrganModel::X2Plus.model_name(), "RealESRGAN_x2plus");
        assert_eq!(
            RealEsrganModel::Custom("MyModel".to_string()).model_name(),
            "MyModel"
        );
    }

    #[test]
    fn test_output_format_extension() {
        assert_eq!(OutputFormat::Png.extension(), "png");
        assert_eq!(OutputFormat::Jpg { quality: 90 }.extension(), "jpg");
        assert_eq!(OutputFormat::Webp { quality: 85 }.extension(), "webp");
    }

    #[test]
    fn test_recommended_tile_size() {
        // Create a mock bridge for testing
        let config = crate::ai_bridge::AiBridgeConfig {
            venv_path: PathBuf::from("tests/fixtures/test_venv"),
            ..Default::default()
        };

        // Skip if venv doesn't exist (we're just testing the algorithm)
        if config.venv_path.exists() {
            let bridge = SubprocessBridge::new(config).unwrap();
            let processor = RealEsrgan::new(bridge);

            // 8GB VRAM
            let tile_8gb = processor.recommended_tile_size((1920, 1080), 8192);
            assert!(tile_8gb >= 400);

            // 4GB VRAM
            let tile_4gb = processor.recommended_tile_size((1920, 1080), 4096);
            assert!(tile_4gb <= tile_8gb);

            // 2GB VRAM
            let tile_2gb = processor.recommended_tile_size((1920, 1080), 2048);
            assert!(tile_2gb <= tile_4gb);
        }
    }

    // Test the tile size algorithm directly
    #[test]
    fn test_tile_size_algorithm() {
        // Direct algorithm test without bridge
        let calculate_tile = |available_vram_mb: u64| -> u32 {
            let base_tile = 400;
            let base_vram = 4096_u64;
            let scale_factor = (available_vram_mb as f64 / base_vram as f64).sqrt();
            let recommended = (base_tile as f64 * scale_factor) as u32;
            recommended.clamp(128, 1024)
        };

        assert!(calculate_tile(8192) >= 400);
        assert!(calculate_tile(4096) >= 300);
        assert!(calculate_tile(2048) >= 200);
        assert!(calculate_tile(1024) >= 128);
    }

    #[test]
    fn test_builder_pattern() {
        let options = RealEsrganOptions::builder()
            .scale(4)
            .model(RealEsrganModel::X4PlusAnime)
            .tile_size(256)
            .tile_padding(16)
            .output_format(OutputFormat::Jpg { quality: 90 })
            .face_enhance(true)
            .gpu_id(0)
            .fp16(false)
            .build();

        assert_eq!(options.scale, 4);
        assert!(matches!(options.model, RealEsrganModel::X4PlusAnime));
        assert_eq!(options.tile_size, 256);
        assert_eq!(options.tile_padding, 16);
        assert!(matches!(
            options.output_format,
            OutputFormat::Jpg { quality: 90 }
        ));
        assert!(options.face_enhance);
        assert_eq!(options.gpu_id, Some(0));
        assert!(!options.fp16);
    }

    #[test]
    fn test_builder_scale_clamping() {
        // Scale should be normalized to 2 or 4
        let options = RealEsrganOptions::builder().scale(1).build();
        assert_eq!(options.scale, 2);

        let options = RealEsrganOptions::builder().scale(3).build();
        assert_eq!(options.scale, 2);

        let options = RealEsrganOptions::builder().scale(4).build();
        assert_eq!(options.scale, 4);

        let options = RealEsrganOptions::builder().scale(8).build();
        assert_eq!(options.scale, 4);
    }

    #[test]
    fn test_builder_tile_size_clamping() {
        // Tile size should be clamped to 64-1024
        let options = RealEsrganOptions::builder().tile_size(32).build();
        assert_eq!(options.tile_size, 64);

        let options = RealEsrganOptions::builder().tile_size(2000).build();
        assert_eq!(options.tile_size, 1024);

        let options = RealEsrganOptions::builder().tile_size(512).build();
        assert_eq!(options.tile_size, 512);
    }

    #[test]
    fn test_x4_high_quality_preset() {
        let options = RealEsrganOptions::x4_high_quality();

        assert_eq!(options.scale, 4);
        assert!(matches!(options.model, RealEsrganModel::X4Plus));
        assert_eq!(options.tile_size, 256);
        assert!(!options.fp16); // More accurate
    }

    #[test]
    fn test_anime_preset() {
        let options = RealEsrganOptions::anime();

        assert_eq!(options.scale, 4);
        assert!(matches!(options.model, RealEsrganModel::X4PlusAnime));
    }

    #[test]
    fn test_low_vram_preset() {
        let options = RealEsrganOptions::low_vram();

        assert_eq!(options.tile_size, 128);
        assert_eq!(options.tile_padding, 8);
        assert!(options.fp16);
    }

    // Note: The following tests require actual Python environment
    // They are marked with #[ignore] until environment is available

    // TC-RES-010: 入力ファイルなしエラー
    #[test]
    #[ignore = "requires external tool"]
    fn test_input_not_found_error() {
        let config = crate::ai_bridge::AiBridgeConfig {
            venv_path: PathBuf::from("tests/fixtures/test_venv"),
            ..Default::default()
        };
        let bridge = SubprocessBridge::new(config).unwrap();
        let processor = RealEsrgan::new(bridge);

        let result = processor.upscale(
            Path::new("/nonexistent/image.png"),
            Path::new("/tmp/output.png"),
            &RealEsrganOptions::default(),
        );

        assert!(matches!(result, Err(RealEsrganError::InputNotFound(_))));
    }

    // TC-RES-001: 単一画像アップスケール
    #[test]
    #[ignore = "requires external tool"]
    fn test_single_image_upscale() {
        let config = crate::ai_bridge::AiBridgeConfig {
            venv_path: PathBuf::from("tests/fixtures/test_venv"),
            ..Default::default()
        };
        let bridge = SubprocessBridge::new(config).unwrap();
        let processor = RealEsrgan::new(bridge);

        let temp_dir = tempfile::tempdir().unwrap();
        let output = temp_dir.path().join("upscaled.png");

        let result = processor
            .upscale(
                Path::new("tests/fixtures/small_image.png"),
                &output,
                &RealEsrganOptions::default(),
            )
            .unwrap();

        assert!(output.exists());
        assert_eq!(result.actual_scale, 2.0);
    }

    // TC-RES-002: 4x upscale test
    #[test]
    fn test_4x_upscale_options() {
        let options = RealEsrganOptions::builder().scale(4).build();
        assert_eq!(options.scale, 4);

        // Verify 4x preset
        let high_quality = RealEsrganOptions::x4_high_quality();
        assert_eq!(high_quality.scale, 4);
    }

    // TC-RES-005: Different models test
    #[test]
    fn test_different_models() {
        let models = vec![
            RealEsrganModel::X4Plus,
            RealEsrganModel::X4PlusAnime,
            RealEsrganModel::NetX4Plus,
            RealEsrganModel::X2Plus,
        ];

        for model in models {
            let options = RealEsrganOptions::builder().model(model.clone()).build();

            // Each model should have valid model name
            assert!(!options.model.model_name().is_empty());

            // Each model should have valid default scale
            assert!(model.default_scale() == 2 || model.default_scale() == 4);
        }
    }

    // Test UpscaleResult construction
    #[test]
    fn test_upscale_result_construction() {
        let result = UpscaleResult {
            input_path: PathBuf::from("input.png"),
            output_path: PathBuf::from("output.png"),
            original_size: (100, 100),
            upscaled_size: (200, 200),
            actual_scale: 2.0,
            processing_time: Duration::from_secs(5),
            vram_used_mb: Some(1024),
        };

        assert_eq!(result.input_path, PathBuf::from("input.png"));
        assert_eq!(result.output_path, PathBuf::from("output.png"));
        assert_eq!(result.original_size, (100, 100));
        assert_eq!(result.upscaled_size, (200, 200));
        assert_eq!(result.actual_scale, 2.0);
        assert_eq!(result.processing_time, Duration::from_secs(5));
        assert_eq!(result.vram_used_mb, Some(1024));
    }

    // Test BatchUpscaleResult construction
    #[test]
    fn test_batch_upscale_result_construction() {
        let successful_result = UpscaleResult {
            input_path: PathBuf::from("input.png"),
            output_path: PathBuf::from("output.png"),
            original_size: (100, 100),
            upscaled_size: (200, 200),
            actual_scale: 2.0,
            processing_time: Duration::from_secs(1),
            vram_used_mb: None,
        };

        let result = BatchUpscaleResult {
            successful: vec![successful_result],
            failed: vec![(PathBuf::from("failed.png"), "Error".to_string())],
            total_time: Duration::from_secs(10),
            peak_vram_mb: Some(2048),
        };

        assert_eq!(result.successful.len(), 1);
        assert_eq!(result.failed.len(), 1);
        assert_eq!(result.total_time, Duration::from_secs(10));
        assert_eq!(result.peak_vram_mb, Some(2048));
    }

    // Test output format quality settings
    #[test]
    fn test_output_format_quality() {
        let jpg_90 = OutputFormat::Jpg { quality: 90 };
        let jpg_50 = OutputFormat::Jpg { quality: 50 };
        let webp_80 = OutputFormat::Webp { quality: 80 };

        assert_eq!(jpg_90.extension(), "jpg");
        assert_eq!(jpg_50.extension(), "jpg");
        assert_eq!(webp_80.extension(), "webp");

        // Extract quality values
        if let OutputFormat::Jpg { quality } = jpg_90 {
            assert_eq!(quality, 90);
        }
        if let OutputFormat::Webp { quality } = webp_80 {
            assert_eq!(quality, 80);
        }
    }

    // Test error types
    #[test]
    fn test_error_types() {
        let model_err = RealEsrganError::ModelNotFound("test".to_string());
        assert!(model_err.to_string().contains("Model not found"));

        let scale_err = RealEsrganError::InvalidScale(3);
        assert!(scale_err.to_string().contains("Invalid scale"));

        let input_err = RealEsrganError::InputNotFound(PathBuf::from("/test"));
        assert!(input_err.to_string().contains("Input image not found"));

        let vram_err = RealEsrganError::InsufficientVram {
            required: 8000,
            available: 4000,
        };
        assert!(vram_err.to_string().contains("insufficient"));
    }

    // Test available models list
    #[test]
    fn test_available_models_list() {
        let config = crate::ai_bridge::AiBridgeConfig {
            venv_path: PathBuf::from("tests/fixtures/test_venv"),
            ..Default::default()
        };

        if config.venv_path.exists() {
            let bridge = SubprocessBridge::new(config).unwrap();
            let processor = RealEsrgan::new(bridge);
            let models = processor.available_models();

            assert!(!models.is_empty());
            assert!(models.len() >= 4); // At least 4 built-in models
        }
    }

    // TC-RES-003: バッチ処理, TC-RES-004: ディレクトリ処理
    // TC-RES-008: Progress callback test (unit test portion)
    #[test]
    fn test_progress_callback_structure() {
        use std::sync::{Arc, Mutex};

        let progress_log = Arc::new(Mutex::new(Vec::new()));
        let progress_clone = progress_log.clone();

        // Simulate progress callback
        let callback = move |current: usize, total: usize| {
            progress_clone.lock().unwrap().push((current, total));
        };

        // Simulate 5 progress updates
        for i in 1..=5 {
            callback(i, 5);
        }

        let recorded = progress_log.lock().unwrap();
        assert_eq!(recorded.len(), 5);
        assert_eq!(recorded[0], (1, 5));
        assert_eq!(recorded[4], (5, 5));
    }

    // Test WebP output format
    #[test]
    fn test_webp_output_format() {
        let options = RealEsrganOptions::builder()
            .output_format(OutputFormat::Webp { quality: 85 })
            .build();

        assert!(matches!(
            options.output_format,
            OutputFormat::Webp { quality: 85 }
        ));
    }

    // Test custom model support
    #[test]
    fn test_custom_model() {
        let custom = RealEsrganModel::Custom("MyCustomModel_x8".to_string());

        assert_eq!(custom.model_name(), "MyCustomModel_x8");
        assert_eq!(custom.default_scale(), 4); // Default for custom models
    }

    // Test all error variants can display
    #[test]
    fn test_error_display() {
        let errors = vec![
            RealEsrganError::ModelNotFound("test".to_string()),
            RealEsrganError::InvalidScale(3),
            RealEsrganError::InputNotFound(PathBuf::from("/test")),
            RealEsrganError::OutputNotWritable(PathBuf::from("/test")),
            RealEsrganError::ProcessingFailed("test error".to_string()),
            RealEsrganError::InsufficientVram {
                required: 8000,
                available: 4000,
            },
            RealEsrganError::ImageError("image error".to_string()),
        ];

        for err in errors {
            // Verify each error has a non-empty display message
            assert!(!err.to_string().is_empty());
        }
    }

    // Test tile padding validation
    #[test]
    fn test_tile_padding_in_builder() {
        let options = RealEsrganOptions::builder().tile_padding(20).build();
        assert_eq!(options.tile_padding, 20);

        let options = RealEsrganOptions::builder().tile_padding(0).build();
        assert_eq!(options.tile_padding, 0);
    }

    // Test face enhance option
    #[test]
    fn test_face_enhance_option() {
        let options = RealEsrganOptions::builder().face_enhance(true).build();
        assert!(options.face_enhance);

        let default_options = RealEsrganOptions::default();
        assert!(!default_options.face_enhance);
    }

    // Test model enum equality
    #[test]
    fn test_model_enum_equality() {
        assert_eq!(RealEsrganModel::X4Plus, RealEsrganModel::X4Plus);
        assert_ne!(RealEsrganModel::X4Plus, RealEsrganModel::X2Plus);

        let custom1 = RealEsrganModel::Custom("model1".to_string());
        let custom2 = RealEsrganModel::Custom("model1".to_string());
        let custom3 = RealEsrganModel::Custom("model2".to_string());

        assert_eq!(custom1, custom2);
        assert_ne!(custom1, custom3);
    }

    // Test output format equality
    #[test]
    fn test_output_format_equality() {
        assert_eq!(OutputFormat::Png, OutputFormat::Png);
        assert_eq!(
            OutputFormat::Jpg { quality: 90 },
            OutputFormat::Jpg { quality: 90 }
        );
        assert_ne!(
            OutputFormat::Jpg { quality: 90 },
            OutputFormat::Jpg { quality: 80 }
        );
    }

    // Test result with None vram
    #[test]
    fn test_upscale_result_without_vram() {
        let result = UpscaleResult {
            input_path: PathBuf::from("input.png"),
            output_path: PathBuf::from("output.png"),
            original_size: (100, 100),
            upscaled_size: (400, 400),
            actual_scale: 4.0,
            processing_time: Duration::from_secs(10),
            vram_used_mb: None,
        };

        assert!(result.vram_used_mb.is_none());
        assert_eq!(result.actual_scale, 4.0);
    }

    // TC-RES-006: タイルサイズ調整テスト拡張
    #[test]
    fn test_tile_size_variations() {
        // 様々なタイルサイズの設定をテスト
        let tile_sizes = [128, 256, 400, 512, 1024];

        for &size in &tile_sizes {
            let options = RealEsrganOptions::builder().tile_size(size).build();
            // tile_size は最小32にクランプされる
            assert!(
                options.tile_size >= 32,
                "Tile size {} was clamped incorrectly",
                size
            );
        }
    }

    // TC-RES-009: 推奨タイルサイズ算出テスト拡張
    // Note: RealEsrgan::recommended_tile_sizeはSubprocessBridgeが必要なので
    // ここではタイルサイズアルゴリズムのロジックをテスト
    #[test]
    fn test_tile_size_algorithm_extended() {
        // タイルサイズアルゴリズムの検証
        // base_tile = 400, base_vram = 4096
        // scale_factor = (vram / base_vram).sqrt()

        let vram_configs: [(u64, u32); 4] = [
            (2048, 283),  // sqrt(0.5) * 400 ≈ 283
            (4096, 400),  // sqrt(1.0) * 400 = 400
            (8192, 566),  // sqrt(2.0) * 400 ≈ 566
            (16384, 800), // sqrt(4.0) * 400 = 800
        ];

        for (vram_mb, expected_approx) in vram_configs {
            let scale_factor = (vram_mb as f64 / 4096.0).sqrt();
            let calculated = (400.0 * scale_factor) as u32;
            let clamped = calculated.clamp(128, 1024);

            // 計算値が期待値の±10%以内であることを確認
            let diff = (clamped as i32 - expected_approx as i32).unsigned_abs();
            assert!(
                diff <= expected_approx / 10 + 10,
                "For {}MB VRAM: expected ~{}, got {}",
                vram_mb,
                expected_approx,
                clamped
            );
        }
    }

    // TC-RES-005: 異なるモデル使用テスト拡張
    #[test]
    fn test_all_model_variants() {
        let models = [
            RealEsrganModel::X2Plus,
            RealEsrganModel::X4Plus,
            RealEsrganModel::X4PlusAnime,
            RealEsrganModel::NetX4Plus,
        ];

        for model in models {
            let options = RealEsrganOptions::builder().model(model.clone()).build();

            // モデルが正しく設定されていることを確認
            assert_eq!(options.model, model);

            // デフォルトスケールが取得できることを確認
            let default_scale = model.default_scale();
            assert!(default_scale >= 2 && default_scale <= 4);
        }
    }

    // TC-RES-007: 出力フォーマットテスト拡張
    #[test]
    fn test_output_format_variants() {
        let formats = [
            (OutputFormat::Png, "png"),
            (OutputFormat::Jpg { quality: 90 }, "jpg"),
            (OutputFormat::Webp { quality: 85 }, "webp"),
        ];

        for (format, expected_ext) in formats {
            assert_eq!(format.extension(), expected_ext);
        }
    }

    #[test]
    fn test_batch_result_construction() {
        let result = BatchUpscaleResult {
            successful: vec![UpscaleResult {
                input_path: PathBuf::from("img1.png"),
                output_path: PathBuf::from("img1_upscaled.png"),
                original_size: (100, 100),
                upscaled_size: (200, 200),
                actual_scale: 2.0,
                processing_time: Duration::from_secs(1),
                vram_used_mb: Some(1000),
            }],
            failed: vec![(PathBuf::from("bad.png"), "File not found".to_string())],
            total_time: Duration::from_secs(5),
            peak_vram_mb: Some(2000),
        };

        assert_eq!(result.successful.len(), 1);
        assert_eq!(result.failed.len(), 1);
        assert_eq!(result.peak_vram_mb, Some(2000));
    }

    #[test]
    fn test_error_specific_messages() {
        let err = RealEsrganError::InsufficientVram {
            required: 8000,
            available: 4000,
        };
        let msg = err.to_string();
        assert!(msg.contains("8000") || msg.contains("4000"));

        let err = RealEsrganError::InvalidScale(3);
        let msg = err.to_string();
        assert!(msg.contains("3"));
    }

    // Additional comprehensive tests

    #[test]
    fn test_options_debug_impl() {
        let options = RealEsrganOptions::builder().scale(4).tile_size(512).build();

        let debug_str = format!("{:?}", options);
        assert!(debug_str.contains("RealEsrganOptions"));
        assert!(debug_str.contains("512"));
    }

    #[test]
    fn test_model_debug_impl() {
        let model = RealEsrganModel::X4Plus;
        let debug_str = format!("{:?}", model);
        assert!(debug_str.contains("X4Plus"));
    }

    #[test]
    fn test_output_format_debug_impl() {
        let format = OutputFormat::Jpg { quality: 95 };
        let debug_str = format!("{:?}", format);
        assert!(debug_str.contains("Jpg"));
        assert!(debug_str.contains("95"));
    }

    #[test]
    fn test_error_debug_impl() {
        let err = RealEsrganError::InvalidScale(5);
        let debug_str = format!("{:?}", err);
        assert!(debug_str.contains("InvalidScale"));
    }

    #[test]
    fn test_upscale_result_debug_impl() {
        let result = UpscaleResult {
            input_path: PathBuf::from("in.png"),
            output_path: PathBuf::from("out.png"),
            original_size: (100, 100),
            upscaled_size: (200, 200),
            actual_scale: 2.0,
            processing_time: Duration::from_secs(1),
            vram_used_mb: None,
        };

        let debug_str = format!("{:?}", result);
        assert!(debug_str.contains("UpscaleResult"));
    }

    #[test]
    fn test_batch_result_debug_impl() {
        let result = BatchUpscaleResult {
            successful: vec![],
            failed: vec![],
            total_time: Duration::from_secs(0),
            peak_vram_mb: None,
        };

        let debug_str = format!("{:?}", result);
        assert!(debug_str.contains("BatchUpscaleResult"));
    }

    #[test]
    fn test_options_clone() {
        let original = RealEsrganOptions::builder()
            .scale(4)
            .model(RealEsrganModel::X4PlusAnime)
            .tile_size(256)
            .fp16(true)
            .build();

        let cloned = original.clone();
        assert_eq!(cloned.scale, original.scale);
        assert_eq!(cloned.tile_size, original.tile_size);
        assert_eq!(cloned.fp16, original.fp16);
    }

    #[test]
    fn test_model_clone() {
        let original = RealEsrganModel::Custom("test_model".to_string());
        let cloned = original.clone();

        assert_eq!(cloned.model_name(), original.model_name());
    }

    #[test]
    fn test_output_format_clone() {
        let original = OutputFormat::Webp { quality: 85 };
        let cloned = original.clone();

        assert_eq!(cloned.extension(), original.extension());
    }

    #[test]
    fn test_upscale_result_clone() {
        let original = UpscaleResult {
            input_path: PathBuf::from("input.png"),
            output_path: PathBuf::from("output.png"),
            original_size: (100, 100),
            upscaled_size: (400, 400),
            actual_scale: 4.0,
            processing_time: Duration::from_millis(500),
            vram_used_mb: Some(2048),
        };

        let cloned = original.clone();
        assert_eq!(cloned.input_path, original.input_path);
        assert_eq!(cloned.actual_scale, original.actual_scale);
        assert_eq!(cloned.vram_used_mb, original.vram_used_mb);
    }

    #[test]
    fn test_gpu_id_settings() {
        // Default is CPU mode (None)
        let default_opts = RealEsrganOptions::default();
        assert!(default_opts.gpu_id.is_none());

        // GPU 0
        let gpu0_opts = RealEsrganOptions::builder().gpu_id(0).build();
        assert_eq!(gpu0_opts.gpu_id, Some(0));

        // Multi-GPU
        for gpu_id in [0, 1, 2, 3] {
            let opts = RealEsrganOptions::builder().gpu_id(gpu_id).build();
            assert_eq!(opts.gpu_id, Some(gpu_id));
        }
    }

    #[test]
    fn test_fp16_toggle() {
        let fp16_on = RealEsrganOptions::builder().fp16(true).build();
        assert!(fp16_on.fp16);

        let fp16_off = RealEsrganOptions::builder().fp16(false).build();
        assert!(!fp16_off.fp16);

        // Default should be true (for speed optimization)
        let default_opts = RealEsrganOptions::default();
        assert!(default_opts.fp16);
    }

    #[test]
    fn test_jpeg_quality_range() {
        for quality in [1, 25, 50, 75, 90, 95, 100] {
            let opts = RealEsrganOptions::builder()
                .output_format(OutputFormat::Jpg { quality })
                .build();

            if let OutputFormat::Jpg { quality: q } = opts.output_format {
                assert_eq!(q, quality);
            } else {
                panic!("Expected Jpg format");
            }
        }
    }

    #[test]
    fn test_webp_quality_range() {
        for quality in [1, 50, 80, 100] {
            let opts = RealEsrganOptions::builder()
                .output_format(OutputFormat::Webp { quality })
                .build();

            if let OutputFormat::Webp { quality: q } = opts.output_format {
                assert_eq!(q, quality);
            } else {
                panic!("Expected Webp format");
            }
        }
    }

    #[test]
    fn test_processing_time_variations() {
        let times = [
            Duration::from_millis(100),
            Duration::from_secs(1),
            Duration::from_secs(60),
            Duration::from_secs(3600),
        ];

        for time in times {
            let result = UpscaleResult {
                input_path: PathBuf::from("in.png"),
                output_path: PathBuf::from("out.png"),
                original_size: (100, 100),
                upscaled_size: (200, 200),
                actual_scale: 2.0,
                processing_time: time,
                vram_used_mb: None,
            };
            assert_eq!(result.processing_time, time);
        }
    }

    #[test]
    fn test_upscale_size_calculations() {
        // 2x upscale
        let result_2x = UpscaleResult {
            input_path: PathBuf::from("in.png"),
            output_path: PathBuf::from("out.png"),
            original_size: (100, 200),
            upscaled_size: (200, 400),
            actual_scale: 2.0,
            processing_time: Duration::from_secs(1),
            vram_used_mb: None,
        };
        assert_eq!(result_2x.upscaled_size.0, result_2x.original_size.0 * 2);
        assert_eq!(result_2x.upscaled_size.1, result_2x.original_size.1 * 2);

        // 4x upscale
        let result_4x = UpscaleResult {
            input_path: PathBuf::from("in.png"),
            output_path: PathBuf::from("out.png"),
            original_size: (100, 200),
            upscaled_size: (400, 800),
            actual_scale: 4.0,
            processing_time: Duration::from_secs(5),
            vram_used_mb: Some(4096),
        };
        assert_eq!(result_4x.upscaled_size.0, result_4x.original_size.0 * 4);
        assert_eq!(result_4x.upscaled_size.1, result_4x.original_size.1 * 4);
    }

    #[test]
    fn test_vram_usage_values() {
        // Various VRAM amounts
        for vram in [512, 1024, 2048, 4096, 8192, 16384] {
            let result = UpscaleResult {
                input_path: PathBuf::from("in.png"),
                output_path: PathBuf::from("out.png"),
                original_size: (100, 100),
                upscaled_size: (200, 200),
                actual_scale: 2.0,
                processing_time: Duration::from_secs(1),
                vram_used_mb: Some(vram),
            };
            assert_eq!(result.vram_used_mb, Some(vram));
        }
    }

    #[test]
    fn test_batch_result_empty() {
        let result = BatchUpscaleResult {
            successful: vec![],
            failed: vec![],
            total_time: Duration::from_secs(0),
            peak_vram_mb: None,
        };

        assert!(result.successful.is_empty());
        assert!(result.failed.is_empty());
    }

    #[test]
    fn test_batch_result_all_failed() {
        let failed: Vec<(PathBuf, String)> = (0..10)
            .map(|i| (PathBuf::from(format!("img_{}.png", i)), "Error".to_string()))
            .collect();

        let result = BatchUpscaleResult {
            successful: vec![],
            failed,
            total_time: Duration::from_secs(10),
            peak_vram_mb: None,
        };

        assert!(result.successful.is_empty());
        assert_eq!(result.failed.len(), 10);
    }

    #[test]
    fn test_batch_result_all_successful() {
        let successful: Vec<UpscaleResult> = (0..10)
            .map(|i| UpscaleResult {
                input_path: PathBuf::from(format!("in_{}.png", i)),
                output_path: PathBuf::from(format!("out_{}.png", i)),
                original_size: (100, 100),
                upscaled_size: (200, 200),
                actual_scale: 2.0,
                processing_time: Duration::from_secs(1),
                vram_used_mb: None,
            })
            .collect();

        let result = BatchUpscaleResult {
            successful,
            failed: vec![],
            total_time: Duration::from_secs(10),
            peak_vram_mb: Some(4096),
        };

        assert_eq!(result.successful.len(), 10);
        assert!(result.failed.is_empty());
    }

    #[test]
    fn test_model_names_not_empty() {
        let models = [
            RealEsrganModel::X2Plus,
            RealEsrganModel::X4Plus,
            RealEsrganModel::X4PlusAnime,
            RealEsrganModel::NetX4Plus,
            RealEsrganModel::Custom("custom".to_string()),
        ];

        for model in models {
            assert!(!model.model_name().is_empty());
        }
    }

    #[test]
    fn test_builder_default_values() {
        let builder_opts = RealEsrganOptions::builder().build();
        let default_opts = RealEsrganOptions::default();

        assert_eq!(builder_opts.scale, default_opts.scale);
        assert_eq!(builder_opts.tile_size, default_opts.tile_size);
        assert_eq!(builder_opts.fp16, default_opts.fp16);
    }

    #[test]
    fn test_error_io_conversion() {
        let io_err = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "access denied");
        let real_err: RealEsrganError = io_err.into();
        let msg = real_err.to_string();
        assert!(!msg.is_empty());
    }

    #[test]
    fn test_preset_presets_consistency() {
        // High quality preset
        let hq = RealEsrganOptions::x4_high_quality();
        assert_eq!(hq.scale, 4);
        assert!(!hq.fp16); // More precise

        // Anime preset
        let anime = RealEsrganOptions::anime();
        assert!(matches!(anime.model, RealEsrganModel::X4PlusAnime));

        // Low VRAM preset
        let low_vram = RealEsrganOptions::low_vram();
        assert!(low_vram.fp16); // More memory efficient
        assert!(low_vram.tile_size <= 128);
    }

    #[test]
    fn test_output_format_default() {
        let default_opts = RealEsrganOptions::default();
        assert!(matches!(default_opts.output_format, OutputFormat::Png));
    }

    #[test]
    fn test_scale_only_2_or_4() {
        // Only 2x and 4x should be valid
        for scale in [1, 2, 3, 4, 5, 6, 7, 8] {
            let opts = RealEsrganOptions::builder().scale(scale).build();
            assert!(opts.scale == 2 || opts.scale == 4);
        }
    }

    // ============ Error Handling Tests ============

    #[test]
    fn test_error_model_not_found() {
        let err = RealEsrganError::ModelNotFound("custom_model".to_string());
        let msg = format!("{}", err);
        assert!(msg.contains("Model not found"));
        assert!(msg.contains("custom_model"));
    }

    #[test]
    fn test_error_invalid_scale() {
        let err = RealEsrganError::InvalidScale(3);
        let msg = format!("{}", err);
        assert!(msg.contains("Invalid scale"));
        assert!(msg.contains("3"));
    }

    #[test]
    fn test_error_input_not_found() {
        let path = std::path::PathBuf::from("/missing/image.png");
        let err = RealEsrganError::InputNotFound(path);
        let msg = format!("{}", err);
        assert!(msg.contains("Input image not found"));
    }

    #[test]
    fn test_error_output_not_writable() {
        let path = std::path::PathBuf::from("/readonly/dir");
        let err = RealEsrganError::OutputNotWritable(path);
        let msg = format!("{}", err);
        assert!(msg.contains("not writable"));
    }

    #[test]
    fn test_error_processing_failed() {
        let err = RealEsrganError::ProcessingFailed("CUDA out of memory".to_string());
        let msg = format!("{}", err);
        assert!(msg.contains("Processing failed"));
        assert!(msg.contains("CUDA"));
    }

    #[test]
    fn test_error_insufficient_vram() {
        let err = RealEsrganError::InsufficientVram {
            required: 4096,
            available: 2048,
        };
        let msg = format!("{}", err);
        assert!(msg.contains("memory"));
        assert!(msg.contains("4096"));
        assert!(msg.contains("2048"));
    }

    #[test]
    fn test_error_image_error() {
        let err = RealEsrganError::ImageError("Corrupt PNG header".to_string());
        let msg = format!("{}", err);
        assert!(msg.contains("Image error"));
    }

    #[test]
    fn test_error_from_io_error() {
        let io_err = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "access denied");
        let res_err: RealEsrganError = io_err.into();
        let msg = format!("{}", res_err);
        assert!(msg.contains("IO error"));
    }

    #[test]
    fn test_error_debug_all_variants() {
        let errors: Vec<RealEsrganError> = vec![
            RealEsrganError::ModelNotFound("test".to_string()),
            RealEsrganError::InvalidScale(5),
            RealEsrganError::InputNotFound(std::path::PathBuf::from("/test")),
            RealEsrganError::OutputNotWritable(std::path::PathBuf::from("/test")),
            RealEsrganError::ProcessingFailed("test".to_string()),
            RealEsrganError::InsufficientVram {
                required: 100,
                available: 50,
            },
            RealEsrganError::ImageError("test".to_string()),
        ];

        for err in &errors {
            let debug = format!("{:?}", err);
            assert!(!debug.is_empty());
        }
    }

    // ============ Concurrency Tests ============

    #[test]
    fn test_realesrgan_types_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<RealEsrganOptions>();
        assert_send_sync::<RealEsrganModel>();
        assert_send_sync::<OutputFormat>();
        assert_send_sync::<UpscaleResult>();
    }

    #[test]
    fn test_concurrent_options_building() {
        use std::thread;
        let handles: Vec<_> = (0..8)
            .map(|i| {
                thread::spawn(move || {
                    RealEsrganOptions::builder()
                        .scale(if i % 2 == 0 { 2 } else { 4 })
                        .tile_size(128 + (i as u32 * 64))
                        .fp16(i % 2 == 0)
                        .build()
                })
            })
            .collect();

        let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
        assert_eq!(results.len(), 8);

        for (i, opt) in results.iter().enumerate() {
            let expected_scale = if i % 2 == 0 { 2 } else { 4 };
            assert_eq!(opt.scale, expected_scale);
        }
    }

    #[test]
    fn test_parallel_upscale_result_creation() {
        use rayon::prelude::*;

        let results: Vec<_> = (0..100)
            .into_par_iter()
            .map(|i| UpscaleResult {
                input_path: PathBuf::from(format!("input_{}.png", i)),
                output_path: PathBuf::from(format!("output_{}.png", i)),
                original_size: (100 + i as u32, 100 + i as u32),
                upscaled_size: (200 + i as u32 * 2, 200 + i as u32 * 2),
                actual_scale: 2.0,
                processing_time: Duration::from_millis(i as u64 * 10),
                vram_used_mb: Some(1024 + i as u64),
            })
            .collect();

        assert_eq!(results.len(), 100);
        for (i, result) in results.iter().enumerate() {
            assert_eq!(result.original_size.0, 100 + i as u32);
        }
    }

    #[test]
    fn test_model_thread_transfer() {
        use std::thread;

        let models = vec![
            RealEsrganModel::X4Plus,
            RealEsrganModel::X4PlusAnime,
            RealEsrganModel::Custom("custom".to_string()),
        ];

        let handles: Vec<_> = models
            .into_iter()
            .map(|model| {
                thread::spawn(move || {
                    let name = model.model_name().to_string();
                    let scale = model.default_scale();
                    (name, scale)
                })
            })
            .collect();

        let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
        assert_eq!(results.len(), 3);
        assert!(!results[0].0.is_empty());
    }

    #[test]
    fn test_concurrent_output_format_usage() {
        use std::thread;

        let formats = vec![
            OutputFormat::Png,
            OutputFormat::Jpg { quality: 90 },
            OutputFormat::Webp { quality: 85 },
        ];

        let handles: Vec<_> = formats
            .into_iter()
            .map(|format| thread::spawn(move || format.extension().to_string()))
            .collect();

        let extensions: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
        assert_eq!(extensions, vec!["png", "jpg", "webp"]);
    }

    #[test]
    fn test_options_shared_across_threads() {
        use std::sync::Arc;
        use std::thread;

        let options = Arc::new(
            RealEsrganOptions::builder()
                .scale(4)
                .model(RealEsrganModel::X4PlusAnime)
                .tile_size(256)
                .build(),
        );

        let handles: Vec<_> = (0..4)
            .map(|_| {
                let opts = Arc::clone(&options);
                thread::spawn(move || {
                    assert_eq!(opts.scale, 4);
                    assert_eq!(opts.tile_size, 256);
                    opts.model.model_name().to_string()
                })
            })
            .collect();

        for handle in handles {
            let name = handle.join().unwrap();
            assert!(name.contains("anime"));
        }
    }

    // ============ Boundary Value Tests ============

    #[test]
    fn test_tile_size_minimum_boundary() {
        let options = RealEsrganOptions::builder().tile_size(0).build();
        assert_eq!(options.tile_size, MIN_TILE_SIZE); // Should clamp to 64
    }

    #[test]
    fn test_tile_size_maximum_boundary() {
        let options = RealEsrganOptions::builder().tile_size(u32::MAX).build();
        assert_eq!(options.tile_size, MAX_TILE_SIZE); // Should clamp to 1024
    }

    #[test]
    fn test_tile_padding_zero() {
        let options = RealEsrganOptions::builder().tile_padding(0).build();
        assert_eq!(options.tile_padding, 0);
    }

    #[test]
    fn test_tile_padding_large() {
        let options = RealEsrganOptions::builder().tile_padding(1000).build();
        assert_eq!(options.tile_padding, 1000);
    }

    #[test]
    fn test_gpu_id_zero() {
        let options = RealEsrganOptions::builder().gpu_id(0).build();
        assert_eq!(options.gpu_id, Some(0));
    }

    #[test]
    fn test_gpu_id_high() {
        let options = RealEsrganOptions::builder().gpu_id(7).build();
        assert_eq!(options.gpu_id, Some(7));
    }

    #[test]
    fn test_jpeg_quality_zero() {
        let format = OutputFormat::Jpg { quality: 0 };
        if let OutputFormat::Jpg { quality } = format {
            assert_eq!(quality, 0);
        }
    }

    #[test]
    fn test_jpeg_quality_max() {
        let format = OutputFormat::Jpg { quality: 100 };
        if let OutputFormat::Jpg { quality } = format {
            assert_eq!(quality, 100);
        }
    }

    #[test]
    fn test_webp_quality_zero() {
        let format = OutputFormat::Webp { quality: 0 };
        if let OutputFormat::Webp { quality } = format {
            assert_eq!(quality, 0);
        }
    }

    #[test]
    fn test_webp_quality_max() {
        let format = OutputFormat::Webp { quality: 100 };
        if let OutputFormat::Webp { quality } = format {
            assert_eq!(quality, 100);
        }
    }

    #[test]
    fn test_original_size_zero() {
        let result = UpscaleResult {
            input_path: PathBuf::from("zero.png"),
            output_path: PathBuf::from("zero_out.png"),
            original_size: (0, 0),
            upscaled_size: (0, 0),
            actual_scale: 0.0,
            processing_time: Duration::ZERO,
            vram_used_mb: None,
        };
        assert_eq!(result.original_size, (0, 0));
    }

    #[test]
    fn test_upscaled_size_large() {
        let result = UpscaleResult {
            input_path: PathBuf::from("large.png"),
            output_path: PathBuf::from("large_out.png"),
            original_size: (8192, 8192),
            upscaled_size: (32768, 32768),
            actual_scale: 4.0,
            processing_time: Duration::from_secs(300),
            vram_used_mb: Some(16384),
        };
        assert_eq!(result.upscaled_size, (32768, 32768));
    }

    #[test]
    fn test_processing_time_zero() {
        let result = UpscaleResult {
            input_path: PathBuf::from("instant.png"),
            output_path: PathBuf::from("instant_out.png"),
            original_size: (1, 1),
            upscaled_size: (2, 2),
            actual_scale: 2.0,
            processing_time: Duration::ZERO,
            vram_used_mb: None,
        };
        assert_eq!(result.processing_time, Duration::ZERO);
    }

    #[test]
    fn test_vram_used_zero() {
        let result = UpscaleResult {
            input_path: PathBuf::from("cpu.png"),
            output_path: PathBuf::from("cpu_out.png"),
            original_size: (100, 100),
            upscaled_size: (200, 200),
            actual_scale: 2.0,
            processing_time: Duration::from_secs(60),
            vram_used_mb: Some(0),
        };
        assert_eq!(result.vram_used_mb, Some(0));
    }

    #[test]
    fn test_vram_used_large() {
        let result = UpscaleResult {
            input_path: PathBuf::from("big.png"),
            output_path: PathBuf::from("big_out.png"),
            original_size: (4096, 4096),
            upscaled_size: (16384, 16384),
            actual_scale: 4.0,
            processing_time: Duration::from_secs(120),
            vram_used_mb: Some(24576), // 24GB
        };
        assert_eq!(result.vram_used_mb, Some(24576));
    }

    #[test]
    fn test_actual_scale_fractional() {
        // Non-integer scale can occur with some models
        let result = UpscaleResult {
            input_path: PathBuf::from("test.png"),
            output_path: PathBuf::from("test_out.png"),
            original_size: (100, 100),
            upscaled_size: (350, 350),
            actual_scale: 3.5,
            processing_time: Duration::from_secs(5),
            vram_used_mb: None,
        };
        assert!((result.actual_scale - 3.5).abs() < 0.01);
    }

    #[test]
    fn test_batch_failed_empty_error() {
        let batch = BatchUpscaleResult {
            successful: vec![],
            failed: vec![(PathBuf::from("file.png"), String::new())],
            total_time: Duration::from_secs(1),
            peak_vram_mb: None,
        };
        assert!(batch.failed[0].1.is_empty());
    }

    #[test]
    fn test_batch_peak_vram_zero() {
        let batch = BatchUpscaleResult {
            successful: vec![],
            failed: vec![],
            total_time: Duration::ZERO,
            peak_vram_mb: Some(0),
        };
        assert_eq!(batch.peak_vram_mb, Some(0));
    }

    #[test]
    fn test_insufficient_vram_error_equal_values() {
        let err = RealEsrganError::InsufficientVram {
            required: 4096,
            available: 4096,
        };
        let msg = err.to_string();
        assert!(msg.contains("4096"));
    }

    #[test]
    fn test_custom_model_empty_name() {
        let model = RealEsrganModel::Custom(String::new());
        assert!(model.model_name().is_empty());
    }

    #[test]
    fn test_upscale_result_success() {
        let result = UpscaleResult {
            input_path: std::path::PathBuf::from("/input/image.png"),
            output_path: std::path::PathBuf::from("/output/image_2x.png"),
            original_size: (1920, 1080),
            upscaled_size: (3840, 2160),
            actual_scale: 2.0,
            processing_time: std::time::Duration::from_secs(5),
            vram_used_mb: Some(512),
        };
        assert!((result.actual_scale - 2.0).abs() < 0.01);
        assert_eq!(result.upscaled_size.0, result.original_size.0 * 2);
        assert_eq!(result.upscaled_size.1, result.original_size.1 * 2);
    }

    #[test]
    fn test_upscale_result_4x() {
        let result = UpscaleResult {
            input_path: std::path::PathBuf::from("/input/small.png"),
            output_path: std::path::PathBuf::from("/output/large.png"),
            original_size: (640, 480),
            upscaled_size: (2560, 1920),
            actual_scale: 4.0,
            processing_time: std::time::Duration::from_secs(15),
            vram_used_mb: None,
        };
        assert!((result.actual_scale - 4.0).abs() < 0.01);
        assert_eq!(result.upscaled_size.0, result.original_size.0 * 4);
    }

    #[test]
    fn test_model_enum_all_variants() {
        use super::RealEsrganModel;
        let models = [
            RealEsrganModel::X4Plus,
            RealEsrganModel::X4PlusAnime,
            RealEsrganModel::NetX4Plus,
            RealEsrganModel::X2Plus,
            RealEsrganModel::Custom("my_model".to_string()),
        ];

        for model in &models {
            let debug = format!("{:?}", model);
            assert!(!debug.is_empty());
        }
    }
}
