//! Color Statistics and Global Color Adjustment module
//!
//! Provides functionality for analyzing color statistics across pages
//! and applying global color correction for consistent book appearance.
//!
//! # Features
//!
//! - Paper/ink color extraction via histogram analysis
//! - MAD-based outlier exclusion
//! - Linear scale/offset color adjustment
//! - Ghost suppression for see-through pages
//!
//! # Example
//!
//! ```rust,no_run
//! use superbook_pdf::color_stats::{ColorStats, ColorAnalyzer, GlobalColorParam};
//! use std::path::Path;
//!
//! // Analyze a page
//! let stats = ColorAnalyzer::calculate_stats(Path::new("page.png")).unwrap();
//! println!("Paper color: RGB({:.0}, {:.0}, {:.0})",
//!     stats.paper_r, stats.paper_g, stats.paper_b);
//!
//! // Get global adjustment parameters
//! let params = ColorAnalyzer::decide_global_adjustment(&[stats]);
//! println!("Scale R: {:.2}", params.scale_r);
//! ```

use image::{Rgb, RgbImage};
use rayon::prelude::*;
use std::path::{Path, PathBuf};
use thiserror::Error;

// ============================================================
// Constants
// ============================================================

/// Sample step for histogram building (skip pixels for performance)
const SAMPLE_STEP: u32 = 4;

/// Percentile for ink color detection (dark pixels)
const INK_PERCENTILE: f64 = 0.05;

/// Percentile for paper color detection (light pixels)
const PAPER_PERCENTILE: f64 = 0.95;

/// Minimum scale factor for color adjustment
const MIN_SCALE: f64 = 0.8;

/// Maximum scale factor for color adjustment
const MAX_SCALE: f64 = 4.0;

/// Default saturation threshold for paper detection
const DEFAULT_SAT_THRESHOLD: u8 = 55;

/// Default color distance threshold
const DEFAULT_COLOR_DIST_THRESHOLD: u8 = 35;

/// Default white clip range
const DEFAULT_WHITE_CLIP_RANGE: u8 = 30;

// ============================================================
// Error Types
// ============================================================

/// Color analysis error types
#[derive(Debug, Error)]
pub enum ColorStatsError {
    #[error("Image not found: {0}")]
    ImageNotFound(PathBuf),

    #[error("Invalid image: {0}")]
    InvalidImage(String),

    #[error("No valid pages for analysis")]
    NoValidPages,

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, ColorStatsError>;

// ============================================================
// Data Structures
// ============================================================

/// Color statistics for a single page
#[derive(Debug, Clone, Default)]
pub struct ColorStats {
    /// Page number (1-based)
    pub page_number: usize,

    /// Paper (background) color - Red channel average
    pub paper_r: f64,
    /// Paper (background) color - Green channel average
    pub paper_g: f64,
    /// Paper (background) color - Blue channel average
    pub paper_b: f64,

    /// Ink (foreground) color - Red channel average
    pub ink_r: f64,
    /// Ink (foreground) color - Green channel average
    pub ink_g: f64,
    /// Ink (foreground) color - Blue channel average
    pub ink_b: f64,

    /// Mean R (for backward compatibility, same as paper_r)
    pub mean_r: f64,
    /// Mean G (for backward compatibility, same as paper_g)
    pub mean_g: f64,
    /// Mean B (for backward compatibility, same as paper_b)
    pub mean_b: f64,
}

impl ColorStats {
    /// Calculate paper luminance (ITU-R BT.601)
    pub fn paper_luminance(&self) -> f64 {
        0.299 * self.paper_r + 0.587 * self.paper_g + 0.114 * self.paper_b
    }

    /// Calculate ink luminance
    pub fn ink_luminance(&self) -> f64 {
        0.299 * self.ink_r + 0.587 * self.ink_g + 0.114 * self.ink_b
    }
}

/// Global color adjustment parameters
#[derive(Debug, Clone)]
pub struct GlobalColorParam {
    /// Scale factor for Red channel
    pub scale_r: f64,
    /// Scale factor for Green channel
    pub scale_g: f64,
    /// Scale factor for Blue channel
    pub scale_b: f64,

    /// Offset for Red channel
    pub offset_r: f64,
    /// Offset for Green channel
    pub offset_g: f64,
    /// Offset for Blue channel
    pub offset_b: f64,

