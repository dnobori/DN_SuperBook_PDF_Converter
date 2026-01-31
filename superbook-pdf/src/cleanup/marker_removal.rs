//! Highlighter Marker Removal module
//!
//! Provides functionality to detect and remove highlighter marks and annotations
//! from scanned book pages.
//!
//! # Issue #34 Implementation
//!
//! Highlighter marks are detected using HSV color ranges specific to common
//! highlighter colors (yellow, pink, green, blue, orange).
//!
//! # Algorithm
//!
//! 1. Convert pixels to HSV color space
//! 2. Match against predefined highlighter color ranges
//! 3. Apply Sobel edge detection to preserve text edges
//! 4. Fade matched pixels toward white

use image::{GrayImage, Luma, Rgb, RgbImage};
use std::path::Path;

use super::types::{CleanupError, Result};

// ============================================================
// Constants - HSV Color Ranges for Highlighters
// ============================================================

/// Yellow highlighter HSV range
const YELLOW_HUE_MIN: f32 = 50.0;
const YELLOW_HUE_MAX: f32 = 70.0;
const YELLOW_SAT_MIN: f32 = 0.30;
const YELLOW_SAT_MAX: f32 = 1.0;
const YELLOW_VAL_MIN: f32 = 0.70;
const YELLOW_VAL_MAX: f32 = 1.0;

/// Pink highlighter HSV range
const PINK_HUE_MIN: f32 = 300.0;
const PINK_HUE_MAX: f32 = 345.0;
const PINK_SAT_MIN: f32 = 0.25;
const PINK_SAT_MAX: f32 = 0.80;
const PINK_VAL_MIN: f32 = 0.70;
const PINK_VAL_MAX: f32 = 1.0;

/// Green highlighter HSV range
const GREEN_HUE_MIN: f32 = 80.0;
const GREEN_HUE_MAX: f32 = 140.0;
const GREEN_SAT_MIN: f32 = 0.30;
const GREEN_SAT_MAX: f32 = 0.90;
const GREEN_VAL_MIN: f32 = 0.50;
const GREEN_VAL_MAX: f32 = 1.0;

/// Blue highlighter HSV range
const BLUE_HUE_MIN: f32 = 190.0;
const BLUE_HUE_MAX: f32 = 240.0;
const BLUE_SAT_MIN: f32 = 0.30;
const BLUE_SAT_MAX: f32 = 0.90;
const BLUE_VAL_MIN: f32 = 0.50;
const BLUE_VAL_MAX: f32 = 1.0;

/// Orange highlighter HSV range
const ORANGE_HUE_MIN: f32 = 15.0;
const ORANGE_HUE_MAX: f32 = 45.0;
const ORANGE_SAT_MIN: f32 = 0.40;
const ORANGE_SAT_MAX: f32 = 1.0;
const ORANGE_VAL_MIN: f32 = 0.70;
const ORANGE_VAL_MAX: f32 = 1.0;

/// Edge preservation threshold (Sobel magnitude)
const EDGE_THRESHOLD: u8 = 50;

// ============================================================
// Types
// ============================================================

/// Highlighter colors that can be detected and removed
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HighlighterColor {
    Yellow,
    Pink,
    Green,
    Blue,
    Orange,
    Custom {
        hue_min: u16,
        hue_max: u16,
        sat_min: u8,
        sat_max: u8,
        val_min: u8,
        val_max: u8,
    },
}

impl HighlighterColor {
    /// Get all standard highlighter colors
    pub fn all() -> Vec<HighlighterColor> {
        vec![
            HighlighterColor::Yellow,
            HighlighterColor::Pink,
            HighlighterColor::Green,
            HighlighterColor::Blue,
            HighlighterColor::Orange,
        ]
    }

