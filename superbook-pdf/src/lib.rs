//! superbook-pdf - High-quality PDF converter for scanned books
//!
//! A complete Rust implementation for converting scanned book PDFs into
//! high-quality digital books with AI enhancement.
//!
//! # Features
//!
//! - **PDF Reading** ([`pdf_reader`]) - Extract metadata, pages, and images from PDFs
//! - **PDF Writing** ([`pdf_writer`]) - Generate PDFs from images with optional OCR layer
//! - **Image Extraction** ([`image_extract`]) - Extract page images using `ImageMagick`
//! - **AI Enhancement** ([`realesrgan`]) - Upscale images using `RealESRGAN`
//! - **Deskew Correction** ([`deskew`]) - Detect and correct page skew
//! - **Margin Detection** ([`margin`]) - Detect and trim page margins
//! - **Page Number Detection** ([`page_number`]) - OCR-based page number recognition
//! - **AI Bridge** ([`ai_bridge`]) - Python subprocess bridge for AI tools
//! - **`YomiToku` OCR** ([`yomitoku`]) - Japanese AI-OCR for searchable PDFs
//!
//! # Quick Start
//!
//! ## Reading a PDF
//!
//! ```rust,no_run
//! use superbook_pdf::{LopdfReader, PdfWriterOptions, PrintPdfWriter};
//!
//! // Read a PDF
//! let reader = LopdfReader::new("input.pdf").unwrap();
//! println!("Pages: {}", reader.info.page_count);
//! ```
//!
//! ## Using Builder Patterns
//!
//! All option structs support fluent builder patterns:
//!
//! ```rust
//! use superbook_pdf::{PdfWriterOptions, DeskewOptions, RealEsrganOptions};
//!
//! // PDF Writer options
//! let pdf_opts = PdfWriterOptions::builder()
//!     .dpi(600)
//!     .jpeg_quality(95)
//!     .build();
//!
//! // Or use presets
//! let high_quality = PdfWriterOptions::high_quality();
//! let compact = PdfWriterOptions::compact();
//!
//! // Deskew options
//! let deskew_opts = DeskewOptions::builder()
//!     .max_angle(15.0)
//!     .build();
//!
//! // RealESRGAN options
//! let upscale_opts = RealEsrganOptions::builder()
//!     .scale(4)
//!     .tile_size(256)
//!     .build();
//! ```
//!
//! # Architecture
//!
//! The library is organized into independent modules that can be used separately:
//!
//! ```text
//! PDF Input -> Image Extraction -> Deskew -> Margin Detection
//!                                    |
//!                            AI Upscaling (RealESRGAN)
//!                                    |
//!                         Page Number Detection -> OCR -> PDF Output
//! ```
//!
//! # Error Handling
//!
//! Each module has its own error type that can be matched for specific handling:
//!
//! - [`PdfReaderError`] - PDF reading errors
//! - [`PdfWriterError`] - PDF writing errors
//! - [`ExtractError`] - Image extraction errors
//! - [`DeskewError`] - Deskew processing errors
//! - [`MarginError`] - Margin detection errors
//! - [`PageNumberError`] - Page number detection errors
//! - [`AiBridgeError`] - AI tool communication errors
//! - [`RealEsrganError`] - `RealESRGAN` upscaling errors
//! - [`YomiTokuError`] - `YomiToku` OCR errors
//!
//! # CLI Exit Codes
//!
//! Use [`ExitCode`] for type-safe exit code handling:
//!
//! ```rust
//! use superbook_pdf::ExitCode;
//!
//! let code = ExitCode::Success;
//! assert_eq!(code.code(), 0);
//! assert_eq!(code.description(), "Success");
//! ```
//!
//! # Error Handling Example
//!
//! ```rust
//! use superbook_pdf::{PdfReaderError, MarginError, DeskewError};
//! use std::path::PathBuf;
//!
//! fn handle_pdf_error(err: PdfReaderError) -> String {
//!     match err {
//!         PdfReaderError::FileNotFound(path) => format!("File not found: {}", path.display()),
//!         PdfReaderError::InvalidFormat(msg) => format!("Invalid PDF: {}", msg),
//!         PdfReaderError::EncryptedPdf => "Encrypted PDFs are not supported".to_string(),
//!         _ => format!("Other error: {}", err),
//!     }
//! }
//!
//! let err = PdfReaderError::FileNotFound(PathBuf::from("/test.pdf"));
//! assert!(handle_pdf_error(err).contains("/test.pdf"));
//! ```
//!
//! # License
//!
//! AGPL-3.0

pub mod ai_bridge;
pub mod cache;
pub mod cli;
pub mod config;
pub mod color_stats;
pub mod deskew;
pub mod finalize;
pub mod image_extract;
pub mod margin;
pub mod normalize;
pub mod page_number;
pub mod parallel;
pub mod pdf_reader;
pub mod pipeline;
pub mod progress;
pub mod pdf_writer;
pub mod realesrgan;
pub mod util;
pub mod vertical_detect;
#[cfg(feature = "web")]
pub mod web;
pub mod yomitoku;

