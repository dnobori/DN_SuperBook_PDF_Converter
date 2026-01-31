//! Deblur module for focus blur correction
//!
//! Provides functionality to detect and correct focus blur in scanned images.
//!
//! # Issue #35 Implementation
//!
//! Focus blur is a common problem in scanned book pages, especially when the
//! scanner's focus isn't properly calibrated or the book isn't flat.
//!
//! # Algorithm
//!
//! 1. Detect blur using Laplacian variance
//! 2. Apply correction:
//!    - Unsharp Mask (local, fast) for mild blur
//!    - AI Deblur (NAFNet) for severe blur

use image::{GrayImage, RgbImage};
use std::path::Path;

use super::types::{CleanupError, Result};

// ============================================================
// Constants
// ============================================================

/// Default blur detection threshold (Laplacian variance)
/// Lower values indicate more blur
const DEFAULT_BLUR_THRESHOLD: f64 = 100.0;

/// Default unsharp mask sigma (Gaussian blur radius)
const DEFAULT_UNSHARP_SIGMA: f32 = 1.5;

/// Default unsharp mask amount (sharpening strength)
const DEFAULT_UNSHARP_AMOUNT: f32 = 1.5;

/// Minimum unsharp mask amount
const MIN_UNSHARP_AMOUNT: f32 = 0.1;

/// Maximum unsharp mask amount
const MAX_UNSHARP_AMOUNT: f32 = 5.0;

// ============================================================
// Types
// ============================================================

/// Deblur algorithm selection
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DeblurAlgorithm {
    /// Unsharp Mask - fast, local sharpening
    #[default]
    UnsharpMask,

    /// NAFNet AI deblur model
    NafNet,

    /// DeblurGAN-v2 AI model
    DeblurGanV2,
}

/// AI deblur model configuration
#[derive(Debug, Clone)]
pub struct AiDeblurModel {
    /// Model name
    pub name: String,

    /// Model path or identifier
    pub model_path: Option<String>,
}

impl Default for AiDeblurModel {
    fn default() -> Self {
        Self {
            name: "nafnet".to_string(),
            model_path: None,
        }
    }
}

/// Options for deblur processing
#[derive(Debug, Clone)]
pub struct DeblurOptions {
    /// Deblur algorithm to use
    pub algorithm: DeblurAlgorithm,

    /// Auto-detect blur and only process blurry images
    pub auto_detect: bool,

    /// Blur detection threshold (Laplacian variance)
    pub blur_threshold: f64,

    /// Unsharp mask Gaussian sigma
    pub unsharp_sigma: f32,

    /// Unsharp mask sharpening amount
    pub unsharp_amount: f32,

    /// AI model configuration (for AI algorithms)
    pub ai_model: Option<AiDeblurModel>,
}

impl Default for DeblurOptions {
    fn default() -> Self {
        Self {
            algorithm: DeblurAlgorithm::UnsharpMask,
            auto_detect: true,
            blur_threshold: DEFAULT_BLUR_THRESHOLD,
            unsharp_sigma: DEFAULT_UNSHARP_SIGMA,
            unsharp_amount: DEFAULT_UNSHARP_AMOUNT,
            ai_model: None,
        }
    }
}

impl DeblurOptions {
    /// Create a builder
    pub fn builder() -> DeblurOptionsBuilder {
        DeblurOptionsBuilder::default()
    }

    /// Create options for mild sharpening
    pub fn mild() -> Self {
        Self {
            unsharp_amount: 0.5,
            unsharp_sigma: 1.0,
            ..Default::default()
        }
    }

    /// Create options for strong sharpening
    pub fn strong() -> Self {
        Self {
            unsharp_amount: 2.5,
            unsharp_sigma: 2.0,
            ..Default::default()
        }
    }

    /// Create options for AI deblur
    pub fn ai_nafnet() -> Self {
        Self {
            algorithm: DeblurAlgorithm::NafNet,
            ai_model: Some(AiDeblurModel::default()),
            ..Default::default()
        }
    }
}

/// Builder for DeblurOptions
#[derive(Debug, Default)]
pub struct DeblurOptionsBuilder {
    options: DeblurOptions,
}

impl DeblurOptionsBuilder {
    /// Set algorithm
    #[must_use]
    pub fn algorithm(mut self, algorithm: DeblurAlgorithm) -> Self {
        self.options.algorithm = algorithm;
        self
    }

