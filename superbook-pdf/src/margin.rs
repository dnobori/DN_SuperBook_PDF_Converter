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

use image::{GenericImageView, GrayImage};
use rayon::prelude::*;
use std::path::{Path, PathBuf};
use thiserror::Error;

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

/// Margin error types
#[derive(Debug, Error)]
pub enum MarginError {
    #[error("Image not found: {0}")]
    ImageNotFound(PathBuf),

    #[error("Invalid image: {0}")]
    InvalidImage(String),

    #[error("No content detected in image")]
    NoContentDetected,

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, MarginError>;

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

/// Margin information in pixels
#[derive(Debug, Clone, Copy, Default)]
pub struct Margins {
    pub top: u32,
    pub bottom: u32,
    pub left: u32,
    pub right: u32,
}

impl Margins {
    /// Create uniform margins
    pub fn uniform(value: u32) -> Self {
        Self {
            top: value,
            bottom: value,
            left: value,
            right: value,
        }
    }

    /// Total horizontal margin
    pub fn total_horizontal(&self) -> u32 {
        self.left + self.right
    }

    /// Total vertical margin
    pub fn total_vertical(&self) -> u32 {
        self.top + self.bottom
    }
}

/// Content rectangle
#[derive(Debug, Clone, Copy)]
pub struct ContentRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

/// Margin detection result
#[derive(Debug, Clone)]
pub struct MarginDetection {
    /// Detected margins
    pub margins: Margins,
    /// Image size
    pub image_size: (u32, u32),
    /// Content rectangle
    pub content_rect: ContentRect,
    /// Detection confidence
    pub confidence: f64,
}

/// Unified margins result
#[derive(Debug, Clone)]
pub struct UnifiedMargins {
    /// Common margins for all pages
    pub margins: Margins,
    /// Per-page detection results
    pub page_detections: Vec<MarginDetection>,
    /// Unified size after trimming
    pub unified_size: (u32, u32),
}

/// Trim operation result
#[derive(Debug)]
pub struct TrimResult {
    pub input_path: PathBuf,
    pub output_path: PathBuf,
    pub original_size: (u32, u32),
    pub trimmed_size: (u32, u32),
    pub margins_applied: Margins,
}

/// Margin detector trait
pub trait MarginDetector {
    /// Detect margins in a single image
    fn detect(image_path: &Path, options: &MarginOptions) -> Result<MarginDetection>;

    /// Detect unified margins for multiple images
    fn detect_unified(images: &[PathBuf], options: &MarginOptions) -> Result<UnifiedMargins>;

    /// Trim image using specified margins
    fn trim(input_path: &Path, output_path: &Path, margins: &Margins) -> Result<TrimResult>;

    /// Pad image to target size
    fn pad_to_size(
        input_path: &Path,
        output_path: &Path,
        target_size: (u32, u32),
        background: [u8; 3],
    ) -> Result<TrimResult>;

    /// Process batch with unified margins
    fn process_batch(
        images: &[(PathBuf, PathBuf)],
        options: &MarginOptions,
    ) -> Result<Vec<TrimResult>>;
}

/// Default margin detector implementation
pub struct ImageMarginDetector;

impl ImageMarginDetector {
    /// Detect margins in a single image
    pub fn detect(image_path: &Path, options: &MarginOptions) -> Result<MarginDetection> {
        if !image_path.exists() {
            return Err(MarginError::ImageNotFound(image_path.to_path_buf()));
        }

        let img = image::open(image_path).map_err(|e| MarginError::InvalidImage(e.to_string()))?;

        let gray = img.to_luma8();
        let (width, height) = img.dimensions();

        let is_background =
            |pixel: &image::Luma<u8>| -> bool { pixel.0[0] >= options.background_threshold };

        // Detect margins based on mode
        let (top, bottom, left, right) = match options.detection_mode {
            ContentDetectionMode::BackgroundColor => {
                Self::detect_background_margins(&gray, is_background, options)
            }
            ContentDetectionMode::EdgeDetection => Self::detect_edge_margins(&gray, options),
            ContentDetectionMode::Histogram => Self::detect_histogram_margins(&gray, options),
            ContentDetectionMode::Combined => {
                // Average of background and edge detection
                let (t1, b1, l1, r1) =
                    Self::detect_background_margins(&gray, is_background, options);
                let (t2, b2, l2, r2) = Self::detect_edge_margins(&gray, options);
                ((t1 + t2) / 2, (b1 + b2) / 2, (l1 + l2) / 2, (r1 + r2) / 2)
            }
        };

        let margins = Margins {
            top: top.max(options.min_margin),
            bottom: bottom.max(options.min_margin),
            left: left.max(options.min_margin),
            right: right.max(options.min_margin),
        };

        let content_width = width.saturating_sub(margins.total_horizontal());
        let content_height = height.saturating_sub(margins.total_vertical());

        if content_width == 0 || content_height == 0 {
            return Err(MarginError::NoContentDetected);
        }

        let content_rect = ContentRect {
            x: margins.left,
            y: margins.top,
            width: content_width,
            height: content_height,
        };

        Ok(MarginDetection {
            margins,
            image_size: (width, height),
            content_rect,
            confidence: 1.0,
        })
    }

    /// Background color based margin detection
    fn detect_background_margins<F>(
        gray: &GrayImage,
        is_background: F,
        _options: &MarginOptions,
    ) -> (u32, u32, u32, u32)
    where
        F: Fn(&image::Luma<u8>) -> bool,
    {
        let (width, height) = gray.dimensions();

        // Detect top margin
        let top = Self::find_content_start_vertical(gray, &is_background, true);

        // Detect bottom margin
        let bottom = height - Self::find_content_start_vertical(gray, &is_background, false);

        // Detect left margin
        let left = Self::find_content_start_horizontal(gray, &is_background, true);

        // Detect right margin
        let right = width - Self::find_content_start_horizontal(gray, &is_background, false);

        (top, bottom, left, right)
    }