// Re-exports for convenience
pub use ai_bridge::{
    AiBridgeConfig, AiBridgeConfigBuilder, AiBridgeError, AiTool, SubprocessBridge,
};
pub use cli::{
    create_page_progress_bar, create_progress_bar, create_spinner, CacheInfoArgs, Cli, Commands,
    ConvertArgs, ExitCode,
};
#[cfg(feature = "web")]
pub use cli::ServeArgs;
pub use config::{
    AdvancedConfig, CliOverrides, Config, ConfigError, GeneralConfig, OcrConfig, OutputConfig,
    ProcessingConfig,
};
pub use deskew::{
    DeskewAlgorithm, DeskewError, DeskewOptions, DeskewOptionsBuilder, DeskewResult,
    ImageProcDeskewer, QualityMode, SkewDetection,
};
pub use image_extract::{
    ColorSpace, ExtractError, ExtractOptions, ExtractOptionsBuilder, ExtractedPage, ImageFormat,
    LopdfExtractor, MagickExtractor,
};
pub use margin::{
    ContentDetectionMode, ContentRect, GroupCropAnalyzer, GroupCropRegion, ImageMarginDetector,
    MarginDetection, MarginError, MarginOptions, MarginOptionsBuilder, Margins, PageBoundingBox,
    TrimResult, UnifiedCropRegions, UnifiedMargins,
};
pub use page_number::{
    BookOffsetAnalysis, DetectedPageNumber, OffsetCorrection, PageNumberAnalysis, PageNumberError,
    PageNumberOptions, PageNumberOptionsBuilder, PageNumberPosition, PageNumberRect,
    PageOffsetAnalyzer, PageOffsetResult, TesseractPageDetector,
};
pub use pdf_reader::{LopdfReader, PdfDocument, PdfMetadata, PdfPage, PdfReaderError};
pub use pdf_writer::{PdfWriterError, PdfWriterOptions, PdfWriterOptionsBuilder, PrintPdfWriter};
pub use realesrgan::{RealEsrgan, RealEsrganError, RealEsrganOptions, RealEsrganOptionsBuilder};
pub use util::{
    clamp, ensure_dir_writable, ensure_file_exists, format_duration, format_file_size, load_image,
    mm_to_pixels, mm_to_points, percentage, pixels_to_mm, points_to_mm,
};
pub use yomitoku::{
    BatchOcrResult, OcrResult, TextBlock, TextDirection, YomiToku, YomiTokuError, YomiTokuOptions,
    YomiTokuOptionsBuilder,
};

// Phase 1-6: Advanced processing modules
pub use color_stats::{ColorAnalyzer, ColorStats, ColorStatsError, GlobalColorParam};
pub use finalize::{
    FinalizeError, FinalizeOptions, FinalizeOptionsBuilder, FinalizeResult, PageFinalizer,
};
pub use normalize::{
    ImageNormalizer, NormalizeError, NormalizeOptions, NormalizeOptionsBuilder, NormalizeResult,
    PaddingMode, PaperColor, Resampler,
};
pub use vertical_detect::{
    detect_book_vertical_writing, detect_vertical_probability, BookVerticalResult,
    VerticalDetectError, VerticalDetectOptions, VerticalDetectResult,
};
pub use parallel::{
    parallel_map, parallel_process, ParallelError, ParallelOptions, ParallelProcessor,
    ParallelResult,
};
pub use progress::{build_progress_bar, OutputMode, ProcessingStage, ProgressTracker};
pub use cache::{
    should_skip_processing, CacheDigest, ProcessingCache, ProcessingResult, CACHE_EXTENSION,
    CACHE_VERSION,
};
pub use pipeline::{
    PdfPipeline, PipelineConfig, PipelineError, PipelineResult, ProcessingContext,
    ProgressCallback, SilentProgress,
};

// Web server (optional feature)
#[cfg(feature = "web")]
pub use web::{
    ConvertOptions as WebConvertOptions, Job, JobQueue, JobStatus, Progress as WebProgress,
    ServerConfig, WebServer,
};

/// Exit codes for CLI (deprecated: prefer using `ExitCode` enum)
///
/// These constants are provided for backward compatibility.
/// The `ExitCode` enum provides a more type-safe alternative.
pub mod exit_codes {
    use super::ExitCode;

    pub const SUCCESS: i32 = ExitCode::Success as i32;
    pub const GENERAL_ERROR: i32 = ExitCode::GeneralError as i32;
    pub const INVALID_ARGS: i32 = ExitCode::InvalidArgs as i32;
    pub const INPUT_NOT_FOUND: i32 = ExitCode::InputNotFound as i32;
    pub const OUTPUT_ERROR: i32 = ExitCode::OutputError as i32;
    pub const PROCESSING_ERROR: i32 = ExitCode::ProcessingError as i32;
    pub const GPU_ERROR: i32 = ExitCode::GpuError as i32;
    pub const EXTERNAL_TOOL_ERROR: i32 = ExitCode::ExternalToolError as i32;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    // ============ Re-export Verification Tests ============

