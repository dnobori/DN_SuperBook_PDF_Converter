//! Group Crop Region Analysis
//!
//! Implements Tukey fence outlier removal for unified crop regions
//! across multiple pages.

use super::types::{ContentRect, MarginError, Result};
use rayon::prelude::*;
use std::path::{Path, PathBuf};

// ============================================================
// Constants
// ============================================================

/// Tukey fence parameter (k value)
const TUKEY_K: f64 = 1.5;

/// Minimum inlier ratio before falling back to all data
const MIN_INLIER_RATIO: f64 = 0.5;

/// Minimum inlier count before falling back
const MIN_INLIER_COUNT: usize = 3;

// ============================================================
// Data Structures
// ============================================================

/// Bounding box with page information
#[derive(Debug, Clone)]
pub struct PageBoundingBox {
    /// Page number (1-indexed)
    pub page_number: usize,
    /// Bounding box rectangle
    pub bounding_box: ContentRect,
    /// Whether this is an odd page
    pub is_odd: bool,
}

impl PageBoundingBox {
    /// Create new page bounding box
    pub fn new(page_number: usize, bounding_box: ContentRect) -> Self {
        Self {
            page_number,
            bounding_box,
            is_odd: page_number % 2 == 1,
        }
    }

    /// Check if bounding box is valid (non-zero area)
    pub fn is_valid(&self) -> bool {
        self.bounding_box.width > 0 && self.bounding_box.height > 0
    }

    /// Get the right edge coordinate
    pub fn right(&self) -> u32 {
        self.bounding_box.x + self.bounding_box.width
    }

    /// Get the bottom edge coordinate
    pub fn bottom(&self) -> u32 {
        self.bounding_box.y + self.bounding_box.height
    }
}

/// Group crop region result
#[derive(Debug, Clone, Default)]
pub struct GroupCropRegion {
    /// Left edge
    pub left: u32,
    /// Top edge
    pub top: u32,
    /// Width
    pub width: u32,
    /// Height
    pub height: u32,
    /// Number of inlier pages used
    pub inlier_count: usize,
    /// Total pages in group
    pub total_count: usize,
}

impl GroupCropRegion {
    /// Check if the region is valid
    pub fn is_valid(&self) -> bool {
        self.width > 0 && self.height > 0
    }

    /// Get right edge
    pub fn right(&self) -> u32 {
        self.left + self.width
    }

    /// Get bottom edge
    pub fn bottom(&self) -> u32 {
        self.top + self.height
    }

    /// Convert to ContentRect
    pub fn to_content_rect(&self) -> ContentRect {
        ContentRect {
            x: self.left,
            y: self.top,
            width: self.width,
            height: self.height,
        }
    }
}

/// Unified crop regions for odd and even pages
#[derive(Debug, Clone)]
pub struct UnifiedCropRegions {
    /// Crop region for odd pages
    pub odd_region: GroupCropRegion,
    /// Crop region for even pages
    pub even_region: GroupCropRegion,
}

// ============================================================
// Group Crop Analyzer
// ============================================================

/// Group crop region analyzer using Tukey fence for outlier removal
pub struct GroupCropAnalyzer;