    /// Get HSV range for this color
    fn hsv_range(&self) -> HsvRange {
        match self {
            HighlighterColor::Yellow => HsvRange {
                hue_min: YELLOW_HUE_MIN,
                hue_max: YELLOW_HUE_MAX,
                sat_min: YELLOW_SAT_MIN,
                sat_max: YELLOW_SAT_MAX,
                val_min: YELLOW_VAL_MIN,
                val_max: YELLOW_VAL_MAX,
            },
            HighlighterColor::Pink => HsvRange {
                hue_min: PINK_HUE_MIN,
                hue_max: PINK_HUE_MAX,
                sat_min: PINK_SAT_MIN,
                sat_max: PINK_SAT_MAX,
                val_min: PINK_VAL_MIN,
                val_max: PINK_VAL_MAX,
            },
            HighlighterColor::Green => HsvRange {
                hue_min: GREEN_HUE_MIN,
                hue_max: GREEN_HUE_MAX,
                sat_min: GREEN_SAT_MIN,
                sat_max: GREEN_SAT_MAX,
                val_min: GREEN_VAL_MIN,
                val_max: GREEN_VAL_MAX,
            },
            HighlighterColor::Blue => HsvRange {
                hue_min: BLUE_HUE_MIN,
                hue_max: BLUE_HUE_MAX,
                sat_min: BLUE_SAT_MIN,
                sat_max: BLUE_SAT_MAX,
                val_min: BLUE_VAL_MIN,
                val_max: BLUE_VAL_MAX,
            },
            HighlighterColor::Orange => HsvRange {
                hue_min: ORANGE_HUE_MIN,
                hue_max: ORANGE_HUE_MAX,
                sat_min: ORANGE_SAT_MIN,
                sat_max: ORANGE_SAT_MAX,
                val_min: ORANGE_VAL_MIN,
                val_max: ORANGE_VAL_MAX,
            },
            HighlighterColor::Custom {
                hue_min,
                hue_max,
                sat_min,
                sat_max,
                val_min,
                val_max,
            } => HsvRange {
                hue_min: *hue_min as f32,
                hue_max: *hue_max as f32,
                sat_min: *sat_min as f32 / 100.0,
                sat_max: *sat_max as f32 / 100.0,
                val_min: *val_min as f32 / 100.0,
                val_max: *val_max as f32 / 100.0,
            },
        }
    }
}

/// HSV range for color matching
#[derive(Debug, Clone, Copy)]
struct HsvRange {
    hue_min: f32,
    hue_max: f32,
    sat_min: f32,
    sat_max: f32,
    val_min: f32,
    val_max: f32,
}

impl HsvRange {
    /// Check if HSV values match this range
    fn matches(&self, h: f32, s: f32, v: f32) -> bool {
        let hue_match = if self.hue_min > self.hue_max {
            // Wrapping range (e.g., 350-10)
            h >= self.hue_min || h <= self.hue_max
        } else {
            h >= self.hue_min && h <= self.hue_max
        };

        hue_match
            && s >= self.sat_min
            && s <= self.sat_max
            && v >= self.val_min
            && v <= self.val_max
    }
}

/// Options for marker removal
#[derive(Debug, Clone)]
pub struct MarkerRemovalOptions {
    /// Colors to detect and remove
    pub colors: Vec<HighlighterColor>,

    /// Removal strength (0.0 = no change, 1.0 = full white)
    pub strength: f32,

    /// Preserve text edges using Sobel edge detection
    pub preserve_text_edges: bool,

    /// Edge detection threshold
    pub edge_threshold: u8,
}

impl Default for MarkerRemovalOptions {
    fn default() -> Self {
        Self {
            colors: HighlighterColor::all(),
            strength: 1.0,
            preserve_text_edges: true,
            edge_threshold: EDGE_THRESHOLD,
        }
    }
}

impl MarkerRemovalOptions {
    /// Create a builder
    pub fn builder() -> MarkerRemovalOptionsBuilder {
        MarkerRemovalOptionsBuilder::default()
    }

    /// Create options for yellow markers only
    pub fn yellow_only() -> Self {
        Self {
            colors: vec![HighlighterColor::Yellow],
            ..Default::default()
        }
    }

    /// Create options for all markers with partial removal
    pub fn partial(strength: f32) -> Self {
        Self {
            strength: strength.clamp(0.0, 1.0),
            ..Default::default()
        }
    }
}

/// Builder for MarkerRemovalOptions
#[derive(Debug, Default)]
pub struct MarkerRemovalOptionsBuilder {
    options: MarkerRemovalOptions,
}