    /// Ghost suppression luminance threshold
    pub ghost_suppress_threshold: u8,
    /// White clip range (how close to white to clip)
    pub white_clip_range: u8,

    /// Representative paper color RGB
    pub paper_r: u8,
    pub paper_g: u8,
    pub paper_b: u8,

    /// Saturation threshold for paper detection (0-255)
    pub sat_threshold: u8,
    /// Color distance threshold (L1 norm)
    pub color_dist_threshold: u8,

    /// Bleed-through hue range minimum (degrees)
    pub bleed_hue_min: f32,
    /// Bleed-through hue range maximum (degrees)
    pub bleed_hue_max: f32,
    /// Bleed-through minimum value (HSV)
    pub bleed_value_min: f32,
}

impl Default for GlobalColorParam {
    fn default() -> Self {
        Self {
            scale_r: 1.0,
            scale_g: 1.0,
            scale_b: 1.0,
            offset_r: 0.0,
            offset_g: 0.0,
            offset_b: 0.0,
            ghost_suppress_threshold: 200,
            white_clip_range: DEFAULT_WHITE_CLIP_RANGE,
            paper_r: 255,
            paper_g: 255,
            paper_b: 255,
            sat_threshold: DEFAULT_SAT_THRESHOLD,
            color_dist_threshold: DEFAULT_COLOR_DIST_THRESHOLD,
            bleed_hue_min: 20.0,
            bleed_hue_max: 65.0,
            bleed_value_min: 0.35,
        }
    }
}

// ============================================================
// Color Analyzer
// ============================================================

/// Color statistics analyzer
pub struct ColorAnalyzer;

impl ColorAnalyzer {
    /// Calculate color statistics from an image file
    pub fn calculate_stats(image_path: &Path) -> Result<ColorStats> {
        if !image_path.exists() {
            return Err(ColorStatsError::ImageNotFound(image_path.to_path_buf()));
        }

        let img =
            image::open(image_path).map_err(|e| ColorStatsError::InvalidImage(e.to_string()))?;

        let rgb = img.to_rgb8();
        Ok(Self::calculate_stats_from_image(&rgb, 0))
    }

    /// Calculate color statistics from an RGB image
    pub fn calculate_stats_from_image(image: &RgbImage, page_number: usize) -> ColorStats {
        let (w, h) = image.dimensions();
        let step = SAMPLE_STEP;

        // Build luminance histogram
        let mut histogram = [0u64; 256];
        let mut total = 0u64;

        for y in (0..h).step_by(step as usize) {
            for x in (0..w).step_by(step as usize) {
                let pixel = image.get_pixel(x, y);
                let lum = Self::luminance(pixel.0[0], pixel.0[1], pixel.0[2]);
                histogram[lum as usize] += 1;
                total += 1;
            }
        }

        // Find 5% (ink) and 95% (paper) percentile luminance thresholds
        let low_target = (total as f64 * INK_PERCENTILE) as u64;
        let high_target = (total as f64 * PAPER_PERCENTILE) as u64;

        let mut low_lum = 0u8;
        let mut high_lum = 255u8;
        let mut acc = 0u64;

        for (i, &count) in histogram.iter().enumerate() {
            acc += count;
            if acc >= low_target && low_lum == 0 {
                low_lum = i as u8;
            }
            if acc >= high_target {
                high_lum = i as u8;
                break;
            }
        }

        // Calculate RGB averages for paper (bright) and ink (dark) pixels
        let mut sum_paper_r = 0u64;
        let mut sum_paper_g = 0u64;
        let mut sum_paper_b = 0u64;
        let mut cnt_paper = 0u64;

        let mut sum_ink_r = 0u64;
        let mut sum_ink_g = 0u64;
        let mut sum_ink_b = 0u64;
        let mut cnt_ink = 0u64;

        for y in (0..h).step_by(step as usize) {
            for x in (0..w).step_by(step as usize) {
                let pixel = image.get_pixel(x, y);
                let (r, g, b) = (pixel.0[0], pixel.0[1], pixel.0[2]);
                let lum = Self::luminance(r, g, b);

                if lum >= high_lum {
                    // Paper pixel
                    sum_paper_r += r as u64;
                    sum_paper_g += g as u64;
                    sum_paper_b += b as u64;
                    cnt_paper += 1;
                } else if lum <= low_lum {
                    // Ink pixel
                    sum_ink_r += r as u64;
                    sum_ink_g += g as u64;
                    sum_ink_b += b as u64;
                    cnt_ink += 1;
                }
            }
        }

        // Prevent division by zero
        if cnt_paper == 0 {
            cnt_paper = 1;
        }
        if cnt_ink == 0 {
            cnt_ink = 1;
        }

        let paper_r = sum_paper_r as f64 / cnt_paper as f64;
        let paper_g = sum_paper_g as f64 / cnt_paper as f64;
        let paper_b = sum_paper_b as f64 / cnt_paper as f64;

        let ink_r = sum_ink_r as f64 / cnt_ink as f64;
        let ink_g = sum_ink_g as f64 / cnt_ink as f64;
        let ink_b = sum_ink_b as f64 / cnt_ink as f64;

        ColorStats {
            page_number,
            paper_r,
            paper_g,
            paper_b,
            ink_r,
            ink_g,
            ink_b,
            mean_r: paper_r,
            mean_g: paper_g,
            mean_b: paper_b,
        }
    }