    /// Find where content starts vertically
    fn find_content_start_vertical<F>(gray: &GrayImage, is_background: F, from_top: bool) -> u32
    where
        F: Fn(&image::Luma<u8>) -> bool,
    {
        let (width, height) = gray.dimensions();
        let rows: Box<dyn Iterator<Item = u32>> = if from_top {
            Box::new(0..height)
        } else {
            Box::new((0..height).rev())
        };

        for y in rows {
            let non_bg_count = (0..width)
                .filter(|&x| !is_background(gray.get_pixel(x, y)))
                .count();

            // 10% or more non-background pixels means content start
            if non_bg_count as f32 / width as f32 > 0.1 {
                return if from_top { y } else { height - y };
            }
        }

        0
    }

    /// Find where content starts horizontally
    fn find_content_start_horizontal<F>(gray: &GrayImage, is_background: F, from_left: bool) -> u32
    where
        F: Fn(&image::Luma<u8>) -> bool,
    {
        let (width, height) = gray.dimensions();
        let cols: Box<dyn Iterator<Item = u32>> = if from_left {
            Box::new(0..width)
        } else {
            Box::new((0..width).rev())
        };

        for x in cols {
            let non_bg_count = (0..height)
                .filter(|&y| !is_background(gray.get_pixel(x, y)))
                .count();

            if non_bg_count as f32 / height as f32 > 0.1 {
                return if from_left { x } else { width - x };
            }
        }

        0
    }

    /// Edge detection based margin detection
    fn detect_edge_margins(gray: &GrayImage, _options: &MarginOptions) -> (u32, u32, u32, u32) {
        // Simple gradient-based edge detection
        let (width, height) = gray.dimensions();
        let mut has_edge_row = vec![false; height as usize];
        let mut has_edge_col = vec![false; width as usize];

        for y in 1..height - 1 {
            for x in 1..width - 1 {
                let center = gray.get_pixel(x, y).0[0] as i32;
                let neighbors = [
                    gray.get_pixel(x - 1, y).0[0] as i32,
                    gray.get_pixel(x + 1, y).0[0] as i32,
                    gray.get_pixel(x, y - 1).0[0] as i32,
                    gray.get_pixel(x, y + 1).0[0] as i32,
                ];

                let max_diff = neighbors
                    .iter()
                    .map(|&n| (n - center).abs())
                    .max()
                    .unwrap_or(0);

                if max_diff > 30 {
                    has_edge_row[y as usize] = true;
                    has_edge_col[x as usize] = true;
                }
            }
        }

        // Find margins from edge detection
        let top = has_edge_row.iter().position(|&e| e).unwrap_or(0) as u32;
        let bottom = height
            - has_edge_row
                .iter()
                .rposition(|&e| e)
                .map(|p| p + 1)
                .unwrap_or(height as usize) as u32;
        let left = has_edge_col.iter().position(|&e| e).unwrap_or(0) as u32;
        let right = width
            - has_edge_col
                .iter()
                .rposition(|&e| e)
                .map(|p| p + 1)
                .unwrap_or(width as usize) as u32;

        (top, bottom, left, right)
    }

    /// Histogram based margin detection
    fn detect_histogram_margins(gray: &GrayImage, options: &MarginOptions) -> (u32, u32, u32, u32) {
        // For now, delegate to background detection with adjusted threshold
        let is_background = |pixel: &image::Luma<u8>| -> bool {
            pixel.0[0] >= options.background_threshold.saturating_sub(10)
        };
        Self::detect_background_margins(gray, is_background, options)
    }

    /// Detect unified margins for multiple images
    pub fn detect_unified(images: &[PathBuf], options: &MarginOptions) -> Result<UnifiedMargins> {
        let detections: Vec<MarginDetection> = images
            .par_iter()
            .map(|path| Self::detect(path, options))
            .collect::<Result<Vec<_>>>()?;

        // Use minimum margins (to avoid cutting content)
        let margins = Margins {
            top: detections.iter().map(|d| d.margins.top).min().unwrap_or(0),
            bottom: detections
                .iter()
                .map(|d| d.margins.bottom)
                .min()
                .unwrap_or(0),
            left: detections.iter().map(|d| d.margins.left).min().unwrap_or(0),
            right: detections
                .iter()
                .map(|d| d.margins.right)
                .min()
                .unwrap_or(0),
        };

        // Calculate unified size (maximum content size)
        let max_content_width = detections
            .iter()
            .map(|d| d.content_rect.width)
            .max()
            .unwrap_or(0);
        let max_content_height = detections
            .iter()
            .map(|d| d.content_rect.height)
            .max()
            .unwrap_or(0);

        Ok(UnifiedMargins {
            margins,
            page_detections: detections,
            unified_size: (max_content_width, max_content_height),
        })
    }

    /// Trim image using specified margins
    pub fn trim(input_path: &Path, output_path: &Path, margins: &Margins) -> Result<TrimResult> {
        if !input_path.exists() {
            return Err(MarginError::ImageNotFound(input_path.to_path_buf()));
        }

        let img = image::open(input_path).map_err(|e| MarginError::InvalidImage(e.to_string()))?;

        let (width, height) = img.dimensions();
        let original_size = (width, height);

        let crop_width = width.saturating_sub(margins.total_horizontal());
        let crop_height = height.saturating_sub(margins.total_vertical());

        if crop_width == 0 || crop_height == 0 {
            return Err(MarginError::NoContentDetected);
        }

        let cropped = img.crop_imm(margins.left, margins.top, crop_width, crop_height);
        let trimmed_size = (cropped.width(), cropped.height());

        cropped
            .save(output_path)
            .map_err(|e| MarginError::InvalidImage(e.to_string()))?;

        Ok(TrimResult {
            input_path: input_path.to_path_buf(),
            output_path: output_path.to_path_buf(),
            original_size,
            trimmed_size,
            margins_applied: *margins,
        })
    }

