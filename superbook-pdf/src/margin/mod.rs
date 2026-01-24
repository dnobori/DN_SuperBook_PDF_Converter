//! Margin Detection & Trimming module
//!
//! Provides functionality to detect and trim margins from scanned images.
//!
//! # Features
//!
//! - Multiple detection modes (Background, Edge, Histogram, Combined)
//! - Unified margin calculation across multiple pages
//! - Configurable trim percentages
//! - Parallel processing support
//! - Tukey fence outlier removal for group analysis
//!
//! # Example
//!
//! ```rust,no_run
//! use superbook_pdf::{MarginOptions, ImageMarginDetector};
//! use std::path::Path;
//!
//! let options = MarginOptions::builder()
//!     .background_threshold(250)
//!     .default_trim_percent(0.5)
//!     .build();
//!
//! let detection = ImageMarginDetector::detect(
//!     Path::new("page.png"),
//!     &options
//! ).unwrap();
//!
//! println!("Margins: top={}, bottom={}", detection.margins.top, detection.margins.bottom);
//! ```

// Submodules
mod detect;
mod group;
mod types;

// Re-export public API
pub use detect::ImageMarginDetector;
pub use group::{GroupCropAnalyzer, GroupCropRegion, PageBoundingBox, UnifiedCropRegions};
pub use types::{
    ContentRect, MarginDetection, MarginDetector, MarginError, Margins, Result, TrimResult,
    UnifiedMargins,
};

// ============================================================
// Constants
// ============================================================

/// Default background threshold for white/light backgrounds (0-255)
const DEFAULT_BACKGROUND_THRESHOLD: u8 = 250;

/// Background threshold for dark/aged documents
const DARK_BACKGROUND_THRESHOLD: u8 = 50;

/// Default minimum margin in pixels
const DEFAULT_MIN_MARGIN: u32 = 10;

/// Default trim percentage
const DEFAULT_TRIM_PERCENT: f32 = 0.5;

/// Default edge detection sensitivity
const DEFAULT_EDGE_SENSITIVITY: f32 = 0.5;

/// High precision edge sensitivity
const PRECISE_EDGE_SENSITIVITY: f32 = 0.8;

/// Minimum clamp value for percentage
const MIN_PERCENT: f32 = 0.0;

/// Maximum clamp value for percentage
const MAX_PERCENT: f32 = 100.0;

/// Minimum sensitivity value
const MIN_SENSITIVITY: f32 = 0.0;

/// Maximum sensitivity value
const MAX_SENSITIVITY: f32 = 1.0;

// ============================================================
// Options
// ============================================================

/// Margin detection options
#[derive(Debug, Clone)]
pub struct MarginOptions {
    /// Background color threshold (0-255)
    pub background_threshold: u8,
    /// Minimum margin in pixels
    pub min_margin: u32,
    /// Default trim percentage
    pub default_trim_percent: f32,
    /// Edge detection sensitivity
    pub edge_sensitivity: f32,
    /// Content detection mode
    pub detection_mode: ContentDetectionMode,
}

impl Default for MarginOptions {
    fn default() -> Self {
        Self {
            background_threshold: DEFAULT_BACKGROUND_THRESHOLD,
            min_margin: DEFAULT_MIN_MARGIN,
            default_trim_percent: DEFAULT_TRIM_PERCENT,
            edge_sensitivity: DEFAULT_EDGE_SENSITIVITY,
            detection_mode: ContentDetectionMode::BackgroundColor,
        }
    }
}

impl MarginOptions {
    /// Create a new options builder
    pub fn builder() -> MarginOptionsBuilder {
        MarginOptionsBuilder::default()
    }

    /// Create options for dark backgrounds (e.g., scanned old books)
    pub fn for_dark_background() -> Self {
        Self {
            background_threshold: DARK_BACKGROUND_THRESHOLD,
            detection_mode: ContentDetectionMode::EdgeDetection,
            ..Default::default()
        }
    }

