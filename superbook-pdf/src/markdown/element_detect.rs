//! Element Detection module
//!
//! Provides functionality to detect figures, tables, and other
//! structural elements in scanned document pages.

use super::types::{BoundingBox, TextBlock};

// ============================================================
// Types
// ============================================================

/// Type of detected element
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ElementType {
    /// Text paragraph
    Text,
    /// Heading
    Heading(u8), // Level 1-6
    /// Figure/Image
    Figure,
    /// Table
    Table,
    /// Caption (for figure or table)
    Caption,
    /// List item
    ListItem,
    /// Code block
    Code,
    /// Quote/Blockquote
    Quote,
}

/// A detected structural element
#[derive(Debug, Clone)]
pub struct DetectedElement {
    /// Element type
    pub element_type: ElementType,

    /// Bounding box
    pub bbox: BoundingBox,

    /// Text content (if applicable)
    pub text: Option<String>,

    /// Associated text blocks
    pub blocks: Vec<TextBlock>,

    /// Detection confidence (0.0-1.0)
    pub confidence: f64,

    /// Caption text (for figures/tables)
    pub caption: Option<String>,

    /// Table structure (for tables)
    pub table: Option<TableStructure>,

    /// Image path (for figures)
    pub image_path: Option<String>,
}

impl DetectedElement {
    /// Create a text element
    pub fn text(blocks: Vec<TextBlock>, bbox: BoundingBox) -> Self {
        let text = blocks
            .iter()
            .map(|b| b.text.as_str())
            .collect::<Vec<_>>()
            .join(" ");

        Self {
            element_type: ElementType::Text,
            bbox,
            text: Some(text),
            blocks,
            confidence: 1.0,
            caption: None,
            table: None,
            image_path: None,
        }
    }

    /// Create a heading element
    pub fn heading(text: String, level: u8, bbox: BoundingBox) -> Self {
        Self {
            element_type: ElementType::Heading(level.clamp(1, 6)),
            bbox,
            text: Some(text),
            blocks: Vec::new(),
            confidence: 1.0,
            caption: None,
            table: None,
            image_path: None,
        }
    }

    /// Create a figure element
    pub fn figure(bbox: BoundingBox, caption: Option<String>, image_path: Option<String>) -> Self {
        Self {
            element_type: ElementType::Figure,
            bbox,
            text: None,
            blocks: Vec::new(),
            confidence: 0.8,
            caption,
            table: None,
            image_path,
        }
    }

    /// Create a table element
    pub fn table(bbox: BoundingBox, structure: TableStructure, caption: Option<String>) -> Self {
        Self {
            element_type: ElementType::Table,
            bbox,
            text: None,
            blocks: Vec::new(),
            confidence: 0.7,
            caption,
            table: Some(structure),
            image_path: None,
        }
    }
}

/// Structure of a detected table
#[derive(Debug, Clone)]
pub struct TableStructure {
    /// Number of rows
    pub rows: usize,

    /// Number of columns
    pub cols: usize,

    /// Cell contents (row-major order)
    pub cells: Vec<Vec<String>>,

    /// Has header row
    pub has_header: bool,
}

impl TableStructure {
    /// Create an empty table structure
    pub fn new(rows: usize, cols: usize) -> Self {
        Self {
            rows,
            cols,
            cells: vec![vec![String::new(); cols]; rows],
            has_header: false,
        }
    }

    /// Set a cell value
    pub fn set_cell(&mut self, row: usize, col: usize, value: String) {
        if row < self.rows && col < self.cols {
            self.cells[row][col] = value;
        }
    }

    /// Get a cell value
    pub fn get_cell(&self, row: usize, col: usize) -> Option<&str> {
        self.cells.get(row)?.get(col).map(|s| s.as_str())
    }
}

// ============================================================
// Element Detector
// ============================================================

/// Detector for structural elements in document pages
pub struct ElementDetector;

impl ElementDetector {
    /// Detect elements from text blocks
    pub fn detect_elements(blocks: &[TextBlock], _page_size: (u32, u32)) -> Vec<DetectedElement> {
        let mut elements = Vec::new();

        // Detect headings
        let median_font_size = Self::median_font_size(blocks);

        for block in blocks {
            if let Some(level) = Self::estimate_heading_level(block.font_size, median_font_size) {
                elements.push(DetectedElement::heading(
                    block.text.clone(),
                    level,
                    block.bbox,
                ));
            }
        }

        // Detect figures (low OCR confidence regions)
        for block in blocks {
            if block.confidence < 0.3 && block.bbox.area() > 10000 {
                // Check for figure caption nearby
                let caption = Self::find_caption_for(block, blocks);
                elements.push(DetectedElement::figure(block.bbox, caption, None));
            }
        }

        elements
    }

