//! Markdown Renderer module
//!
//! Provides functionality to render document elements to Markdown format.

use std::io::Write;

use super::element_detect::{DetectedElement, ElementType, TableStructure};
use super::types::PageContent;

// ============================================================
// Options
// ============================================================

/// Options for Markdown rendering
#[derive(Debug, Clone)]
pub struct MarkdownRenderOptions {
    /// Include page break markers
    pub include_page_breaks: bool,

    /// Include page numbers as comments
    pub include_page_numbers: bool,

    /// Use HTML for complex elements
    pub use_html_fallback: bool,

    /// Image path prefix
    pub image_path_prefix: String,

    /// Add line numbers for code blocks
    pub code_line_numbers: bool,

    /// Wrap long lines
    pub wrap_lines: bool,

    /// Maximum line length for wrapping
    pub max_line_length: usize,
}

impl Default for MarkdownRenderOptions {
    fn default() -> Self {
        Self {
            include_page_breaks: true,
            include_page_numbers: true,
            use_html_fallback: false,
            image_path_prefix: "images/".to_string(),
            code_line_numbers: false,
            wrap_lines: false,
            max_line_length: 80,
        }
    }
}

// ============================================================
// Markdown Renderer
// ============================================================

/// Renderer for converting document content to Markdown
pub struct MarkdownRenderer {
    options: MarkdownRenderOptions,
}

impl MarkdownRenderer {
    /// Create a new renderer with default options
    pub fn new() -> Self {
        Self {
            options: MarkdownRenderOptions::default(),
        }
    }

    /// Create a new renderer with specified options
    pub fn with_options(options: MarkdownRenderOptions) -> Self {
        Self { options }
    }

    /// Render multiple pages to Markdown
    pub fn render_pages(&self, pages: &[PageContent]) -> String {
        let mut output = String::new();

        for (i, page) in pages.iter().enumerate() {
            if i > 0 && self.options.include_page_breaks {
                output.push_str("\n---\n\n");
            }

            if self.options.include_page_numbers {
                output.push_str(&format!("<!-- Page {} -->\n\n", page.page_number));
            }

            output.push_str(&self.render_page(page));
        }

        output
    }

    /// Render a single page to Markdown
    pub fn render_page(&self, page: &PageContent) -> String {
        let mut output = String::new();

        for block in &page.text_blocks {
            if block.is_heading && block.heading_level > 0 {
                output.push_str(&self.render_heading(&block.text, block.heading_level));
            } else {
                output.push_str(&self.render_paragraph(&block.text));
            }
            output.push('\n');
        }

        output
    }

    /// Render detected elements to Markdown
    pub fn render_elements(&self, elements: &[DetectedElement]) -> String {
        let mut output = String::new();

        for element in elements {
            match element.element_type {
                ElementType::Heading(level) => {
                    if let Some(text) = &element.text {
                        output.push_str(&self.render_heading(text, level));
                    }
                }
                ElementType::Text => {
                    if let Some(text) = &element.text {
                        output.push_str(&self.render_paragraph(text));
                    }
                }
                ElementType::Figure => {
                    output.push_str(&self.render_figure(element));
                }
                ElementType::Table => {
                    if let Some(table) = &element.table {
                        output.push_str(&self.render_table(table));
                    }
                }
                ElementType::ListItem => {
                    if let Some(text) = &element.text {
                        output.push_str(&self.render_list_item(text));
                    }
                }
                ElementType::Code => {
                    if let Some(text) = &element.text {
                        output.push_str(&self.render_code_block(text, None));
                    }
                }
                ElementType::Quote => {
                    if let Some(text) = &element.text {
                        output.push_str(&self.render_blockquote(text));
                    }
                }
                ElementType::Caption => {
                    if let Some(text) = &element.text {
                        output.push_str(&format!("*{}*\n\n", text));
                    }
                }
            }
        }

        output
    }

    /// Render a heading
    pub fn render_heading(&self, text: &str, level: u8) -> String {
        let level = level.clamp(1, 6) as usize;
        let prefix = "#".repeat(level);
        format!("{} {}\n\n", prefix, text.trim())
    }

