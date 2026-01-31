//! Common types for the markdown module

use std::path::PathBuf;
use thiserror::Error;

// ============================================================
// Error Types
// ============================================================

/// Markdown conversion error types
#[derive(Debug, Error)]
pub enum MarkdownError {
    #[error("PDF not found: {0}")]
    PdfNotFound(PathBuf),

    #[error("Invalid PDF: {0}")]
    InvalidPdf(String),

    #[error("OCR failed: {0}")]
    OcrFailed(String),

    #[error("Processing failed: {0}")]
    ProcessingFailed(String),

    #[error("API validation failed: {0}")]
    ValidationFailed(String),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Image error: {0}")]
    ImageError(String),
}

pub type Result<T> = std::result::Result<T, MarkdownError>;

// ============================================================
// Core Data Structures
// ============================================================

/// Bounding box for text/element positions
#[derive(Debug, Clone, Copy, Default)]
pub struct BoundingBox {
    /// X coordinate (left)
    pub x: u32,
    /// Y coordinate (top)
    pub y: u32,
    /// Width
    pub width: u32,
    /// Height
    pub height: u32,
}

impl BoundingBox {
    /// Create a new bounding box
    pub fn new(x: u32, y: u32, width: u32, height: u32) -> Self {
        Self { x, y, width, height }
    }

    /// Get the right edge
    pub fn right(&self) -> u32 {
        self.x + self.width
    }

    /// Get the bottom edge
    pub fn bottom(&self) -> u32 {
        self.y + self.height
    }

    /// Get the center X coordinate
    pub fn center_x(&self) -> u32 {
        self.x + self.width / 2
    }

    /// Get the center Y coordinate
    pub fn center_y(&self) -> u32 {
        self.y + self.height / 2
    }

    /// Get the area
    pub fn area(&self) -> u64 {
        self.width as u64 * self.height as u64
    }

    /// Check if this box contains a point
    pub fn contains(&self, x: u32, y: u32) -> bool {
        x >= self.x && x < self.right() && y >= self.y && y < self.bottom()
    }

    /// Check if this box overlaps with another
    pub fn overlaps(&self, other: &BoundingBox) -> bool {
        !(self.right() <= other.x
            || other.right() <= self.x
            || self.bottom() <= other.y
            || other.bottom() <= self.y)
    }

    /// Merge with another bounding box
    pub fn merge(&self, other: &BoundingBox) -> BoundingBox {
        let x = self.x.min(other.x);
        let y = self.y.min(other.y);
        let right = self.right().max(other.right());
        let bottom = self.bottom().max(other.bottom());

        BoundingBox {
            x,
            y,
            width: right - x,
            height: bottom - y,
        }
    }
}

/// A block of text with position and metadata
#[derive(Debug, Clone)]
pub struct TextBlock {
    /// The text content
    pub text: String,

    /// Bounding box position
    pub bbox: BoundingBox,

    /// Font size estimate (0 if unknown)
    pub font_size: f32,

    /// OCR confidence (0.0-1.0)
    pub confidence: f64,

    /// Is this likely a heading?
    pub is_heading: bool,

    /// Estimated heading level (1-6, 0 if not a heading)
    pub heading_level: u8,
}

impl TextBlock {
    /// Create a new text block
    pub fn new(text: String, bbox: BoundingBox) -> Self {
        Self {
            text,
            bbox,
            font_size: 0.0,
            confidence: 1.0,
            is_heading: false,
            heading_level: 0,
        }
    }

    /// Check if this block is empty
    pub fn is_empty(&self) -> bool {
        self.text.trim().is_empty()
    }
}

/// Content extracted from a single page
#[derive(Debug, Clone)]
pub struct PageContent {
    /// Page number (1-based)
    pub page_number: usize,

    /// Text blocks on this page
    pub text_blocks: Vec<TextBlock>,

    /// Page dimensions
    pub page_size: (u32, u32),

    /// Is this page vertical (Japanese) text?
    pub is_vertical: bool,
}

impl PageContent {
    /// Create a new page content
    pub fn new(page_number: usize, page_size: (u32, u32)) -> Self {
        Self {
            page_number,
            text_blocks: Vec::new(),
            page_size,
            is_vertical: false,
        }
    }

    /// Add a text block
    pub fn add_block(&mut self, block: TextBlock) {
        self.text_blocks.push(block);
    }

    /// Get all text as a single string
    pub fn all_text(&self) -> String {
        self.text_blocks
            .iter()
            .map(|b| b.text.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }
}

// ============================================================
// Options
// ============================================================

/// Options for Markdown conversion
#[derive(Debug, Clone)]
pub struct MarkdownOptions {
    /// Extract images from PDF
    pub extract_images: bool,

    /// Detect and convert tables
    pub detect_tables: bool,

    /// Text direction (auto, vertical, horizontal)
    pub text_direction: TextDirectionOption,

    /// Enable external API validation
    pub validate: bool,

    /// API provider for validation
    pub api_provider: Option<String>,

    /// Output directory for images
    pub image_output_dir: Option<PathBuf>,

    /// Include page numbers in output
    pub include_page_numbers: bool,

    /// Generate metadata JSON file
    pub generate_metadata: bool,