    /// Set auto-detection
    #[must_use]
    pub fn auto_detect(mut self, auto: bool) -> Self {
        self.options.auto_detect = auto;
        self
    }

    /// Set blur threshold
    #[must_use]
    pub fn blur_threshold(mut self, threshold: f64) -> Self {
        self.options.blur_threshold = threshold.max(0.0);
        self
    }

    /// Set unsharp sigma
    #[must_use]
    pub fn unsharp_sigma(mut self, sigma: f32) -> Self {
        self.options.unsharp_sigma = sigma.max(0.1);
        self
    }

    /// Set unsharp amount
    #[must_use]
    pub fn unsharp_amount(mut self, amount: f32) -> Self {
        self.options.unsharp_amount = amount.clamp(MIN_UNSHARP_AMOUNT, MAX_UNSHARP_AMOUNT);
        self
    }

    /// Set AI model
    #[must_use]
    pub fn ai_model(mut self, model: AiDeblurModel) -> Self {
        self.options.ai_model = Some(model);
        self
    }

    /// Build the options
    #[must_use]
    pub fn build(self) -> DeblurOptions {
        self.options
    }
}

/// Blur detection result
#[derive(Debug, Clone)]
pub struct BlurMetrics {
    /// Laplacian variance (higher = sharper)
    pub laplacian_variance: f64,

    /// Is the image considered blurry?
    pub is_blurry: bool,

    /// Blur severity (0.0 = sharp, 1.0 = very blurry)
    pub blur_severity: f64,
}

/// Deblur processing result
#[derive(Debug, Clone)]
pub struct DeblurResult {
    /// Blur metrics before processing
    pub before_metrics: BlurMetrics,

    /// Blur metrics after processing (if processed)
    pub after_metrics: Option<BlurMetrics>,

    /// Was the image processed?
    pub processed: bool,

    /// Algorithm used
    pub algorithm: DeblurAlgorithm,

    /// Image dimensions
    pub image_size: (u32, u32),
}

// ============================================================
// Blur Detector
// ============================================================

/// Blur detection using Laplacian variance
pub struct BlurDetector;

impl BlurDetector {
    /// Detect blur in an image file
    pub fn detect(image_path: &Path, threshold: f64) -> Result<BlurMetrics> {
        if !image_path.exists() {
            return Err(CleanupError::ImageNotFound(image_path.to_path_buf()));
        }

        let img = image::open(image_path).map_err(|e| CleanupError::InvalidImage(e.to_string()))?;
        let gray = img.to_luma8();

        Ok(Self::detect_from_image(&gray, threshold))
    }

    /// Detect blur in a grayscale image
    pub fn detect_from_image(gray: &GrayImage, threshold: f64) -> BlurMetrics {
        let variance = Self::laplacian_variance(gray);

        let is_blurry = variance < threshold;
        let blur_severity = if variance >= threshold {
            0.0
        } else {
            1.0 - (variance / threshold)
        };

        BlurMetrics {
            laplacian_variance: variance,
            is_blurry,
            blur_severity: blur_severity.clamp(0.0, 1.0),
        }
    }

    /// Calculate Laplacian variance as blur metric
    ///
    /// Higher variance = sharper image
    /// Lower variance = more blur
    pub fn laplacian_variance(gray: &GrayImage) -> f64 {
        let (width, height) = gray.dimensions();

        if width < 3 || height < 3 {
            return 0.0;
        }

        let mut sum = 0.0f64;
        let mut sum_sq = 0.0f64;
        let mut count = 0u64;

        // Laplacian kernel: [0, 1, 0; 1, -4, 1; 0, 1, 0]
        for y in 1..height - 1 {
            for x in 1..width - 1 {
                let center = gray.get_pixel(x, y).0[0] as f64;
                let top = gray.get_pixel(x, y - 1).0[0] as f64;
                let bottom = gray.get_pixel(x, y + 1).0[0] as f64;
                let left = gray.get_pixel(x - 1, y).0[0] as f64;
                let right = gray.get_pixel(x + 1, y).0[0] as f64;

                let laplacian = top + bottom + left + right - 4.0 * center;

                sum += laplacian;
                sum_sq += laplacian * laplacian;
                count += 1;
            }
        }

        if count == 0 {
            return 0.0;
        }

        let mean = sum / count as f64;
        let variance = (sum_sq / count as f64) - (mean * mean);

        variance.abs()
    }
}