    #[test]
    fn test_all_public_types_accessible() {
        // Verify all public types are accessible via re-exports

        // PDF Reader types
        let _reader_err: Option<PdfReaderError> = None;
        let _doc: Option<PdfDocument> = None;
        let _meta: PdfMetadata = Default::default();
        let _page: Option<PdfPage> = None;

        // PDF Writer types
        let _writer_err: Option<PdfWriterError> = None;
        let _opts = PdfWriterOptions::default();
        let _builder = PdfWriterOptions::builder();

        // Image Extract types
        let _ext_err: Option<ExtractError> = None;
        let _ext_opts = ExtractOptions::default();
        let _format = ImageFormat::Png;
        let _colorspace = ColorSpace::Rgb;

        // Deskew types
        let _dsk_err: Option<DeskewError> = None;
        let _dsk_opts = DeskewOptions::default();
        let _algo = DeskewAlgorithm::HoughLines;
        let _quality = QualityMode::Standard;

        // Margin types
        let _mrg_err: Option<MarginError> = None;
        let _mrg_opts = MarginOptions::default();
        let _detection_mode = ContentDetectionMode::BackgroundColor;
        let _margins = Margins::default();

        // Page Number types
        let _pgn_err: Option<PageNumberError> = None;
        let _pgn_opts = PageNumberOptions::default();
        let _position = PageNumberPosition::BottomCenter;

        // AI Bridge types
        let _ai_err: Option<AiBridgeError> = None;
        let _ai_config = AiBridgeConfig::default();

        // RealESRGAN types
        let _res_err: Option<RealEsrganError> = None;
        let _res_opts = RealEsrganOptions::default();

        // YomiToku types
        let _yomi_err: Option<YomiTokuError> = None;
        let _yomi_opts = YomiTokuOptions::default();
        let _direction = TextDirection::Horizontal;

        // CLI types
        let code = ExitCode::Success;
        assert_eq!(code.code(), 0);
    }

    #[test]
    fn test_exit_codes_module() {
        assert_eq!(exit_codes::SUCCESS, 0);
        assert_ne!(exit_codes::GENERAL_ERROR, 0);
        assert_ne!(exit_codes::INVALID_ARGS, 0);
        assert_ne!(exit_codes::INPUT_NOT_FOUND, 0);
        assert_ne!(exit_codes::OUTPUT_ERROR, 0);
        assert_ne!(exit_codes::PROCESSING_ERROR, 0);
        assert_ne!(exit_codes::GPU_ERROR, 0);
        assert_ne!(exit_codes::EXTERNAL_TOOL_ERROR, 0);
    }

    #[test]
    fn test_exit_codes_match_enum() {
        assert_eq!(exit_codes::SUCCESS, ExitCode::Success.code());
        assert_eq!(exit_codes::GENERAL_ERROR, ExitCode::GeneralError.code());
        assert_eq!(exit_codes::INVALID_ARGS, ExitCode::InvalidArgs.code());
        assert_eq!(exit_codes::INPUT_NOT_FOUND, ExitCode::InputNotFound.code());
        assert_eq!(exit_codes::OUTPUT_ERROR, ExitCode::OutputError.code());
        assert_eq!(
            exit_codes::PROCESSING_ERROR,
            ExitCode::ProcessingError.code()
        );
        assert_eq!(exit_codes::GPU_ERROR, ExitCode::GpuError.code());
        assert_eq!(
            exit_codes::EXTERNAL_TOOL_ERROR,
            ExitCode::ExternalToolError.code()
        );
    }

    // ============ Builder Pattern Consistency Tests ============

    #[test]
    fn test_all_builders_follow_same_pattern() {
        // All builders should work with build() method
        let _pdf = PdfWriterOptions::builder().dpi(300).build();
        let _dsk = DeskewOptions::builder().max_angle(10.0).build();
        let _ext = ExtractOptions::builder().dpi(300).build();
        let _mrg = MarginOptions::builder().min_margin(5).build();
        let _pgn = PageNumberOptions::builder().min_confidence(70.0).build();
        let _res = RealEsrganOptions::builder().scale(2).build();
        let _yomi = YomiTokuOptions::builder()
            .language(yomitoku::Language::Japanese)
            .build();
        let _ai = AiBridgeConfig::builder().max_retries(3).build();
    }

    #[test]
    fn test_all_defaults_are_valid() {
        // All default options should be usable
        let pdf = PdfWriterOptions::default();
        assert!(pdf.dpi > 0);
        assert!(pdf.jpeg_quality > 0 && pdf.jpeg_quality <= 100);

        let dsk = DeskewOptions::default();
        assert!(dsk.max_angle > 0.0);
        assert!(dsk.threshold_angle >= 0.0);

        let ext = ExtractOptions::default();
        assert!(ext.dpi > 0);

        let mrg = MarginOptions::default();
        assert!(mrg.background_threshold > 0);

        let pgn = PageNumberOptions::default();
        assert!(pgn.min_confidence > 0.0);
        assert!(!pgn.ocr_language.is_empty());

        let res = RealEsrganOptions::default();
        assert!(res.scale > 0);

        let yomi = YomiTokuOptions::default();
        // Language enum has a default value (Japanese)
        assert!(matches!(yomi.language, yomitoku::Language::Japanese));
    }