    /// OCR language (default: Japanese)
    pub ocr_language: String,
}

impl Default for MarkdownOptions {
    fn default() -> Self {
        Self {
            extract_images: true,
            detect_tables: true,
            text_direction: TextDirectionOption::Auto,
            validate: false,
            api_provider: None,
            image_output_dir: None,
            include_page_numbers: true,
            generate_metadata: true,
            ocr_language: "ja".to_string(),
        }
    }
}

impl MarkdownOptions {
    /// Create a builder
    pub fn builder() -> MarkdownOptionsBuilder {
        MarkdownOptionsBuilder::default()
    }
}

/// Text direction option
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TextDirectionOption {
    /// Auto-detect from content
    #[default]
    Auto,
    /// Force vertical (Japanese)
    Vertical,
    /// Force horizontal
    Horizontal,
}

/// Builder for MarkdownOptions
#[derive(Debug, Default)]
pub struct MarkdownOptionsBuilder {
    options: MarkdownOptions,
}

impl MarkdownOptionsBuilder {
    /// Set image extraction
    #[must_use]
    pub fn extract_images(mut self, extract: bool) -> Self {
        self.options.extract_images = extract;
        self
    }

    /// Set table detection
    #[must_use]
    pub fn detect_tables(mut self, detect: bool) -> Self {
        self.options.detect_tables = detect;
        self
    }

    /// Set text direction
    #[must_use]
    pub fn text_direction(mut self, direction: TextDirectionOption) -> Self {
        self.options.text_direction = direction;
        self
    }

    /// Enable validation
    #[must_use]
    pub fn validate(mut self, validate: bool) -> Self {
        self.options.validate = validate;
        self
    }

    /// Set API provider
    #[must_use]
    pub fn api_provider(mut self, provider: impl Into<String>) -> Self {
        self.options.api_provider = Some(provider.into());
        self
    }

    /// Set API provider (optional)
    #[must_use]
    pub fn api_provider_opt(mut self, provider: Option<String>) -> Self {
        self.options.api_provider = provider;
        self
    }

    /// Set image output directory
    #[must_use]
    pub fn image_output_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.options.image_output_dir = Some(dir.into());
        self
    }

    /// Include page numbers
    #[must_use]
    pub fn include_page_numbers(mut self, include: bool) -> Self {
        self.options.include_page_numbers = include;
        self
    }

    /// Generate metadata
    #[must_use]
    pub fn generate_metadata(mut self, generate: bool) -> Self {
        self.options.generate_metadata = generate;
        self
    }

    /// Set OCR language
    #[must_use]
    pub fn ocr_language(mut self, language: impl Into<String>) -> Self {
        self.options.ocr_language = language.into();
        self
    }

    /// Build the options
    #[must_use]
    pub fn build(self) -> MarkdownOptions {
        self.options
    }
}

// ============================================================
// Tests
// ============================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bounding_box() {
        let bbox = BoundingBox::new(10, 20, 100, 50);
        assert_eq!(bbox.right(), 110);
        assert_eq!(bbox.bottom(), 70);
        assert_eq!(bbox.center_x(), 60);
        assert_eq!(bbox.center_y(), 45);
        assert_eq!(bbox.area(), 5000);
    }

    #[test]
    fn test_bounding_box_contains() {
        let bbox = BoundingBox::new(10, 20, 100, 50);
        assert!(bbox.contains(50, 40));
        assert!(!bbox.contains(0, 0));
        assert!(!bbox.contains(200, 200));
    }

    #[test]
    fn test_bounding_box_overlaps() {
        let b1 = BoundingBox::new(0, 0, 100, 100);
        let b2 = BoundingBox::new(50, 50, 100, 100);
        let b3 = BoundingBox::new(200, 200, 50, 50);

        assert!(b1.overlaps(&b2));
        assert!(!b1.overlaps(&b3));
    }

    #[test]
    fn test_bounding_box_merge() {
        let b1 = BoundingBox::new(10, 20, 30, 40);
        let b2 = BoundingBox::new(50, 60, 70, 80);
        let merged = b1.merge(&b2);

        assert_eq!(merged.x, 10);
        assert_eq!(merged.y, 20);
        assert_eq!(merged.right(), 120);
        assert_eq!(merged.bottom(), 140);
    }

    #[test]
    fn test_text_block() {
        let bbox = BoundingBox::new(0, 0, 100, 50);
        let block = TextBlock::new("Hello".to_string(), bbox);

        assert_eq!(block.text, "Hello");
        assert!(!block.is_empty());
        assert!(!block.is_heading);
    }

    #[test]
    fn test_page_content() {
        let mut page = PageContent::new(1, (800, 600));
        page.add_block(TextBlock::new("Line 1".to_string(), BoundingBox::default()));
        page.add_block(TextBlock::new("Line 2".to_string(), BoundingBox::default()));

        assert_eq!(page.page_number, 1);
        assert_eq!(page.text_blocks.len(), 2);
        assert!(page.all_text().contains("Line 1"));
    }

    #[test]
    fn test_markdown_options_default() {
        let opts = MarkdownOptions::default();
        assert!(opts.extract_images);
        assert!(opts.detect_tables);
        assert!(!opts.validate);
        assert_eq!(opts.ocr_language, "ja");
    }

    #[test]
    fn test_markdown_options_builder() {
        let opts = MarkdownOptions::builder()
            .extract_images(false)
            .detect_tables(false)
            .text_direction(TextDirectionOption::Vertical)
            .validate(true)
            .api_provider("claude")
            .ocr_language("en")
            .build();

        assert!(!opts.extract_images);
        assert!(!opts.detect_tables);
        assert_eq!(opts.text_direction, TextDirectionOption::Vertical);
        assert!(opts.validate);
        assert_eq!(opts.api_provider, Some("claude".to_string()));
        assert_eq!(opts.ocr_language, "en");
    }
}
