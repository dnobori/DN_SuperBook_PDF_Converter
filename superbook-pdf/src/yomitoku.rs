//! YomiToku Japanese AI-OCR module
//!
//! Provides integration with YomiToku for Japanese text recognition in images.
//!
//! # Features
//!
//! - High-accuracy Japanese OCR
//! - Vertical/horizontal text detection
//! - Searchable PDF layer generation
//! - Batch processing support
//!
//! # Example
//!
//! ```rust,no_run
//! use superbook_pdf::{YomiToku, YomiTokuOptions};
//! use superbook_pdf::yomitoku::Language;
//!
//! // Configure OCR
//! let options = YomiTokuOptions::builder()
//!     .language(Language::Japanese)
//!     .confidence_threshold(0.5)
//!     .build();
//!
//! // Perform OCR
//! // let result = YomiToku::new().recognize("page.png", &options);
//! ```

use std::path::{Path, PathBuf};
use std::time::Duration;
use thiserror::Error;

use crate::ai_bridge::{AiBridgeError, AiTool, SubprocessBridge};

// ============================================================
// Constants
// ============================================================

/// Default confidence threshold for OCR results
const DEFAULT_CONFIDENCE_THRESHOLD: f32 = 0.5;

/// Lower confidence threshold for book scanning (captures more text)
const BOOK_CONFIDENCE_THRESHOLD: f32 = 0.3;

/// Default timeout for OCR processing (5 minutes)
const DEFAULT_TIMEOUT_SECS: u64 = 300;

/// Minimum confidence threshold
const MIN_CONFIDENCE: f32 = 0.0;

/// Maximum confidence threshold
const MAX_CONFIDENCE: f32 = 1.0;

/// YomiToku error types
#[derive(Debug, Error)]
pub enum YomiTokuError {
    #[error("Input file not found: {0}")]
    InputNotFound(PathBuf),

    #[error("Output directory not writable: {0}")]
    OutputNotWritable(PathBuf),

    #[error("YomiToku execution failed: {0}")]
    ExecutionFailed(String),

    #[error("YomiToku not installed or not found")]
    NotInstalled,

