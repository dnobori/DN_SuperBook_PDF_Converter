//! Margin module core types
//!
//! Contains basic data structures for margin detection and processing.

use std::path::{Path, PathBuf};
use thiserror::Error;

// ============================================================
// Error Types
// ============================================================

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

// ============================================================
// Core Data Structures
// ============================================================

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

// ============================================================
// Detector Trait
// ============================================================

use super::MarginOptions;

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_margins_uniform() {
        let margins = Margins::uniform(10);
        assert_eq!(margins.top, 10);
        assert_eq!(margins.bottom, 10);
        assert_eq!(margins.left, 10);
        assert_eq!(margins.right, 10);
    }

    #[test]
    fn test_margins_total() {
        let margins = Margins {
            top: 10,
            bottom: 20,
            left: 15,
            right: 25,
        };
        assert_eq!(margins.total_horizontal(), 40);
        assert_eq!(margins.total_vertical(), 30);
    }

    #[test]
    fn test_margins_default() {
        let margins = Margins::default();
        assert_eq!(margins.top, 0);
        assert_eq!(margins.bottom, 0);
        assert_eq!(margins.left, 0);
        assert_eq!(margins.right, 0);
    }
}