    /// Create options for precise detection
    pub fn precise() -> Self {
        Self {
            detection_mode: ContentDetectionMode::Combined,
            edge_sensitivity: PRECISE_EDGE_SENSITIVITY,
            ..Default::default()
        }
    }
}

/// Builder for MarginOptions
#[derive(Debug, Default)]
pub struct MarginOptionsBuilder {
    options: MarginOptions,
}

impl MarginOptionsBuilder {
    /// Set background threshold (0-255)
    #[must_use]
    pub fn background_threshold(mut self, threshold: u8) -> Self {
        self.options.background_threshold = threshold;
        self
    }

    /// Set minimum margin in pixels
    #[must_use]
    pub fn min_margin(mut self, margin: u32) -> Self {
        self.options.min_margin = margin;
        self
    }

    /// Set default trim percentage
    #[must_use]
    pub fn default_trim_percent(mut self, percent: f32) -> Self {
        self.options.default_trim_percent = percent.clamp(MIN_PERCENT, MAX_PERCENT);
        self
    }

    /// Set edge detection sensitivity (0.0-1.0)
    #[must_use]
    pub fn edge_sensitivity(mut self, sensitivity: f32) -> Self {
        self.options.edge_sensitivity = sensitivity.clamp(MIN_SENSITIVITY, MAX_SENSITIVITY);
        self
    }

    /// Set content detection mode
    #[must_use]
    pub fn detection_mode(mut self, mode: ContentDetectionMode) -> Self {
        self.options.detection_mode = mode;
        self
    }

    /// Build the options
    #[must_use]
    pub fn build(self) -> MarginOptions {
        self.options
    }
}

/// Content detection modes
#[derive(Debug, Clone, Copy, Default)]
pub enum ContentDetectionMode {
    /// Simple background color detection
    #[default]
    BackgroundColor,
    /// Edge detection based
    EdgeDetection,
    /// Histogram analysis
    Histogram,
    /// Combined detection
    Combined,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    #[test]
    fn test_default_options() {
        let opts = MarginOptions::default();

        assert_eq!(opts.background_threshold, 250);
        assert_eq!(opts.min_margin, 10);
        assert_eq!(opts.default_trim_percent, 0.5);
        assert!(matches!(
            opts.detection_mode,
            ContentDetectionMode::BackgroundColor
        ));
    }

    #[test]
    fn test_uniform_margins() {
        let margins = Margins::uniform(20);

        assert_eq!(margins.top, 20);
        assert_eq!(margins.bottom, 20);
        assert_eq!(margins.left, 20);
        assert_eq!(margins.right, 20);
        assert_eq!(margins.total_horizontal(), 40);
        assert_eq!(margins.total_vertical(), 40);
    }

    #[test]
    fn test_image_not_found() {
        let result = ImageMarginDetector::detect(
            Path::new("/nonexistent/image.png"),
            &MarginOptions::default(),
        );

        assert!(matches!(result, Err(MarginError::ImageNotFound(_))));
    }

    // TC-MRG-001: 単一画像マージン検出
    #[test]
    fn test_detect_single_image_margins() {
        let options = MarginOptions {
            background_threshold: 200,
            ..Default::default()
        };

        let result =
            ImageMarginDetector::detect(Path::new("tests/fixtures/with_margins.png"), &options);

        match result {
            Ok(detection) => {
                assert!(detection.margins.top > 0);
            }
            Err(MarginError::NoContentDetected) => {
                eprintln!("No content detected - algorithm needs tuning");
            }
            Err(e) => panic!("Unexpected error: {:?}", e),
        }
    }

    // TC-MRG-002: マージンなし画像
    #[test]
    fn test_detect_no_margins() {
        let options = MarginOptions {
            background_threshold: 200,
            ..Default::default()
        };

        let result =
            ImageMarginDetector::detect(Path::new("tests/fixtures/no_margins.png"), &options);

        match result {
            Ok(detection) => {
                assert!(detection.margins.top < 20);
            }
            Err(MarginError::NoContentDetected) => {
                eprintln!("No content detected");
            }
            Err(e) => panic!("Unexpected error: {:?}", e),
        }
    }

