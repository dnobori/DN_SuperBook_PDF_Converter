//! Markdown Converter module
//!
//! Main entry point for PDF to Markdown conversion.

use std::path::{Path, PathBuf};

use super::api_validate::{ApiValidator, ValidationProvider, ValidationResult};
use super::element_detect::ElementDetector;
use super::reading_order::{ReadingOrderSorter, TextDirection};
use super::renderer::{MarkdownRenderer, MarkdownRenderOptions};
use super::types::{
    BoundingBox, MarkdownError, MarkdownOptions, PageContent, Result, TextBlock,
    TextDirectionOption,
};

// ============================================================
// Types
// ============================================================

/// Result of Markdown conversion
#[derive(Debug)]
pub struct MarkdownConversionResult {
    /// Output Markdown file path
    pub output_path: PathBuf,

    /// Number of pages processed
    pub pages_processed: usize,

    /// Total text blocks extracted
    pub total_blocks: usize,

    /// Validation result (if validation was enabled)
    pub validation: Option<ValidationResult>,

    /// Image files extracted
    pub extracted_images: Vec<PathBuf>,

    /// Metadata file path (if generated)
    pub metadata_path: Option<PathBuf>,
}

// ============================================================
// Markdown Converter
// ============================================================

/// Main converter for PDF to Markdown
pub struct MarkdownConverter {
    options: MarkdownOptions,
}

impl MarkdownConverter {
    /// Create a new converter with default options
    pub fn new() -> Self {
        Self {
            options: MarkdownOptions::default(),
        }
    }

    /// Create a new converter with specified options
    pub fn with_options(options: MarkdownOptions) -> Self {
        Self { options }
    }

    /// Convert a PDF file to Markdown
    pub fn convert(
        &self,
        pdf_path: &Path,
        output_dir: &Path,
    ) -> Result<MarkdownConversionResult> {
        if !pdf_path.exists() {
            return Err(MarkdownError::PdfNotFound(pdf_path.to_path_buf()));
        }

        // Create output directory
        std::fs::create_dir_all(output_dir).map_err(MarkdownError::IoError)?;

        // Extract pages and OCR content
        let pages = self.extract_pages(pdf_path)?;
        let total_blocks: usize = pages.iter().map(|p| p.text_blocks.len()).sum();

        // Detect text direction
        let direction = self.detect_direction(&pages);

        // Sort blocks in reading order
        let sorted_pages = self.sort_pages(pages, direction);

        // Render to Markdown
        let renderer = MarkdownRenderer::with_options(MarkdownRenderOptions {
            include_page_numbers: self.options.include_page_numbers,
            image_path_prefix: if self.options.extract_images {
                "images/".to_string()
            } else {
                String::new()
            },
            ..Default::default()
        });

        let markdown_content = renderer.render_pages(&sorted_pages);

        // Write output file
        let output_path = output_dir.join("document.md");
        std::fs::write(&output_path, &markdown_content).map_err(MarkdownError::IoError)?;

        // Extract images if enabled
        let extracted_images = if self.options.extract_images {
            self.extract_images(pdf_path, output_dir)?
        } else {
            Vec::new()
        };

        // Generate metadata if enabled
        let metadata_path = if self.options.generate_metadata {
            let path = output_dir.join("metadata.json");
            self.generate_metadata(&sorted_pages, &path)?;
            Some(path)
        } else {
            None
        };

        // Validate if enabled
        let validation = if self.options.validate {
            let provider = self
                .options
                .api_provider
                .as_ref()
                .map(|p| self.create_provider(p))
                .unwrap_or_else(|| ValidationProvider::local("internal"));

            let validator = ApiValidator::new(provider);
            Some(validator.validate(&markdown_content)?)
        } else {
            None
        };

        Ok(MarkdownConversionResult {
            output_path,
            pages_processed: sorted_pages.len(),
            total_blocks,
            validation,
            extracted_images,
            metadata_path,
        })
    }

    /// Extract pages from PDF
    fn extract_pages(&self, pdf_path: &Path) -> Result<Vec<PageContent>> {
        // This would integrate with the existing pdf_reader and yomitoku modules
        // For now, return a placeholder implementation

        // In a full implementation:
        // 1. Use pdf_reader to extract images
        // 2. Use yomitoku for OCR
        // 3. Convert OCR results to PageContent

        // TODO: Integrate with pdf_reader and yomitoku for actual extraction
        // log::info!("Extracting pages from: {:?}", pdf_path);
        let _ = pdf_path; // Suppress unused warning

        // Placeholder: return empty pages
        // Real implementation would call YomiToku OCR
        Ok(Vec::new())
    }

    /// Detect text direction from pages
    fn detect_direction(&self, pages: &[PageContent]) -> TextDirection {
        match self.options.text_direction {
            TextDirectionOption::Vertical => TextDirection::Vertical,
            TextDirectionOption::Horizontal => TextDirection::Horizontal,
            TextDirectionOption::Auto => {
                // Analyze pages to detect direction
                let all_blocks: Vec<_> = pages
                    .iter()
                    .flat_map(|p| p.text_blocks.iter().cloned())
                    .collect();

                ReadingOrderSorter::detect_direction(&all_blocks)
            }
        }
    }