    #[error("Invalid output format")]
    InvalidOutput,

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("JSON parse error: {0}")]
    JsonError(#[from] serde_json::Error),

    #[error("AI Bridge error: {0}")]
    BridgeError(#[from] AiBridgeError),
}

pub type Result<T> = std::result::Result<T, YomiTokuError>;

/// YomiToku OCR options
#[derive(Debug, Clone)]
pub struct YomiTokuOptions {
    /// Output format
    pub output_format: OutputFormat,
    /// Enable GPU acceleration
    pub use_gpu: bool,
    /// GPU device ID
    pub gpu_id: Option<u32>,
    /// Confidence threshold (0.0-1.0)
    pub confidence_threshold: f32,
    /// Timeout in seconds
    pub timeout_secs: u64,
    /// Enable vertical text detection
    pub detect_vertical: bool,
    /// Language hint
    pub language: Language,
}

impl Default for YomiTokuOptions {
    fn default() -> Self {
        Self {
            output_format: OutputFormat::Json,
            use_gpu: true,
            gpu_id: None,
            confidence_threshold: DEFAULT_CONFIDENCE_THRESHOLD,
            timeout_secs: DEFAULT_TIMEOUT_SECS,
            detect_vertical: true,
            language: Language::Japanese,
        }
    }
}

impl YomiTokuOptions {
    /// Create a new options builder
    pub fn builder() -> YomiTokuOptionsBuilder {
        YomiTokuOptionsBuilder::default()
    }

    /// Create options optimized for book scanning
    pub fn for_books() -> Self {
        Self {
            detect_vertical: true,
            confidence_threshold: BOOK_CONFIDENCE_THRESHOLD,
            ..Default::default()
        }
    }

    /// Create options for horizontal text only
    pub fn horizontal_only() -> Self {
        Self {
            detect_vertical: false,
            ..Default::default()
        }
    }
}

/// Builder for YomiTokuOptions
#[derive(Debug, Default)]
pub struct YomiTokuOptionsBuilder {
    options: YomiTokuOptions,
}

impl YomiTokuOptionsBuilder {
    /// Set output format
    pub fn output_format(mut self, format: OutputFormat) -> Self {
        self.options.output_format = format;
        self
    }

    /// Enable/disable GPU
    pub fn use_gpu(mut self, use_gpu: bool) -> Self {
        self.options.use_gpu = use_gpu;
        self
    }

    /// Set GPU device ID
    pub fn gpu_id(mut self, id: u32) -> Self {
        self.options.gpu_id = Some(id);
        self
    }

    /// Set confidence threshold
    pub fn confidence_threshold(mut self, threshold: f32) -> Self {
        self.options.confidence_threshold = threshold.clamp(MIN_CONFIDENCE, MAX_CONFIDENCE);
        self
    }

    /// Set timeout in seconds
    pub fn timeout(mut self, secs: u64) -> Self {
        self.options.timeout_secs = secs;
        self
    }

    /// Enable/disable vertical text detection
    pub fn detect_vertical(mut self, detect: bool) -> Self {
        self.options.detect_vertical = detect;
        self
    }

    /// Set language hint
    pub fn language(mut self, lang: Language) -> Self {
        self.options.language = lang;
        self
    }

    /// Build the options
    pub fn build(self) -> YomiTokuOptions {
        self.options
    }
}

/// Output format for OCR results
#[derive(Debug, Clone, Copy, Default)]
pub enum OutputFormat {
    #[default]
    Json,
    Text,
    Hocr,
    Pdf,
}

impl OutputFormat {
    /// Get file extension for format
    pub fn extension(&self) -> &str {
        match self {
            OutputFormat::Json => "json",
            OutputFormat::Text => "txt",
            OutputFormat::Hocr => "hocr",
            OutputFormat::Pdf => "pdf",
        }
    }
}

/// Language hint for OCR
#[derive(Debug, Clone, Copy, Default)]
pub enum Language {
    #[default]
    Japanese,
    English,
    ChineseSimplified,
    ChineseTraditional,
    Korean,
    Mixed,
}

impl Language {
    /// Get language code
    pub fn code(&self) -> &str {
        match self {
            Language::Japanese => "ja",
            Language::English => "en",
            Language::ChineseSimplified => "zh-CN",
            Language::ChineseTraditional => "zh-TW",
            Language::Korean => "ko",
            Language::Mixed => "mixed",
        }
    }
}

/// OCR result for a single page
#[derive(Debug, Clone)]
pub struct OcrResult {
    /// Input image path
    pub input_path: PathBuf,
    /// Recognized text blocks
    pub text_blocks: Vec<TextBlock>,
    /// Overall confidence score
    pub confidence: f32,
    /// Processing time
    pub processing_time: Duration,
    /// Detected text direction
    pub text_direction: TextDirection,
}

/// A recognized text block
#[derive(Debug, Clone)]
pub struct TextBlock {
    /// Recognized text content
    pub text: String,
    /// Bounding box (x, y, width, height)
    pub bbox: (u32, u32, u32, u32),
    /// Confidence score (0.0-1.0)
    pub confidence: f32,
    /// Text direction
    pub direction: TextDirection,
    /// Font size estimate (points)
    pub font_size: Option<f32>,
}

/// Text direction
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TextDirection {
    #[default]
    Horizontal,
    Vertical,
    Mixed,
}

/// Batch OCR result
#[derive(Debug)]
pub struct BatchOcrResult {
    /// Successful results
    pub successful: Vec<OcrResult>,
    /// Failed files
    pub failed: Vec<(PathBuf, String)>,
    /// Total processing time
    pub total_time: Duration,
}

/// YomiToku OCR processor
pub struct YomiToku {
    bridge: SubprocessBridge,
}

impl YomiToku {
    /// Create a new YomiToku processor
    pub fn new(bridge: SubprocessBridge) -> Self {
        Self { bridge }
    }

    /// Check if YomiToku is available
    pub fn is_available(&self) -> bool {
        self.bridge.check_tool(AiTool::YomiToku).unwrap_or(false)
    }

    /// Perform OCR on a single image
    pub fn ocr(&self, input_path: &Path, options: &YomiTokuOptions) -> Result<OcrResult> {
        let start_time = std::time::Instant::now();

        if !input_path.exists() {
            return Err(YomiTokuError::InputNotFound(input_path.to_path_buf()));
        }

        // Build command arguments
        let mut args = vec![
            "-m".to_string(),
            "yomitoku".to_string(),
            input_path.to_string_lossy().to_string(),
            "--output-format".to_string(),
            "json".to_string(),
        ];

        if options.use_gpu {
            args.push("--gpu".to_string());
            if let Some(gpu_id) = options.gpu_id {
                args.push("--gpu-id".to_string());
                args.push(gpu_id.to_string());
            }
        } else {
            args.push("--cpu".to_string());
        }

        if options.detect_vertical {
            args.push("--detect-vertical".to_string());
        }

        args.push("--confidence".to_string());
        args.push(options.confidence_threshold.to_string());

        args.push("--lang".to_string());
        args.push(options.language.code().to_string());

        // Execute YomiToku
        let output = self
            .bridge
            .execute_with_timeout(&args, Duration::from_secs(options.timeout_secs))?;

        // Parse JSON output
        let json_result: serde_json::Value = serde_json::from_str(&output).map_err(|e| {
            YomiTokuError::ExecutionFailed(format!("Failed to parse output: {}", e))
        })?;

        // Extract text blocks
        let text_blocks = self.parse_text_blocks(&json_result)?;
        let overall_confidence = self.calculate_overall_confidence(&text_blocks);
        let text_direction = self.detect_dominant_direction(&text_blocks);

        Ok(OcrResult {
            input_path: input_path.to_path_buf(),
            text_blocks,
            confidence: overall_confidence,
            processing_time: start_time.elapsed(),
            text_direction,
        })
    }

    /// Perform OCR on multiple images
    pub fn ocr_batch(
        &self,
        input_files: &[PathBuf],
        options: &YomiTokuOptions,
        progress: Option<Box<dyn Fn(usize, usize) + Send>>,
    ) -> Result<BatchOcrResult> {
        let start_time = std::time::Instant::now();
        let mut successful = Vec::new();
        let mut failed = Vec::new();

        for (i, input_path) in input_files.iter().enumerate() {
            if let Some(ref progress_fn) = progress {
                progress_fn(i + 1, input_files.len());
            }

            match self.ocr(input_path, options) {
                Ok(result) => successful.push(result),
                Err(e) => failed.push((input_path.clone(), e.to_string())),
            }
        }

        Ok(BatchOcrResult {
            successful,
            failed,
            total_time: start_time.elapsed(),
        })
    }

    /// Extract full text from OCR result
    pub fn extract_text(result: &OcrResult) -> String {
        result
            .text_blocks
            .iter()
            .map(|block| block.text.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Parse text blocks from JSON output
    fn parse_text_blocks(&self, json: &serde_json::Value) -> Result<Vec<TextBlock>> {
        let blocks = json
            .get("blocks")
            .and_then(|b| b.as_array())
            .ok_or(YomiTokuError::InvalidOutput)?;

        let mut text_blocks = Vec::new();

        for block in blocks {
            let text = block
                .get("text")
                .and_then(|t| t.as_str())
                .unwrap_or("")
                .to_string();

            let bbox = (
                block.get("x").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
                block.get("y").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
                block.get("width").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
                block.get("height").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
            );

            let confidence = block
                .get("confidence")
                .and_then(|c| c.as_f64())
                .unwrap_or(0.0) as f32;

            let direction = match block.get("direction").and_then(|d| d.as_str()) {
                Some("vertical") => TextDirection::Vertical,
                Some("horizontal") => TextDirection::Horizontal,
                _ => TextDirection::Horizontal,
            };

            let font_size = block
                .get("font_size")
                .and_then(|f| f.as_f64())
                .map(|f| f as f32);

            text_blocks.push(TextBlock {
                text,
                bbox,
                confidence,
                direction,
                font_size,
            });
        }

        Ok(text_blocks)
    }

    /// Calculate overall confidence from text blocks
    fn calculate_overall_confidence(&self, blocks: &[TextBlock]) -> f32 {
        if blocks.is_empty() {
            return 0.0;
        }

        let total: f32 = blocks.iter().map(|b| b.confidence).sum();
        total / blocks.len() as f32
    }

    /// Detect dominant text direction
    fn detect_dominant_direction(&self, blocks: &[TextBlock]) -> TextDirection {
        if blocks.is_empty() {
            return TextDirection::Horizontal;
        }

        let vertical_count = blocks
            .iter()
            .filter(|b| b.direction == TextDirection::Vertical)
            .count();

        let horizontal_count = blocks.len() - vertical_count;

        if vertical_count > horizontal_count * 2 {
            TextDirection::Vertical
        } else if horizontal_count > vertical_count * 2 {
            TextDirection::Horizontal
        } else {
            TextDirection::Mixed
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_options() {
        let opts = YomiTokuOptions::default();

        assert!(opts.use_gpu);
        assert!(opts.detect_vertical);
        assert_eq!(opts.confidence_threshold, 0.5);
        assert!(matches!(opts.output_format, OutputFormat::Json));
        assert!(matches!(opts.language, Language::Japanese));
    }

    #[test]
    fn test_builder_pattern() {
        let opts = YomiTokuOptions::builder()
            .use_gpu(false)
            .confidence_threshold(0.8)
            .detect_vertical(false)
            .language(Language::English)
            .timeout(600)
            .build();

        assert!(!opts.use_gpu);
        assert_eq!(opts.confidence_threshold, 0.8);
        assert!(!opts.detect_vertical);
        assert!(matches!(opts.language, Language::English));
        assert_eq!(opts.timeout_secs, 600);
    }

    #[test]
    fn test_confidence_clamping() {
        let opts = YomiTokuOptions::builder().confidence_threshold(1.5).build();
        assert_eq!(opts.confidence_threshold, 1.0);

        let opts = YomiTokuOptions::builder()
            .confidence_threshold(-0.5)
            .build();
        assert_eq!(opts.confidence_threshold, 0.0);
    }

    #[test]
    fn test_output_format_extension() {
        assert_eq!(OutputFormat::Json.extension(), "json");
        assert_eq!(OutputFormat::Text.extension(), "txt");
        assert_eq!(OutputFormat::Hocr.extension(), "hocr");
        assert_eq!(OutputFormat::Pdf.extension(), "pdf");
    }

    #[test]
    fn test_language_code() {
        assert_eq!(Language::Japanese.code(), "ja");
        assert_eq!(Language::English.code(), "en");
        assert_eq!(Language::ChineseSimplified.code(), "zh-CN");
        assert_eq!(Language::Korean.code(), "ko");
    }

    #[test]
    fn test_for_books_preset() {
        let opts = YomiTokuOptions::for_books();

        assert!(opts.detect_vertical);
        assert_eq!(opts.confidence_threshold, 0.3);
    }

    #[test]
    fn test_horizontal_only_preset() {
        let opts = YomiTokuOptions::horizontal_only();

        assert!(!opts.detect_vertical);
    }

    #[test]
    fn test_text_block_construction() {
        let block = TextBlock {
            text: "テスト".to_string(),
            bbox: (10, 20, 100, 50),
            confidence: 0.95,
            direction: TextDirection::Horizontal,
            font_size: Some(12.0),
        };

        assert_eq!(block.text, "テスト");
        assert_eq!(block.bbox, (10, 20, 100, 50));
        assert_eq!(block.confidence, 0.95);
        assert!(matches!(block.direction, TextDirection::Horizontal));
        assert_eq!(block.font_size, Some(12.0));
    }

    #[test]
    fn test_ocr_result_construction() {
        let result = OcrResult {
            input_path: PathBuf::from("/test/page.png"),
            text_blocks: vec![],
            confidence: 0.85,
            processing_time: Duration::from_secs(5),
            text_direction: TextDirection::Vertical,
        };

        assert_eq!(result.input_path, PathBuf::from("/test/page.png"));
        assert_eq!(result.confidence, 0.85);
        assert!(matches!(result.text_direction, TextDirection::Vertical));
    }

    #[test]
    fn test_text_direction_variants() {
        assert!(matches!(
            TextDirection::Horizontal,
            TextDirection::Horizontal
        ));
        assert!(matches!(TextDirection::Vertical, TextDirection::Vertical));
        assert!(matches!(TextDirection::Mixed, TextDirection::Mixed));
    }

    #[test]
    fn test_error_types() {
        let _err1 = YomiTokuError::InputNotFound(PathBuf::from("/test/path"));
        let _err2 = YomiTokuError::OutputNotWritable(PathBuf::from("/test/path"));
        let _err3 = YomiTokuError::ExecutionFailed("test".to_string());
        let _err4 = YomiTokuError::NotInstalled;
        let _err5 = YomiTokuError::InvalidOutput;
    }

    #[test]
    fn test_extract_text() {
        let result = OcrResult {
            input_path: PathBuf::from("/test.png"),
            text_blocks: vec![
                TextBlock {
                    text: "行1".to_string(),
                    bbox: (0, 0, 100, 20),
                    confidence: 0.9,
                    direction: TextDirection::Horizontal,
                    font_size: None,
                },
                TextBlock {
                    text: "行2".to_string(),
                    bbox: (0, 20, 100, 20),
                    confidence: 0.85,
                    direction: TextDirection::Horizontal,
                    font_size: None,
                },
            ],
            confidence: 0.875,
            processing_time: Duration::from_millis(100),
            text_direction: TextDirection::Horizontal,
        };

        let text = YomiToku::extract_text(&result);
        assert_eq!(text, "行1\n行2");
    }

    #[test]
    fn test_gpu_id_setting() {
        let opts = YomiTokuOptions::builder().gpu_id(1).build();

        assert_eq!(opts.gpu_id, Some(1));
    }

    #[test]
    fn test_batch_result_construction() {
        let batch = BatchOcrResult {
            successful: vec![],
            failed: vec![],
            total_time: Duration::from_secs(10),
        };

        assert!(batch.successful.is_empty());
        assert!(batch.failed.is_empty());
        assert_eq!(batch.total_time, Duration::from_secs(10));
    }

    // Test extract_text with empty blocks
    #[test]
    fn test_extract_text_empty() {
        let result = OcrResult {
            input_path: PathBuf::from("/test.png"),
            text_blocks: vec![],
            confidence: 0.0,
            processing_time: Duration::from_millis(10),
            text_direction: TextDirection::Horizontal,
        };

        let text = YomiToku::extract_text(&result);
        assert!(text.is_empty());
    }

    // Test extract_text with single block
    #[test]
    fn test_extract_text_single_block() {
        let result = OcrResult {
            input_path: PathBuf::from("/test.png"),
            text_blocks: vec![TextBlock {
                text: "単一ブロック".to_string(),
                bbox: (0, 0, 100, 20),
                confidence: 0.9,
                direction: TextDirection::Horizontal,
                font_size: None,
            }],
            confidence: 0.9,
            processing_time: Duration::from_millis(50),
            text_direction: TextDirection::Horizontal,
        };

        let text = YomiToku::extract_text(&result);
        assert_eq!(text, "単一ブロック");
    }

    // Test all language codes
    #[test]
    fn test_all_language_codes() {
        assert_eq!(Language::Japanese.code(), "ja");
        assert_eq!(Language::English.code(), "en");
        assert_eq!(Language::ChineseSimplified.code(), "zh-CN");
        assert_eq!(Language::ChineseTraditional.code(), "zh-TW");
        assert_eq!(Language::Korean.code(), "ko");
    }

    // Test text direction equality
    #[test]
    fn test_text_direction_equality() {
        assert_eq!(TextDirection::Horizontal, TextDirection::Horizontal);
        assert_eq!(TextDirection::Vertical, TextDirection::Vertical);
        assert_eq!(TextDirection::Mixed, TextDirection::Mixed);
        assert_ne!(TextDirection::Horizontal, TextDirection::Vertical);
    }

    // Test output format conversion
    #[test]
    fn test_output_format_all() {
        let formats = vec![
            (OutputFormat::Json, "json"),
            (OutputFormat::Text, "txt"),
            (OutputFormat::Hocr, "hocr"),
            (OutputFormat::Pdf, "pdf"),
        ];

        for (format, expected_ext) in formats {
            assert_eq!(format.extension(), expected_ext);
        }
    }

    // Test builder with all options
    #[test]
    fn test_builder_all_options() {
        let opts = YomiTokuOptions::builder()
            .use_gpu(true)
            .confidence_threshold(0.7)
            .detect_vertical(true)
            .output_format(OutputFormat::Hocr)
            .language(Language::ChineseSimplified)
            .timeout(1800)
            .gpu_id(2)
            .build();

        assert!(opts.use_gpu);
        assert_eq!(opts.confidence_threshold, 0.7);
        assert!(opts.detect_vertical);
        assert!(matches!(opts.output_format, OutputFormat::Hocr));
        assert!(matches!(opts.language, Language::ChineseSimplified));
        assert_eq!(opts.timeout_secs, 1800);
        assert_eq!(opts.gpu_id, Some(2));
    }

    // Test OcrResult with multiple blocks
    #[test]
    fn test_ocr_result_multiple_blocks() {
        let blocks = vec![
            TextBlock {
                text: "第一章".to_string(),
                bbox: (100, 50, 200, 30),
                confidence: 0.95,
                direction: TextDirection::Horizontal,
                font_size: Some(24.0),
            },
            TextBlock {
                text: "本文内容".to_string(),
                bbox: (50, 100, 300, 400),
                confidence: 0.88,
                direction: TextDirection::Vertical,
                font_size: Some(12.0),
            },
        ];

        let result = OcrResult {
            input_path: PathBuf::from("/page1.png"),
            text_blocks: blocks,
            confidence: 0.915,
            processing_time: Duration::from_secs(2),
            text_direction: TextDirection::Mixed,
        };

        assert_eq!(result.text_blocks.len(), 2);
        assert!(matches!(result.text_direction, TextDirection::Mixed));
    }

    // Test batch result with failures
    #[test]
    fn test_batch_result_with_failures() {
        let successful = vec![OcrResult {
            input_path: PathBuf::from("/page1.png"),
            text_blocks: vec![],
            confidence: 0.8,
            processing_time: Duration::from_secs(1),
            text_direction: TextDirection::Horizontal,
        }];

        let failed = vec![
            (PathBuf::from("/page2.png"), "File not found".to_string()),
            (PathBuf::from("/page3.png"), "Invalid image".to_string()),
        ];

        let batch = BatchOcrResult {
            successful,
            failed,
            total_time: Duration::from_secs(3),
        };

        assert_eq!(batch.successful.len(), 1);
        assert_eq!(batch.failed.len(), 2);
    }

    // Test error display messages
    #[test]
    fn test_error_display_messages() {
        let errors: Vec<(YomiTokuError, &str)> = vec![
            (
                YomiTokuError::InputNotFound(PathBuf::from("/test")),
                "not found",
            ),
            (
                YomiTokuError::OutputNotWritable(PathBuf::from("/test")),
                "not writable",
            ),
            (
                YomiTokuError::ExecutionFailed("test".to_string()),
                "Execution failed",
            ),
            (YomiTokuError::NotInstalled, "not installed"),
            (YomiTokuError::InvalidOutput, "Invalid output"),
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

    // Test TextBlock without font_size
    #[test]
    fn test_text_block_no_font_size() {
        let block = TextBlock {
            text: "テスト".to_string(),
            bbox: (0, 0, 50, 20),
            confidence: 0.9,
            direction: TextDirection::Horizontal,
            font_size: None,
        };

        assert!(block.font_size.is_none());
    }

    // Test TextBlock bbox boundary values
    #[test]
    fn test_text_block_bbox_boundaries() {
        // Zero-size bbox
        let zero_bbox = TextBlock {
            text: "".to_string(),
            bbox: (0, 0, 0, 0),
            confidence: 0.0,
            direction: TextDirection::Horizontal,
            font_size: None,
        };
        assert_eq!(zero_bbox.bbox.2, 0); // width
        assert_eq!(zero_bbox.bbox.3, 0); // height

        // Large bbox
        let large_bbox = TextBlock {
            text: "Large".to_string(),
            bbox: (0, 0, 10000, 15000),
            confidence: 1.0,
            direction: TextDirection::Vertical,
            font_size: Some(72.0),
        };
        assert_eq!(large_bbox.bbox.2, 10000);
        assert_eq!(large_bbox.bbox.3, 15000);
    }

    // Test confidence threshold edge cases
    #[test]
    fn test_confidence_threshold_edges() {
        // Minimum confidence
        let opts_min = YomiTokuOptions::builder().confidence_threshold(0.0).build();
        assert_eq!(opts_min.confidence_threshold, 0.0);

        // Maximum confidence
        let opts_max = YomiTokuOptions::builder().confidence_threshold(1.0).build();
        assert_eq!(opts_max.confidence_threshold, 1.0);

        // Default confidence
        let opts_default = YomiTokuOptions::default();
        assert!(opts_default.confidence_threshold > 0.0);
        assert!(opts_default.confidence_threshold <= 1.0);
    }

    // Test batch result with empty arrays
    #[test]
    fn test_batch_result_empty() {
        let batch = BatchOcrResult {
            successful: vec![],
            failed: vec![],
            total_time: Duration::ZERO,
        };

        assert!(batch.successful.is_empty());
        assert!(batch.failed.is_empty());
        assert_eq!(batch.total_time, Duration::ZERO);
    }

    // Test batch result all successful
    #[test]
    fn test_batch_result_all_successful() {
        let results: Vec<OcrResult> = (0..5)
            .map(|i| OcrResult {
                input_path: PathBuf::from(format!("/page{}.png", i)),
                text_blocks: vec![TextBlock {
                    text: format!("Page {}", i),
                    bbox: (0, 0, 100, 50),
                    confidence: 0.9,
                    direction: TextDirection::Horizontal,
                    font_size: Some(12.0),
                }],
                confidence: 0.9,
                processing_time: Duration::from_millis(100),
                text_direction: TextDirection::Horizontal,
            })
            .collect();

        let batch = BatchOcrResult {
            successful: results,
            failed: vec![],
            total_time: Duration::from_millis(500),
        };

        assert_eq!(batch.successful.len(), 5);
        assert!(batch.failed.is_empty());
    }

    // Test Language default
    #[test]
    fn test_language_default() {
        let opts = YomiTokuOptions::default();
        assert!(matches!(opts.language, Language::Japanese));
    }

    // Test OutputFormat default
    #[test]
    fn test_output_format_default() {
        let opts = YomiTokuOptions::default();
        assert!(matches!(opts.output_format, OutputFormat::Json));
    }

    // Test GPU configuration options
    #[test]
    fn test_gpu_configuration() {
        // GPU disabled
        let opts_no_gpu = YomiTokuOptions::builder().use_gpu(false).build();
        assert!(!opts_no_gpu.use_gpu);
        assert!(opts_no_gpu.gpu_id.is_none());

        // GPU enabled with specific device
        let opts_gpu = YomiTokuOptions::builder().use_gpu(true).gpu_id(1).build();
        assert!(opts_gpu.use_gpu);
        assert_eq!(opts_gpu.gpu_id, Some(1));
    }

    // Test text extraction with empty blocks
    #[test]
    fn test_extract_text_empty_blocks() {
        let result = OcrResult {
            input_path: PathBuf::from("/empty.png"),
            text_blocks: vec![],
            confidence: 0.0,
            processing_time: Duration::ZERO,
            text_direction: TextDirection::Horizontal,
        };

        let text = YomiToku::extract_text(&result);
        assert!(text.is_empty());
    }

    // Test text extraction preserves order
    #[test]
    fn test_extract_text_preserves_order() {
        let result = OcrResult {
            input_path: PathBuf::from("/ordered.png"),
            text_blocks: vec![
                TextBlock {
                    text: "First".to_string(),
                    bbox: (0, 0, 50, 20),
                    confidence: 0.9,
                    direction: TextDirection::Horizontal,
                    font_size: None,
                },
                TextBlock {
                    text: "Second".to_string(),
                    bbox: (0, 20, 50, 20),
                    confidence: 0.9,
                    direction: TextDirection::Horizontal,
                    font_size: None,
                },
                TextBlock {
                    text: "Third".to_string(),
                    bbox: (0, 40, 50, 20),
                    confidence: 0.9,
                    direction: TextDirection::Horizontal,
                    font_size: None,
                },
            ],
            confidence: 0.9,
            processing_time: Duration::from_millis(100),
            text_direction: TextDirection::Horizontal,
        };

        let text = YomiToku::extract_text(&result);
        assert!(text.contains("First"));
        assert!(text.contains("Second"));
        assert!(text.contains("Third"));
        // Verify order: First appears before Second, Second before Third
        let first_pos = text.find("First").unwrap();
        let second_pos = text.find("Second").unwrap();
        let third_pos = text.find("Third").unwrap();
        assert!(first_pos < second_pos);
        assert!(second_pos < third_pos);
    }

    // Test timeout configuration
    #[test]
    fn test_timeout_configuration() {
        // Very short timeout
        let opts_short = YomiTokuOptions::builder().timeout(1).build();
        assert_eq!(opts_short.timeout_secs, 1);

        // Very long timeout (24 hours)
        let opts_long = YomiTokuOptions::builder().timeout(86400).build();
        assert_eq!(opts_long.timeout_secs, 86400);
    }

    // Test all text directions in OcrResult
    #[test]
    fn test_all_text_directions() {
        let directions = [
            TextDirection::Horizontal,
            TextDirection::Vertical,
            TextDirection::Mixed,
        ];

        for dir in directions {
            let result = OcrResult {
                input_path: PathBuf::from("/test.png"),
                text_blocks: vec![],
                confidence: 0.5,
                processing_time: Duration::from_secs(1),
                text_direction: dir,
            };
            // Verify direction is set correctly
            match result.text_direction {
                TextDirection::Horizontal => assert!(matches!(dir, TextDirection::Horizontal)),
                TextDirection::Vertical => assert!(matches!(dir, TextDirection::Vertical)),
                TextDirection::Mixed => assert!(matches!(dir, TextDirection::Mixed)),
            }
        }
    }

    // Additional comprehensive tests

    #[test]
    fn test_options_debug_impl() {
        let options = YomiTokuOptions::builder().confidence_threshold(0.8).build();
        let debug_str = format!("{:?}", options);
        assert!(debug_str.contains("YomiTokuOptions"));
        assert!(debug_str.contains("0.8"));
    }

    #[test]
    fn test_options_clone() {
        let original = YomiTokuOptions::builder()
            .use_gpu(false)
            .confidence_threshold(0.7)
            .language(Language::Korean)
            .build();
        let cloned = original.clone();
        assert_eq!(cloned.use_gpu, original.use_gpu);
        assert_eq!(cloned.confidence_threshold, original.confidence_threshold);
        assert!(matches!(cloned.language, Language::Korean));
    }

    #[test]
    fn test_text_block_debug_impl() {
        let block = TextBlock {
            text: "テスト".to_string(),
            bbox: (10, 20, 30, 40),
            confidence: 0.95,
            direction: TextDirection::Vertical,
            font_size: Some(14.0),
        };
        let debug_str = format!("{:?}", block);
        assert!(debug_str.contains("TextBlock"));
        assert!(debug_str.contains("テスト"));
    }

    #[test]
    fn test_text_block_clone() {
        let original = TextBlock {
            text: "Clone test".to_string(),
            bbox: (0, 0, 100, 50),
            confidence: 0.9,
            direction: TextDirection::Horizontal,
            font_size: Some(12.0),
        };
        let cloned = original.clone();
        assert_eq!(cloned.text, original.text);
        assert_eq!(cloned.bbox, original.bbox);
        assert_eq!(cloned.confidence, original.confidence);
    }

    #[test]
    fn test_ocr_result_debug_impl() {
        let result = OcrResult {
            input_path: PathBuf::from("/test.png"),
            text_blocks: vec![],
            confidence: 0.85,
            processing_time: Duration::from_secs(1),
            text_direction: TextDirection::Horizontal,
        };
        let debug_str = format!("{:?}", result);
        assert!(debug_str.contains("OcrResult"));
    }

    #[test]
    fn test_batch_result_debug_impl() {
        let batch = BatchOcrResult {
            successful: vec![],
            failed: vec![],
            total_time: Duration::from_secs(5),
        };
        let debug_str = format!("{:?}", batch);
        assert!(debug_str.contains("BatchOcrResult"));
    }

    #[test]
    fn test_error_debug_impl() {
        let err = YomiTokuError::NotInstalled;
        let debug_str = format!("{:?}", err);
        assert!(debug_str.contains("NotInstalled"));
    }

    #[test]
    fn test_language_debug_impl() {
        let lang = Language::ChineseTraditional;
        let debug_str = format!("{:?}", lang);
        assert!(debug_str.contains("ChineseTraditional"));
    }

    #[test]
    fn test_output_format_debug_impl() {
        let format = OutputFormat::Hocr;
        let debug_str = format!("{:?}", format);
        assert!(debug_str.contains("Hocr"));
    }

    #[test]
    fn test_text_direction_debug_impl() {
        let dir = TextDirection::Mixed;
        let debug_str = format!("{:?}", dir);
        assert!(debug_str.contains("Mixed"));
    }

    #[test]
    fn test_text_direction_clone() {
        let original = TextDirection::Vertical;
        let cloned = original.clone();
        assert!(matches!(cloned, TextDirection::Vertical));
    }

    #[test]
    fn test_language_clone() {
        let original = Language::English;
        let cloned = original.clone();
        assert_eq!(cloned.code(), original.code());
    }

    #[test]
    fn test_output_format_clone() {
        let original = OutputFormat::Pdf;
        let cloned = original.clone();
        assert_eq!(cloned.extension(), original.extension());
    }

    #[test]
    fn test_error_io_conversion() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
        let yomi_err: YomiTokuError = io_err.into();
        let msg = yomi_err.to_string().to_lowercase();
        assert!(msg.contains("io") || msg.contains("error"));
    }

    #[test]
    fn test_builder_default_produces_valid_options() {
        let opts = YomiTokuOptionsBuilder::default().build();
        assert!(opts.confidence_threshold >= 0.0 && opts.confidence_threshold <= 1.0);
        assert!(opts.timeout_secs > 0);
    }

    #[test]
    fn test_text_block_with_unicode_text() {
        let block = TextBlock {
            text: "日本語テスト 한국어 中文 🎉".to_string(),
            bbox: (0, 0, 100, 50),
            confidence: 0.9,
            direction: TextDirection::Horizontal,
            font_size: None,
        };
        assert!(block.text.contains("日本語"));
        assert!(block.text.contains("한국어"));
        assert!(block.text.contains("中文"));
    }

    #[test]
    fn test_text_block_with_empty_text() {
        let block = TextBlock {
            text: String::new(),
            bbox: (0, 0, 0, 0),
            confidence: 0.0,
            direction: TextDirection::Horizontal,
            font_size: None,
        };
        assert!(block.text.is_empty());
    }

    #[test]
    fn test_ocr_result_path_types() {
        // Absolute path
        let result_abs = OcrResult {
            input_path: PathBuf::from("/absolute/path/image.png"),
            text_blocks: vec![],
            confidence: 0.5,
            processing_time: Duration::ZERO,
            text_direction: TextDirection::Horizontal,
        };
        assert!(result_abs.input_path.is_absolute());

        // Relative path
        let result_rel = OcrResult {
            input_path: PathBuf::from("relative/path/image.png"),
            text_blocks: vec![],
            confidence: 0.5,
            processing_time: Duration::ZERO,
            text_direction: TextDirection::Horizontal,
        };
        assert!(result_rel.input_path.is_relative());
    }

    #[test]
    fn test_processing_time_variations() {
        // Zero time
        let result_zero = OcrResult {
            input_path: PathBuf::from("/fast.png"),
            text_blocks: vec![],
            confidence: 1.0,
            processing_time: Duration::ZERO,
            text_direction: TextDirection::Horizontal,
        };
        assert_eq!(result_zero.processing_time, Duration::ZERO);

        // Very long processing time
        let result_long = OcrResult {
            input_path: PathBuf::from("/slow.png"),
            text_blocks: vec![],
            confidence: 0.5,
            processing_time: Duration::from_secs(3600), // 1 hour
            text_direction: TextDirection::Horizontal,
        };
        assert_eq!(result_long.processing_time.as_secs(), 3600);
    }

    #[test]
    fn test_font_size_variations() {
        // Very small font
        let small_font = TextBlock {
            text: "tiny".to_string(),
            bbox: (0, 0, 10, 5),
            confidence: 0.5,
            direction: TextDirection::Horizontal,
            font_size: Some(4.0),
        };
        assert_eq!(small_font.font_size, Some(4.0));

        // Very large font
        let large_font = TextBlock {
            text: "HUGE".to_string(),
            bbox: (0, 0, 500, 200),
            confidence: 0.9,
            direction: TextDirection::Horizontal,
            font_size: Some(144.0),
        };
        assert_eq!(large_font.font_size, Some(144.0));
    }

    #[test]
    fn test_preset_consistency() {
        let books = YomiTokuOptions::for_books();
        let horizontal = YomiTokuOptions::horizontal_only();

        // Books preset should detect vertical
        assert!(books.detect_vertical);

        // Horizontal only should not detect vertical
        assert!(!horizontal.detect_vertical);
    }

    #[test]
    fn test_error_path_extraction() {
        let path = PathBuf::from("/some/input/file.png");
        let err = YomiTokuError::InputNotFound(path.clone());

        if let YomiTokuError::InputNotFound(p) = err {
            assert_eq!(p, path);
        } else {
            panic!("Wrong error variant");
        }
    }

    #[test]
    fn test_batch_result_mixed() {
        // Mix of success and failure
        let successful = vec![
            OcrResult {
                input_path: PathBuf::from("/page1.png"),
                text_blocks: vec![],
                confidence: 0.9,
                processing_time: Duration::from_millis(100),
                text_direction: TextDirection::Horizontal,
            },
            OcrResult {
                input_path: PathBuf::from("/page3.png"),
                text_blocks: vec![],
                confidence: 0.8,
                processing_time: Duration::from_millis(150),
                text_direction: TextDirection::Vertical,
            },
        ];

        let failed = vec![(PathBuf::from("/page2.png"), "corrupt image".to_string())];

        let batch = BatchOcrResult {
            successful,
            failed,
            total_time: Duration::from_millis(350),
        };

        assert_eq!(batch.successful.len(), 2);
        assert_eq!(batch.failed.len(), 1);
        assert!(batch.failed[0].1.contains("corrupt"));
    }

    #[test]
    fn test_confidence_zero_to_one_range() {
        for i in 0..=10 {
            let conf = i as f32 / 10.0;
            let block = TextBlock {
                text: format!("conf_{}", i),
                bbox: (0, 0, 10, 10),
                confidence: conf,
                direction: TextDirection::Horizontal,
                font_size: None,
            };
            assert!(block.confidence >= 0.0 && block.confidence <= 1.0);
        }
    }

    #[test]
    fn test_all_language_variants() {
        let languages = [
            Language::Japanese,
            Language::English,
            Language::ChineseSimplified,
            Language::ChineseTraditional,
            Language::Korean,
        ];

        for lang in languages {
            let code = lang.code();
            assert!(!code.is_empty());
        }
    }

    #[test]
    fn test_all_output_format_variants() {
        let formats = [
            OutputFormat::Json,
            OutputFormat::Text,
            OutputFormat::Hocr,
            OutputFormat::Pdf,
        ];

        for format in formats {
            let ext = format.extension();
            assert!(!ext.is_empty());
        }
    }

    // ============ Error Handling Tests ============

    #[test]
    fn test_error_input_not_found() {
        let path = std::path::PathBuf::from("/nonexistent/image.png");
        let err = YomiTokuError::InputNotFound(path.clone());
        let msg = format!("{}", err);
        assert!(msg.contains("Input file not found"));
        assert!(msg.contains("/nonexistent/image.png"));
    }

    #[test]
    fn test_error_execution_failed() {
        let err = YomiTokuError::ExecutionFailed("Engine initialization failed".to_string());
        let msg = format!("{}", err);
        assert!(msg.contains("execution failed"));
    }

    #[test]
    fn test_error_not_installed() {
        let err = YomiTokuError::NotInstalled;
        let msg = format!("{}", err);
        assert!(msg.contains("not installed") || msg.contains("not found"));
    }

    #[test]
    fn test_error_output_not_writable() {
        let path = std::path::PathBuf::from("/readonly/dir");
        let err = YomiTokuError::OutputNotWritable(path);
        let msg = format!("{}", err);
        assert!(msg.contains("not writable"));
    }

    #[test]
    fn test_error_invalid_output() {
        let err = YomiTokuError::InvalidOutput;
        let msg = format!("{}", err);
        assert!(msg.contains("Invalid output"));
    }

    #[test]
    fn test_error_from_io_error() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
        let yomi_err: YomiTokuError = io_err.into();
        let msg = format!("{}", yomi_err);
        assert!(!msg.is_empty());
    }

    #[test]
    fn test_error_execution_failed_debug() {
        let err = YomiTokuError::ExecutionFailed("test".to_string());
        let debug = format!("{:?}", err);
        assert!(debug.contains("ExecutionFailed"));
    }

    #[test]
    fn test_ocr_result_with_error_state() {
        // Test OcrResult when no text was detected
        let result = OcrResult {
            input_path: std::path::PathBuf::from("test.png"),
            text_blocks: vec![],
            confidence: 0.0,
            processing_time: std::time::Duration::from_secs(1),
            text_direction: TextDirection::Horizontal,
        };
        assert!(result.text_blocks.is_empty());
        assert_eq!(result.confidence, 0.0);
    }

    #[test]
    fn test_batch_result_partial_failure() {
        let results = BatchOcrResult {
            successful: vec![OcrResult {
                input_path: std::path::PathBuf::from("good.png"),
                text_blocks: vec![],
                confidence: 0.9,
                processing_time: std::time::Duration::from_millis(100),
                text_direction: TextDirection::Horizontal,
            }],
            failed: vec![(
                std::path::PathBuf::from("bad.png"),
                "OCR failed".to_string(),
            )],
            total_time: std::time::Duration::from_millis(200),
        };

        assert_eq!(results.successful.len(), 1);
        assert_eq!(results.failed.len(), 1);
    }

    #[test]
    fn test_batch_result_all_errors() {
        let results = BatchOcrResult {
            successful: vec![],
            failed: vec![
                (std::path::PathBuf::from("a.png"), "Timeout".to_string()),
                (
                    std::path::PathBuf::from("b.png"),
                    "Python not found".to_string(),
                ),
            ],
            total_time: std::time::Duration::from_secs(30),
        };

        assert!(results.successful.is_empty());
        assert_eq!(results.failed.len(), 2);
    }

    #[test]
    fn test_batch_result_all_success() {
        let results = BatchOcrResult {
            successful: vec![
                OcrResult {
                    input_path: std::path::PathBuf::from("page1.png"),
                    text_blocks: vec![],
                    confidence: 0.95,
                    processing_time: std::time::Duration::from_millis(50),
                    text_direction: TextDirection::Horizontal,
                },
                OcrResult {
                    input_path: std::path::PathBuf::from("page2.png"),
                    text_blocks: vec![],
                    confidence: 0.92,
                    processing_time: std::time::Duration::from_millis(55),
                    text_direction: TextDirection::Vertical,
                },
            ],
            failed: vec![],
            total_time: std::time::Duration::from_millis(105),
        };

        assert_eq!(results.successful.len(), 2);
        assert!(results.failed.is_empty());
    }

    // ==================== Concurrency Tests ====================

    #[test]
    fn test_yomitoku_types_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<YomiTokuOptions>();
        assert_send_sync::<TextBlock>();
        assert_send_sync::<OcrResult>();
        assert_send_sync::<BatchOcrResult>();
    }

    #[test]
    fn test_concurrent_options_building() {
        use std::thread;

        let handles: Vec<_> = (0..4)
            .map(|i| {
                thread::spawn(move || -> YomiTokuOptions {
                    YomiTokuOptions::builder()
                        .confidence_threshold(0.3 + (i as f32 * 0.1))
                        .timeout(100 + i as u64 * 50)
                        .build()
                })
            })
            .collect();

        for (i, handle) in handles.into_iter().enumerate() {
            let opts: YomiTokuOptions = handle.join().unwrap();
            let expected_conf = 0.3 + (i as f32 * 0.1);
            assert!((opts.confidence_threshold - expected_conf).abs() < 0.001);
        }
    }

    #[test]
    fn test_ocr_result_thread_transfer() {
        use std::thread;

        let result = OcrResult {
            input_path: std::path::PathBuf::from("test.png"),
            text_blocks: vec![TextBlock {
                text: "テスト".to_string(),
                bbox: (10, 20, 100, 50),
                confidence: 0.95,
                direction: TextDirection::Vertical,
                font_size: Some(12.0),
            }],
            confidence: 0.95,
            processing_time: Duration::from_millis(200),
            text_direction: TextDirection::Vertical,
        };

        let handle = thread::spawn(move || {
            assert_eq!(result.text_blocks.len(), 1);
            assert_eq!(result.text_blocks[0].text, "テスト");
            result
        });

        let received = handle.join().unwrap();
        assert_eq!(received.confidence, 0.95);
    }

    #[test]
    fn test_options_shared_across_threads() {
        use std::sync::Arc;
        use std::thread;

        let opts = Arc::new(
            YomiTokuOptions::builder()
                .confidence_threshold(0.7)
                .language(Language::Japanese)
                .build(),
        );

        let handles: Vec<_> = (0..4)
            .map(|_| {
                let o = Arc::clone(&opts);
                thread::spawn(move || {
                    assert!((o.confidence_threshold - 0.7).abs() < 0.001);
                    o.confidence_threshold
                })
            })
            .collect();

        for handle in handles {
            let conf = handle.join().unwrap();
            assert!((conf - 0.7).abs() < 0.001);
        }
    }

    // ==================== Boundary Value Tests ====================

    #[test]
    fn test_confidence_threshold_boundary_zero() {
        let opts = YomiTokuOptions::builder()
            .confidence_threshold(0.0)
            .build();
        assert_eq!(opts.confidence_threshold, 0.0);
    }

    #[test]
    fn test_confidence_threshold_boundary_one() {
        let opts = YomiTokuOptions::builder()
            .confidence_threshold(1.0)
            .build();
        assert_eq!(opts.confidence_threshold, 1.0);
    }

    #[test]
    fn test_timeout_secs_zero() {
        let opts = YomiTokuOptions::builder().timeout(0).build();
        assert_eq!(opts.timeout_secs, 0);
    }

    #[test]
    fn test_timeout_secs_large() {
        let opts = YomiTokuOptions::builder().timeout(86400).build();
        assert_eq!(opts.timeout_secs, 86400);
    }

    #[test]
    fn test_bbox_zero_dimensions() {
        let block = TextBlock {
            text: "test".to_string(),
            bbox: (0, 0, 0, 0),
            confidence: 0.5,
            direction: TextDirection::Horizontal,
            font_size: None,
        };
        assert_eq!(block.bbox.2, 0); // width
        assert_eq!(block.bbox.3, 0); // height
    }

    #[test]
    fn test_bbox_large_dimensions() {
        let block = TextBlock {
            text: "large".to_string(),
            bbox: (10000, 20000, 5000, 3000),
            confidence: 0.9,
            direction: TextDirection::Horizontal,
            font_size: Some(24.0),
        };
        assert_eq!(block.bbox.0, 10000); // x
        assert_eq!(block.bbox.2, 5000); // width
    }

    #[test]
    fn test_text_block_empty_text_boundary() {
        let block = TextBlock {
            text: String::new(),
            bbox: (0, 0, 100, 50),
            confidence: 0.5,
            direction: TextDirection::Horizontal,
            font_size: None,
        };
        assert!(block.text.is_empty());
    }

    #[test]
    fn test_text_block_unicode_boundary() {
        let block = TextBlock {
            text: "日本語テキスト🎉".to_string(),
            bbox: (0, 0, 200, 100),
            confidence: 0.99,
            direction: TextDirection::Vertical,
            font_size: Some(16.0),
        };
        assert!(block.text.contains("日本語"));
        assert!(block.text.contains("🎉"));
    }

    #[test]
    fn test_processing_time_nanos_boundary() {
        let result = OcrResult {
            input_path: std::path::PathBuf::from("nano.png"),
            text_blocks: vec![],
            confidence: 1.0,
            processing_time: Duration::from_nanos(1),
            text_direction: TextDirection::Horizontal,
        };
        assert_eq!(result.processing_time.as_nanos(), 1);
    }

    #[test]
    fn test_batch_result_empty_boundary() {
        let results = BatchOcrResult {
            successful: vec![],
            failed: vec![],
            total_time: Duration::ZERO,
        };
        assert!(results.successful.is_empty());
        assert!(results.failed.is_empty());
        assert_eq!(results.total_time, Duration::ZERO);
    }

    #[test]
    fn test_font_size_none() {
        let block = TextBlock {
            text: "no size".to_string(),
            bbox: (0, 0, 50, 20),
            confidence: 0.8,
            direction: TextDirection::Horizontal,
            font_size: None,
        };
        assert!(block.font_size.is_none());
    }

    #[test]
    fn test_font_size_zero() {
        let block = TextBlock {
            text: "zero size".to_string(),
            bbox: (0, 0, 50, 20),
            confidence: 0.8,
            direction: TextDirection::Horizontal,
            font_size: Some(0.0),
        };
        assert_eq!(block.font_size, Some(0.0));
    }
}