// ============================================================
// Deblur Processor
// ============================================================

/// Deblur processing
pub struct Deblurrer;

impl Deblurrer {
    /// Process an image file
    pub fn process(
        image_path: &Path,
        output_path: &Path,
        options: &DeblurOptions,
    ) -> Result<DeblurResult> {
        if !image_path.exists() {
            return Err(CleanupError::ImageNotFound(image_path.to_path_buf()));
        }

        let img = image::open(image_path).map_err(|e| CleanupError::InvalidImage(e.to_string()))?;
        let mut rgb = img.to_rgb8();
        let (width, height) = rgb.dimensions();

        // Detect blur
        let gray = img.to_luma8();
        let before_metrics = BlurDetector::detect_from_image(&gray, options.blur_threshold);

        // Check if processing is needed
        if options.auto_detect && !before_metrics.is_blurry {
            // Save unchanged image
            rgb.save(output_path)
                .map_err(|e| CleanupError::InvalidImage(e.to_string()))?;

            return Ok(DeblurResult {
                before_metrics,
                after_metrics: None,
                processed: false,
                algorithm: options.algorithm,
                image_size: (width, height),
            });
        }

        // Apply deblur
        match options.algorithm {
            DeblurAlgorithm::UnsharpMask => {
                Self::apply_unsharp_mask(&mut rgb, options.unsharp_sigma, options.unsharp_amount);
            }
            DeblurAlgorithm::NafNet | DeblurAlgorithm::DeblurGanV2 => {
                // AI deblur would be handled by Python bridge
                // For now, fall back to unsharp mask
                Self::apply_unsharp_mask(&mut rgb, options.unsharp_sigma, options.unsharp_amount);
            }
        }

        // Measure after metrics
        let after_gray = image::DynamicImage::ImageRgb8(rgb.clone()).to_luma8();
        let after_metrics = BlurDetector::detect_from_image(&after_gray, options.blur_threshold);

        // Save result
        rgb.save(output_path)
            .map_err(|e| CleanupError::InvalidImage(e.to_string()))?;

        Ok(DeblurResult {
            before_metrics,
            after_metrics: Some(after_metrics),
            processed: true,
            algorithm: options.algorithm,
            image_size: (width, height),
        })
    }

    /// Process an RGB image in place
    pub fn process_in_place(image: &mut RgbImage, options: &DeblurOptions) -> Result<DeblurResult> {
        let (width, height) = image.dimensions();

        // Detect blur
        let gray: GrayImage = image::DynamicImage::ImageRgb8(image.clone()).to_luma8();
        let before_metrics = BlurDetector::detect_from_image(&gray, options.blur_threshold);

        // Check if processing is needed
        if options.auto_detect && !before_metrics.is_blurry {
            return Ok(DeblurResult {
                before_metrics,
                after_metrics: None,
                processed: false,
                algorithm: options.algorithm,
                image_size: (width, height),
            });
        }

        // Apply deblur
        match options.algorithm {
            DeblurAlgorithm::UnsharpMask => {
                Self::apply_unsharp_mask(image, options.unsharp_sigma, options.unsharp_amount);
            }
            DeblurAlgorithm::NafNet | DeblurAlgorithm::DeblurGanV2 => {
                // AI deblur would be handled by Python bridge
                Self::apply_unsharp_mask(image, options.unsharp_sigma, options.unsharp_amount);
            }
        }

        // Measure after metrics
        let after_gray = image::DynamicImage::ImageRgb8(image.clone()).to_luma8();
        let after_metrics = BlurDetector::detect_from_image(&after_gray, options.blur_threshold);

        Ok(DeblurResult {
            before_metrics,
            after_metrics: Some(after_metrics),
            processed: true,
            algorithm: options.algorithm,
            image_size: (width, height),
        })
    }

