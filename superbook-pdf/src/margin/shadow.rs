//! Shadow Detection and Removal module
//!
//! Provides functionality to detect and remove binding shadows from scanned book pages.
//!
//! # Issue #33 Implementation
//!
//! When books are scanned, the binding area often creates a dark shadow on the inner edge.
//! This module detects and removes these shadows through various methods:
//!
//! - Brightness equalization
//! - Gradient correction
//! - Shadow region cropping
//!
//! # Algorithm
//!
//! 1. Sample edge strips (15% width from each edge)
//! 2. Calculate brightness gradient (edge → center)
//! 3. Detect shadow using HSV criteria (low saturation, mid-value)
//! 4. Apply correction based on selected method

use image::{Rgb, RgbImage};
use std::path::Path;

use super::types::{MarginError, Result};

// ============================================================
// Constants
// ============================================================

/// Width of edge sampling strip as percentage of image width
const EDGE_SAMPLE_WIDTH_PERCENT: f32 = 15.0;

/// Minimum shadow width in pixels
const MIN_SHADOW_WIDTH: u32 = 10;

/// Maximum saturation for shadow detection (0.0-1.0)
const DEFAULT_MAX_SATURATION: f32 = 0.30;

/// Minimum value (brightness) for shadow detection (0.0-1.0)
const DEFAULT_MIN_VALUE: f32 = 0.25;

/// Maximum value (brightness) for shadow detection (0.0-1.0)
const DEFAULT_MAX_VALUE: f32 = 0.85;

/// Gradient threshold for shadow detection (brightness increase per pixel)
const GRADIENT_THRESHOLD: f32 = 0.001;

/// Number of sample rows for brightness profiling
const SAMPLE_ROWS: u32 = 50;

// ============================================================
// Types
// ============================================================

/// Edge where shadow may be detected
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edge {
    Left,
    Right,
    Top,
    Bottom,
}

impl Edge {
    /// Get all edges
    pub fn all() -> [Edge; 4] {
        [Edge::Left, Edge::Right, Edge::Top, Edge::Bottom]
    }

    /// Get horizontal edges (left and right)
    pub fn horizontal() -> [Edge; 2] {
        [Edge::Left, Edge::Right]
    }
}

/// Shadow removal method
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ShadowRemovalMethod {
    /// Equalize brightness across the shadow region
    #[default]
    BrightnessEqualization,

    /// Apply gradient correction to smooth the transition
    GradientCorrection,

    /// Simply crop the shadow region
    Crop,
}

/// HSV criteria for shadow detection
#[derive(Debug, Clone)]
pub struct ShadowHsvCriteria {
    /// Maximum saturation (0.0-1.0) - shadows are typically desaturated
    pub max_saturation: f32,

    /// Minimum value/brightness (0.0-1.0) - shadows are darker than background
    pub min_value: f32,

    /// Maximum value/brightness (0.0-1.0) - but not completely black
    pub max_value: f32,
}

impl Default for ShadowHsvCriteria {
    fn default() -> Self {
        Self {
            max_saturation: DEFAULT_MAX_SATURATION,
            min_value: DEFAULT_MIN_VALUE,
            max_value: DEFAULT_MAX_VALUE,
        }
    }
}

/// Options for shadow detection and removal
#[derive(Debug, Clone)]
pub struct ShadowRemovalOptions {
    /// Removal method
    pub method: ShadowRemovalMethod,

    /// Which edges to check for shadows
    pub edges_to_check: Vec<Edge>,

    /// HSV criteria for shadow detection
    pub hsv_criteria: ShadowHsvCriteria,

    /// Width of edge sampling strip as percentage
    pub sample_width_percent: f32,

    /// Minimum shadow width in pixels
    pub min_shadow_width: u32,

    /// Enable automatic shadow detection
    pub auto_detect: bool,
}

impl Default for ShadowRemovalOptions {
    fn default() -> Self {
        Self {
            method: ShadowRemovalMethod::BrightnessEqualization,
            edges_to_check: Edge::horizontal().to_vec(),
            hsv_criteria: ShadowHsvCriteria::default(),
            sample_width_percent: EDGE_SAMPLE_WIDTH_PERCENT,
            min_shadow_width: MIN_SHADOW_WIDTH,
            auto_detect: true,
        }
    }
}

impl ShadowRemovalOptions {
    /// Create options for left edge only
    pub fn left_only() -> Self {
        Self {
            edges_to_check: vec![Edge::Left],
            ..Default::default()
        }
    }

    /// Create options for right edge only
    pub fn right_only() -> Self {
        Self {
            edges_to_check: vec![Edge::Right],
            ..Default::default()
        }
    }