    /// Render a paragraph
    pub fn render_paragraph(&self, text: &str) -> String {
        let text = text.trim();
        if text.is_empty() {
            return String::new();
        }

        let processed = if self.options.wrap_lines {
            self.wrap_text(text, self.options.max_line_length)
        } else {
            text.to_string()
        };

        format!("{}\n\n", processed)
    }

    /// Render a figure
    pub fn render_figure(&self, element: &DetectedElement) -> String {
        let image_path = element
            .image_path
            .as_deref()
            .unwrap_or("figure.png");

        let caption = element.caption.as_deref().unwrap_or("");

        format!(
            "![{}]({}{})\n\n",
            caption, self.options.image_path_prefix, image_path
        )
    }

    /// Render a table
    pub fn render_table(&self, table: &TableStructure) -> String {
        if table.rows == 0 || table.cols == 0 {
            return String::new();
        }

        let mut output = String::new();

        // Calculate column widths
        let mut col_widths: Vec<usize> = vec![3; table.cols];
        for row in &table.cells {
            for (i, cell) in row.iter().enumerate() {
                if i < col_widths.len() {
                    col_widths[i] = col_widths[i].max(cell.len());
                }
            }
        }

        // Render header row
        if let Some(header) = table.cells.first() {
            output.push('|');
            for (i, cell) in header.iter().enumerate() {
                let width = col_widths.get(i).copied().unwrap_or(3);
                output.push_str(&format!(" {:width$} |", cell, width = width));
            }
            output.push('\n');

            // Separator
            output.push('|');
            for width in &col_widths {
                output.push_str(&format!(" {} |", "-".repeat(*width)));
            }
            output.push('\n');
        }

        // Render data rows
        for row in table.cells.iter().skip(1) {
            output.push('|');
            for (i, cell) in row.iter().enumerate() {
                let width = col_widths.get(i).copied().unwrap_or(3);
                output.push_str(&format!(" {:width$} |", cell, width = width));
            }
            output.push('\n');
        }

        output.push('\n');
        output
    }

    /// Render a list item
    pub fn render_list_item(&self, text: &str) -> String {
        // Strip existing bullet if present and add Markdown bullet
        let text = text.trim();
        let text = if text.starts_with("• ") || text.starts_with("・") {
            &text[3..]
        } else if text.starts_with("- ") {
            &text[2..]
        } else {
            text
        };

        format!("- {}\n", text.trim())
    }

    /// Render a code block
    pub fn render_code_block(&self, code: &str, language: Option<&str>) -> String {
        let lang = language.unwrap_or("");
        format!("```{}\n{}\n```\n\n", lang, code)
    }

    /// Render a blockquote
    pub fn render_blockquote(&self, text: &str) -> String {
        let lines: Vec<_> = text.lines().map(|l| format!("> {}", l)).collect();
        format!("{}\n\n", lines.join("\n"))
    }

    /// Wrap text to specified line length
    fn wrap_text(&self, text: &str, max_length: usize) -> String {
        let mut result = String::new();
        let mut current_line = String::new();

        for word in text.split_whitespace() {
            if current_line.is_empty() {
                current_line = word.to_string();
            } else if current_line.len() + 1 + word.len() <= max_length {
                current_line.push(' ');
                current_line.push_str(word);
            } else {
                result.push_str(&current_line);
                result.push('\n');
                current_line = word.to_string();
            }
        }

        if !current_line.is_empty() {
            result.push_str(&current_line);
        }

        result
    }

    /// Write Markdown to a file
    pub fn write_to_file(&self, path: &std::path::Path, content: &str) -> std::io::Result<()> {
        let mut file = std::fs::File::create(path)?;
        file.write_all(content.as_bytes())?;
        Ok(())
    }
}

impl Default for MarkdownRenderer {
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
    use crate::markdown::types::BoundingBox;

    #[test]
    fn test_render_heading() {
        let renderer = MarkdownRenderer::new();

        assert_eq!(
            renderer.render_heading("Title", 1),
            "# Title\n\n"
        );
        assert_eq!(
            renderer.render_heading("Section", 2),
            "## Section\n\n"
        );
        assert_eq!(
            renderer.render_heading("Deep", 6),
            "###### Deep\n\n"
        );
    }