impl MarkerRemovalOptionsBuilder {
    /// Set colors to remove
    #[must_use]
    pub fn colors(mut self, colors: Vec<HighlighterColor>) -> Self {
        self.options.colors = colors;
        self
    }

    /// Add a color to remove
    #[must_use]
    pub fn add_color(mut self, color: HighlighterColor) -> Self {
        if !self.options.colors.contains(&color) {
            self.options.colors.push(color);
        }
        self
    }

    /// Set removal strength
    #[must_use]
    pub fn strength(mut self, strength: f32) -> Self {
        self.options.strength = strength.clamp(0.0, 1.0);
        self
    }

    /// Set edge preservation
    #[must_use]
    pub fn preserve_text_edges(mut self, preserve: bool) -> Self {
        self.options.preserve_text_edges = preserve;
        self
    }

    /// Set edge threshold
    #[must_use]
    pub fn edge_threshold(mut self, threshold: u8) -> Self {
        self.options.edge_threshold = threshold;
        self
    }

    /// Build the options
    #[must_use]
    pub fn build(self) -> MarkerRemovalOptions {
        self.options
    }
}

/// Marker detection result
#[derive(Debug, Clone)]
pub struct MarkerDetectionResult {
    /// Pixels detected as marker (by color)
    pub detected_pixels: Vec<(HighlighterColor, u32)>,

    /// Total marker pixels
    pub total_marker_pixels: u32,

    /// Total image pixels
    pub total_pixels: u32,

    /// Image dimensions
    pub image_size: (u32, u32),
}

impl MarkerDetectionResult {
    /// Get marker coverage percentage
    pub fn coverage_percent(&self) -> f64 {
        if self.total_pixels == 0 {
            return 0.0;
        }
        (self.total_marker_pixels as f64 / self.total_pixels as f64) * 100.0
    }

    /// Check if any markers were detected
    pub fn has_markers(&self) -> bool {
        self.total_marker_pixels > 0
    }
}

// ============================================================
// Marker Remover
// ============================================================

/// Marker removal processor
pub struct MarkerRemover;

impl MarkerRemover {
    /// Detect markers in an image file
    pub fn detect(
        image_path: &Path,
        options: &MarkerRemovalOptions,
    ) -> Result<MarkerDetectionResult> {
        if !image_path.exists() {
            return Err(CleanupError::ImageNotFound(image_path.to_path_buf()));
        }

        let img = image::open(image_path).map_err(|e| CleanupError::InvalidImage(e.to_string()))?;
        let rgb = img.to_rgb8();

        Self::detect_from_image(&rgb, options)
    }

    /// Detect markers in an RGB image
    pub fn detect_from_image(
        image: &RgbImage,
        options: &MarkerRemovalOptions,
    ) -> Result<MarkerDetectionResult> {
        let (width, height) = image.dimensions();
        let total_pixels = width * height;

        let mut color_counts: Vec<(HighlighterColor, u32)> = options
            .colors
            .iter()
            .map(|c| (*c, 0u32))
            .collect();

        let mut total_marker_pixels = 0u32;

        for y in 0..height {
            for x in 0..width {
                let pixel = image.get_pixel(x, y);
                let (h, s, v) = Self::rgb_to_hsv(pixel.0[0], pixel.0[1], pixel.0[2]);

                for (color, count) in &mut color_counts {
                    let range = color.hsv_range();
                    if range.matches(h, s, v) {
                        *count += 1;
                        total_marker_pixels += 1;
                        break; // Count each pixel only once
                    }
                }
            }
        }

        Ok(MarkerDetectionResult {
            detected_pixels: color_counts,
            total_marker_pixels,
            total_pixels,
            image_size: (width, height),
        })
    }

    /// Remove markers from an image file
    pub fn remove(
        image_path: &Path,
        output_path: &Path,
        options: &MarkerRemovalOptions,
    ) -> Result<MarkerDetectionResult> {
        if !image_path.exists() {
            return Err(CleanupError::ImageNotFound(image_path.to_path_buf()));
        }

        let img = image::open(image_path).map_err(|e| CleanupError::InvalidImage(e.to_string()))?;
        let mut rgb = img.to_rgb8();

        let result = Self::remove_in_place(&mut rgb, options)?;

        rgb.save(output_path)
            .map_err(|e| CleanupError::InvalidImage(e.to_string()))?;

        Ok(result)
    }