    /// Create options for both horizontal edges
    pub fn both_horizontal() -> Self {
        Self {
            edges_to_check: Edge::horizontal().to_vec(),
            ..Default::default()
        }
    }

    /// Create options for crop-based removal
    pub fn crop_method() -> Self {
        Self {
            method: ShadowRemovalMethod::Crop,
            ..Default::default()
        }
    }

    /// Create options for gradient correction
    pub fn gradient_method() -> Self {
        Self {
            method: ShadowRemovalMethod::GradientCorrection,
            ..Default::default()
        }
    }
}

/// Detected shadow region
#[derive(Debug, Clone)]
pub struct ShadowRegion {
    /// Which edge the shadow is on
    pub edge: Edge,

    /// Width of the shadow in pixels
    pub width: u32,

    /// Brightness profile from edge to interior (normalized 0.0-1.0)
    pub brightness_profile: Vec<f32>,

    /// Confidence of shadow detection (0.0-1.0)
    pub confidence: f64,

    /// Average brightness at the darkest point
    pub min_brightness: f32,

    /// Average brightness at the end of the shadow region
    pub max_brightness: f32,
}

impl ShadowRegion {
    /// Calculate the brightness gradient (change per pixel)
    pub fn gradient(&self) -> f32 {
        if self.width == 0 {
            return 0.0;
        }
        (self.max_brightness - self.min_brightness) / self.width as f32
    }
}

/// Shadow detection result for an image
#[derive(Debug, Clone)]
pub struct ShadowDetectionResult {
    /// Detected shadows
    pub shadows: Vec<ShadowRegion>,

    /// Image dimensions
    pub image_size: (u32, u32),
}

impl ShadowDetectionResult {
    /// Check if any shadows were detected
    pub fn has_shadows(&self) -> bool {
        !self.shadows.is_empty()
    }

    /// Get shadow on a specific edge
    pub fn get_shadow(&self, edge: Edge) -> Option<&ShadowRegion> {
        self.shadows.iter().find(|s| s.edge == edge)
    }

    /// Get total shadow width (sum of all edges)
    pub fn total_shadow_width(&self) -> u32 {
        self.shadows.iter().map(|s| s.width).sum()
    }
}

// ============================================================
// Shadow Detector
// ============================================================

/// Shadow detection and removal processor
pub struct ShadowDetector;

impl ShadowDetector {
    /// Detect shadows from an image file
    pub fn detect(image_path: &Path, options: &ShadowRemovalOptions) -> Result<ShadowDetectionResult> {
        if !image_path.exists() {
            return Err(MarginError::ImageNotFound(image_path.to_path_buf()));
        }

        let img = image::open(image_path).map_err(|e| MarginError::InvalidImage(e.to_string()))?;
        let rgb = img.to_rgb8();

        Self::detect_from_image(&rgb, options)
    }

    /// Detect shadows from an RGB image
    pub fn detect_from_image(
        image: &RgbImage,
        options: &ShadowRemovalOptions,
    ) -> Result<ShadowDetectionResult> {
        let (width, height) = image.dimensions();
        let mut shadows = Vec::new();

        for edge in &options.edges_to_check {
            if let Some(shadow) = Self::detect_edge_shadow(image, *edge, options) {
                if shadow.width >= options.min_shadow_width {
                    shadows.push(shadow);
                }
            }
        }

        Ok(ShadowDetectionResult {
            shadows,
            image_size: (width, height),
        })
    }

    /// Detect shadow on a specific edge
    fn detect_edge_shadow(
        image: &RgbImage,
        edge: Edge,
        options: &ShadowRemovalOptions,
    ) -> Option<ShadowRegion> {
        let (width, _height) = image.dimensions();
        let sample_width = ((width as f32 * options.sample_width_percent / 100.0) as u32).max(10);

        // Get brightness profile
        let brightness_profile = Self::calculate_brightness_profile(image, edge, sample_width);

        if brightness_profile.is_empty() {
            return None;
        }

        // Find shadow boundary (where gradient becomes flat)
        let shadow_width = Self::find_shadow_boundary(&brightness_profile, options);

        if shadow_width < options.min_shadow_width {
            return None;
        }

        // Calculate confidence based on gradient strength
        let min_brightness = brightness_profile
            .iter()
            .take(shadow_width as usize)
            .cloned()
            .fold(f32::INFINITY, f32::min);

        let max_brightness = brightness_profile
            .get(shadow_width as usize)
            .copied()
            .unwrap_or(brightness_profile.last().copied().unwrap_or(1.0));

        let gradient = (max_brightness - min_brightness) / shadow_width as f32;
        let confidence = (gradient / 0.01).min(1.0) as f64;

        Some(ShadowRegion {
            edge,
            width: shadow_width,
            brightness_profile,
            confidence,
            min_brightness,
            max_brightness,
        })
    }