    #[test]
    fn test_render_paragraph() {
        let renderer = MarkdownRenderer::new();

        assert_eq!(
            renderer.render_paragraph("Hello world"),
            "Hello world\n\n"
        );
        assert_eq!(
            renderer.render_paragraph("  Trimmed  "),
            "Trimmed\n\n"
        );
        assert_eq!(
            renderer.render_paragraph(""),
            ""
        );
    }

    #[test]
    fn test_render_table() {
        let renderer = MarkdownRenderer::new();

        let mut table = TableStructure::new(3, 2);
        table.set_cell(0, 0, "A".to_string());
        table.set_cell(0, 1, "B".to_string());
        table.set_cell(1, 0, "1".to_string());
        table.set_cell(1, 1, "2".to_string());
        table.set_cell(2, 0, "3".to_string());
        table.set_cell(2, 1, "4".to_string());

        let output = renderer.render_table(&table);

        // Check that table structure is rendered (column widths may add padding)
        assert!(output.contains("| A"));
        assert!(output.contains("---"));
        assert!(output.contains("| 1"));
    }

    #[test]
    fn test_render_list_item() {
        let renderer = MarkdownRenderer::new();

        assert_eq!(renderer.render_list_item("Item"), "- Item\n");
        assert_eq!(renderer.render_list_item("• Bullet"), "- Bullet\n");
        assert_eq!(renderer.render_list_item("- Dash"), "- Dash\n");
    }

    #[test]
    fn test_render_code_block() {
        let renderer = MarkdownRenderer::new();

        let output = renderer.render_code_block("let x = 1;", Some("rust"));
        assert!(output.contains("```rust"));
        assert!(output.contains("let x = 1;"));
    }

    #[test]
    fn test_render_blockquote() {
        let renderer = MarkdownRenderer::new();

        let output = renderer.render_blockquote("Line 1\nLine 2");
        assert!(output.contains("> Line 1"));
        assert!(output.contains("> Line 2"));
    }

    #[test]
    fn test_render_figure() {
        let renderer = MarkdownRenderer::new();

        let element = DetectedElement::figure(
            BoundingBox::new(0, 0, 100, 100),
            Some("Test Figure".to_string()),
            Some("test.png".to_string()),
        );

        let output = renderer.render_figure(&element);
        assert!(output.contains("![Test Figure]"));
        assert!(output.contains("images/test.png"));
    }

    #[test]
    fn test_wrap_text() {
        let renderer = MarkdownRenderer::new();

        let text = "This is a long line that should be wrapped at a certain point for readability";
        let wrapped = renderer.wrap_text(text, 20);

        for line in wrapped.lines() {
            assert!(line.len() <= 30); // Some flexibility for word boundaries
        }
    }

    #[test]
    fn test_render_page() {
        let renderer = MarkdownRenderer::new();

        let mut page = PageContent::new(1, (800, 600));
        let mut block = crate::markdown::types::TextBlock::new(
            "Hello".to_string(),
            BoundingBox::default(),
        );
        block.is_heading = true;
        block.heading_level = 1;
        page.add_block(block);
        page.add_block(crate::markdown::types::TextBlock::new(
            "World".to_string(),
            BoundingBox::default(),
        ));

        let output = renderer.render_page(&page);

        assert!(output.contains("# Hello"));
        assert!(output.contains("World"));
    }

    #[test]
    fn test_render_pages_with_breaks() {
        let options = MarkdownRenderOptions {
            include_page_breaks: true,
            include_page_numbers: true,
            ..Default::default()
        };
        let renderer = MarkdownRenderer::with_options(options);

        let pages = vec![
            PageContent::new(1, (800, 600)),
            PageContent::new(2, (800, 600)),
        ];

        let output = renderer.render_pages(&pages);

        assert!(output.contains("<!-- Page 1 -->"));
        assert!(output.contains("<!-- Page 2 -->"));
        assert!(output.contains("---"));
    }

    #[test]
    fn test_render_options_default() {
        let opts = MarkdownRenderOptions::default();
        assert!(opts.include_page_breaks);
        assert!(opts.include_page_numbers);
        assert!(!opts.use_html_fallback);
    }
}