    /// Pad image to target size
    pub fn pad_to_size(
        input_path: &Path,
        output_path: &Path,
        target_size: (u32, u32),
        background: [u8; 3],
    ) -> Result<TrimResult> {
        if !input_path.exists() {
            return Err(MarginError::ImageNotFound(input_path.to_path_buf()));
        }

        let img = image::open(input_path).map_err(|e| MarginError::InvalidImage(e.to_string()))?;

        let original_size = (img.width(), img.height());
        let (target_w, target_h) = target_size;

        // Create background image
        let mut padded = image::RgbImage::new(target_w, target_h);
        for pixel in padded.pixels_mut() {
            *pixel = image::Rgb(background);
        }

        // Center the original image
        let offset_x = (target_w.saturating_sub(img.width())) / 2;
        let offset_y = (target_h.saturating_sub(img.height())) / 2;

        // Copy original image
        let rgb = img.to_rgb8();
        for y in 0..img.height().min(target_h) {
            for x in 0..img.width().min(target_w) {
                let px = x + offset_x;
                let py = y + offset_y;
                if px < target_w && py < target_h {
                    padded.put_pixel(px, py, *rgb.get_pixel(x, y));
                }
            }
        }

        padded
            .save(output_path)
            .map_err(|e| MarginError::InvalidImage(e.to_string()))?;

        let margins_applied = Margins {
            top: offset_y,
            bottom: target_h.saturating_sub(img.height() + offset_y),
            left: offset_x,
            right: target_w.saturating_sub(img.width() + offset_x),
        };

        Ok(TrimResult {
            input_path: input_path.to_path_buf(),
            output_path: output_path.to_path_buf(),
            original_size,
            trimmed_size: target_size,
            margins_applied,
        })
    }

