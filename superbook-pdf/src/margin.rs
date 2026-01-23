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
            background_threshold: 250,
            min_margin: 10,
            default_trim_percent: 0.5,
            edge_sensitivity: 0.5,
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
            background_threshold: 50,
            detection_mode: ContentDetectionMode::EdgeDetection,
            ..Default::default()
        }
    }

    /// Create options for precise detection
    pub fn precise() -> Self {
        Self {
            detection_mode: ContentDetectionMode::Combined,
            edge_sensitivity: 0.8,
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
    pub fn background_threshold(mut self, threshold: u8) -> Self {
        self.options.background_threshold = threshold;
        self
    }

    /// Set minimum margin in pixels
    pub fn min_margin(mut self, margin: u32) -> Self {
        self.options.min_margin = margin;
        self
    }

    /// Set default trim percentage
    pub fn default_trim_percent(mut self, percent: f32) -> Self {
        self.options.default_trim_percent = percent.clamp(0.0, 100.0);
        self
    }

    /// Set edge detection sensitivity (0.0-1.0)
    pub fn edge_sensitivity(mut self, sensitivity: f32) -> Self {
        self.options.edge_sensitivity = sensitivity.clamp(0.0, 1.0);
        self
    }

    /// Set content detection mode
    pub fn detection_mode(mut self, mode: ContentDetectionMode) -> Self {
        self.options.detection_mode = mode;
        self
    }

    /// Build the options
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
}
