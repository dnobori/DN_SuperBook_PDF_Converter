//! Margin Detection Implementation
//!
//! Provides image margin detection using various algorithms.

use super::types::{ContentRect, MarginDetection, MarginError, Margins, Result, TrimResult};
use super::{ContentDetectionMode, MarginOptions};
use image::{GenericImageView, GrayImage};
use rayon::prelude::*;
use std::path::{Path, PathBuf};

use super::types::UnifiedMargins;

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
    fn test_detect_nonexistent_file() {
        let options = MarginOptions::default();
        let result = ImageMarginDetector::detect(Path::new("/nonexistent/image.png"), &options);
        assert!(matches!(result, Err(MarginError::ImageNotFound(_))));
    }

    #[test]
    fn test_trim_nonexistent_file() {
        let margins = Margins::uniform(10);
        let result = ImageMarginDetector::trim(
            Path::new("/nonexistent.png"),
            Path::new("/out.png"),
            &margins,
        );
        assert!(matches!(result, Err(MarginError::ImageNotFound(_))));
    }

    #[test]
    fn test_pad_nonexistent_file() {
        let result = ImageMarginDetector::pad_to_size(
            Path::new("/nonexistent.png"),
            Path::new("/out.png"),
            (100, 100),
            [255, 255, 255],
        );
        assert!(matches!(result, Err(MarginError::ImageNotFound(_))));
    }
}