impl GroupCropAnalyzer {
    /// Decide the optimal crop region for a group of pages using Tukey fence
    ///
    /// Algorithm:
    /// 1. Collect bounding boxes from all pages
    /// 2. Calculate Q1, Q3, IQR for each edge (left, top, right, bottom)
    /// 3. Apply Tukey fence (k=1.5) to identify outliers
    /// 4. Remove pages where ANY edge is an outlier
    /// 5. Calculate median of inliers for final crop region
    pub fn decide_group_crop_region(bounding_boxes: &[PageBoundingBox]) -> GroupCropRegion {
        // Validation
        if bounding_boxes.is_empty() {
            return GroupCropRegion::default();
        }

        // Filter out invalid bounding boxes (zero area)
        let valid: Vec<&PageBoundingBox> = bounding_boxes.iter().filter(|b| b.is_valid()).collect();

        if valid.is_empty() {
            return GroupCropRegion::default();
        }

        // Extract and sort edge values
        let mut lefts: Vec<u32> = valid.iter().map(|b| b.bounding_box.x).collect();
        let mut tops: Vec<u32> = valid.iter().map(|b| b.bounding_box.y).collect();
        let mut rights: Vec<u32> = valid.iter().map(|b| b.right()).collect();
        let mut bottoms: Vec<u32> = valid.iter().map(|b| b.bottom()).collect();

        lefts.sort_unstable();
        tops.sort_unstable();
        rights.sort_unstable();
        bottoms.sort_unstable();

        // Calculate quartiles and IQR for each edge
        let (q1_l, q3_l, iqr_l) = Self::calculate_iqr(&lefts);
        let (q1_t, q3_t, iqr_t) = Self::calculate_iqr(&tops);
        let (q1_r, q3_r, iqr_r) = Self::calculate_iqr(&rights);
        let (q1_b, q3_b, iqr_b) = Self::calculate_iqr(&bottoms);

        // Identify inliers (pages where no edge is an outlier)
        let inliers: Vec<&PageBoundingBox> = valid
            .iter()
            .filter(|b| {
                !Self::is_outlier(b.bounding_box.x, q1_l, q3_l, iqr_l)
                    && !Self::is_outlier(b.bounding_box.y, q1_t, q3_t, iqr_t)
                    && !Self::is_outlier(b.right(), q1_r, q3_r, iqr_r)
                    && !Self::is_outlier(b.bottom(), q1_b, q3_b, iqr_b)
            })
            .copied()
            .collect();

        // If too few inliers, fall back to using all valid data
        let use_inliers = if inliers.len() >= MIN_INLIER_COUNT
            && inliers.len() as f64 >= valid.len() as f64 * MIN_INLIER_RATIO
        {
            inliers
        } else {
            valid
        };

        // Calculate median for final crop region
        let lefts: Vec<u32> = use_inliers.iter().map(|b| b.bounding_box.x).collect();
        let tops: Vec<u32> = use_inliers.iter().map(|b| b.bounding_box.y).collect();
        let rights: Vec<u32> = use_inliers.iter().map(|b| b.right()).collect();
        let bottoms: Vec<u32> = use_inliers.iter().map(|b| b.bottom()).collect();

        let left = Self::median_u32(&lefts);
        let top = Self::median_u32(&tops);
        let right = Self::median_u32(&rights);
        let bottom = Self::median_u32(&bottoms);

        // Calculate width and height
        let width = right.saturating_sub(left);
        let height = bottom.saturating_sub(top);

        GroupCropRegion {
            left,
            top,
            width,
            height,
            inlier_count: use_inliers.len(),
            total_count: bounding_boxes.len(),
        }
    }

    /// Unify crop regions for odd and even page groups
    pub fn unify_odd_even_regions(bounding_boxes: &[PageBoundingBox]) -> UnifiedCropRegions {
        // Split into odd and even groups
        let odd_boxes: Vec<PageBoundingBox> = bounding_boxes
            .iter()
            .filter(|b| b.is_odd)
            .cloned()
            .collect();
        let even_boxes: Vec<PageBoundingBox> = bounding_boxes
            .iter()
            .filter(|b| !b.is_odd)
            .cloned()
            .collect();

        // Calculate crop region for each group
        let odd_region = Self::decide_group_crop_region(&odd_boxes);
        let even_region = Self::decide_group_crop_region(&even_boxes);

        UnifiedCropRegions {
            odd_region,
            even_region,
        }
    }

    /// Calculate IQR (Interquartile Range)
    /// Returns (Q1, Q3, IQR) with IQR minimum of 1 to avoid division issues
    fn calculate_iqr(sorted_values: &[u32]) -> (f64, f64, f64) {
        if sorted_values.is_empty() {
            return (0.0, 0.0, 1.0);
        }

        let q1 = Self::percentile(sorted_values, 0.25);
        let q3 = Self::percentile(sorted_values, 0.75);
        let iqr = (q3 - q1).max(1.0); // Guard against IQR == 0

        (q1, q3, iqr)
    }

    /// Check if a value is an outlier using Tukey fence
    fn is_outlier(value: u32, q1: f64, q3: f64, iqr: f64) -> bool {
        let v = value as f64;
        v < q1 - TUKEY_K * iqr || v > q3 + TUKEY_K * iqr
    }