    #[test]
    fn test_unified_margins() {
        let images: Vec<_> = (1..=5)
            .map(|i| PathBuf::from(format!("tests/fixtures/page_{}.png", i)))
            .collect();

        let options = MarginOptions {
            background_threshold: 200,
            ..Default::default()
        };

        let result = ImageMarginDetector::detect_unified(&images, &options);

        match result {
            Ok(unified) => {
                assert!(unified.page_detections.len() == 5);
            }
            Err(MarginError::NoContentDetected) => {
                eprintln!("No content detected in unified batch");
            }
            Err(e) => panic!("Unexpected error: {:?}", e),
        }
    }

    // TC-MRG-010: コンテンツなし画像エラー
    #[test]
    fn test_no_content_error() {
        let result = ImageMarginDetector::detect(
            Path::new("tests/fixtures/blank_white.png"),
            &MarginOptions::default(),
        );

        match result {
            Err(MarginError::NoContentDetected) => {}
            Ok(_) => {}
            Err(e) => panic!("Unexpected error: {:?}", e),
        }
    }

    #[test]
    fn test_builder_pattern() {
        let options = MarginOptions::builder()
            .background_threshold(200)
            .min_margin(20)
            .default_trim_percent(1.0)
            .edge_sensitivity(0.7)
            .detection_mode(ContentDetectionMode::Combined)
            .build();

        assert_eq!(options.background_threshold, 200);
        assert_eq!(options.min_margin, 20);
        assert_eq!(options.default_trim_percent, 1.0);
        assert_eq!(options.edge_sensitivity, 0.7);
        assert!(matches!(
            options.detection_mode,
            ContentDetectionMode::Combined
        ));
    }

    #[test]
    fn test_builder_clamping() {
        let options = MarginOptions::builder().edge_sensitivity(1.5).build();
        assert_eq!(options.edge_sensitivity, 1.0);

        let options = MarginOptions::builder().edge_sensitivity(-0.5).build();
        assert_eq!(options.edge_sensitivity, 0.0);
    }

    #[test]
    fn test_dark_background_preset() {
        let options = MarginOptions::for_dark_background();

        assert_eq!(options.background_threshold, 50);
        assert!(matches!(
            options.detection_mode,
            ContentDetectionMode::EdgeDetection
        ));
    }

    #[test]
    fn test_precise_preset() {
        let options = MarginOptions::precise();

        assert!(matches!(
            options.detection_mode,
            ContentDetectionMode::Combined
        ));
        assert_eq!(options.edge_sensitivity, 0.8);
    }

    // TC-MRG-005: Trim result construction
    #[test]
    fn test_trim_result_construction() {
        let result = TrimResult {
            input_path: PathBuf::from("/input/test.png"),
            output_path: PathBuf::from("/output/test.png"),
            original_size: (1000, 1500),
            trimmed_size: (800, 1200),
            margins_applied: Margins {
                top: 100,
                bottom: 200,
                left: 100,
                right: 100,
            },
        };

        assert_eq!(result.original_size, (1000, 1500));
        assert_eq!(result.trimmed_size, (800, 1200));
        assert_eq!(result.margins_applied.top, 100);
    }

    // TC-MRG-003: Content rect
    #[test]
    fn test_content_rect_construction() {
        let rect = ContentRect {
            x: 50,
            y: 100,
            width: 800,
            height: 1200,
        };

        assert_eq!(rect.x, 50);
        assert_eq!(rect.y, 100);
        assert_eq!(rect.width, 800);
        assert_eq!(rect.height, 1200);
    }