    /// Exclude outlier pages using MAD (Median Absolute Deviation)
    pub fn exclude_outliers(stats_list: &[ColorStats]) -> Vec<ColorStats> {
        if stats_list.len() < 3 {
            return stats_list.to_vec();
        }

        // Calculate paper luminance for each page
        let mut luminances: Vec<(usize, f64)> = stats_list
            .iter()
            .enumerate()
            .map(|(i, s)| (i, s.paper_luminance()))
            .collect();

        luminances.sort_by(|a, b| a.1.total_cmp(&b.1));

        // Calculate median
        let median = Self::percentile_f64(
            &luminances.iter().map(|(_, l)| *l).collect::<Vec<_>>(),
            50.0,
        );

        // Calculate MAD
        let mut deviations: Vec<f64> = luminances.iter().map(|(_, l)| (l - median).abs()).collect();
        deviations.sort_by(|a, b| a.total_cmp(b));
        let mad = Self::percentile_f64(&deviations, 50.0);

        // Filter out outliers (> 1.5 * MAD from median)
        let threshold = mad * 1.5;
        let valid_indices: Vec<usize> = luminances
            .iter()
            .filter(|(_, l)| (l - median).abs() <= threshold)
            .map(|(i, _)| *i)
            .collect();

        if valid_indices.is_empty() {
            return stats_list.to_vec();
        }

        valid_indices
            .iter()
            .map(|&i| stats_list[i].clone())
            .collect()
    }

    /// Decide global color adjustment parameters from filtered stats
    pub fn decide_global_adjustment(stats_list: &[ColorStats]) -> GlobalColorParam {
        if stats_list.is_empty() {
            return GlobalColorParam::default();
        }

        // Exclude outliers using MAD
        let filtered = Self::exclude_outliers(stats_list);
        if filtered.is_empty() {
            return GlobalColorParam::default();
        }

        // Calculate median of paper/ink colors
        let bg_r = Self::percentile_f64(
            &filtered.iter().map(|s| s.paper_r).collect::<Vec<_>>(),
            50.0,
        );
        let bg_g = Self::percentile_f64(
            &filtered.iter().map(|s| s.paper_g).collect::<Vec<_>>(),
            50.0,
        );
        let bg_b = Self::percentile_f64(
            &filtered.iter().map(|s| s.paper_b).collect::<Vec<_>>(),
            50.0,
        );

        let ink_r =
            Self::percentile_f64(&filtered.iter().map(|s| s.ink_r).collect::<Vec<_>>(), 50.0);
        let ink_g =
            Self::percentile_f64(&filtered.iter().map(|s| s.ink_g).collect::<Vec<_>>(), 50.0);
        let ink_b =
            Self::percentile_f64(&filtered.iter().map(|s| s.ink_b).collect::<Vec<_>>(), 50.0);

        // Calculate linear scale: ink -> 0, paper -> 255
        let (scale_r, offset_r) = Self::linear_scale(bg_r, ink_r);
        let (scale_g, offset_g) = Self::linear_scale(bg_g, ink_g);
        let (scale_b, offset_b) = Self::linear_scale(bg_b, ink_b);

        // Calculate ghost suppression threshold
        let clamp8 = |v: f64| v.clamp(0.0, 255.0) as u8;

        let bg_lum_scaled = 0.299 * clamp8(bg_r * scale_r + offset_r) as f64
            + 0.587 * clamp8(bg_g * scale_g + offset_g) as f64
            + 0.114 * clamp8(bg_b * scale_b + offset_b) as f64;

        let ink_lum_scaled = 0.299 * clamp8(ink_r * scale_r + offset_r) as f64
            + 0.587 * clamp8(ink_g * scale_g + offset_g) as f64
            + 0.114 * clamp8(ink_b * scale_b + offset_b) as f64;

        // Ghost threshold = midpoint between paper and ink
        let ghost_threshold = ((ink_lum_scaled + bg_lum_scaled) * 0.5).clamp(0.0, 255.0) as u8;

        GlobalColorParam {
            scale_r,
            scale_g,
            scale_b,
            offset_r,
            offset_g,
            offset_b,
            ghost_suppress_threshold: ghost_threshold,
            white_clip_range: DEFAULT_WHITE_CLIP_RANGE,
            paper_r: bg_r.round() as u8,
            paper_g: bg_g.round() as u8,
            paper_b: bg_b.round() as u8,
            sat_threshold: DEFAULT_SAT_THRESHOLD,
            color_dist_threshold: DEFAULT_COLOR_DIST_THRESHOLD,
            bleed_hue_min: 20.0,
            bleed_hue_max: 65.0,
            bleed_value_min: 0.35,
        }
    }

