//! Reading Order Detection and Sorting
//!
//! Provides functionality to detect and sort text elements
//! in the correct reading order (vertical or horizontal).

use super::types::TextBlock;
#[cfg(test)]
use super::types::BoundingBox;

// ============================================================
// Types
// ============================================================

/// Text direction for reading order
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TextDirection {
    /// Horizontal (left-to-right, top-to-bottom) - Western text
    #[default]
    Horizontal,
    /// Vertical (right-to-left, top-to-bottom) - Japanese vertical text
    Vertical,
}

/// Options for reading order detection
#[derive(Debug, Clone)]
pub struct ReadingOrderOptions {
    /// Force a specific text direction
    pub forced_direction: Option<TextDirection>,

    /// Column detection tolerance (pixels)
    pub column_tolerance: u32,

    /// Line detection tolerance (pixels)
    pub line_tolerance: u32,
}

impl Default for ReadingOrderOptions {
    fn default() -> Self {
        Self {
            forced_direction: None,
            column_tolerance: 30,
            line_tolerance: 10,
        }
    }
}

// ============================================================
// Reading Order Sorter
// ============================================================

/// Sorter for arranging text blocks in reading order
pub struct ReadingOrderSorter;

impl ReadingOrderSorter {
    /// Sort text blocks in reading order
    pub fn sort(blocks: &mut [TextBlock], direction: TextDirection) {
        match direction {
            TextDirection::Horizontal => Self::sort_horizontal(blocks),
            TextDirection::Vertical => Self::sort_vertical(blocks),
        }
    }

    /// Sort for horizontal reading (Western text)
    /// Order: left-to-right, top-to-bottom
    pub fn sort_horizontal(blocks: &mut [TextBlock]) {
        blocks.sort_by(|a, b| {
            // Primary: top-to-bottom (Y coordinate)
            let y_cmp = a.bbox.y.cmp(&b.bbox.y);
            if y_cmp != std::cmp::Ordering::Equal {
                return y_cmp;
            }
            // Secondary: left-to-right (X coordinate)
            a.bbox.x.cmp(&b.bbox.x)
        });
    }

    /// Sort for vertical reading (Japanese text)
    /// Order: right-to-left, top-to-bottom
    pub fn sort_vertical(blocks: &mut [TextBlock]) {
        blocks.sort_by(|a, b| {
            // Primary: right-to-left (X coordinate, descending)
            let x_cmp = b.bbox.x.cmp(&a.bbox.x);
            if x_cmp != std::cmp::Ordering::Equal {
                return x_cmp;
            }
            // Secondary: top-to-bottom (Y coordinate)
            a.bbox.y.cmp(&b.bbox.y)
        });
    }

    /// Detect text direction from block arrangement
    pub fn detect_direction(blocks: &[TextBlock]) -> TextDirection {
        if blocks.len() < 2 {
            return TextDirection::Horizontal;
        }

        // Analyze block positions to determine direction
        // Vertical text: blocks tend to be tall and narrow, arranged right-to-left
        // Horizontal text: blocks tend to be wide and short, arranged left-to-right

        let mut tall_count = 0;
        let mut wide_count = 0;

        for block in blocks {
            if block.bbox.height > block.bbox.width {
                tall_count += 1;
            } else {
                wide_count += 1;
            }
        }

        // If most blocks are tall, assume vertical text
        if tall_count > wide_count * 2 {
            TextDirection::Vertical
        } else {
            TextDirection::Horizontal
        }
    }

    /// Group blocks into lines/columns
    pub fn group_into_lines(
        blocks: &[TextBlock],
        direction: TextDirection,
        tolerance: u32,
    ) -> Vec<Vec<&TextBlock>> {
        if blocks.is_empty() {
            return Vec::new();
        }

        let mut groups: Vec<Vec<&TextBlock>> = Vec::new();

        for block in blocks {
            let mut found_group = false;

            for group in &mut groups {
                let reference = group[0];

                let in_same_line = match direction {
                    TextDirection::Horizontal => {
                        // Same line if Y coordinates are similar
                        (block.bbox.y as i32 - reference.bbox.y as i32).unsigned_abs() <= tolerance
                    }
                    TextDirection::Vertical => {
                        // Same column if X coordinates are similar
                        (block.bbox.x as i32 - reference.bbox.x as i32).unsigned_abs() <= tolerance
                    }
                };

                if in_same_line {
                    group.push(block);
                    found_group = true;
                    break;
                }
            }

            if !found_group {
                groups.push(vec![block]);
            }
        }

        // Sort blocks within each group
        for group in &mut groups {
            match direction {
                TextDirection::Horizontal => {
                    group.sort_by(|a, b| a.bbox.x.cmp(&b.bbox.x));
                }
                TextDirection::Vertical => {
                    group.sort_by(|a, b| a.bbox.y.cmp(&b.bbox.y));
                }
            }
        }

        // Sort groups
        match direction {
            TextDirection::Horizontal => {
                groups.sort_by(|a, b| a[0].bbox.y.cmp(&b[0].bbox.y));
            }
            TextDirection::Vertical => {
                groups.sort_by(|a, b| b[0].bbox.x.cmp(&a[0].bbox.x)); // Right-to-left
            }
        }

        groups
    }