    /// Remove markers from an RGB image in place
    pub fn remove_in_place(
        image: &mut RgbImage,
        options: &MarkerRemovalOptions,
    ) -> Result<MarkerDetectionResult> {
        let (width, height) = image.dimensions();
        let total_pixels = width * height;

        // Compute edge map if preserving edges
        let edge_map = if options.preserve_text_edges {
            Some(Self::compute_edge_map(image))
        } else {
            None
        };

        let mut color_counts: Vec<(HighlighterColor, u32)> = options
            .colors
            .iter()
            .map(|c| (*c, 0u32))
            .collect();

        let mut total_marker_pixels = 0u32;

        for y in 0..height {
            for x in 0..width {
                // Skip if on edge
                if let Some(ref edges) = edge_map {
                    if edges.get_pixel(x, y).0[0] > options.edge_threshold {
                        continue;
                    }
                }

                let pixel = image.get_pixel(x, y);
                let (h, s, v) = Self::rgb_to_hsv(pixel.0[0], pixel.0[1], pixel.0[2]);

                let mut is_marker = false;
                for (color, count) in &mut color_counts {
                    let range = color.hsv_range();
                    if range.matches(h, s, v) {
                        *count += 1;
                        total_marker_pixels += 1;
                        is_marker = true;
                        break;
                    }
                }

                if is_marker {
                    // Fade toward white
                    let new_pixel = Self::fade_to_white(pixel, options.strength);
                    image.put_pixel(x, y, new_pixel);
                }
            }
        }

        Ok(MarkerDetectionResult {
            detected_pixels: color_counts,
            total_marker_pixels,
            total_pixels,
            image_size: (width, height),
        })
    }

    /// Compute Sobel edge map
    fn compute_edge_map(image: &RgbImage) -> GrayImage {
        let (width, height) = image.dimensions();
        let mut edges = GrayImage::new(width, height);

        // Convert to grayscale first
        let gray: Vec<u8> = image
            .pixels()
            .map(|p| Self::luminance(p.0[0], p.0[1], p.0[2]))
            .collect();

        for y in 1..height - 1 {
            for x in 1..width - 1 {
                let idx = |x: u32, y: u32| -> usize { (y * width + x) as usize };

                // Sobel kernels
                let gx = gray[idx(x + 1, y - 1)] as i32
                    + 2 * gray[idx(x + 1, y)] as i32
                    + gray[idx(x + 1, y + 1)] as i32
                    - gray[idx(x - 1, y - 1)] as i32
                    - 2 * gray[idx(x - 1, y)] as i32
                    - gray[idx(x - 1, y + 1)] as i32;

                let gy = gray[idx(x - 1, y + 1)] as i32
                    + 2 * gray[idx(x, y + 1)] as i32
                    + gray[idx(x + 1, y + 1)] as i32
                    - gray[idx(x - 1, y - 1)] as i32
                    - 2 * gray[idx(x, y - 1)] as i32
                    - gray[idx(x + 1, y - 1)] as i32;

                let magnitude = ((gx * gx + gy * gy) as f64).sqrt() as u8;
                edges.put_pixel(x, y, Luma([magnitude]));
            }
        }

        edges
    }

    /// Fade a pixel toward white
    fn fade_to_white(pixel: &Rgb<u8>, strength: f32) -> Rgb<u8> {
        let fade = |v: u8| -> u8 { ((v as f32 + (255.0 - v as f32) * strength).min(255.0)) as u8 };
        Rgb([fade(pixel.0[0]), fade(pixel.0[1]), fade(pixel.0[2])])
    }

    /// Calculate luminance
    fn luminance(r: u8, g: u8, b: u8) -> u8 {
        (0.299 * r as f64 + 0.587 * g as f64 + 0.114 * b as f64).round() as u8
    }