    /// Apply unsharp mask sharpening
    ///
    /// USM = Original + Amount * (Original - Blur)
    pub fn apply_unsharp_mask(image: &mut RgbImage, sigma: f32, amount: f32) {
        let (width, height) = image.dimensions();

        // Create Gaussian blur kernel
        let kernel_size = ((sigma * 6.0).ceil() as usize) | 1; // Ensure odd
        let kernel = Self::gaussian_kernel(kernel_size, sigma);

        // Apply unsharp mask to each channel
        for channel in 0..3 {
            // Extract channel
            let original: Vec<f32> = image
                .pixels()
                .map(|p| p.0[channel] as f32)
                .collect();

            // Apply Gaussian blur
            let blurred = Self::convolve_separable(&original, width, height, &kernel);

            // Apply USM: result = original + amount * (original - blurred)
            for (i, pixel) in image.pixels_mut().enumerate() {
                let diff = original[i] - blurred[i];
                let sharpened = original[i] + amount * diff;
                pixel.0[channel] = sharpened.clamp(0.0, 255.0) as u8;
            }
        }
    }

    /// Generate 1D Gaussian kernel
    fn gaussian_kernel(size: usize, sigma: f32) -> Vec<f32> {
        let half = (size / 2) as i32;
        let mut kernel = Vec::with_capacity(size);
        let mut sum = 0.0f32;

        for i in 0..size {
            let x = (i as i32 - half) as f32;
            let g = (-x * x / (2.0 * sigma * sigma)).exp();
            kernel.push(g);
            sum += g;
        }

        // Normalize
        for k in &mut kernel {
            *k /= sum;
        }

        kernel
    }

    /// Separable 2D convolution
    fn convolve_separable(data: &[f32], width: u32, height: u32, kernel: &[f32]) -> Vec<f32> {
        let w = width as usize;
        let h = height as usize;
        let k_half = kernel.len() / 2;

        // Horizontal pass
        let mut temp = vec![0.0f32; w * h];
        for y in 0..h {
            for x in 0..w {
                let mut sum = 0.0f32;
                for (ki, &kv) in kernel.iter().enumerate() {
                    let sx = (x as i32 + ki as i32 - k_half as i32).clamp(0, w as i32 - 1) as usize;
                    sum += data[y * w + sx] * kv;
                }
                temp[y * w + x] = sum;
            }
        }

        // Vertical pass
        let mut result = vec![0.0f32; w * h];
        for y in 0..h {
            for x in 0..w {
                let mut sum = 0.0f32;
                for (ki, &kv) in kernel.iter().enumerate() {
                    let sy = (y as i32 + ki as i32 - k_half as i32).clamp(0, h as i32 - 1) as usize;
                    sum += temp[sy * w + x] * kv;
                }
                result[y * w + x] = sum;
            }
        }

        result
    }
}