    /// Sort pages in reading order
    fn sort_pages(&self, mut pages: Vec<PageContent>, direction: TextDirection) -> Vec<PageContent> {
        for page in &mut pages {
            ReadingOrderSorter::sort(&mut page.text_blocks, direction);
        }
        pages
    }

    /// Extract images from PDF
    fn extract_images(&self, _pdf_path: &Path, output_dir: &Path) -> Result<Vec<PathBuf>> {
        let images_dir = output_dir.join("images");
        std::fs::create_dir_all(&images_dir).map_err(MarkdownError::IoError)?;

        // Placeholder: in real implementation, extract figures from PDF
        Ok(Vec::new())
    }

    /// Generate metadata JSON
    fn generate_metadata(&self, pages: &[PageContent], path: &Path) -> Result<()> {
        let metadata = serde_json::json!({
            "version": "1.0",
            "pages": pages.len(),
            "total_blocks": pages.iter().map(|p| p.text_blocks.len()).sum::<usize>(),
            "text_direction": match self.detect_direction(pages) {
                TextDirection::Vertical => "vertical",
                TextDirection::Horizontal => "horizontal",
            },
        });

        let content =
            serde_json::to_string_pretty(&metadata).map_err(|e| MarkdownError::ProcessingFailed(e.to_string()))?;

        std::fs::write(path, content).map_err(MarkdownError::IoError)?;

        Ok(())
    }

    /// Create validation provider from string
    fn create_provider(&self, name: &str) -> ValidationProvider {
        match name.to_lowercase().as_str() {
            "claude" => ValidationProvider::claude(
                std::env::var("ANTHROPIC_API_KEY").unwrap_or_default(),
            ),
            "openai" => ValidationProvider::openai(
                std::env::var("OPENAI_API_KEY").unwrap_or_default(),
            ),
            _ => ValidationProvider::local(name),
        }
    }

    /// Convert OCR result to PageContent
    pub fn ocr_result_to_page_content(
        page_number: usize,
        page_size: (u32, u32),
        text_blocks: Vec<(String, (u32, u32, u32, u32), f64)>, // (text, (x, y, w, h), confidence)
    ) -> PageContent {
        let mut page = PageContent::new(page_number, page_size);

        for (text, (x, y, w, h), confidence) in text_blocks {
            let mut block = TextBlock::new(text, BoundingBox::new(x, y, w, h));
            block.confidence = confidence;
            page.add_block(block);
        }

        // Detect elements (headings, figures, etc.)
        let elements = ElementDetector::detect_elements(&page.text_blocks, page_size);

        // Mark headings
        for element in elements {
            if let super::element_detect::ElementType::Heading(level) = element.element_type {
                for block in &mut page.text_blocks {
                    if block.bbox.overlaps(&element.bbox) {
                        block.is_heading = true;
                        block.heading_level = level;
                    }
                }
            }
        }

        page
    }
}

impl Default for MarkdownConverter {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================
// Tests
// ============================================================

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_converter_default() {
        let converter = MarkdownConverter::new();
        assert!(converter.options.extract_images);
        assert!(!converter.options.validate);
    }

    #[test]
    fn test_converter_with_options() {
        let options = MarkdownOptions::builder()
            .extract_images(false)
            .validate(true)
            .api_provider("claude")
            .build();

        let converter = MarkdownConverter::with_options(options);
        assert!(!converter.options.extract_images);
        assert!(converter.options.validate);
    }

    #[test]
    fn test_detect_direction_forced() {
        let converter = MarkdownConverter::with_options(MarkdownOptions {
            text_direction: TextDirectionOption::Vertical,
            ..Default::default()
        });

        let pages = vec![];
        let direction = converter.detect_direction(&pages);
        assert_eq!(direction, TextDirection::Vertical);
    }

    #[test]
    fn test_ocr_result_to_page_content() {
        let blocks = vec![
            ("Title".to_string(), (0, 0, 200, 50), 0.95),
            ("Body text".to_string(), (0, 100, 300, 30), 0.90),
        ];

        let page = MarkdownConverter::ocr_result_to_page_content(1, (800, 600), blocks);

        assert_eq!(page.page_number, 1);
        assert_eq!(page.text_blocks.len(), 2);
        assert_eq!(page.text_blocks[0].text, "Title");
    }

    #[test]
    fn test_pdf_not_found() {
        let converter = MarkdownConverter::new();
        let temp_dir = tempdir().unwrap();

        let result = converter.convert(
            Path::new("/nonexistent/file.pdf"),
            temp_dir.path(),
        );

        assert!(matches!(result, Err(MarkdownError::PdfNotFound(_))));
    }

    #[test]
    fn test_create_provider() {
        let converter = MarkdownConverter::new();

        let claude = converter.create_provider("claude");
        assert_eq!(claude.name(), "claude");

        let openai = converter.create_provider("openai");
        assert_eq!(openai.name(), "openai");

        let local = converter.create_provider("custom");
        assert_eq!(local.name(), "local");
    }

    #[test]
    fn test_conversion_result() {
        let result = MarkdownConversionResult {
            output_path: PathBuf::from("output.md"),
            pages_processed: 10,
            total_blocks: 100,
            validation: None,
            extracted_images: Vec::new(),
            metadata_path: Some(PathBuf::from("metadata.json")),
        };

        assert_eq!(result.pages_processed, 10);
        assert_eq!(result.total_blocks, 100);
        assert!(result.metadata_path.is_some());
    }
}