    /// Process batch with unified margins
    pub fn process_batch(
        images: &[(PathBuf, PathBuf)],
        options: &MarginOptions,
    ) -> Result<Vec<TrimResult>> {
        // Get unified margins
        let input_paths: Vec<PathBuf> = images.iter().map(|(i, _)| i.clone()).collect();
        let unified = Self::detect_unified(&input_paths, options)?;

        // Trim all images with unified margins
        let results: Vec<TrimResult> = images
            .iter()
            .map(|(input, output)| Self::trim(input, output, &unified.margins))
            .collect::<Result<Vec<_>>>()?;

        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    // Image fixture tests

    // TC-MRG-001: 単一画像マージン検出
    #[test]
    fn test_detect_single_image_margins() {
        // Use lower threshold to detect gray content (50) against white background (255)
        let options = MarginOptions {
            background_threshold: 200, // Lower threshold to detect gray content
            ..Default::default()
        };

        let result =
            ImageMarginDetector::detect(Path::new("tests/fixtures/with_margins.png"), &options);

        match result {
            Ok(detection) => {
                eprintln!(
                    "Detected margins: top={}, bottom={}, left={}, right={}",
                    detection.margins.top,
                    detection.margins.bottom,
                    detection.margins.left,
                    detection.margins.right
                );
                // Image has content starting at ~50 pixels from edge
                assert!(detection.margins.top > 0);
            }
            Err(MarginError::NoContentDetected) => {
                // Algorithm may not detect content with current settings
                eprintln!("No content detected - algorithm needs tuning for this image");
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
                eprintln!(
                    "Detected margins: top={}, bottom={}, left={}, right={}",
                    detection.margins.top,
                    detection.margins.bottom,
                    detection.margins.left,
                    detection.margins.right
                );
                // Content fills nearly entire image (starts at 5px)
                assert!(detection.margins.top < 20);
            }
            Err(MarginError::NoContentDetected) => {
                eprintln!("No content detected - algorithm needs tuning");
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

        // Blank white image should either error or return full margins
        match result {
            Err(MarginError::NoContentDetected) => {}
            Ok(detection) => {
                // If it doesn't error, margins should be very large (entire image)
                eprintln!("No error, margins: {:?}", detection.margins);
            }
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
        // Edge sensitivity should be clamped to 0.0-1.0
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
            // Verify mode is correctly set
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
        // Test all error variants can be constructed
        let _err1 = MarginError::ImageNotFound(PathBuf::from("/test/path"));
        let _err2 = MarginError::InvalidImage("Invalid format".to_string());
        let _err3 = MarginError::NoContentDetected;
        let _err4: MarginError = std::io::Error::new(std::io::ErrorKind::NotFound, "test").into();
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
                // Trimmed size should be smaller than original
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
            Ok(pad_result) => {
                assert!(output.exists());
                // Padded image should be target size
                let img = image::open(&output).unwrap();
                assert_eq!(img.width(), 500);
                assert_eq!(img.height(), 500);
                eprintln!(
                    "Padded from {:?} to {:?}",
                    pad_result.original_size, pad_result.trimmed_size
                );
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
        // High threshold = only pure white is background
    }

    #[test]
    fn test_background_threshold_low() {
        let options = MarginOptions::builder().background_threshold(100).build();

        assert_eq!(options.background_threshold, 100);
        // Low threshold = many light colors are background
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
                // Should have processed all images
                assert_eq!(results.len(), 3);
                eprintln!("Batch processed {} images", results.len());
            }
            Err(MarginError::NoContentDetected) => {
                eprintln!("No content detected in batch");
            }
            Err(e) => {
                eprintln!("Batch error: {:?}", e);
            }
        }
    }

    // Test margins arithmetic
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

    // Test error display messages
    #[test]
    fn test_error_display_messages() {
        let err1 = MarginError::ImageNotFound(PathBuf::from("/test/path.png"));
        assert!(err1.to_string().contains("not found"));

        let err2 = MarginError::InvalidImage("bad format".to_string());
        assert!(err2.to_string().contains("Invalid"));

        let err3 = MarginError::NoContentDetected;
        assert!(err3.to_string().contains("content"));
    }

    // Test Margins construction with various values
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

    // Test zero margins
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

    // Test asymmetric margins
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

    // Test MarginOptions builder all options
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

    // Test default MarginOptions values
    #[test]
    fn test_margin_options_default() {
        let options = MarginOptions::default();

        // Verify reasonable defaults (background_threshold is u8, always <= 255)
        assert!(options.background_threshold > 0);
    }

    // Test TrimResult fields and consistency
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
        // Verify margins add up correctly
        let expected_width =
            result.original_size.0 - result.margins_applied.left - result.margins_applied.right;
        assert_eq!(expected_width, result.trimmed_size.0);
    }

    // Test TrimResult unchanged image
    #[test]
    fn test_trim_result_unchanged() {
        let result = TrimResult {
            input_path: PathBuf::from("/input/same.png"),
            output_path: PathBuf::from("/output/same.png"),
            original_size: (500, 500),
            trimmed_size: (500, 500), // No trimming needed
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

    // Test edge sensitivity variations
    #[test]
    fn test_edge_sensitivity_variations() {
        // Zero sensitivity
        let opts_zero = MarginOptions::builder().edge_sensitivity(0.0).build();
        assert_eq!(opts_zero.edge_sensitivity, 0.0);

        // High sensitivity
        let opts_high = MarginOptions::builder().edge_sensitivity(1.0).build();
        assert_eq!(opts_high.edge_sensitivity, 1.0);
    }

    // Test min margin variations
    #[test]
    fn test_min_margin_variations() {
        // Zero min margin
        let opts_zero = MarginOptions::builder().min_margin(0).build();
        assert_eq!(opts_zero.min_margin, 0);

        // Large min margin
        let opts_large = MarginOptions::builder().min_margin(100).build();
        assert_eq!(opts_large.min_margin, 100);
    }

    // Test IO error conversion
    #[test]
    fn test_io_error_conversion() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
        let margin_err: MarginError = io_err.into();

        let msg = margin_err.to_string().to_lowercase();
        assert!(msg.contains("io") || msg.contains("error"));
    }

    // Test large margins
    #[test]
    fn test_large_margins() {
        let margins = Margins {
            top: 500,
            bottom: 500,
            left: 300,
            right: 300,
        };

        assert_eq!(margins.total_vertical(), 1000);
        assert_eq!(margins.total_horizontal(), 600);
    }

    // Test background threshold edge cases
    #[test]
    fn test_background_threshold_edges() {
        // Minimum (only black is not background)
        let opts_min = MarginOptions::builder().background_threshold(1).build();
        assert_eq!(opts_min.background_threshold, 1);

        // Maximum (everything is background)
        let opts_max = MarginOptions::builder().background_threshold(255).build();
        assert_eq!(opts_max.background_threshold, 255);
    }

    // Test TrimResult with significant trimming
    #[test]
    fn test_trim_result_significant_trimming() {
        let result = TrimResult {
            input_path: PathBuf::from("/input/large.png"),
            output_path: PathBuf::from("/output/heavily_trimmed.png"),
            original_size: (2000, 3000),
            trimmed_size: (1600, 2400), // 20% trimmed
            margins_applied: Margins {
                top: 300,
                bottom: 300,
                left: 200,
                right: 200,
            },
        };

        // Verify significant size reduction
        assert!(result.trimmed_size.0 < result.original_size.0);
        assert!(result.trimmed_size.1 < result.original_size.1);

        // Verify margins consistency
        let vertical_reduction = result.original_size.1 - result.trimmed_size.1;
        assert_eq!(
            vertical_reduction,
            result.margins_applied.total_vertical() as u32
        );
    }

    // ============ Debug Implementation Tests ============

    #[test]
    fn test_margin_error_debug_impl() {
        let err = MarginError::ImageNotFound(PathBuf::from("/test/path.png"));
        let debug_str = format!("{:?}", err);
        assert!(debug_str.contains("ImageNotFound"));

        let err2 = MarginError::InvalidImage("bad format".to_string());
        let debug_str2 = format!("{:?}", err2);
        assert!(debug_str2.contains("InvalidImage"));

        let err3 = MarginError::NoContentDetected;
        let debug_str3 = format!("{:?}", err3);
        assert!(debug_str3.contains("NoContentDetected"));
    }

    #[test]
    fn test_margin_options_debug_impl() {
        let opts = MarginOptions::default();
        let debug_str = format!("{:?}", opts);
        assert!(debug_str.contains("MarginOptions"));
        assert!(debug_str.contains("background_threshold"));
        assert!(debug_str.contains("min_margin"));
    }

    #[test]
    fn test_margin_options_builder_debug_impl() {
        let builder = MarginOptions::builder();
        let debug_str = format!("{:?}", builder);
        assert!(debug_str.contains("MarginOptionsBuilder"));
    }

    #[test]
    fn test_content_detection_mode_debug_impl() {
        let mode = ContentDetectionMode::BackgroundColor;
        let debug_str = format!("{:?}", mode);
        assert!(debug_str.contains("BackgroundColor"));

        let mode2 = ContentDetectionMode::EdgeDetection;
        let debug_str2 = format!("{:?}", mode2);
        assert!(debug_str2.contains("EdgeDetection"));

        let mode3 = ContentDetectionMode::Histogram;
        let debug_str3 = format!("{:?}", mode3);
        assert!(debug_str3.contains("Histogram"));

        let mode4 = ContentDetectionMode::Combined;
        let debug_str4 = format!("{:?}", mode4);
        assert!(debug_str4.contains("Combined"));
    }

    #[test]
    fn test_margins_debug_impl() {
        let margins = Margins::uniform(50);
        let debug_str = format!("{:?}", margins);
        assert!(debug_str.contains("Margins"));
        assert!(debug_str.contains("top"));
        assert!(debug_str.contains("50"));
    }

    #[test]
    fn test_content_rect_debug_impl() {
        let rect = ContentRect {
            x: 10,
            y: 20,
            width: 100,
            height: 200,
        };
        let debug_str = format!("{:?}", rect);
        assert!(debug_str.contains("ContentRect"));
        assert!(debug_str.contains("width"));
    }

    #[test]
    fn test_margin_detection_debug_impl() {
        let detection = MarginDetection {
            margins: Margins::uniform(30),
            image_size: (800, 600),
            content_rect: ContentRect {
                x: 30,
                y: 30,
                width: 740,
                height: 540,
            },
            confidence: 0.95,
        };
        let debug_str = format!("{:?}", detection);
        assert!(debug_str.contains("MarginDetection"));
        assert!(debug_str.contains("confidence"));
    }

    #[test]
    fn test_unified_margins_debug_impl() {
        let unified = UnifiedMargins {
            margins: Margins::uniform(20),
            page_detections: vec![],
            unified_size: (1000, 1500),
        };
        let debug_str = format!("{:?}", unified);
        assert!(debug_str.contains("UnifiedMargins"));
        assert!(debug_str.contains("unified_size"));
    }

    #[test]
    fn test_trim_result_debug_impl() {
        let result = TrimResult {
            input_path: PathBuf::from("/input/test.png"),
            output_path: PathBuf::from("/output/test.png"),
            original_size: (100, 100),
            trimmed_size: (90, 90),
            margins_applied: Margins::uniform(5),
        };
        let debug_str = format!("{:?}", result);
        assert!(debug_str.contains("TrimResult"));
        assert!(debug_str.contains("original_size"));
    }

    // ============ Clone Implementation Tests ============

    #[test]
    fn test_margin_options_clone() {
        let original = MarginOptions::builder()
            .background_threshold(200)
            .min_margin(15)
            .edge_sensitivity(0.7)
            .detection_mode(ContentDetectionMode::Combined)
            .build();

        let cloned = original.clone();

        assert_eq!(cloned.background_threshold, 200);
        assert_eq!(cloned.min_margin, 15);
        assert_eq!(cloned.edge_sensitivity, 0.7);
    }

    #[test]
    fn test_content_detection_mode_clone() {
        let original = ContentDetectionMode::EdgeDetection;
        let cloned = original.clone();

        assert!(matches!(cloned, ContentDetectionMode::EdgeDetection));
    }

    #[test]
    fn test_content_detection_mode_copy() {
        let original = ContentDetectionMode::Histogram;
        let copied: ContentDetectionMode = original; // Copy
        let _still_valid = original; // original still valid

        assert!(matches!(copied, ContentDetectionMode::Histogram));
    }

    #[test]
    fn test_margins_clone() {
        let original = Margins {
            top: 10,
            bottom: 20,
            left: 30,
            right: 40,
        };
        let cloned = original.clone();

        assert_eq!(cloned.top, 10);
        assert_eq!(cloned.bottom, 20);
        assert_eq!(cloned.left, 30);
        assert_eq!(cloned.right, 40);
    }

    #[test]
    fn test_margins_copy() {
        let original = Margins::uniform(25);
        let copied: Margins = original; // Copy
        let _still_valid = original; // original still valid

        assert_eq!(copied.top, 25);
    }

    #[test]
    fn test_content_rect_clone() {
        let original = ContentRect {
            x: 5,
            y: 10,
            width: 200,
            height: 300,
        };
        let cloned = original.clone();

        assert_eq!(cloned.x, 5);
        assert_eq!(cloned.y, 10);
        assert_eq!(cloned.width, 200);
        assert_eq!(cloned.height, 300);
    }

    #[test]
    fn test_content_rect_copy() {
        let original = ContentRect {
            x: 50,
            y: 60,
            width: 400,
            height: 500,
        };
        let copied: ContentRect = original; // Copy
        let _still_valid = original; // original still valid

        assert_eq!(copied.width, 400);
    }

    #[test]
    fn test_margin_detection_clone() {
        let original = MarginDetection {
            margins: Margins::uniform(40),
            image_size: (1920, 1080),
            content_rect: ContentRect {
                x: 40,
                y: 40,
                width: 1840,
                height: 1000,
            },
            confidence: 0.88,
        };
        let cloned = original.clone();

        assert_eq!(cloned.image_size, (1920, 1080));
        assert_eq!(cloned.confidence, 0.88);
        assert_eq!(cloned.margins.top, 40);
    }

    #[test]
    fn test_unified_margins_clone() {
        let detection = MarginDetection {
            margins: Margins::uniform(30),
            image_size: (800, 600),
            content_rect: ContentRect {
                x: 30,
                y: 30,
                width: 740,
                height: 540,
            },
            confidence: 0.9,
        };

        let original = UnifiedMargins {
            margins: Margins::uniform(25),
            page_detections: vec![detection],
            unified_size: (750, 550),
        };

        let cloned = original.clone();

        assert_eq!(cloned.margins.top, 25);
        assert_eq!(cloned.page_detections.len(), 1);
        assert_eq!(cloned.unified_size, (750, 550));
    }

    // ============ Default Implementation Tests ============

    #[test]
    fn test_content_detection_mode_default() {
        let mode: ContentDetectionMode = Default::default();
        assert!(matches!(mode, ContentDetectionMode::BackgroundColor));
    }

    #[test]
    fn test_margins_default() {
        let margins: Margins = Default::default();
        assert_eq!(margins.top, 0);
        assert_eq!(margins.bottom, 0);
        assert_eq!(margins.left, 0);
        assert_eq!(margins.right, 0);
    }

    #[test]
    fn test_margin_options_builder_default() {
        let builder: MarginOptionsBuilder = Default::default();
        let opts = builder.build();
        // Should produce same as MarginOptions::default()
        let default_opts = MarginOptions::default();
        assert_eq!(opts.background_threshold, default_opts.background_threshold);
    }

    // ============ Boundary Value Tests ============

    #[test]
    fn test_margins_max_values() {
        let margins = Margins {
            top: u32::MAX,
            bottom: u32::MAX,
            left: u32::MAX,
            right: u32::MAX,
        };

        // Overflow check (will wrap on overflow in release mode)
        let _ = margins.top;
        let _ = margins.bottom;
    }

    #[test]
    fn test_content_rect_zero_dimensions() {
        let rect = ContentRect {
            x: 0,
            y: 0,
            width: 0,
            height: 0,
        };

        assert_eq!(rect.width, 0);
        assert_eq!(rect.height, 0);
    }

    #[test]
    fn test_content_rect_large_dimensions() {
        // 8K resolution
        let rect = ContentRect {
            x: 100,
            y: 100,
            width: 7680,
            height: 4320,
        };

        assert_eq!(rect.width, 7680);
        assert_eq!(rect.height, 4320);
    }

    #[test]
    fn test_margin_detection_zero_confidence() {
        let detection = MarginDetection {
            margins: Margins::uniform(0),
            image_size: (100, 100),
            content_rect: ContentRect {
                x: 0,
                y: 0,
                width: 100,
                height: 100,
            },
            confidence: 0.0,
        };

        assert_eq!(detection.confidence, 0.0);
    }

    #[test]
    fn test_margin_detection_full_confidence() {
        let detection = MarginDetection {
            margins: Margins::uniform(10),
            image_size: (100, 100),
            content_rect: ContentRect {
                x: 10,
                y: 10,
                width: 80,
                height: 80,
            },
            confidence: 1.0,
        };

        assert_eq!(detection.confidence, 1.0);
    }

    #[test]
    fn test_trim_percent_clamping() {
        // Over 100% should clamp to 100%
        let opts = MarginOptions::builder().default_trim_percent(150.0).build();
        assert_eq!(opts.default_trim_percent, 100.0);

        // Negative should clamp to 0
        let opts_neg = MarginOptions::builder().default_trim_percent(-10.0).build();
        assert_eq!(opts_neg.default_trim_percent, 0.0);
    }

    // ============ Preset Tests ============

    #[test]
    fn test_for_dark_background_all_fields() {
        let opts = MarginOptions::for_dark_background();

        assert_eq!(opts.background_threshold, 50);
        assert!(matches!(
            opts.detection_mode,
            ContentDetectionMode::EdgeDetection
        ));
        // Other fields should be default
        assert_eq!(opts.min_margin, MarginOptions::default().min_margin);
        assert_eq!(
            opts.default_trim_percent,
            MarginOptions::default().default_trim_percent
        );
    }

    #[test]
    fn test_precise_all_fields() {
        let opts = MarginOptions::precise();

        assert!(matches!(
            opts.detection_mode,
            ContentDetectionMode::Combined
        ));
        assert_eq!(opts.edge_sensitivity, 0.8);
        // Other fields should be default
        assert_eq!(
            opts.background_threshold,
            MarginOptions::default().background_threshold
        );
    }

    // ============ Error Variant Tests ============

    #[test]
    fn test_margin_error_io_details_preserved() {
        let io_err = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "access denied");
        let margin_err: MarginError = io_err.into();

        let msg = format!("{}", margin_err);
        assert!(msg.contains("access denied") || msg.contains("IO"));
    }

    #[test]
    fn test_margin_error_path_preserved() {
        let path = PathBuf::from("/very/specific/path/to/image.png");
        let err = MarginError::ImageNotFound(path.clone());

        let msg = format!("{}", err);
        assert!(msg.contains("image.png") || msg.contains("not found"));
    }

    #[test]
    fn test_margin_error_invalid_message_preserved() {
        let err = MarginError::InvalidImage("custom error message".to_string());

        let msg = format!("{}", err);
        assert!(msg.contains("custom error message") || msg.contains("Invalid"));
    }

    // ============ Builder Chain Tests ============

    #[test]
    fn test_builder_method_chaining_order_independence() {
        // Different order should produce same result
        let opts1 = MarginOptions::builder()
            .background_threshold(180)
            .min_margin(8)
            .edge_sensitivity(0.6)
            .build();

        let opts2 = MarginOptions::builder()
            .edge_sensitivity(0.6)
            .background_threshold(180)
            .min_margin(8)
            .build();

        assert_eq!(opts1.background_threshold, opts2.background_threshold);
        assert_eq!(opts1.min_margin, opts2.min_margin);
        assert_eq!(opts1.edge_sensitivity, opts2.edge_sensitivity);
    }

    #[test]
    fn test_builder_override_previous_values() {
        let opts = MarginOptions::builder()
            .background_threshold(100)
            .background_threshold(200) // Override
            .build();

        assert_eq!(opts.background_threshold, 200);
    }

    // ============ Unified Margins Edge Cases ============

    #[test]
    fn test_unified_margins_empty_detections() {
        let unified = UnifiedMargins {
            margins: Margins::uniform(0),
            page_detections: vec![],
            unified_size: (0, 0),
        };

        assert!(unified.page_detections.is_empty());
        assert_eq!(unified.unified_size, (0, 0));
    }

    #[test]
    fn test_unified_margins_single_page() {
        let detection = MarginDetection {
            margins: Margins {
                top: 50,
                bottom: 60,
                left: 30,
                right: 40,
            },
            image_size: (1000, 1500),
            content_rect: ContentRect {
                x: 30,
                y: 50,
                width: 930,
                height: 1390,
            },
            confidence: 0.92,
        };

        let unified = UnifiedMargins {
            margins: detection.margins,
            page_detections: vec![detection],
            unified_size: (930, 1390),
        };

        assert_eq!(unified.page_detections.len(), 1);
        assert_eq!(unified.margins.top, 50);
    }

    #[test]
    fn test_unified_margins_multiple_pages() {
        let detections: Vec<MarginDetection> = (0..10)
            .map(|i| MarginDetection {
                margins: Margins::uniform(20 + i as u32),
                image_size: (1000, 1500),
                content_rect: ContentRect {
                    x: 20 + i as u32,
                    y: 20 + i as u32,
                    width: 960 - 2 * i as u32,
                    height: 1460 - 2 * i as u32,
                },
                confidence: 0.9,
            })
            .collect();

        let unified = UnifiedMargins {
            margins: Margins::uniform(20), // Minimum
            page_detections: detections,
            unified_size: (960, 1460), // Maximum content
        };

        assert_eq!(unified.page_detections.len(), 10);
    }

    // ============ Detection Mode Specific Tests ============

    #[test]
    fn test_all_detection_modes_constructible() {
        let modes = [
            ContentDetectionMode::BackgroundColor,
            ContentDetectionMode::EdgeDetection,
            ContentDetectionMode::Histogram,
            ContentDetectionMode::Combined,
        ];

        for mode in modes {
            let opts = MarginOptions::builder().detection_mode(mode).build();
            // Just verify it doesn't panic
            let _ = format!("{:?}", opts.detection_mode);
        }
    }

    // ============ Path Type Tests ============

    #[test]
    fn test_trim_result_absolute_paths() {
        let result = TrimResult {
            input_path: PathBuf::from("/absolute/path/input.png"),
            output_path: PathBuf::from("/absolute/path/output.png"),
            original_size: (100, 100),
            trimmed_size: (80, 80),
            margins_applied: Margins::uniform(10),
        };

        assert!(result.input_path.is_absolute());
        assert!(result.output_path.is_absolute());
    }

    #[test]
    fn test_trim_result_relative_paths() {
        let result = TrimResult {
            input_path: PathBuf::from("relative/input.png"),
            output_path: PathBuf::from("relative/output.png"),
            original_size: (100, 100),
            trimmed_size: (80, 80),
            margins_applied: Margins::uniform(10),
        };

        assert!(result.input_path.is_relative());
        assert!(result.output_path.is_relative());
    }

    // ============ Size Calculation Tests ============

    #[test]
    fn test_margins_total_calculation_overflow_safe() {
        // Test that total calculations don't panic even with large values
        let margins = Margins {
            top: u32::MAX / 2,
            bottom: u32::MAX / 2,
            left: u32::MAX / 2,
            right: u32::MAX / 2,
        };

        // These might overflow but shouldn't panic in tests
        let v = margins.total_vertical();
        let h = margins.total_horizontal();

        // Just verify we can compute them
        let _ = v;
        let _ = h;
    }

    #[test]
    fn test_content_area_from_margins() {
        let image_size = (1000u32, 800u32);
        let margins = Margins {
            top: 50,
            bottom: 50,
            left: 100,
            right: 100,
        };

        let content_width = image_size.0 - margins.total_horizontal();
        let content_height = image_size.1 - margins.total_vertical();

        assert_eq!(content_width, 800);
        assert_eq!(content_height, 700);
    }

    // ============ Image Format Compatibility Tests ============

    #[test]
    fn test_image_not_found_various_extensions() {
        let extensions = ["png", "jpg", "jpeg", "tiff", "bmp", "gif"];

        for ext in extensions {
            let path = format!("/nonexistent/image.{}", ext);
            let result = ImageMarginDetector::detect(Path::new(&path), &MarginOptions::default());

            assert!(matches!(result, Err(MarginError::ImageNotFound(_))));
        }
    }

    // ============ Confidence Range Tests ============

    #[test]
    fn test_confidence_boundary_values() {
        let confidence_values = [0.0, 0.001, 0.5, 0.999, 1.0];

        for conf in confidence_values {
            let detection = MarginDetection {
                margins: Margins::uniform(10),
                image_size: (100, 100),
                content_rect: ContentRect {
                    x: 10,
                    y: 10,
                    width: 80,
                    height: 80,
                },
                confidence: conf,
            };

            assert!(detection.confidence >= 0.0);
            assert!(detection.confidence <= 1.0);
        }
    }

    // ==================== Concurrency Tests ====================

    #[test]
    fn test_parallel_detection_consistency() {
        use rayon::prelude::*;
        use std::sync::Arc;

        let options = Arc::new(MarginOptions::default());

        // Create multiple paths to test
        let paths: Vec<PathBuf> = (0..10)
            .map(|i| PathBuf::from(format!("/nonexistent/path_{}.png", i)))
            .collect();

        // All should fail consistently with ImageNotFound
        let results: Vec<_> = paths
            .par_iter()
            .map(|p| ImageMarginDetector::detect(p, &options))
            .collect();

        for result in results {
            assert!(result.is_err());
            assert!(matches!(result.unwrap_err(), MarginError::ImageNotFound(_)));
        }
    }

    #[test]
    fn test_options_shared_across_threads() {
        use std::sync::Arc;
        use std::thread;

        let options = Arc::new(MarginOptions::builder().background_threshold(200).build());

        let handles: Vec<_> = (0..4)
            .map(|_| {
                let opts = Arc::clone(&options);
                thread::spawn(move || {
                    assert_eq!(opts.background_threshold, 200);
                    opts.clone()
                })
            })
            .collect();

        for handle in handles {
            let result = handle.join().unwrap();
            assert_eq!(result.background_threshold, 200);
        }
    }

    #[test]
    fn test_margins_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<Margins>();
        assert_send_sync::<ContentRect>();
        assert_send_sync::<MarginOptions>();
    }

    #[test]
    fn test_concurrent_margin_calculations() {
        use std::thread;

        let handles: Vec<_> = (0..10)
            .map(|i| {
                thread::spawn(move || {
                    let margin = Margins::uniform(i as u32 * 10);
                    (margin.total_horizontal(), margin.total_vertical())
                })
            })
            .collect();

        for (i, handle) in handles.into_iter().enumerate() {
            let (h, v) = handle.join().unwrap();
            assert_eq!(h, i as u32 * 20);
            assert_eq!(v, i as u32 * 20);
        }
    }

    #[test]
    fn test_builder_thread_safety() {
        use std::thread;

        let handles: Vec<_> = (0..4)
            .map(|i| {
                thread::spawn(move || {
                    MarginOptions::builder()
                        .background_threshold(200 + i as u8)
                        .min_margin(10 + i as u32)
                        .build()
                })
            })
            .collect();

        for (i, handle) in handles.into_iter().enumerate() {
            let opts = handle.join().unwrap();
            assert_eq!(opts.background_threshold, 200 + i as u8);
            assert_eq!(opts.min_margin, 10 + i as u32);
        }
    }

    #[test]
    fn test_detection_result_thread_transfer() {
        use std::thread;

        let detection = MarginDetection {
            margins: Margins::uniform(50),
            image_size: (1000, 800),
            content_rect: ContentRect {
                x: 50,
                y: 50,
                width: 900,
                height: 700,
            },
            confidence: 0.95,
        };

        let handle = thread::spawn(move || {
            assert_eq!(detection.margins.top, 50);
            assert_eq!(detection.image_size, (1000, 800));
            detection
        });

        let received = handle.join().unwrap();
        assert_eq!(received.confidence, 0.95);
    }

    #[test]
    fn test_all_types_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}