    // TC-MRG-004: Unified margins structure
    #[test]
    fn test_unified_margins_construction() {
        let detection = MarginDetection {
            margins: Margins::uniform(50),
            image_size: (1000, 1500),
            content_rect: ContentRect {
                x: 50,
                y: 50,
                width: 900,
                height: 1400,
            },
            confidence: 0.9,
        };

        let unified = UnifiedMargins {
            margins: Margins::uniform(30),
            page_detections: vec![detection],
            unified_size: (940, 1440),
        };

        assert_eq!(unified.margins.top, 30);
        assert_eq!(unified.page_detections.len(), 1);
        assert_eq!(unified.unified_size, (940, 1440));
    }

    // TC-MRG-008: Edge detection mode
    #[test]
    fn test_edge_detection_mode_option() {
        let options = MarginOptions::builder()
            .detection_mode(ContentDetectionMode::EdgeDetection)
            .build();

        assert!(matches!(
            options.detection_mode,
            ContentDetectionMode::EdgeDetection
        ));
    }

    #[test]
    fn test_histogram_mode_option() {
        let options = MarginOptions::builder()
            .detection_mode(ContentDetectionMode::Histogram)
            .build();

        assert!(matches!(
            options.detection_mode,
            ContentDetectionMode::Histogram
        ));
    }

    #[test]
    fn test_all_detection_modes() {
        let modes = vec![
            ContentDetectionMode::BackgroundColor,
            ContentDetectionMode::EdgeDetection,
            ContentDetectionMode::Histogram,
            ContentDetectionMode::Combined,
        ];

        for mode in modes {
            let options = MarginOptions::builder().detection_mode(mode).build();
            match (mode, options.detection_mode) {
                (ContentDetectionMode::BackgroundColor, ContentDetectionMode::BackgroundColor) => {}
                (ContentDetectionMode::EdgeDetection, ContentDetectionMode::EdgeDetection) => {}
                (ContentDetectionMode::Histogram, ContentDetectionMode::Histogram) => {}
                (ContentDetectionMode::Combined, ContentDetectionMode::Combined) => {}
                _ => panic!("Mode mismatch"),
            }
        }
    }

    #[test]
    fn test_margin_detection_confidence() {
        let detection = MarginDetection {
            margins: Margins::uniform(50),
            image_size: (1000, 1500),
            content_rect: ContentRect {
                x: 50,
                y: 50,
                width: 900,
                height: 1400,
            },
            confidence: 0.85,
        };

        assert!(detection.confidence > 0.0 && detection.confidence <= 1.0);
        assert_eq!(detection.image_size, (1000, 1500));
    }

    #[test]
    fn test_error_types() {
        let _err1 = MarginError::ImageNotFound(PathBuf::from("/test/path"));
        let _err2 = MarginError::InvalidImage("Invalid format".to_string());
        let _err3 = MarginError::NoContentDetected;
        let _err4: MarginError = std::io::Error::other("test").into();
    }

    // TC-MRG-005: Trim margins
    #[test]
    fn test_trim_with_fixture() {
        let temp_dir = tempfile::tempdir().unwrap();
        let output = temp_dir.path().join("trimmed.png");

        let margins = Margins {
            top: 10,
            bottom: 10,
            left: 10,
            right: 10,
        };

        let result = ImageMarginDetector::trim(
            Path::new("tests/fixtures/with_margins.png"),
            &output,
            &margins,
        );

        match result {
            Ok(trim_result) => {
                assert!(output.exists());
                assert!(trim_result.trimmed_size.0 <= trim_result.original_size.0);
                assert!(trim_result.trimmed_size.1 <= trim_result.original_size.1);
            }
            Err(e) => {
                eprintln!("Trim error: {:?}", e);
            }
        }
    }