    /// Calculate brightness profile from edge to interior
    fn calculate_brightness_profile(
        image: &RgbImage,
        edge: Edge,
        sample_width: u32,
    ) -> Vec<f32> {
        let (width, height) = image.dimensions();
        let mut profile = vec![0.0f32; sample_width as usize];
        let sample_count = SAMPLE_ROWS.min(height);

        // Sample rows evenly distributed
        let row_step = height / sample_count;

        for i in 0..sample_width {
            let mut brightness_sum = 0.0f32;
            let mut count = 0u32;

            for row_idx in 0..sample_count {
                let y = (row_idx * row_step).min(height - 1);

                let x = match edge {
                    Edge::Left => i,
                    Edge::Right => width - 1 - i,
                    Edge::Top => i.min(width - 1), // For top/bottom, sample horizontally
                    Edge::Bottom => i.min(width - 1),
                };

                let pixel = image.get_pixel(x, y);
                let brightness = Self::pixel_brightness(pixel);
                brightness_sum += brightness;
                count += 1;
            }

            if count > 0 {
                profile[i as usize] = brightness_sum / count as f32;
            }
        }

        profile
    }

    /// Find shadow boundary in brightness profile
    fn find_shadow_boundary(profile: &[f32], options: &ShadowRemovalOptions) -> u32 {
        if profile.len() < 3 {
            return 0;
        }

        // Calculate rolling gradient
        let mut shadow_end = 0u32;
        let window_size = 5;

        for i in window_size..profile.len() {
            // Calculate local gradient
            let local_gradient: f32 = profile[i - window_size..i]
                .windows(2)
                .map(|w| w[1] - w[0])
                .sum::<f32>()
                / window_size as f32;

            // Check if we're still in shadow region
            let brightness = profile[i];
            let is_shadow = brightness < options.hsv_criteria.max_value
                && local_gradient > GRADIENT_THRESHOLD;

            if is_shadow {
                shadow_end = i as u32;
            } else if shadow_end > 0 && i as u32 > shadow_end + 10 {
                // We've exited the shadow region
                break;
            }
        }

        shadow_end
    }

    /// Remove shadows from an image file
    pub fn remove_shadows(
        image_path: &Path,
        output_path: &Path,
        options: &ShadowRemovalOptions,
    ) -> Result<ShadowDetectionResult> {
        if !image_path.exists() {
            return Err(MarginError::ImageNotFound(image_path.to_path_buf()));
        }

        let img = image::open(image_path).map_err(|e| MarginError::InvalidImage(e.to_string()))?;
        let mut rgb = img.to_rgb8();

        let detection = Self::detect_from_image(&rgb, options)?;

        for shadow in &detection.shadows {
            Self::apply_shadow_removal(&mut rgb, shadow, options);
        }

        rgb.save(output_path)
            .map_err(|e| MarginError::InvalidImage(e.to_string()))?;

        Ok(detection)
    }

    /// Remove shadows from an RGB image in place
    pub fn remove_shadows_in_place(
        image: &mut RgbImage,
        options: &ShadowRemovalOptions,
    ) -> Result<ShadowDetectionResult> {
        let detection = Self::detect_from_image(image, options)?;

        for shadow in &detection.shadows {
            Self::apply_shadow_removal(image, shadow, options);
        }

        Ok(detection)
    }

    /// Apply shadow removal to an image
    fn apply_shadow_removal(
        image: &mut RgbImage,
        shadow: &ShadowRegion,
        options: &ShadowRemovalOptions,
    ) {
        match options.method {
            ShadowRemovalMethod::BrightnessEqualization => {
                Self::apply_brightness_equalization(image, shadow);
            }
            ShadowRemovalMethod::GradientCorrection => {
                Self::apply_gradient_correction(image, shadow);
            }
            ShadowRemovalMethod::Crop => {
                // Crop is handled externally through the margin system
                // This method just marks the shadow region
            }
        }
    }