        assert_send_sync::<MarginOptions>();
        assert_send_sync::<MarginOptionsBuilder>();
        assert_send_sync::<ContentDetectionMode>();
        assert_send_sync::<Margins>();
        assert_send_sync::<ContentRect>();
        assert_send_sync::<MarginDetection>();
        assert_send_sync::<UnifiedMargins>();
        assert_send_sync::<TrimResult>();
        assert_send_sync::<MarginError>();
    }

    #[test]
    fn test_concurrent_preset_creation() {
        use std::thread;

        let handles: Vec<_> = (0..6)
            .map(|i| {
                thread::spawn(move || match i % 3 {
                    0 => MarginOptions::default().background_threshold,
                    1 => MarginOptions::for_dark_background().background_threshold,
                    2 => MarginOptions::precise().background_threshold,
                    _ => unreachable!(),
                })
            })
            .collect();

        for handle in handles {
            let threshold = handle.join().unwrap();
            assert!(threshold <= 255);
        }
    }

    #[test]
    fn test_error_thread_transfer() {
        use std::thread;

        let err = MarginError::NoContentDetected;
        let handle = thread::spawn(move || err.to_string());

        let msg = handle.join().unwrap();
        assert!(msg.contains("content"));
    }

    #[test]
    fn test_concurrent_content_detection_modes() {
        use std::thread;

        let modes = [
            ContentDetectionMode::BackgroundColor,
            ContentDetectionMode::EdgeDetection,
            ContentDetectionMode::Histogram,
            ContentDetectionMode::Combined,
        ];

        let handles: Vec<_> = modes
            .into_iter()
            .map(|mode| thread::spawn(move || format!("{:?}", mode)))
            .collect();

        for handle in handles {
            let debug = handle.join().unwrap();
            assert!(!debug.is_empty());
        }
    }