    /// Apply global color adjustment to an image
    pub fn apply_adjustment(image: &mut RgbImage, params: &GlobalColorParam) {
        let (w, h) = image.dimensions();
        let clip_start = params.ghost_suppress_threshold as i32;
        let clip_end = (255 - params.white_clip_range as i32).clamp(0, 255);

        for y in 0..h {
            for x in 0..w {
                let pixel = image.get_pixel(x, y);
                let (src_r, src_g, src_b) = (pixel.0[0], pixel.0[1], pixel.0[2]);

                // Linear color correction
                let mut r = Self::clamp8(src_r as f64 * params.scale_r + params.offset_r);
                let mut g = Self::clamp8(src_g as f64 * params.scale_g + params.offset_g);
                let mut b = Self::clamp8(src_b as f64 * params.scale_b + params.offset_b);

                // Paper-like pixel whitening (smooth-step)
                let lum = Self::luminance(r, g, b) as i32;
                if lum >= clip_start {
                    let max = r.max(g).max(b);
                    let min = r.min(g).min(b);
                    let sat = if max == 0 {
                        0
                    } else {
                        (max - min) as i32 * 255 / max as i32
                    };

                    let dist = (r as i32 - params.paper_r as i32).abs()
                        + (g as i32 - params.paper_g as i32).abs()
                        + (b as i32 - params.paper_b as i32).abs();

                    if sat < params.sat_threshold as i32
                        && dist < params.color_dist_threshold as i32
                    {
                        let t = ((lum - clip_start) as f64 / (clip_end - clip_start + 1) as f64)
                            .clamp(0.0, 1.0);
                        let wgt = t * t * (3.0 - 2.0 * t); // Smooth-step

                        r = Self::clamp8(r as f64 + (255.0 - r as f64) * wgt);
                        g = Self::clamp8(g as f64 + (255.0 - g as f64) * wgt);
                        b = Self::clamp8(b as f64 + (255.0 - b as f64) * wgt);
                    }
                }

                // Orange/pink noise removal
                let (hue, _sat, _val) = Self::rgb_to_hsv(r, g, b);
                let max2 = r.max(g).max(b);
                let min2 = r.min(g).min(b);
                let sat2 = if max2 == 0 {
                    0
                } else {
                    (max2 - min2) as i32 * 255 / max2 as i32
                };
                let lum2 = Self::luminance(r, g, b);

                let is_pastel_pink = lum2 > 230 && sat2 < 30 && (hue <= 40.0 || hue >= 330.0);

                if is_pastel_pink {
                    r = 255;
                    g = 255;
                    b = 255;
                }

                image.put_pixel(x, y, Rgb([r, g, b]));
            }
        }
    }