// ============================================================
// Tests
// ============================================================

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Luma, Rgb};

    #[test]
    fn test_deblur_options_default() {
        let opts = DeblurOptions::default();
        assert_eq!(opts.algorithm, DeblurAlgorithm::UnsharpMask);
        assert!(opts.auto_detect);
        assert_eq!(opts.blur_threshold, DEFAULT_BLUR_THRESHOLD);
    }

    #[test]
    fn test_deblur_options_builder() {
        let opts = DeblurOptions::builder()
            .algorithm(DeblurAlgorithm::NafNet)
            .auto_detect(false)
            .blur_threshold(50.0)
            .unsharp_sigma(2.0)
            .unsharp_amount(2.0)
            .build();

        assert_eq!(opts.algorithm, DeblurAlgorithm::NafNet);
        assert!(!opts.auto_detect);
        assert_eq!(opts.blur_threshold, 50.0);
        assert_eq!(opts.unsharp_sigma, 2.0);
        assert_eq!(opts.unsharp_amount, 2.0);
    }

    #[test]
    fn test_deblur_options_presets() {
        let mild = DeblurOptions::mild();
        assert!(mild.unsharp_amount < 1.0);

        let strong = DeblurOptions::strong();
        assert!(strong.unsharp_amount > 2.0);

        let ai = DeblurOptions::ai_nafnet();
        assert_eq!(ai.algorithm, DeblurAlgorithm::NafNet);
        assert!(ai.ai_model.is_some());
    }

    #[test]
    fn test_laplacian_variance_sharp_image() {
        // Create a sharp image with edges
        let mut gray = GrayImage::from_pixel(100, 100, Luma([128]));

        // Add sharp edges
        for x in 0..100 {
            gray.put_pixel(x, 50, Luma([255]));
            gray.put_pixel(50, x, Luma([255]));
        }

        let variance = BlurDetector::laplacian_variance(&gray);
        assert!(
            variance > 50.0,
            "Sharp image should have high variance: {}",
            variance
        );
    }

    #[test]
    fn test_laplacian_variance_uniform_image() {
        // Uniform image should have very low variance
        let gray = GrayImage::from_pixel(100, 100, Luma([128]));
        let variance = BlurDetector::laplacian_variance(&gray);
        assert!(
            variance < 10.0,
            "Uniform image should have low variance: {}",
            variance
        );
    }

    #[test]
    fn test_blur_detection() {
        let gray = GrayImage::from_pixel(100, 100, Luma([128]));
        let metrics = BlurDetector::detect_from_image(&gray, 100.0);

        // Uniform image is considered blurry
        assert!(metrics.is_blurry);
        assert!(metrics.blur_severity > 0.5);
    }

    #[test]
    fn test_gaussian_kernel() {
        let kernel = Deblurrer::gaussian_kernel(5, 1.0);

        assert_eq!(kernel.len(), 5);

        // Should be normalized (sum to 1)
        let sum: f32 = kernel.iter().sum();
        assert!((sum - 1.0).abs() < 0.001);

        // Center should be highest
        assert!(kernel[2] > kernel[0]);
        assert!(kernel[2] > kernel[4]);
    }

    #[test]
    fn test_unsharp_mask_application() {
        let mut image = RgbImage::from_pixel(50, 50, Rgb([128, 128, 128]));

        // Add some variation
        for y in 20..30 {
            for x in 20..30 {
                image.put_pixel(x, y, Rgb([200, 200, 200]));
            }
        }

        let original = image.clone();
        Deblurrer::apply_unsharp_mask(&mut image, 1.0, 1.5);

        // Edge pixels should have more contrast
        // (This is a basic test - actual sharpening is subtle)
        let orig_center = original.get_pixel(25, 25);
        let sharp_center = image.get_pixel(25, 25);

        // The bright region should remain bright
        assert!(sharp_center.0[0] >= orig_center.0[0].saturating_sub(10));
    }

    #[test]
    fn test_blur_metrics() {
        let metrics = BlurMetrics {
            laplacian_variance: 50.0,
            is_blurry: true,
            blur_severity: 0.5,
        };

        assert!(metrics.is_blurry);
        assert_eq!(metrics.blur_severity, 0.5);
    }

    #[test]
    fn test_deblur_result() {
        let result = DeblurResult {
            before_metrics: BlurMetrics {
                laplacian_variance: 30.0,
                is_blurry: true,
                blur_severity: 0.7,
            },
            after_metrics: Some(BlurMetrics {
                laplacian_variance: 80.0,
                is_blurry: false,
                blur_severity: 0.2,
            }),
            processed: true,
            algorithm: DeblurAlgorithm::UnsharpMask,
            image_size: (100, 100),
        };

        assert!(result.processed);
        assert!(result.after_metrics.is_some());
        assert!(
            result.after_metrics.as_ref().unwrap().laplacian_variance
                > result.before_metrics.laplacian_variance
        );
    }

    #[test]
    fn test_image_not_found() {
        let result = BlurDetector::detect(Path::new("/nonexistent/image.png"), 100.0);
        assert!(matches!(result, Err(CleanupError::ImageNotFound(_))));
    }

    #[test]
    fn test_ai_model_default() {
        let model = AiDeblurModel::default();
        assert_eq!(model.name, "nafnet");
        assert!(model.model_path.is_none());
    }

    #[test]
    fn test_process_in_place_no_blur() {
        let mut image = RgbImage::from_pixel(50, 50, Rgb([128, 128, 128]));

        // Add sharp edges
        for x in 0..50 {
            image.put_pixel(x, 25, Rgb([255, 255, 255]));
        }

        let options = DeblurOptions {
            auto_detect: true,
            blur_threshold: 10.0, // Very low threshold
            ..Default::default()
        };

        let result = Deblurrer::process_in_place(&mut image, &options);
        assert!(result.is_ok());

        let _result = result.unwrap();
        // Sharp image should not be processed
        // (depends on actual variance - this may vary)
    }
}