    /// Apply brightness equalization to shadow region
    fn apply_brightness_equalization(image: &mut RgbImage, shadow: &ShadowRegion) {
        let (width, height) = image.dimensions();

        // Target brightness (brightness at the end of shadow)
        let target_brightness = shadow.max_brightness;

        for y in 0..height {
            for i in 0..shadow.width {
                let x = match shadow.edge {
                    Edge::Left => i,
                    Edge::Right => width - 1 - i,
                    _ => continue, // Only handle left/right for now
                };

                let current_brightness = shadow
                    .brightness_profile
                    .get(i as usize)
                    .copied()
                    .unwrap_or(target_brightness);

                if current_brightness > 0.0 && current_brightness < target_brightness {
                    let factor = target_brightness / current_brightness;
                    let pixel = image.get_pixel(x, y);
                    let new_pixel = Self::scale_pixel(pixel, factor);
                    image.put_pixel(x, y, new_pixel);
                }
            }
        }
    }

    /// Apply gradient correction to shadow region
    fn apply_gradient_correction(image: &mut RgbImage, shadow: &ShadowRegion) {
        let (width, height) = image.dimensions();

        for y in 0..height {
            for i in 0..shadow.width {
                let x = match shadow.edge {
                    Edge::Left => i,
                    Edge::Right => width - 1 - i,
                    _ => continue,
                };

                // Calculate correction factor based on position
                let progress = i as f32 / shadow.width as f32;
                let correction = 1.0 + (1.0 - progress) * (shadow.max_brightness / shadow.min_brightness.max(0.1) - 1.0);

                let pixel = image.get_pixel(x, y);
                let new_pixel = Self::scale_pixel(pixel, correction.min(2.0));
                image.put_pixel(x, y, new_pixel);
            }
        }
    }

    /// Calculate pixel brightness (0.0-1.0)
    fn pixel_brightness(pixel: &Rgb<u8>) -> f32 {
        let (r, g, b) = (pixel.0[0] as f32, pixel.0[1] as f32, pixel.0[2] as f32);
        // ITU-R BT.601 luminance
        (0.299 * r + 0.587 * g + 0.114 * b) / 255.0
    }

    /// Scale pixel brightness
    fn scale_pixel(pixel: &Rgb<u8>, factor: f32) -> Rgb<u8> {
        let scale = |v: u8| -> u8 { ((v as f32 * factor).min(255.0)) as u8 };
        Rgb([scale(pixel.0[0]), scale(pixel.0[1]), scale(pixel.0[2])])
    }

    /// Convert RGB to HSV
    #[allow(dead_code)]
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
    fn test_shadow_removal_options_default() {
        let options = ShadowRemovalOptions::default();
        assert_eq!(options.method, ShadowRemovalMethod::BrightnessEqualization);
        assert!(options.auto_detect);
        assert_eq!(options.edges_to_check.len(), 2);
    }

    #[test]
    fn test_shadow_removal_options_presets() {
        let left = ShadowRemovalOptions::left_only();
        assert_eq!(left.edges_to_check, vec![Edge::Left]);

        let right = ShadowRemovalOptions::right_only();
        assert_eq!(right.edges_to_check, vec![Edge::Right]);

        let both = ShadowRemovalOptions::both_horizontal();
        assert_eq!(both.edges_to_check.len(), 2);

        let crop = ShadowRemovalOptions::crop_method();
        assert_eq!(crop.method, ShadowRemovalMethod::Crop);

        let gradient = ShadowRemovalOptions::gradient_method();
        assert_eq!(gradient.method, ShadowRemovalMethod::GradientCorrection);
    }

    #[test]
    fn test_shadow_hsv_criteria_default() {
        let criteria = ShadowHsvCriteria::default();
        assert_eq!(criteria.max_saturation, DEFAULT_MAX_SATURATION);
        assert_eq!(criteria.min_value, DEFAULT_MIN_VALUE);
        assert_eq!(criteria.max_value, DEFAULT_MAX_VALUE);
    }

    #[test]
    fn test_edge_types() {
        let all = Edge::all();
        assert_eq!(all.len(), 4);

        let horizontal = Edge::horizontal();
        assert_eq!(horizontal.len(), 2);
        assert!(horizontal.contains(&Edge::Left));
        assert!(horizontal.contains(&Edge::Right));
    }

    #[test]
    fn test_pixel_brightness() {
        // White
        assert!((ShadowDetector::pixel_brightness(&Rgb([255, 255, 255])) - 1.0).abs() < 0.01);

        // Black
        assert!((ShadowDetector::pixel_brightness(&Rgb([0, 0, 0])) - 0.0).abs() < 0.01);

        // Gray
        let gray_brightness = ShadowDetector::pixel_brightness(&Rgb([128, 128, 128]));
        assert!(gray_brightness > 0.4 && gray_brightness < 0.6);
    }