    /// Analyze multiple pages and return statistics with outliers filtered per group
    pub fn analyze_book_pages(
        image_paths: &[PathBuf],
    ) -> Result<(Vec<ColorStats>, Vec<ColorStats>)> {
        let stats_results: Vec<Result<ColorStats>> = image_paths
            .par_iter()
            .enumerate()
            .map(|(i, path)| {
                let mut stats = Self::calculate_stats(path)?;
                stats.page_number = i + 1;
                Ok(stats)
            })
            .collect();

        let stats: Vec<ColorStats> = stats_results.into_iter().filter_map(|r| r.ok()).collect();

        if stats.is_empty() {
            return Err(ColorStatsError::NoValidPages);
        }

        // Split into odd and even pages
        let odd: Vec<ColorStats> = stats
            .iter()
            .filter(|s| s.page_number % 2 == 1)
            .cloned()
            .collect();
        let even: Vec<ColorStats> = stats
            .iter()
            .filter(|s| s.page_number % 2 == 0)
            .cloned()
            .collect();

        Ok((odd, even))
    }

    // ============================================================
    // Private Helper Functions
    // ============================================================

    fn luminance(r: u8, g: u8, b: u8) -> u8 {
        (0.299 * r as f64 + 0.587 * g as f64 + 0.114 * b as f64).round() as u8
    }

    fn clamp8(v: f64) -> u8 {
        v.clamp(0.0, 255.0).round() as u8
    }

    fn linear_scale(bg: f64, ink: f64) -> (f64, f64) {
        let diff = bg - ink;
        if diff < 1.0 {
            return (1.0, 0.0);
        }
        let s = (255.0 / diff).clamp(MIN_SCALE, MAX_SCALE);
        let o = -ink * s;
        (s, o)
    }

    fn percentile_f64(values: &[f64], p: f64) -> f64 {
        if values.is_empty() {
            return 0.0;
        }
        let mut sorted = values.to_vec();
        sorted.sort_by(|a, b| a.total_cmp(b));

        let rank = (p / 100.0) * (sorted.len() - 1) as f64;
        let lo = rank.floor() as usize;
        let hi = rank.ceil() as usize;

        if lo == hi {
            sorted[lo]
        } else {
            sorted[lo] + (sorted[hi] - sorted[lo]) * (rank - lo as f64)
        }
    }

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
    fn test_default_global_color_param() {
        let params = GlobalColorParam::default();
        assert_eq!(params.scale_r, 1.0);
        assert_eq!(params.scale_g, 1.0);
        assert_eq!(params.scale_b, 1.0);
        assert_eq!(params.offset_r, 0.0);
        assert_eq!(params.ghost_suppress_threshold, 200);
    }

    #[test]
    fn test_color_stats_luminance() {
        let stats = ColorStats {
            page_number: 1,
            paper_r: 255.0,
            paper_g: 255.0,
            paper_b: 255.0,
            ink_r: 0.0,
            ink_g: 0.0,
            ink_b: 0.0,
            ..Default::default()
        };

        assert!((stats.paper_luminance() - 255.0).abs() < 0.1);
        assert!((stats.ink_luminance() - 0.0).abs() < 0.1);
    }

    #[test]
    fn test_luminance_calculation() {
        assert_eq!(ColorAnalyzer::luminance(255, 255, 255), 255);
        assert_eq!(ColorAnalyzer::luminance(0, 0, 0), 0);
    }

    #[test]
    fn test_linear_scale() {
        let (s, o) = ColorAnalyzer::linear_scale(255.0, 0.0);
        assert!((s - 1.0).abs() < 0.01);
        assert!((o - 0.0).abs() < 0.01);

        // When ink is 50, paper is 200, we need to scale to 0-255
        let (s, _o) = ColorAnalyzer::linear_scale(200.0, 50.0);
        // 50 -> 0, 200 -> 255
        // s * 50 + o = 0  =>  o = -50s
        // s * 200 + o = 255  =>  200s - 50s = 255  =>  150s = 255  =>  s = 1.7
        assert!(s > 1.0);
    }

    #[test]
    fn test_percentile() {
        let values = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        assert!((ColorAnalyzer::percentile_f64(&values, 50.0) - 3.0).abs() < 0.01);
        assert!((ColorAnalyzer::percentile_f64(&values, 0.0) - 1.0).abs() < 0.01);
        assert!((ColorAnalyzer::percentile_f64(&values, 100.0) - 5.0).abs() < 0.01);
    }