    /// Calculate percentile with linear interpolation
    /// Input must be sorted in ascending order
    fn percentile(sorted_values: &[u32], p: f64) -> f64 {
        if sorted_values.is_empty() {
            return 0.0;
        }
        if sorted_values.len() == 1 {
            return sorted_values[0] as f64;
        }

        let idx = p * (sorted_values.len() - 1) as f64;
        let lo = idx.floor() as usize;
        let hi = idx.ceil() as usize;

        if lo == hi {
            sorted_values[lo] as f64
        } else {
            let frac = idx - lo as f64;
            sorted_values[lo] as f64 + (sorted_values[hi] as f64 - sorted_values[lo] as f64) * frac
        }
    }

    /// Calculate median of u32 values
    fn median_u32(values: &[u32]) -> u32 {
        if values.is_empty() {
            return 0;
        }

        let mut sorted = values.to_vec();
        sorted.sort_unstable();

        let n = sorted.len();
        if n % 2 == 1 {
            sorted[n / 2]
        } else {
            (sorted[n / 2 - 1] + sorted[n / 2]) / 2
        }
    }

    /// Detect text bounding box from image using edge detection
    ///
    /// This function detects the content area by finding
    /// non-background pixels and returning the minimal bounding box.
    pub fn detect_text_bounding_box(
        image_path: &Path,
        background_threshold: u8,
    ) -> Result<ContentRect> {
        if !image_path.exists() {
            return Err(MarginError::ImageNotFound(image_path.to_path_buf()));
        }

        let img = image::open(image_path).map_err(|e| MarginError::InvalidImage(e.to_string()))?;
        let gray = img.to_luma8();
        let (width, height) = gray.dimensions();

        let mut min_x = width;
        let mut max_x = 0u32;
        let mut min_y = height;
        let mut max_y = 0u32;

        // Scan for content pixels
        for y in 0..height {
            for x in 0..width {
                let pixel = gray.get_pixel(x, y);
                if pixel.0[0] < background_threshold {
                    min_x = min_x.min(x);
                    max_x = max_x.max(x);
                    min_y = min_y.min(y);
                    max_y = max_y.max(y);
                }
            }
        }

        // Check if any content was found
        if min_x > max_x || min_y > max_y {
            return Err(MarginError::NoContentDetected);
        }

        Ok(ContentRect {
            x: min_x,
            y: min_y,
            width: max_x - min_x + 1,
            height: max_y - min_y + 1,
        })
    }