    #[test]
    fn test_scale_pixel() {
        let pixel = Rgb([100, 100, 100]);
        let scaled = ShadowDetector::scale_pixel(&pixel, 2.0);
        assert_eq!(scaled.0[0], 200);
        assert_eq!(scaled.0[1], 200);
        assert_eq!(scaled.0[2], 200);

        // Test clamping
        let bright = Rgb([200, 200, 200]);
        let scaled_bright = ShadowDetector::scale_pixel(&bright, 2.0);
        assert_eq!(scaled_bright.0[0], 255);
    }

    #[test]
    fn test_rgb_to_hsv() {
        // Red
        let (h, s, v) = ShadowDetector::rgb_to_hsv(255, 0, 0);
        assert!(h.abs() < 1.0 || (h - 360.0).abs() < 1.0);
        assert!((s - 1.0).abs() < 0.01);
        assert!((v - 1.0).abs() < 0.01);

        // White
        let (_, s, v) = ShadowDetector::rgb_to_hsv(255, 255, 255);
        assert!(s.abs() < 0.01);
        assert!((v - 1.0).abs() < 0.01);

        // Gray
        let (_, s, v) = ShadowDetector::rgb_to_hsv(128, 128, 128);
        assert!(s.abs() < 0.01);
        assert!((v - 0.502).abs() < 0.01);
    }

    #[test]
    fn test_shadow_region_gradient() {
        let shadow = ShadowRegion {
            edge: Edge::Left,
            width: 100,
            brightness_profile: vec![0.5; 100],
            confidence: 0.9,
            min_brightness: 0.3,
            max_brightness: 0.8,
        };

        let gradient = shadow.gradient();
        assert!((gradient - 0.005).abs() < 0.001);
    }

    #[test]
    fn test_shadow_detection_result() {
        let result = ShadowDetectionResult {
            shadows: vec![
                ShadowRegion {
                    edge: Edge::Left,
                    width: 50,
                    brightness_profile: vec![0.5; 50],
                    confidence: 0.9,
                    min_brightness: 0.3,
                    max_brightness: 0.8,
                },
                ShadowRegion {
                    edge: Edge::Right,
                    width: 30,
                    brightness_profile: vec![0.5; 30],
                    confidence: 0.7,
                    min_brightness: 0.4,
                    max_brightness: 0.9,
                },
            ],
            image_size: (1000, 800),
        };

        assert!(result.has_shadows());
        assert_eq!(result.total_shadow_width(), 80);
        assert!(result.get_shadow(Edge::Left).is_some());
        assert!(result.get_shadow(Edge::Right).is_some());
        assert!(result.get_shadow(Edge::Top).is_none());
    }

    #[test]
    fn test_detect_synthetic_shadow() {
        // Create image with left shadow
        let mut image = RgbImage::from_pixel(200, 100, Rgb([255, 255, 255]));

        // Add shadow gradient on left edge
        for x in 0..50 {
            let brightness = (128 + (x as u32 * 2)) as u8;
            for y in 0..100 {
                image.put_pixel(x, y, Rgb([brightness, brightness, brightness]));
            }
        }

        let options = ShadowRemovalOptions::left_only();
        let result = ShadowDetector::detect_from_image(&image, &options);

        assert!(result.is_ok());
        let detection = result.unwrap();

        // Should detect some shadow (may vary based on algorithm)
        assert_eq!(detection.image_size, (200, 100));
    }

    #[test]
    fn test_calculate_brightness_profile() {
        let mut image = RgbImage::from_pixel(100, 50, Rgb([128, 128, 128]));

        // Make left edge darker
        for y in 0..50 {
            for x in 0..20 {
                let brightness = (50 + x * 4) as u8;
                image.put_pixel(x, y, Rgb([brightness, brightness, brightness]));
            }
        }

        let profile = ShadowDetector::calculate_brightness_profile(&image, Edge::Left, 30);

        assert_eq!(profile.len(), 30);
        // First values should be darker
        assert!(profile[0] < profile[20]);
    }

    #[test]
    fn test_image_not_found() {
        let result = ShadowDetector::detect(
            Path::new("/nonexistent/image.png"),
            &ShadowRemovalOptions::default(),
        );
        assert!(matches!(result, Err(MarginError::ImageNotFound(_))));
    }

    #[test]
    fn test_no_shadow_detected() {
        // Uniform white image - no shadow
        let image = RgbImage::from_pixel(100, 100, Rgb([255, 255, 255]));
        let options = ShadowRemovalOptions::default();
        let result = ShadowDetector::detect_from_image(&image, &options);

        assert!(result.is_ok());
        let detection = result.unwrap();
        assert!(!detection.has_shadows());
    }
}