    /// Convert RGB to HSV
    fn rgb_to_hsv(r: u8, g: u8, b: u8) -> (f32, f32, f32) {
        let rf = r as f32 / 255.0;
        let gf = g as f32 / 255.0;
        let bf = b as f32 / 255.0;

        let max = rf.max(gf).max(bf);
        let min = rf.min(gf).min(bf);
        let v = max;
        let d = max - min;
        let s = if max == 0.0 { 0.0 } else { d / max };

        let h = if d == 0.0 {
            0.0
        } else if max == rf {
            60.0 * (((gf - bf) / d) % 6.0)
        } else if max == gf {
            60.0 * (((bf - rf) / d) + 2.0)
        } else {
            60.0 * (((rf - gf) / d) + 4.0)
        };

        let h = if h < 0.0 { h + 360.0 } else { h };
        (h, s, v)
    }
}

// ============================================================
// Tests
// ============================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_marker_removal_options_default() {
        let opts = MarkerRemovalOptions::default();
        assert_eq!(opts.colors.len(), 5); // All standard colors
        assert_eq!(opts.strength, 1.0);
        assert!(opts.preserve_text_edges);
    }

    #[test]
    fn test_marker_removal_options_builder() {
        let opts = MarkerRemovalOptions::builder()
            .colors(vec![HighlighterColor::Yellow, HighlighterColor::Pink])
            .strength(0.8)
            .preserve_text_edges(false)
            .edge_threshold(100)
            .build();

        assert_eq!(opts.colors.len(), 2);
        assert_eq!(opts.strength, 0.8);
        assert!(!opts.preserve_text_edges);
        assert_eq!(opts.edge_threshold, 100);
    }

    #[test]
    fn test_highlighter_colors() {
        let all = HighlighterColor::all();
        assert_eq!(all.len(), 5);
        assert!(all.contains(&HighlighterColor::Yellow));
        assert!(all.contains(&HighlighterColor::Pink));
        assert!(all.contains(&HighlighterColor::Green));
        assert!(all.contains(&HighlighterColor::Blue));
        assert!(all.contains(&HighlighterColor::Orange));
    }

    #[test]
    fn test_hsv_range_matching() {
        let yellow_range = HighlighterColor::Yellow.hsv_range();

        // Should match yellow
        assert!(yellow_range.matches(60.0, 0.5, 0.9));

        // Should not match blue
        assert!(!yellow_range.matches(220.0, 0.5, 0.9));

        // Should not match low saturation
        assert!(!yellow_range.matches(60.0, 0.1, 0.9));
    }

    #[test]
    fn test_rgb_to_hsv() {
        // Yellow
        let (h, s, v) = MarkerRemover::rgb_to_hsv(255, 255, 0);
        assert!((h - 60.0).abs() < 1.0);
        assert!((s - 1.0).abs() < 0.01);
        assert!((v - 1.0).abs() < 0.01);

        // White
        let (_, s, v) = MarkerRemover::rgb_to_hsv(255, 255, 255);
        assert!(s.abs() < 0.01);
        assert!((v - 1.0).abs() < 0.01);

        // Red
        let (h, _, _) = MarkerRemover::rgb_to_hsv(255, 0, 0);
        assert!(h.abs() < 1.0 || (h - 360.0).abs() < 1.0);
    }

    #[test]
    fn test_fade_to_white() {
        let pixel = Rgb([100, 100, 100]);

        // Full strength
        let faded = MarkerRemover::fade_to_white(&pixel, 1.0);
        assert_eq!(faded.0[0], 255);
        assert_eq!(faded.0[1], 255);
        assert_eq!(faded.0[2], 255);

        // No strength
        let unchanged = MarkerRemover::fade_to_white(&pixel, 0.0);
        assert_eq!(unchanged.0[0], 100);
        assert_eq!(unchanged.0[1], 100);
        assert_eq!(unchanged.0[2], 100);

        // Half strength
        let half = MarkerRemover::fade_to_white(&pixel, 0.5);
        assert!((half.0[0] as i32 - 177).abs() < 2); // 100 + 0.5 * 155
    }

    #[test]
    fn test_luminance() {
        assert_eq!(MarkerRemover::luminance(255, 255, 255), 255);
        assert_eq!(MarkerRemover::luminance(0, 0, 0), 0);

        // Gray
        let gray = MarkerRemover::luminance(128, 128, 128);
        assert!((gray as i32 - 128).abs() < 2);
    }

    #[test]
    fn test_marker_detection_result() {
        let result = MarkerDetectionResult {
            detected_pixels: vec![
                (HighlighterColor::Yellow, 100),
                (HighlighterColor::Pink, 50),
            ],
            total_marker_pixels: 150,
            total_pixels: 10000,
            image_size: (100, 100),
        };

        assert!(result.has_markers());
        assert!((result.coverage_percent() - 1.5).abs() < 0.01);
    }

    #[test]
    fn test_detect_yellow_marker() {
        // Create image with yellow highlight area
        let mut image = RgbImage::from_pixel(100, 100, Rgb([255, 255, 255]));

        // Add yellow highlight (HSV: H≈55, S≈0.5, V≈1.0 → RGB: 255, 240, 128)
        for y in 20..40 {
            for x in 20..80 {
                image.put_pixel(x, y, Rgb([255, 255, 128])); // Yellow highlight
            }
        }

        let options = MarkerRemovalOptions::yellow_only();
        let result = MarkerRemover::detect_from_image(&image, &options);

        assert!(result.is_ok());
        let detection = result.unwrap();
        assert!(detection.has_markers());
        assert!(detection.total_marker_pixels > 0);
    }

    #[test]
    fn test_remove_marker() {
        // Create image with yellow highlight
        let mut image = RgbImage::from_pixel(50, 50, Rgb([255, 255, 255]));

        // Add yellow highlight
        for y in 10..40 {
            for x in 10..40 {
                image.put_pixel(x, y, Rgb([255, 255, 128]));
            }
        }

        let options = MarkerRemovalOptions::default();
        let result = MarkerRemover::remove_in_place(&mut image, &options);

        assert!(result.is_ok());

        // Check that yellow pixels have been faded
        let pixel = image.get_pixel(20, 20);
        assert!(pixel.0[0] > 250); // Should be close to white
        assert!(pixel.0[1] > 250);
        assert!(pixel.0[2] > 250);
    }

    #[test]
    fn test_no_markers_detected() {
        // White image - no markers
        let image = RgbImage::from_pixel(100, 100, Rgb([255, 255, 255]));
        let options = MarkerRemovalOptions::default();
        let result = MarkerRemover::detect_from_image(&image, &options);

        assert!(result.is_ok());
        let detection = result.unwrap();
        assert!(!detection.has_markers());
        assert_eq!(detection.total_marker_pixels, 0);
    }

    #[test]
    fn test_image_not_found() {
        let result = MarkerRemover::detect(
            Path::new("/nonexistent/image.png"),
            &MarkerRemovalOptions::default(),
        );
        assert!(matches!(result, Err(CleanupError::ImageNotFound(_))));
    }

    #[test]
    fn test_custom_color() {
        let custom = HighlighterColor::Custom {
            hue_min: 100,
            hue_max: 120,
            sat_min: 30,
            sat_max: 90,
            val_min: 50,
            val_max: 100,
        };

        let range = custom.hsv_range();
        assert_eq!(range.hue_min, 100.0);
        assert_eq!(range.hue_max, 120.0);
        assert_eq!(range.sat_min, 0.30);
        assert_eq!(range.sat_max, 0.90);
    }

    #[test]
    fn test_partial_strength() {
        let opts = MarkerRemovalOptions::partial(0.5);
        assert_eq!(opts.strength, 0.5);

        // Test clamping
        let opts_over = MarkerRemovalOptions::partial(1.5);
        assert_eq!(opts_over.strength, 1.0);

        let opts_under = MarkerRemovalOptions::partial(-0.5);
        assert_eq!(opts_under.strength, 0.0);
    }

    #[test]
    fn test_wrapping_hue_range() {
        // Test pink range which wraps around (300-345, but should also potentially match 0-10)
        let pink_range = HighlighterColor::Pink.hsv_range();

        // Should match pink hue
        assert!(pink_range.matches(320.0, 0.5, 0.9));

        // Should not match yellow
        assert!(!pink_range.matches(60.0, 0.5, 0.9));
    }
}