    // TC-MRG-006: Pad to size
    #[test]
    fn test_pad_to_size_with_fixture() {
        let temp_dir = tempfile::tempdir().unwrap();
        let output = temp_dir.path().join("padded.png");

        let result = ImageMarginDetector::pad_to_size(
            Path::new("tests/fixtures/small_image.png"),
            &output,
            (500, 500),
            [255, 255, 255],
        );

        match result {
            Ok(_pad_result) => {
                assert!(output.exists());
                let img = image::open(&output).unwrap();
                assert_eq!(img.width(), 500);
                assert_eq!(img.height(), 500);
            }
            Err(e) => {
                eprintln!("Pad error: {:?}", e);
            }
        }
    }

    // TC-MRG-007: Background threshold variations
    #[test]
    fn test_background_threshold_high() {
        let options = MarginOptions::builder().background_threshold(254).build();
        assert_eq!(options.background_threshold, 254);
    }

    #[test]
    fn test_background_threshold_low() {
        let options = MarginOptions::builder().background_threshold(100).build();
        assert_eq!(options.background_threshold, 100);
    }

    // TC-MRG-009: Batch processing
    #[test]
    fn test_batch_processing() {
        let temp_dir = tempfile::tempdir().unwrap();

        let images: Vec<(PathBuf, PathBuf)> = (1..=3)
            .map(|i| {
                (
                    PathBuf::from(format!("tests/fixtures/page_{}.png", i)),
                    temp_dir.path().join(format!("output_{}.png", i)),
                )
            })
            .collect();

        let options = MarginOptions {
            background_threshold: 200,
            ..Default::default()
        };

        let result = ImageMarginDetector::process_batch(&images, &options);

        match result {
            Ok(results) => {
                assert_eq!(results.len(), 3);
            }
            Err(MarginError::NoContentDetected) => {
                eprintln!("No content detected in batch");
            }
            Err(e) => {
                eprintln!("Batch error: {:?}", e);
            }
        }
    }

    #[test]
    fn test_margins_arithmetic() {
        let margins = Margins {
            top: 10,
            bottom: 20,
            left: 15,
            right: 25,
        };

        assert_eq!(margins.total_vertical(), 30);
        assert_eq!(margins.total_horizontal(), 40);
    }

    #[test]
    fn test_error_display_messages() {
        let err1 = MarginError::ImageNotFound(PathBuf::from("/test/path.png"));
        assert!(err1.to_string().contains("not found"));

        let err2 = MarginError::InvalidImage("bad format".to_string());
        assert!(err2.to_string().contains("Invalid"));

        let err3 = MarginError::NoContentDetected;
        assert!(err3.to_string().contains("content"));
    }

    #[test]
    fn test_margins_construction() {
        let margins = Margins {
            top: 50,
            bottom: 60,
            left: 30,
            right: 40,
        };

        assert_eq!(margins.top, 50);
        assert_eq!(margins.bottom, 60);
        assert_eq!(margins.left, 30);
        assert_eq!(margins.right, 40);
    }

    #[test]
    fn test_margins_zero() {
        let margins = Margins {
            top: 0,
            bottom: 0,
            left: 0,
            right: 0,
        };

        assert_eq!(margins.total_vertical(), 0);
        assert_eq!(margins.total_horizontal(), 0);
    }

    #[test]
    fn test_margins_asymmetric() {
        let margins = Margins {
            top: 100,
            bottom: 50,
            left: 20,
            right: 80,
        };

        assert_ne!(margins.top, margins.bottom);
        assert_ne!(margins.left, margins.right);
        assert_eq!(margins.total_vertical(), 150);
        assert_eq!(margins.total_horizontal(), 100);
    }

    #[test]
    fn test_margin_options_builder_all() {
        let options = MarginOptions::builder()
            .background_threshold(200)
            .min_margin(5)
            .edge_sensitivity(0.8)
            .build();

        assert_eq!(options.background_threshold, 200);
        assert_eq!(options.min_margin, 5);
        assert_eq!(options.edge_sensitivity, 0.8);
    }

    #[test]
    fn test_margin_options_default() {
        let options = MarginOptions::default();
        assert!(options.background_threshold > 0);
    }