    /// Calculate median font size from blocks
    fn median_font_size(blocks: &[TextBlock]) -> f32 {
        if blocks.is_empty() {
            return 12.0;
        }

        let mut sizes: Vec<f32> = blocks
            .iter()
            .filter(|b| b.font_size > 0.0)
            .map(|b| b.font_size)
            .collect();

        if sizes.is_empty() {
            return 12.0;
        }

        sizes.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        sizes[sizes.len() / 2]
    }

    /// Estimate heading level from font size
    pub fn estimate_heading_level(font_size: f32, median: f32) -> Option<u8> {
        if font_size <= 0.0 || median <= 0.0 {
            return None;
        }

        let ratio = font_size / median;

        match ratio {
            r if r >= 2.5 => Some(1),
            r if r >= 2.0 => Some(2),
            r if r >= 1.5 => Some(3),
            r if r >= 1.25 => Some(4),
            _ => None,
        }
    }

    /// Find caption text for a figure/table
    fn find_caption_for(element: &TextBlock, blocks: &[TextBlock]) -> Option<String> {
        // Look for text below the element that starts with "図" or "Figure"
        let caption_patterns = ["図", "Figure", "Fig.", "表", "Table"];

        for block in blocks {
            // Check if block is below the element
            if block.bbox.y > element.bbox.bottom()
                && block.bbox.y < element.bbox.bottom() + 100
            {
                let text = block.text.trim();
                for pattern in &caption_patterns {
                    if text.starts_with(pattern) {
                        return Some(text.to_string());
                    }
                }
            }
        }

        None
    }

    /// Detect table from grid lines (simplified)
    pub fn detect_table_structure(
        _image: &[u8],
        _width: u32,
        _height: u32,
        blocks: &[TextBlock],
        bbox: BoundingBox,
    ) -> Option<TableStructure> {
        // Simplified table detection: analyze text blocks within the bounding box
        let contained_blocks: Vec<_> = blocks
            .iter()
            .filter(|b| bbox.overlaps(&b.bbox))
            .collect();

        if contained_blocks.len() < 4 {
            return None;
        }

        // Estimate grid structure from block positions
        let mut x_positions: Vec<u32> = contained_blocks.iter().map(|b| b.bbox.x).collect();
        let mut y_positions: Vec<u32> = contained_blocks.iter().map(|b| b.bbox.y).collect();

        x_positions.sort();
        y_positions.sort();

        x_positions.dedup_by(|a, b| (*a as i32 - *b as i32).unsigned_abs() < 20);
        y_positions.dedup_by(|a, b| (*a as i32 - *b as i32).unsigned_abs() < 20);

        let cols = x_positions.len();
        let rows = y_positions.len();

        if cols < 2 || rows < 2 {
            return None;
        }

        let mut table = TableStructure::new(rows, cols);

        // Assign blocks to cells
        for block in contained_blocks {
            let col = x_positions
                .iter()
                .position(|&x| (block.bbox.x as i32 - x as i32).unsigned_abs() < 20)
                .unwrap_or(0);

            let row = y_positions
                .iter()
                .position(|&y| (block.bbox.y as i32 - y as i32).unsigned_abs() < 20)
                .unwrap_or(0);

            table.set_cell(row, col, block.text.clone());
        }

        // First row is likely header
        table.has_header = true;

        Some(table)
    }

    /// Check if a block is likely a list item
    pub fn is_list_item(text: &str) -> bool {
        let trimmed = text.trim();

        // Check for common list markers
        let patterns = [
            "• ", "・", "- ", "– ", "— ", "* ",
            "1. ", "2. ", "3. ", "4. ", "5. ",
            "a) ", "b) ", "c) ",
            "(1)", "(2)", "(3)",
            "①", "②", "③", "④", "⑤",
        ];

        patterns.iter().any(|p| trimmed.starts_with(p))
    }
}