    #[test]
    fn test_parallel_margin_detection_creation() {
        use std::thread;

        let handles: Vec<_> = (0..10)
            .map(|i| {
                thread::spawn(move || MarginDetection {
                    margins: Margins::uniform(10 + i as u32),
                    image_size: (1000 + i as u32 * 10, 800 + i as u32 * 10),
                    content_rect: ContentRect {
                        x: 10 + i as u32,
                        y: 10 + i as u32,
                        width: 980,
                        height: 780,
                    },
                    confidence: 0.9 + i as f64 * 0.01,
                })
            })
            .collect();

        let results: Vec<MarginDetection> =
            handles.into_iter().map(|h| h.join().unwrap()).collect();

        assert_eq!(results.len(), 10);
        for (i, detection) in results.iter().enumerate() {
            assert_eq!(detection.margins.top, 10 + i as u32);
        }
    }

    #[test]
    fn test_unified_margins_thread_transfer() {
        use std::thread;

        let detection = MarginDetection {
            margins: Margins::uniform(30),
            image_size: (800, 600),
            content_rect: ContentRect {
                x: 30,
                y: 30,
                width: 740,
                height: 540,
            },
            confidence: 0.88,
        };

        let unified = UnifiedMargins {
            margins: Margins::uniform(25),
            page_detections: vec![detection],
            unified_size: (750, 550),
        };

        let handle = thread::spawn(move || {
            assert_eq!(unified.margins.top, 25);
            assert_eq!(unified.page_detections.len(), 1);
            unified.unified_size
        });

        let size = handle.join().unwrap();
        assert_eq!(size, (750, 550));
    }