    // ============ Preset Consistency Tests ============

    #[test]
    fn test_pdf_writer_presets() {
        let high = PdfWriterOptions::high_quality();
        let compact = PdfWriterOptions::compact();

        // High quality should have higher DPI and quality
        assert!(high.dpi >= compact.dpi);
        assert!(high.jpeg_quality >= compact.jpeg_quality);
    }

    #[test]
    fn test_deskew_presets() {
        let high = DeskewOptions::high_quality();
        let fast = DeskewOptions::fast();

        // High quality should use better interpolation
        assert!(matches!(high.quality_mode, QualityMode::HighQuality));
        assert!(matches!(fast.quality_mode, QualityMode::Fast));
    }

    #[test]
    fn test_page_number_presets() {
        let jpn = PageNumberOptions::japanese();
        let eng = PageNumberOptions::english();
        let strict = PageNumberOptions::strict();

        assert_eq!(jpn.ocr_language, "jpn");
        assert_eq!(eng.ocr_language, "eng");
        assert!(strict.min_confidence > PageNumberOptions::default().min_confidence);
    }

    #[test]
    fn test_margin_presets() {
        let dark = MarginOptions::for_dark_background();
        let precise = MarginOptions::precise();

        // Dark background should have lower threshold
        assert!(dark.background_threshold < MarginOptions::default().background_threshold);
        // Precise should use combined detection
        assert!(matches!(
            precise.detection_mode,
            ContentDetectionMode::Combined
        ));
    }

    #[test]
    fn test_ai_bridge_presets() {
        let cpu = AiBridgeConfig::cpu_only();
        let low_vram = AiBridgeConfig::low_vram();

        // CPU only should have GPU disabled
        assert!(!cpu.gpu_config.enabled);
        // Low VRAM should have memory limits
        assert!(low_vram.gpu_config.max_vram_mb.is_some());
    }

    #[test]
    fn test_realesrgan_presets() {
        let quality = RealEsrganOptions::x4_high_quality();
        let anime = RealEsrganOptions::anime();

        // High quality should be 4x
        assert_eq!(quality.scale, 4);
        // Anime preset should have anime model
        assert!(matches!(
            anime.model,
            realesrgan::RealEsrganModel::X4PlusAnime
        ));
    }

    // ============ Util Function Tests ============

    #[test]
    fn test_clamp_function() {
        assert_eq!(clamp(5, 0, 10), 5);
        assert_eq!(clamp(-5, 0, 10), 0);
        assert_eq!(clamp(15, 0, 10), 10);
        assert_eq!(clamp(0, 0, 10), 0);
        assert_eq!(clamp(10, 0, 10), 10);
    }

    #[test]
    fn test_format_file_size() {
        assert!(format_file_size(500).contains("B"));
        assert!(format_file_size(1024).contains("K") || format_file_size(1024).contains("1"));
        assert!(
            format_file_size(1024 * 1024).contains("M")
                || format_file_size(1024 * 1024).contains("1")
        );
    }

    #[test]
    fn test_format_duration() {
        let duration = std::time::Duration::from_secs(65);
        let formatted = format_duration(duration);
        assert!(formatted.contains("1") || formatted.contains("min") || formatted.contains(":"));
    }

    #[test]
    fn test_mm_points_conversion() {
        // 1 inch = 72 points = 25.4 mm
        let mm = 25.4;
        let points = mm_to_points(mm);
        assert!((points - 72.0).abs() < 0.1);

        let back_to_mm = points_to_mm(points);
        assert!((back_to_mm - mm).abs() < 0.1);
    }

    #[test]
    fn test_mm_pixels_conversion() {
        // At 300 DPI: 25.4mm = 1 inch = 300 pixels
        let mm = 25.4;
        let dpi = 300;
        let pixels = mm_to_pixels(mm, dpi);
        assert!((pixels as i32 - 300).abs() < 2);

        let back_to_mm = pixels_to_mm(pixels, dpi);
        assert!((back_to_mm - mm).abs() < 1.0);
    }

    #[test]
    fn test_percentage_function() {
        assert_eq!(percentage(50, 100), 50.0);
        assert_eq!(percentage(25, 100), 25.0);
        assert_eq!(percentage(100, 100), 100.0);
        assert_eq!(percentage(0, 100), 0.0);
    }

    #[test]
    fn test_ensure_file_exists_nonexistent() {
        let result = ensure_file_exists(PathBuf::from("/nonexistent/file.pdf"));
        assert!(result.is_err());
    }

    #[test]
    fn test_ensure_dir_writable_nonexistent() {
        let result = ensure_dir_writable(PathBuf::from("/nonexistent/directory"));
        assert!(result.is_err());
    }

    // ============ Error Type Conversion Tests ============

