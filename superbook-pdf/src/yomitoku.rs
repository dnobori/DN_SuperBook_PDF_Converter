//! YomiToku Japanese AI-OCR module
//!
//! Provides integration with YomiToku for Japanese text recognition in images.

use std::path::{Path, PathBuf};
use std::time::Duration;
use thiserror::Error;

use crate::ai_bridge::{AiBridgeError, AiTool, SubprocessBridge};

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
            confidence_threshold: 0.5,
            timeout_secs: 300,
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
            confidence_threshold: 0.3,
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
        self.options.confidence_threshold = threshold.clamp(0.0, 1.0);
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
        let output = self.bridge.execute_with_timeout(
            &args,
            Duration::from_secs(options.timeout_secs),
        )?;

        // Parse JSON output
        let json_result: serde_json::Value = serde_json::from_str(&output)
            .map_err(|e| YomiTokuError::ExecutionFailed(format!("Failed to parse output: {}", e)))?;

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

            let font_size = block.get("font_size").and_then(|f| f.as_f64()).map(|f| f as f32);

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
        let opts = YomiTokuOptions::builder()
            .confidence_threshold(1.5)
            .build();
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
        assert!(matches!(TextDirection::Horizontal, TextDirection::Horizontal));
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
}