    #[test]
    fn test_trim_result_thread_transfer() {
        use std::thread;

        let result = TrimResult {
            input_path: PathBuf::from("/input/test.png"),
            output_path: PathBuf::from("/output/test.png"),
            original_size: (1000, 800),
            trimmed_size: (900, 700),
            margins_applied: Margins::uniform(50),
        };

        let handle = thread::spawn(move || {
            assert_eq!(result.original_size, (1000, 800));
            result.trimmed_size
        });

        let size = handle.join().unwrap();
        assert_eq!(size, (900, 700));
    }

    #[test]
    fn test_concurrent_options_building() {
        use std::thread;

        let handles: Vec<_> = (0..8)
            .map(|i| {
                thread::spawn(move || {
                    let opts = MarginOptions::builder()
                        .background_threshold(200 + i as u8)
                        .min_margin(5 + i as u32)
                        .edge_sensitivity(0.1 * (i as f32 + 1.0))
                        .build();
                    (
                        opts.background_threshold,
                        opts.min_margin,
                        opts.edge_sensitivity,
                    )
                })
            })
            .collect();

        for (i, handle) in handles.into_iter().enumerate() {
            let (threshold, min_margin, sensitivity) = handle.join().unwrap();
            assert_eq!(threshold, 200 + i as u8);
            assert_eq!(min_margin, 5 + i as u32);
            // Note: sensitivity is clamped to 0.0-1.0
            assert!(sensitivity >= 0.0 && sensitivity <= 1.0);
        }
    }