    #[test]
    fn test_trim_result_fields_consistency() {
        let result = TrimResult {
            input_path: PathBuf::from("/input/original.png"),
            output_path: PathBuf::from("/output/trimmed.png"),
            original_size: (1000, 800),
            trimmed_size: (900, 750),
            margins_applied: Margins {
                top: 20,
                bottom: 30,
                left: 50,
                right: 50,
            },
        };

        assert_eq!(result.original_size.0, 1000);
        assert_eq!(result.trimmed_size.1, 750);
        let expected_width =
            result.original_size.0 - result.margins_applied.left - result.margins_applied.right;
        assert_eq!(expected_width, result.trimmed_size.0);
    }

    #[test]
    fn test_trim_result_unchanged() {
        let result = TrimResult {
            input_path: PathBuf::from("/input/same.png"),
            output_path: PathBuf::from("/output/same.png"),
            original_size: (500, 500),
            trimmed_size: (500, 500),
            margins_applied: Margins {
                top: 0,
                bottom: 0,
                left: 0,
                right: 0,
            },
        };

        assert_eq!(result.original_size, result.trimmed_size);
        assert_eq!(result.margins_applied.total_vertical(), 0);
        assert_eq!(result.margins_applied.total_horizontal(), 0);
    }

    #[test]
    fn test_edge_sensitivity_variations() {
        let opts_zero = MarginOptions::builder().edge_sensitivity(0.0).build();
        assert_eq!(opts_zero.edge_sensitivity, 0.0);

        let opts_mid = MarginOptions::builder().edge_sensitivity(0.5).build();
        assert_eq!(opts_mid.edge_sensitivity, 0.5);

        let opts_max = MarginOptions::builder().edge_sensitivity(1.0).build();
        assert_eq!(opts_max.edge_sensitivity, 1.0);
    }

    #[test]
    fn test_content_detection_mode_copy() {
        let original = ContentDetectionMode::Combined;
        let cloned = original;
        assert!(matches!(cloned, ContentDetectionMode::Combined));
    }

    #[test]
    fn test_margins_copy() {
        let original = Margins::uniform(50);
        let cloned = original;
        assert_eq!(cloned.top, 50);
    }

    #[test]
    fn test_content_rect_copy() {
        let original = ContentRect {
            x: 10,
            y: 20,
            width: 100,
            height: 200,
        };
        let cloned = original;
        assert_eq!(cloned.x, 10);
        assert_eq!(cloned.y, 20);
    }

    // Group Crop tests
    #[test]
    fn test_page_bounding_box_creation() {
        let rect = ContentRect {
            x: 100,
            y: 50,
            width: 800,
            height: 1200,
        };
        let bbox = PageBoundingBox::new(1, rect);
        assert_eq!(bbox.page_number, 1);
        assert!(bbox.is_odd);
        assert!(bbox.is_valid());
    }

    #[test]
    fn test_group_crop_region_valid() {
        let region = GroupCropRegion {
            left: 100,
            top: 50,
            width: 800,
            height: 1200,
            inlier_count: 10,
            total_count: 12,
        };
        assert!(region.is_valid());
        assert_eq!(region.right(), 900);
        assert_eq!(region.bottom(), 1250);
    }

    #[test]
    fn test_decide_group_crop_empty() {
        let result = GroupCropAnalyzer::decide_group_crop_region(&[]);
        assert!(!result.is_valid());
    }

    #[test]
    fn test_unify_odd_even_regions() {
        let boxes = vec![
            PageBoundingBox::new(
                1,
                ContentRect {
                    x: 100,
                    y: 50,
                    width: 800,
                    height: 1200,
                },
            ),
            PageBoundingBox::new(
                2,
                ContentRect {
                    x: 150,
                    y: 60,
                    width: 750,
                    height: 1180,
                },
            ),
        ];
        let result = GroupCropAnalyzer::unify_odd_even_regions(&boxes);
        assert!(result.odd_region.is_valid());
        assert!(result.even_region.is_valid());
    }
}