    #[test]
    fn test_exclude_outliers_small_list() {
        let stats = vec![
            ColorStats {
                page_number: 1,
                paper_r: 250.0,
                paper_g: 250.0,
                paper_b: 250.0,
                ..Default::default()
            },
            ColorStats {
                page_number: 2,
                paper_r: 240.0,
                paper_g: 240.0,
                paper_b: 240.0,
                ..Default::default()
            },
        ];

        let filtered = ColorAnalyzer::exclude_outliers(&stats);
        assert_eq!(filtered.len(), 2); // Too few to filter
    }

    #[test]
    fn test_exclude_outliers() {
        let stats = vec![
            ColorStats {
                page_number: 1,
                paper_r: 250.0,
                paper_g: 250.0,
                paper_b: 250.0,
                ..Default::default()
            },
            ColorStats {
                page_number: 2,
                paper_r: 245.0,
                paper_g: 245.0,
                paper_b: 245.0,
                ..Default::default()
            },
            ColorStats {
                page_number: 3,
                paper_r: 248.0,
                paper_g: 248.0,
                paper_b: 248.0,
                ..Default::default()
            },
            ColorStats {
                page_number: 4,
                paper_r: 100.0,
                paper_g: 100.0,
                paper_b: 100.0,
                ..Default::default()
            }, // Outlier
            ColorStats {
                page_number: 5,
                paper_r: 252.0,
                paper_g: 252.0,
                paper_b: 252.0,
                ..Default::default()
            },
        ];

        let filtered = ColorAnalyzer::exclude_outliers(&stats);
        // Page 4 should be excluded as an outlier
        assert!(filtered.len() < stats.len() || filtered.iter().all(|s| s.page_number != 4));
    }

    #[test]
    fn test_decide_global_adjustment() {
        let stats = vec![ColorStats {
            page_number: 1,
            paper_r: 250.0,
            paper_g: 248.0,
            paper_b: 245.0,
            ink_r: 10.0,
            ink_g: 10.0,
            ink_b: 10.0,
            ..Default::default()
        }];

        let params = ColorAnalyzer::decide_global_adjustment(&stats);
        assert!(params.scale_r > 0.9);
        assert!(params.paper_r > 240);
    }