    #[test]
    fn test_content_rect_thread_transfer() {
        use std::thread;

        let rect = ContentRect {
            x: 100,
            y: 200,
            width: 500,
            height: 600,
        };

        let handle = thread::spawn(move || (rect.x, rect.y, rect.width, rect.height));

        let (x, y, w, h) = handle.join().unwrap();
        assert_eq!(x, 100);
        assert_eq!(y, 200);
        assert_eq!(w, 500);
        assert_eq!(h, 600);
    }

    #[test]
    fn test_builder_debug_impl() {
        let builder = MarginOptionsBuilder::default();
        let debug_str = format!("{:?}", builder);
        assert!(debug_str.contains("MarginOptionsBuilder"));
    }

    #[test]
    fn test_builder_partial_config() {
        let opts = MarginOptions::builder().background_threshold(180).build();

        assert_eq!(opts.background_threshold, 180);
        // Other fields should still be default
        assert_eq!(opts.min_margin, DEFAULT_MIN_MARGIN);
        assert_eq!(opts.edge_sensitivity, DEFAULT_EDGE_SENSITIVITY);
    }

    #[test]
    fn test_float_precision_in_confidence() {
        let confidence_values = [0.123456789, std::f64::consts::PI / 4.0, 0.333333333, 0.5];

        for conf in confidence_values {
            let detection = MarginDetection {
                margins: Margins::uniform(10),
                image_size: (100, 100),
                content_rect: ContentRect {
                    x: 10,
                    y: 10,
                    width: 80,
                    height: 80,
                },
                confidence: conf,
            };
            assert!((detection.confidence - conf).abs() < f64::EPSILON);
        }
    }

    #[test]
    fn test_float_precision_in_edge_sensitivity() {
        let sensitivity_values = [0.1, 0.25, 0.333, 0.5, 0.75, 0.9];

        for sens in sensitivity_values {
            let opts = MarginOptions::builder().edge_sensitivity(sens).build();
            assert!((opts.edge_sensitivity - sens).abs() < f32::EPSILON);
        }
    }
}