    /// Calculate reading order index for a block
    pub fn reading_order_index(
        block: &TextBlock,
        page_width: u32,
        page_height: u32,
        direction: TextDirection,
    ) -> u64 {
        match direction {
            TextDirection::Horizontal => {
                // Y primary, X secondary
                (block.bbox.y as u64 * page_width as u64) + block.bbox.x as u64
            }
            TextDirection::Vertical => {
                // X (inverted) primary, Y secondary
                let inverted_x = page_width.saturating_sub(block.bbox.x);
                (inverted_x as u64 * page_height as u64) + block.bbox.y as u64
            }
        }
    }
}

// ============================================================
// Tests
// ============================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn make_block(text: &str, x: u32, y: u32, w: u32, h: u32) -> TextBlock {
        TextBlock::new(text.to_string(), BoundingBox::new(x, y, w, h))
    }

    #[test]
    fn test_sort_horizontal() {
        let mut blocks = vec![
            make_block("C", 200, 0, 50, 20),
            make_block("A", 0, 0, 50, 20),
            make_block("B", 100, 0, 50, 20),
            make_block("D", 0, 50, 50, 20),
        ];

        ReadingOrderSorter::sort_horizontal(&mut blocks);

        assert_eq!(blocks[0].text, "A");
        assert_eq!(blocks[1].text, "B");
        assert_eq!(blocks[2].text, "C");
        assert_eq!(blocks[3].text, "D");
    }

    #[test]
    fn test_sort_vertical() {
        let mut blocks = vec![
            make_block("A", 0, 0, 20, 50),   // Left column
            make_block("C", 100, 0, 20, 50), // Right column, top
            make_block("B", 0, 60, 20, 50),  // Left column, bottom
            make_block("D", 100, 60, 20, 50), // Right column, bottom
        ];

        ReadingOrderSorter::sort_vertical(&mut blocks);

        // Right-to-left, top-to-bottom
        assert_eq!(blocks[0].text, "C");
        assert_eq!(blocks[1].text, "D");
        assert_eq!(blocks[2].text, "A");
        assert_eq!(blocks[3].text, "B");
    }

    #[test]
    fn test_detect_direction_horizontal() {
        let blocks = vec![
            make_block("Wide", 0, 0, 200, 30),
            make_block("Text", 0, 40, 180, 30),
        ];

        let direction = ReadingOrderSorter::detect_direction(&blocks);
        assert_eq!(direction, TextDirection::Horizontal);
    }

    #[test]
    fn test_detect_direction_vertical() {
        let blocks = vec![
            make_block("Tall", 100, 0, 30, 200),
            make_block("Text", 60, 0, 30, 180),
            make_block("More", 20, 0, 30, 150),
        ];

        let direction = ReadingOrderSorter::detect_direction(&blocks);
        assert_eq!(direction, TextDirection::Vertical);
    }

    #[test]
    fn test_group_into_lines() {
        let blocks = vec![
            make_block("A1", 0, 0, 50, 20),
            make_block("A2", 60, 5, 50, 20),  // Same line
            make_block("B1", 0, 50, 50, 20),  // New line
        ];

        let groups = ReadingOrderSorter::group_into_lines(&blocks, TextDirection::Horizontal, 15);

        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].len(), 2);
        assert_eq!(groups[1].len(), 1);
    }

    #[test]
    fn test_reading_order_index() {
        let block = TextBlock::new("Test".to_string(), BoundingBox::new(100, 50, 50, 20));

        let h_idx = ReadingOrderSorter::reading_order_index(&block, 800, 600, TextDirection::Horizontal);
        let v_idx = ReadingOrderSorter::reading_order_index(&block, 800, 600, TextDirection::Vertical);

        // Horizontal: 50 * 800 + 100 = 40100
        assert_eq!(h_idx, 40100);

        // Vertical: (800 - 100) * 600 + 50 = 420050
        assert_eq!(v_idx, 420050);
    }

    #[test]
    fn test_reading_order_options_default() {
        let opts = ReadingOrderOptions::default();
        assert!(opts.forced_direction.is_none());
        assert_eq!(opts.column_tolerance, 30);
    }
}