    #[test]
    fn test_rgb_to_hsv() {
        // Red
        let (h, s, v) = ColorAnalyzer::rgb_to_hsv(255, 0, 0);
        assert!(h.abs() < 1.0 || (h - 360.0).abs() < 1.0);
        assert!((s - 1.0).abs() < 0.01);
        assert!((v - 1.0).abs() < 0.01);

        // White
        let (_, s, v) = ColorAnalyzer::rgb_to_hsv(255, 255, 255);
        assert!((s - 0.0).abs() < 0.01);
        assert!((v - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_apply_adjustment_identity() {
        let mut img = RgbImage::from_pixel(10, 10, Rgb([128, 128, 128]));
        let params = GlobalColorParam::default();

        ColorAnalyzer::apply_adjustment(&mut img, &params);

        // With identity transform, pixels should be close to original
        let pixel = img.get_pixel(5, 5);
        assert!(pixel.0[0] > 100 && pixel.0[0] < 200);
    }

    #[test]
    fn test_image_not_found() {
        let result = ColorAnalyzer::calculate_stats(Path::new("/nonexistent/image.png"));
        assert!(matches!(result, Err(ColorStatsError::ImageNotFound(_))));
    }

    #[test]
    fn test_calculate_stats_from_image() {
        let img = RgbImage::from_pixel(100, 100, Rgb([240, 238, 235]));
        let stats = ColorAnalyzer::calculate_stats_from_image(&img, 1);

        assert_eq!(stats.page_number, 1);
        // Uniform image should have paper close to pixel values
        assert!(stats.paper_r > 230.0);
    }

    #[test]
    fn test_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<ColorStats>();
        assert_send_sync::<GlobalColorParam>();
        assert_send_sync::<ColorStatsError>();
    }

    #[test]
    fn test_error_types() {
        let _err1 = ColorStatsError::ImageNotFound(PathBuf::from("/test"));
        let _err2 = ColorStatsError::InvalidImage("bad".to_string());
        let _err3 = ColorStatsError::NoValidPages;
    }

    // ============================================================
    // Spec TC ID Tests
    // ============================================================

    // TC-COLOR-001: 白背景・黒文字 - paper≈255, ink≈0
    #[test]
    fn test_tc_color_001_white_background_black_text() {
        // Create image with mostly white background and some black pixels
        let mut img = RgbImage::from_pixel(100, 100, Rgb([255, 255, 255]));
        // Add some black "text" pixels
        for y in 40..60 {
            for x in 20..80 {
                img.put_pixel(x, y, Rgb([0, 0, 0]));
            }
        }

        let stats = ColorAnalyzer::calculate_stats_from_image(&img, 1);

        // Paper should be close to white (255)
        assert!(
            stats.paper_luminance() > 240.0,
            "Paper luminance {} should be > 240",
            stats.paper_luminance()
        );

        // Ink should be close to black (0)
        assert!(
            stats.ink_luminance() < 30.0,
            "Ink luminance {} should be < 30",
            stats.ink_luminance()
        );
    }

    // TC-COLOR-002: 黄ばんだ紙 - paper<255, 補正で白化
    #[test]
    fn test_tc_color_002_yellowed_paper_correction() {
        // Create image with yellowed paper (cream/beige tint)
        let mut img = RgbImage::from_pixel(100, 100, Rgb([245, 235, 210])); // Yellowed paper
        // Add some dark text
        for y in 40..60 {
            for x in 20..80 {
                img.put_pixel(x, y, Rgb([30, 25, 20]));
            }
        }

        let stats = ColorAnalyzer::calculate_stats_from_image(&img, 1);

        // Paper color should be less than pure white
        assert!(
            stats.paper_r < 255.0 || stats.paper_g < 255.0 || stats.paper_b < 255.0,
            "Yellowed paper should not be pure white"
        );

        // Blue channel should be lower than red (yellowing)
        assert!(
            stats.paper_b < stats.paper_r,
            "Yellowed paper should have lower blue than red"
        );

        // After correction, should become more neutral
        let all_stats = vec![stats];
        let params = ColorAnalyzer::decide_global_adjustment(&all_stats);

        // Scale should correct the yellowing (blue needs more boost)
        assert!(
            params.scale_b >= params.scale_r,
            "Blue scale {} should be >= red scale {} to correct yellowing",
            params.scale_b,
            params.scale_r
        );
    }

    // TC-COLOR-003: ゴースト抑制パラメータ計算
    #[test]
    fn test_tc_color_003_ghost_suppression_params() {
        // Create realistic book page: white paper with dark text
        let mut img = RgbImage::from_pixel(100, 100, Rgb([245, 243, 240])); // Slightly off-white paper
        // Add dark text
        for y in 30..40 {
            for x in 20..80 {
                img.put_pixel(x, y, Rgb([30, 28, 25])); // Dark ink
            }
        }
        // Add faint ghost/bleed-through (between ink and paper)
        for y in 60..70 {
            for x in 20..80 {
                img.put_pixel(x, y, Rgb([200, 198, 195])); // Ghost - lighter than ink, darker than paper
            }
        }

        let stats = ColorAnalyzer::calculate_stats_from_image(&img, 1);
        let params = ColorAnalyzer::decide_global_adjustment(&[stats]);

        // Ghost suppression threshold should be between ink and paper luminance
        // Paper luminance ≈ 243, Ink luminance ≈ 28
        // After scaling, threshold should be around midpoint
        assert!(
            params.ghost_suppress_threshold > 0 && params.ghost_suppress_threshold < 255,
            "Ghost suppression threshold {} should be in valid range",
            params.ghost_suppress_threshold
        );

        // Scale should be positive (mapping ink to 0, paper to 255)
        assert!(
            params.scale_r > 0.0 && params.scale_g > 0.0 && params.scale_b > 0.0,
            "Scale factors should be positive: R={}, G={}, B={}",
            params.scale_r,
            params.scale_g,
            params.scale_b
        );

        // Paper color should be detected
        assert!(
            params.paper_r > 200 && params.paper_g > 200 && params.paper_b > 200,
            "Paper color should be light: R={}, G={}, B={}",
            params.paper_r,
            params.paper_g,
            params.paper_b
        );
    }

    // TC-COLOR-004: カラー画像 - 彩度保持
    #[test]
    fn test_tc_color_004_color_image_saturation_preserved() {
        // Create image with colored content
        let mut img = RgbImage::from_pixel(100, 100, Rgb([255, 255, 255]));
        // Add red block
        for y in 20..40 {
            for x in 20..40 {
                img.put_pixel(x, y, Rgb([255, 50, 50])); // Red
            }
        }
        // Add blue block
        for y in 60..80 {
            for x in 60..80 {
                img.put_pixel(x, y, Rgb([50, 50, 255])); // Blue
            }
        }

        let _original_red = *img.get_pixel(30, 30);
        let _original_blue = *img.get_pixel(70, 70);

        let stats = ColorAnalyzer::calculate_stats_from_image(&img, 1);
        let params = ColorAnalyzer::decide_global_adjustment(&[stats]);
        ColorAnalyzer::apply_adjustment(&mut img, &params);

        // Check that colored pixels still have color (not desaturated to gray)
        let adjusted_red = img.get_pixel(30, 30);
        let adjusted_blue = img.get_pixel(70, 70);

        // Red pixel should still be predominantly red
        assert!(
            adjusted_red.0[0] > adjusted_red.0[1] && adjusted_red.0[0] > adjusted_red.0[2],
            "Red pixel should remain red after adjustment"
        );

        // Blue pixel should still be predominantly blue
        assert!(
            adjusted_blue.0[2] > adjusted_blue.0[0] && adjusted_blue.0[2] > adjusted_blue.0[1],
            "Blue pixel should remain blue after adjustment"
        );
    }

    // TC-COLOR-005: 外れ値ページ - MADで除外
    #[test]
    fn test_tc_color_005_outlier_exclusion_mad() {
        // Create stats with one obvious outlier
        let stats = vec![
            ColorStats {
                page_number: 1,
                paper_r: 250.0,
                paper_g: 250.0,
                paper_b: 250.0,
                ink_r: 10.0,
                ink_g: 10.0,
                ink_b: 10.0,
                ..Default::default()
            },
            ColorStats {
                page_number: 2,
                paper_r: 248.0,
                paper_g: 248.0,
                paper_b: 248.0,
                ink_r: 12.0,
                ink_g: 12.0,
                ink_b: 12.0,
                ..Default::default()
            },
            ColorStats {
                page_number: 3,
                paper_r: 252.0,
                paper_g: 252.0,
                paper_b: 252.0,
                ink_r: 8.0,
                ink_g: 8.0,
                ink_b: 8.0,
                ..Default::default()
            },
            ColorStats {
                page_number: 4,
                paper_r: 50.0, // Extreme outlier - dark page
                paper_g: 50.0,
                paper_b: 50.0,
                ink_r: 10.0,
                ink_g: 10.0,
                ink_b: 10.0,
                ..Default::default()
            },
            ColorStats {
                page_number: 5,
                paper_r: 249.0,
                paper_g: 249.0,
                paper_b: 249.0,
                ink_r: 11.0,
                ink_g: 11.0,
                ink_b: 11.0,
                ..Default::default()
            },
            ColorStats {
                page_number: 6,
                paper_r: 251.0,
                paper_g: 251.0,
                paper_b: 251.0,
                ink_r: 9.0,
                ink_g: 9.0,
                ink_b: 9.0,
                ..Default::default()
            },
        ];

        let filtered = ColorAnalyzer::exclude_outliers(&stats);

        // The outlier (page 4 with paper_r=50) should be excluded
        let has_outlier = filtered.iter().any(|s| s.page_number == 4);

        assert!(
            !has_outlier || filtered.len() < stats.len(),
            "Outlier page 4 should be excluded by MAD filter"
        );

        // Remaining pages should have consistent paper color
        if filtered.len() > 1 {
            let paper_values: Vec<f64> = filtered.iter().map(|s| s.paper_r).collect();
            let min = paper_values.iter().cloned().fold(f64::INFINITY, f64::min);
            let max = paper_values
                .iter()
                .cloned()
                .fold(f64::NEG_INFINITY, f64::max);

            assert!(
                max - min < 50.0,
                "Filtered paper values should be consistent (range {} is too large)",
                max - min
            );
        }
    }
}