// ============================================================
// Tests
// ============================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn make_block(text: &str, x: u32, y: u32, w: u32, h: u32, font_size: f32) -> TextBlock {
        let mut block = TextBlock::new(text.to_string(), BoundingBox::new(x, y, w, h));
        block.font_size = font_size;
        block
    }

    #[test]
    fn test_estimate_heading_level() {
        // Level 1: 2.5x median
        assert_eq!(ElementDetector::estimate_heading_level(30.0, 12.0), Some(1));

        // Level 2: 2.0x median
        assert_eq!(ElementDetector::estimate_heading_level(24.0, 12.0), Some(2));

        // Level 3: 1.5x median
        assert_eq!(ElementDetector::estimate_heading_level(18.0, 12.0), Some(3));

        // Level 4: 1.25x median
        assert_eq!(ElementDetector::estimate_heading_level(15.0, 12.0), Some(4));

        // Not a heading
        assert_eq!(ElementDetector::estimate_heading_level(12.0, 12.0), None);
    }

    #[test]
    fn test_table_structure() {
        let mut table = TableStructure::new(3, 2);
        table.set_cell(0, 0, "A".to_string());
        table.set_cell(0, 1, "B".to_string());
        table.set_cell(1, 0, "C".to_string());
        table.set_cell(1, 1, "D".to_string());

        assert_eq!(table.rows, 3);
        assert_eq!(table.cols, 2);
        assert_eq!(table.get_cell(0, 0), Some("A"));
        assert_eq!(table.get_cell(1, 1), Some("D"));
        assert_eq!(table.get_cell(10, 10), None);
    }

    #[test]
    fn test_detected_element_text() {
        let blocks = vec![
            make_block("Hello", 0, 0, 50, 20, 12.0),
            make_block("World", 60, 0, 50, 20, 12.0),
        ];
        let bbox = BoundingBox::new(0, 0, 120, 20);

        let element = DetectedElement::text(blocks.clone(), bbox);

        assert_eq!(element.element_type, ElementType::Text);
        assert!(element.text.as_ref().unwrap().contains("Hello"));
    }

    #[test]
    fn test_detected_element_heading() {
        let element = DetectedElement::heading("Chapter 1".to_string(), 1, BoundingBox::default());

        assert_eq!(element.element_type, ElementType::Heading(1));
        assert_eq!(element.text, Some("Chapter 1".to_string()));
    }

    #[test]
    fn test_detected_element_figure() {
        let element = DetectedElement::figure(
            BoundingBox::new(0, 0, 200, 150),
            Some("Figure 1: Example".to_string()),
            Some("images/fig1.png".to_string()),
        );

        assert_eq!(element.element_type, ElementType::Figure);
        assert!(element.caption.is_some());
        assert!(element.image_path.is_some());
    }

    #[test]
    fn test_is_list_item() {
        assert!(ElementDetector::is_list_item("• Item"));
        assert!(ElementDetector::is_list_item("1. First"));
        assert!(ElementDetector::is_list_item("- Dash item"));
        assert!(ElementDetector::is_list_item("①番目"));

        assert!(!ElementDetector::is_list_item("Normal text"));
        assert!(!ElementDetector::is_list_item(""));
    }

    #[test]
    fn test_detect_elements() {
        // Use more blocks so median is closer to body text size
        let blocks = vec![
            make_block("Title", 0, 0, 200, 50, 30.0), // Large font = heading
            make_block("Body text 1", 0, 100, 300, 20, 12.0),
            make_block("Body text 2", 0, 130, 300, 20, 12.0),
            make_block("Body text 3", 0, 160, 300, 20, 12.0),
        ];

        let elements = ElementDetector::detect_elements(&blocks, (400, 600));

        // Should detect at least one heading (30/12 = 2.5, so level 1)
        let headings: Vec<_> = elements
            .iter()
            .filter(|e| matches!(e.element_type, ElementType::Heading(_)))
            .collect();

        assert!(!headings.is_empty());
    }

    #[test]
    fn test_median_font_size() {
        let blocks = vec![
            make_block("A", 0, 0, 10, 10, 10.0),
            make_block("B", 0, 0, 10, 10, 12.0),
            make_block("C", 0, 0, 10, 10, 14.0),
        ];

        let median = ElementDetector::median_font_size(&blocks);
        assert_eq!(median, 12.0);
    }

    #[test]
    fn test_median_font_size_empty() {
        let median = ElementDetector::median_font_size(&[]);
        assert_eq!(median, 12.0); // Default
    }
}