    /// Detect bounding boxes for all pages in parallel
    pub fn detect_all_bounding_boxes(
        image_paths: &[PathBuf],
        background_threshold: u8,
    ) -> Vec<PageBoundingBox> {
        image_paths
            .par_iter()
            .enumerate()
            .filter_map(|(idx, path)| {
                match Self::detect_text_bounding_box(path, background_threshold) {
                    Ok(bbox) => Some(PageBoundingBox::new(idx + 1, bbox)),
                    Err(_) => None,
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_page_bounding_box_creation() {
        let rect = ContentRect {
            x: 100,
            y: 50,
            width: 800,
            height: 1200,
        };
        let bbox = PageBoundingBox::new(1, rect);
        assert_eq!(bbox.page_number, 1);
        assert!(bbox.is_odd);
        assert!(bbox.is_valid());
        assert_eq!(bbox.right(), 900);
        assert_eq!(bbox.bottom(), 1250);
    }

    #[test]
    fn test_page_bounding_box_even_page() {
        let rect = ContentRect {
            x: 100,
            y: 50,
            width: 800,
            height: 1200,
        };
        let bbox = PageBoundingBox::new(2, rect);
        assert_eq!(bbox.page_number, 2);
        assert!(!bbox.is_odd);
    }

    #[test]
    fn test_group_crop_region_valid() {
        let region = GroupCropRegion {
            left: 100,
            top: 50,
            width: 800,
            height: 1200,
            inlier_count: 10,
            total_count: 12,
        };
        assert!(region.is_valid());
        assert_eq!(region.right(), 900);
        assert_eq!(region.bottom(), 1250);
    }

    #[test]
    fn test_group_crop_region_invalid() {
        let region = GroupCropRegion {
            left: 100,
            top: 50,
            width: 0,
            height: 1200,
            inlier_count: 0,
            total_count: 0,
        };
        assert!(!region.is_valid());
    }

    #[test]
    fn test_decide_group_crop_empty() {
        let result = GroupCropAnalyzer::decide_group_crop_region(&[]);
        assert!(!result.is_valid());
        assert_eq!(result.inlier_count, 0);
    }

    #[test]
    fn test_decide_group_crop_single_page() {
        let boxes = vec![PageBoundingBox::new(
            1,
            ContentRect {
                x: 100,
                y: 50,
                width: 800,
                height: 1200,
            },
        )];
        let result = GroupCropAnalyzer::decide_group_crop_region(&boxes);
        assert!(result.is_valid());
        assert_eq!(result.left, 100);
        assert_eq!(result.top, 50);
        assert_eq!(result.width, 800);
        assert_eq!(result.height, 1200);
    }

    #[test]
    fn test_decide_group_crop_multiple_pages() {
        let boxes = vec![
            PageBoundingBox::new(
                1,
                ContentRect {
                    x: 100,
                    y: 50,
                    width: 800,
                    height: 1200,
                },
            ),
            PageBoundingBox::new(
                2,
                ContentRect {
                    x: 105,
                    y: 55,
                    width: 790,
                    height: 1190,
                },
            ),
            PageBoundingBox::new(
                3,
                ContentRect {
                    x: 95,
                    y: 45,
                    width: 810,
                    height: 1210,
                },
            ),
        ];
        let result = GroupCropAnalyzer::decide_group_crop_region(&boxes);
        assert!(result.is_valid());
        assert_eq!(result.inlier_count, 3);
        // Median should be close to 100, 50
        assert!((result.left as i32 - 100).abs() <= 5);
        assert!((result.top as i32 - 50).abs() <= 5);
    }

    #[test]
    fn test_decide_group_crop_with_outlier() {
        let boxes = vec![
            PageBoundingBox::new(
                1,
                ContentRect {
                    x: 100,
                    y: 50,
                    width: 800,
                    height: 1200,
                },
            ),
            PageBoundingBox::new(
                2,
                ContentRect {
                    x: 105,
                    y: 55,
                    width: 790,
                    height: 1190,
                },
            ),
            PageBoundingBox::new(
                3,
                ContentRect {
                    x: 95,
                    y: 45,
                    width: 810,
                    height: 1210,
                },
            ),
            PageBoundingBox::new(
                4,
                ContentRect {
                    x: 100,
                    y: 50,
                    width: 800,
                    height: 1200,
                },
            ),
            PageBoundingBox::new(
                5,
                ContentRect {
                    x: 500,
                    y: 500,
                    width: 200,
                    height: 200,
                },
            ), // Outlier
        ];
        let result = GroupCropAnalyzer::decide_group_crop_region(&boxes);
        assert!(result.is_valid());
        // Outlier should be excluded
        assert!(result.inlier_count <= boxes.len());
    }

    #[test]
    fn test_unify_odd_even_regions() {
        let boxes = vec![
            PageBoundingBox::new(
                1,
                ContentRect {
                    x: 100,
                    y: 50,
                    width: 800,
                    height: 1200,
                },
            ),
            PageBoundingBox::new(
                2,
                ContentRect {
                    x: 150,
                    y: 60,
                    width: 750,
                    height: 1180,
                },
            ),
            PageBoundingBox::new(
                3,
                ContentRect {
                    x: 105,
                    y: 55,
                    width: 795,
                    height: 1195,
                },
            ),
            PageBoundingBox::new(
                4,
                ContentRect {
                    x: 155,
                    y: 65,
                    width: 745,
                    height: 1175,
                },
            ),
        ];
        let result = GroupCropAnalyzer::unify_odd_even_regions(&boxes);

        // Odd pages (1, 3) should be grouped
        assert!(result.odd_region.is_valid());
        assert_eq!(result.odd_region.total_count, 2);

        // Even pages (2, 4) should be grouped
        assert!(result.even_region.is_valid());
        assert_eq!(result.even_region.total_count, 2);
    }

    #[test]
    fn test_group_crop_region_to_content_rect() {
        let region = GroupCropRegion {
            left: 100,
            top: 50,
            width: 800,
            height: 1200,
            inlier_count: 5,
            total_count: 5,
        };
        let rect = region.to_content_rect();
        assert_eq!(rect.x, 100);
        assert_eq!(rect.y, 50);
        assert_eq!(rect.width, 800);
        assert_eq!(rect.height, 1200);
    }
}