    #[test]
    fn test_io_error_conversions() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "test");

        // All error types should be convertible from io::Error
        let _pdf_err: PdfReaderError = io_err.into();

        let io_err2 = std::io::Error::new(std::io::ErrorKind::NotFound, "test");
        let _writer_err: PdfWriterError = io_err2.into();

        let io_err3 = std::io::Error::new(std::io::ErrorKind::NotFound, "test");
        let _ext_err: ExtractError = io_err3.into();

        let io_err4 = std::io::Error::new(std::io::ErrorKind::NotFound, "test");
        let _dsk_err: DeskewError = io_err4.into();

        let io_err5 = std::io::Error::new(std::io::ErrorKind::NotFound, "test");
        let _mrg_err: MarginError = io_err5.into();

        let io_err6 = std::io::Error::new(std::io::ErrorKind::NotFound, "test");
        let _pgn_err: PageNumberError = io_err6.into();
    }

    // ============ Cross-Module Integration Tests ============

    #[test]
    fn test_margins_with_content_rect() {
        let margins = Margins {
            top: 50,
            bottom: 50,
            left: 30,
            right: 30,
        };

        let content = ContentRect {
            x: margins.left,
            y: margins.top,
            width: 1000 - margins.total_horizontal(),
            height: 1500 - margins.total_vertical(),
        };

        assert_eq!(content.x, 30);
        assert_eq!(content.y, 50);
        assert_eq!(content.width, 940);
        assert_eq!(content.height, 1400);
    }

    #[test]
    fn test_page_metadata_from_reader_to_writer() {
        let metadata = PdfMetadata {
            title: Some("Test Book".to_string()),
            author: Some("Test Author".to_string()),
            subject: Some("Test Subject".to_string()),
            keywords: Some("rust, pdf, test".to_string()),
            creator: Some("superbook-pdf".to_string()),
            producer: Some("superbook-pdf".to_string()),
            creation_date: None,
            modification_date: None,
        };

        // Create writer options with metadata
        let opts = PdfWriterOptions::builder()
            .metadata(metadata.clone())
            .build();

        assert!(opts.metadata.is_some());
        let writer_meta = opts.metadata.unwrap();
        assert_eq!(writer_meta.title, metadata.title);
        assert_eq!(writer_meta.author, metadata.author);
    }

    #[test]
    fn test_exit_code_covers_all_error_types() {
        // Each error type should map to an appropriate exit code

        // File not found errors -> INPUT_NOT_FOUND
        let input_not_found = ExitCode::InputNotFound;
        assert_ne!(input_not_found.code(), 0);

        // Processing errors -> PROCESSING_ERROR
        let processing_error = ExitCode::ProcessingError;
        assert_ne!(processing_error.code(), 0);

        // GPU errors -> GPU_ERROR
        let gpu_error = ExitCode::GpuError;
        assert_ne!(gpu_error.code(), 0);

        // External tool errors -> EXTERNAL_TOOL_ERROR
        let tool_error = ExitCode::ExternalToolError;
        assert_ne!(tool_error.code(), 0);
    }

    #[test]
    fn test_image_format_and_colorspace_combinations() {
        let formats = [
            ImageFormat::Png,
            ImageFormat::Jpeg { quality: 90 },
            ImageFormat::Tiff,
        ];

        let colorspaces = [ColorSpace::Rgb, ColorSpace::Grayscale, ColorSpace::Cmyk];

        // All combinations should be valid
        for format in &formats {
            for colorspace in &colorspaces {
                let opts = ExtractOptions::builder()
                    .format(*format)
                    .colorspace(*colorspace)
                    .build();

                // Just verify they can be combined
                let _ = format!("{:?} + {:?}", opts.format, opts.colorspace);
            }
        }
    }

    #[test]
    fn test_text_direction_with_text_block() {
        let horizontal = TextBlock {
            text: "Horizontal text".to_string(),
            bbox: (0, 0, 200, 20),
            confidence: 0.95,
            direction: TextDirection::Horizontal,
            font_size: Some(12.0),
        };

        let vertical = TextBlock {
            text: "縦書きテキスト".to_string(),
            bbox: (0, 0, 20, 200),
            confidence: 0.95,
            direction: TextDirection::Vertical,
            font_size: Some(12.0),
        };

        assert!(matches!(horizontal.direction, TextDirection::Horizontal));
        assert!(matches!(vertical.direction, TextDirection::Vertical));

        // Aspect ratio check
        let h_width = horizontal.bbox.2 - horizontal.bbox.0;
        let h_height = horizontal.bbox.3 - horizontal.bbox.1;
        assert!(h_width > h_height);

        let v_width = vertical.bbox.2 - vertical.bbox.0;
        let v_height = vertical.bbox.3 - vertical.bbox.1;
        assert!(v_height > v_width);
    }

    #[test]
    fn test_deskew_result_with_detection() {
        let detection = SkewDetection {
            angle: 2.5,
            confidence: 0.92,
            feature_count: 150,
        };

        let result = DeskewResult {
            detection: detection.clone(),
            corrected: true,
            output_path: PathBuf::from("/output/corrected.png"),
            original_size: (2480, 3508),
            corrected_size: (2500, 3530),
        };

        assert_eq!(result.detection.angle, 2.5);
        assert!(result.corrected);
        // Corrected size may be slightly different due to rotation
        assert!(result.corrected_size.0 >= result.original_size.0 - 50);
    }

    #[test]
    fn test_upscale_result_dimensions() {
        // When upscaling 2x, dimensions should double
        let original = (1000u32, 1500u32);
        let scale = 2u32;
        let expected = (original.0 * scale, original.1 * scale);

        assert_eq!(expected, (2000, 3000));

        // 4x upscale
        let scale4 = 4u32;
        let expected4 = (original.0 * scale4, original.1 * scale4);
        assert_eq!(expected4, (4000, 6000));
    }

    #[test]
    fn test_progress_bar_creation() {
        let pb = create_progress_bar(100);
        assert_eq!(pb.length(), Some(100));

        let page_pb = create_page_progress_bar(50);
        assert_eq!(page_pb.length(), Some(50));

        let spinner = create_spinner("Processing...");
        assert_eq!(spinner.message(), "Processing...");
    }

    // ==================== Additional API Tests ====================

    #[test]
    fn test_all_presets_have_valid_defaults() {
        // PDF Writer presets
        let pdf_default = PdfWriterOptions::default();
        let pdf_high = PdfWriterOptions::high_quality();
        let pdf_compact = PdfWriterOptions::compact();
        assert!(pdf_default.dpi > 0);
        assert!(pdf_high.dpi >= pdf_default.dpi);
        assert!(pdf_compact.dpi <= pdf_default.dpi);

        // Deskew presets
        let dsk_default = DeskewOptions::default();
        let dsk_high = DeskewOptions::high_quality();
        let dsk_fast = DeskewOptions::fast();
        assert!(dsk_default.max_angle > 0.0);
        assert!(dsk_high.max_angle > 0.0);
        assert!(dsk_fast.max_angle > 0.0);

        // Margin presets
        let mrg_default = MarginOptions::default();
        let mrg_dark = MarginOptions::for_dark_background();
        let mrg_precise = MarginOptions::precise();
        assert!(mrg_default.background_threshold > 0);
        assert!(mrg_dark.background_threshold < mrg_default.background_threshold);
        assert!(mrg_precise.edge_sensitivity > mrg_default.edge_sensitivity);

        // RealESRGAN presets
        let res_default = RealEsrganOptions::default();
        let res_x4 = RealEsrganOptions::x4_high_quality();
        let res_anime = RealEsrganOptions::anime();
        assert!(res_default.scale > 0);
        assert_eq!(res_x4.scale, 4);
        assert!(res_anime.scale > 0);
    }

    #[test]
    fn test_builder_chain_immutability() {
        // Verify builder methods return new instances
        let builder1 = PdfWriterOptions::builder();
        let builder2 = builder1.dpi(300);
        let opts = builder2.build();
        assert_eq!(opts.dpi, 300);

        // Another chain
        let opts2 = PdfWriterOptions::builder()
            .dpi(600)
            .jpeg_quality(95)
            .build();
        assert_eq!(opts2.dpi, 600);
        assert_eq!(opts2.jpeg_quality, 95);
    }

    #[test]
    fn test_error_messages_are_descriptive() {
        // PDF Reader errors
        let pdf_err = PdfReaderError::FileNotFound(PathBuf::from("/test.pdf"));
        let msg = format!("{}", pdf_err);
        assert!(msg.contains("test.pdf"));

        // Extract errors
        let ext_err = ExtractError::PdfNotFound(PathBuf::from("/input.pdf"));
        let msg = format!("{}", ext_err);
        assert!(msg.contains("input.pdf"));

        // Deskew errors
        let dsk_err = DeskewError::ImageNotFound(PathBuf::from("/image.png"));
        let msg = format!("{}", dsk_err);
        assert!(msg.contains("image.png"));
    }

    #[test]
    fn test_exit_code_uniqueness() {
        // All exit codes should be unique
        let codes = [
            ExitCode::Success,
            ExitCode::GeneralError,
            ExitCode::InvalidArgs,
            ExitCode::InputNotFound,
            ExitCode::OutputError,
            ExitCode::ProcessingError,
            ExitCode::GpuError,
            ExitCode::ExternalToolError,
        ];

        let code_values: Vec<i32> = codes.iter().map(|c| c.code()).collect();
        let mut unique_values = code_values.clone();
        unique_values.sort();
        unique_values.dedup();

        assert_eq!(
            code_values.len(),
            unique_values.len(),
            "Exit codes must be unique"
        );
    }

    #[test]
    fn test_margins_arithmetic() {
        let margins = Margins {
            top: 100,
            bottom: 150,
            left: 50,
            right: 75,
        };

        assert_eq!(margins.total_horizontal(), 125);
        assert_eq!(margins.total_vertical(), 250);

        // Test with zero margins
        let zero_margins = Margins::default();
        assert_eq!(zero_margins.total_horizontal(), 0);
        assert_eq!(zero_margins.total_vertical(), 0);
    }

    #[test]
    fn test_content_rect_calculations() {
        let rect = ContentRect {
            x: 50,
            y: 100,
            width: 800,
            height: 1000,
        };

        // Verify coordinates are stored correctly
        assert_eq!(rect.x, 50);
        assert_eq!(rect.y, 100);
        assert_eq!(rect.width, 800);
        assert_eq!(rect.height, 1000);

        // Calculate right and bottom
        let right = rect.x + rect.width;
        let bottom = rect.y + rect.height;
        assert_eq!(right, 850);
        assert_eq!(bottom, 1100);
    }

    #[test]
    fn test_page_number_positions() {
        let positions = [
            PageNumberPosition::BottomCenter,
            PageNumberPosition::BottomOutside,
            PageNumberPosition::BottomInside,
            PageNumberPosition::TopCenter,
            PageNumberPosition::TopOutside,
        ];

        // All positions should be distinct
        for (i, pos1) in positions.iter().enumerate() {
            for (j, pos2) in positions.iter().enumerate() {
                if i != j {
                    assert_ne!(std::mem::discriminant(pos1), std::mem::discriminant(pos2));
                }
            }
        }
    }

    #[test]
    fn test_yomitoku_language_options() {
        let jpn = YomiTokuOptions::builder()
            .language(yomitoku::Language::Japanese)
            .build();
        assert!(matches!(jpn.language, yomitoku::Language::Japanese));

        let eng = YomiTokuOptions::builder()
            .language(yomitoku::Language::English)
            .build();
        assert!(matches!(eng.language, yomitoku::Language::English));
    }

    #[test]
    fn test_deskew_algorithms() {
        let algorithms = [
            DeskewAlgorithm::HoughLines,
            DeskewAlgorithm::ProjectionProfile,
            DeskewAlgorithm::TextLineDetection,
            DeskewAlgorithm::Combined,
        ];

        // All algorithms should be usable in options
        for algo in algorithms {
            let opts = DeskewOptions::builder().algorithm(algo).build();
            assert!(matches!(
                opts.algorithm,
                DeskewAlgorithm::HoughLines
                    | DeskewAlgorithm::ProjectionProfile
                    | DeskewAlgorithm::TextLineDetection
                    | DeskewAlgorithm::Combined
            ));
        }
    }

    // ============ Concurrency Tests for Re-exported Types ============

    #[test]
    fn test_all_options_types_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<PdfWriterOptions>();
        assert_send_sync::<DeskewOptions>();
        assert_send_sync::<ExtractOptions>();
        assert_send_sync::<MarginOptions>();
        assert_send_sync::<PageNumberOptions>();
        assert_send_sync::<RealEsrganOptions>();
        assert_send_sync::<YomiTokuOptions>();
        assert_send_sync::<AiBridgeConfig>();
    }

    #[test]
    fn test_all_error_types_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<PdfReaderError>();
        assert_send_sync::<PdfWriterError>();
        assert_send_sync::<ExtractError>();
        assert_send_sync::<DeskewError>();
        assert_send_sync::<MarginError>();
        assert_send_sync::<PageNumberError>();
        assert_send_sync::<RealEsrganError>();
        assert_send_sync::<YomiTokuError>();
        assert_send_sync::<AiBridgeError>();
    }

    #[test]
    fn test_all_data_types_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<PdfDocument>();
        assert_send_sync::<PdfMetadata>();
        assert_send_sync::<PdfPage>();
        assert_send_sync::<Margins>();
        assert_send_sync::<ContentRect>();
        assert_send_sync::<TextBlock>();
        assert_send_sync::<ExitCode>();
    }

    #[test]
    fn test_concurrent_options_creation() {
        use std::thread;

        let handles: Vec<_> = (0..8)
            .map(|i| {
                thread::spawn(move || {
                    let pdf = PdfWriterOptions::builder().dpi(150 + i * 50).build();
                    let dsk = DeskewOptions::builder().max_angle(5.0 + i as f64).build();
                    let ext = ExtractOptions::builder().dpi(150 + i * 50).build();
                    (pdf.dpi, dsk.max_angle, ext.dpi)
                })
            })
            .collect();

        let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
        assert_eq!(results.len(), 8);
        for (i, (pdf_dpi, dsk_angle, ext_dpi)) in results.iter().enumerate() {
            assert_eq!(*pdf_dpi, 150 + (i as u32) * 50);
            assert!((dsk_angle - (5.0 + i as f64)).abs() < 0.01);
            assert_eq!(*ext_dpi, 150 + (i as u32) * 50);
        }
    }

    #[test]
    fn test_concurrent_preset_creation() {
        use rayon::prelude::*;

        let results: Vec<_> = (0..100)
            .into_par_iter()
            .map(|i| {
                let preset = match i % 6 {
                    0 => PdfWriterOptions::default(),
                    1 => PdfWriterOptions::high_quality(),
                    2 => PdfWriterOptions::compact(),
                    3 => PdfWriterOptions::builder()
                        .dpi(300)
                        .jpeg_quality(90)
                        .build(),
                    4 => PdfWriterOptions::builder()
                        .dpi(600)
                        .jpeg_quality(95)
                        .build(),
                    _ => PdfWriterOptions::builder().dpi(150).build(),
                };
                preset.dpi
            })
            .collect();

        assert_eq!(results.len(), 100);
    }

    #[test]
    fn test_concurrent_exit_code_handling() {
        use std::thread;

        let codes = vec![
            ExitCode::Success,
            ExitCode::GeneralError,
            ExitCode::InvalidArgs,
            ExitCode::InputNotFound,
            ExitCode::OutputError,
            ExitCode::ProcessingError,
            ExitCode::GpuError,
            ExitCode::ExternalToolError,
        ];

        let handles: Vec<_> = codes
            .into_iter()
            .map(|code| {
                thread::spawn(move || {
                    let value = code.code();
                    let desc = code.description();
                    (value, desc.to_string())
                })
            })
            .collect();

        let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
        assert_eq!(results.len(), 8);

        // Success should be 0
        assert!(results.iter().any(|(code, _)| *code == 0));
    }

    #[test]
    fn test_concurrent_margins_operations() {
        use rayon::prelude::*;

        let results: Vec<_> = (0..100)
            .into_par_iter()
            .map(|i| {
                let margins = Margins {
                    top: i as u32 * 10,
                    bottom: i as u32 * 10,
                    left: i as u32 * 5,
                    right: i as u32 * 5,
                };
                (margins.total_vertical(), margins.total_horizontal())
            })
            .collect();

        assert_eq!(results.len(), 100);
        for (i, (vert, horiz)) in results.iter().enumerate() {
            assert_eq!(*vert, (i as u32) * 20);
            assert_eq!(*horiz, (i as u32) * 10);
        }
    }

    // ============ Additional Integration Tests ============

    #[test]
    fn test_all_modules_have_consistent_builder_pattern() {
        // All builders should support build() without any required parameters
        let _pdf = PdfWriterOptions::builder().build();
        let _dsk = DeskewOptions::builder().build();
        let _ext = ExtractOptions::builder().build();
        let _mrg = MarginOptions::builder().build();
        let _pgn = PageNumberOptions::builder().build();
        let _res = RealEsrganOptions::builder().build();
        let _yomi = YomiTokuOptions::builder().build();
        let _ai = AiBridgeConfig::builder().build();
    }

    #[test]
    fn test_all_defaults_consistent_with_docs() {
        // Verify documented defaults match actual defaults

        // DPI defaults should be reasonable (150-600)
        let ext = ExtractOptions::default();
        assert!(ext.dpi >= 72 && ext.dpi <= 1200);

        let pdf = PdfWriterOptions::default();
        assert!(pdf.dpi >= 72 && pdf.dpi <= 1200);

        // JPEG quality should be valid (1-100)
        assert!(pdf.jpeg_quality >= 1 && pdf.jpeg_quality <= 100);

        // Deskew max angle should be reasonable (1-45 degrees)
        let dsk = DeskewOptions::default();
        assert!(dsk.max_angle >= 1.0 && dsk.max_angle <= 45.0);

        // Confidence thresholds should be 0-100
        let pgn = PageNumberOptions::default();
        assert!(pgn.min_confidence >= 0.0 && pgn.min_confidence <= 100.0);
    }

    #[test]
    fn test_colorspace_image_format_all_combinations() {
        let formats = [
            ImageFormat::Png,
            ImageFormat::Jpeg { quality: 90 },
            ImageFormat::Tiff,
            ImageFormat::Bmp,
        ];

        let colorspaces = [ColorSpace::Rgb, ColorSpace::Grayscale, ColorSpace::Cmyk];

        // All 12 combinations should be valid
        let mut count = 0;
        for format in &formats {
            for colorspace in &colorspaces {
                let _ = ExtractOptions::builder()
                    .format(*format)
                    .colorspace(*colorspace)
                    .build();
                count += 1;
            }
        }
        assert_eq!(count, 12);
    }

    #[test]
    fn test_unified_margins_from_margins() {
        let pages_margins = [
            Margins {
                top: 50,
                bottom: 60,
                left: 30,
                right: 40,
            },
            Margins {
                top: 55,
                bottom: 65,
                left: 35,
                right: 45,
            },
            Margins {
                top: 45,
                bottom: 55,
                left: 25,
                right: 35,
            },
        ];

        // Find unified margins (minimum of each)
        let unified = Margins {
            top: pages_margins.iter().map(|m| m.top).min().unwrap(),
            bottom: pages_margins.iter().map(|m| m.bottom).min().unwrap(),
            left: pages_margins.iter().map(|m| m.left).min().unwrap(),
            right: pages_margins.iter().map(|m| m.right).min().unwrap(),
        };

        assert_eq!(unified.top, 45);
        assert_eq!(unified.bottom, 55);
        assert_eq!(unified.left, 25);
        assert_eq!(unified.right, 35);
    }

    #[test]
    fn test_exit_code_all_descriptions_non_empty() {
        let codes = [
            ExitCode::Success,
            ExitCode::GeneralError,
            ExitCode::InvalidArgs,
            ExitCode::InputNotFound,
            ExitCode::OutputError,
            ExitCode::ProcessingError,
            ExitCode::GpuError,
            ExitCode::ExternalToolError,
        ];

        for code in codes {
            let desc = code.description();
            assert!(
                !desc.is_empty(),
                "Description for {:?} should not be empty",
                code
            );
        }
    }
}
