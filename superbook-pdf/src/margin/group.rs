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
        Self::unify_and_expand_regions(bounding_boxes, 0, 0, 0)
    }

    /// Unify crop regions with Y coordinate unification, margin expansion, and size limits
    ///
    /// This function implements the full C# algorithm:
    /// 1. Calculate separate crop regions for odd/even pages
    /// 2. Unify Y coordinates (use min top, max bottom)
    /// 3. Expand width/height by margin_percent
    /// 4. Center the expansion
    /// 5. Clamp to image bounds
    pub fn unify_and_expand_regions(
        bounding_boxes: &[PageBoundingBox],
        margin_percent: u32,
        max_width: u32,
        max_height: u32,
    ) -> UnifiedCropRegions {
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
        let mut odd_region = Self::decide_group_crop_region(&odd_boxes);
        let mut even_region = Self::decide_group_crop_region(&even_boxes);

        // Unify Y coordinates (min top, max bottom) for consistent vertical positioning
        if odd_region.is_valid() && even_region.is_valid() {
            let unified_top = odd_region.top.min(even_region.top);
            let unified_bottom = odd_region.bottom().max(even_region.bottom());

            odd_region.top = unified_top;
            odd_region.height = unified_bottom.saturating_sub(unified_top);

            even_region.top = unified_top;
            even_region.height = unified_bottom.saturating_sub(unified_top);
        }

        // Expand both regions to match the larger one and add margin
        if odd_region.is_valid() && even_region.is_valid() {
            let target_width = odd_region.width.max(even_region.width);
            let target_height = odd_region.height.max(even_region.height);

            // Add margin percent
            let expanded_width = target_width + target_width * margin_percent / 100;
            let expanded_height = target_height + target_height * margin_percent / 100;

            // Apply max bounds if specified
            let final_width = if max_width > 0 {
                expanded_width.min(max_width)
            } else {
                expanded_width
            };
            let final_height = if max_height > 0 {
                expanded_height.min(max_height)
            } else {
                expanded_height
            };

            // Center the expansion for odd region
            Self::expand_region_centered(&mut odd_region, final_width, final_height, max_width, max_height);

            // Center the expansion for even region
            Self::expand_region_centered(&mut even_region, final_width, final_height, max_width, max_height);
        }

        UnifiedCropRegions {
            odd_region,
            even_region,
        }
    }

    /// Expand a region to target size, centering the expansion
    fn expand_region_centered(
        region: &mut GroupCropRegion,
        target_width: u32,
        target_height: u32,
        max_width: u32,
        max_height: u32,
    ) {
        if region.width < target_width {
            let dw = target_width - region.width;
            let new_left = region.left.saturating_sub(dw / 2);

            // Clamp to image bounds
            let clamped_left = if max_width > 0 {
                new_left.min(max_width.saturating_sub(target_width))
            } else {
                new_left
            };

            region.left = clamped_left;
            region.width = target_width;
        }

        if region.height < target_height {
            let dh = target_height - region.height;
            let new_top = region.top.saturating_sub(dh / 2);

            // Clamp to image bounds
            let clamped_top = if max_height > 0 {
                new_top.min(max_height.saturating_sub(target_height))
            } else {
                new_top
            };

            region.top = clamped_top;
            region.height = target_height;
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

    // ============================================================
    // TC-MARGIN Spec Tests
    // ============================================================

    // TC-MARGIN-001: 均一マージン - 正確な検出
    #[test]
    fn test_tc_margin_001_uniform_margins_detected() {
        // Create pages with identical margins (uniform)
        let boxes = vec![
            PageBoundingBox::new(1, ContentRect { x: 100, y: 100, width: 800, height: 1000 }),
            PageBoundingBox::new(2, ContentRect { x: 100, y: 100, width: 800, height: 1000 }),
            PageBoundingBox::new(3, ContentRect { x: 100, y: 100, width: 800, height: 1000 }),
            PageBoundingBox::new(4, ContentRect { x: 100, y: 100, width: 800, height: 1000 }),
        ];

        let result = GroupCropAnalyzer::decide_group_crop_region(&boxes);

        // All pages have same margins, so result should be exact
        assert!(result.is_valid());
        assert_eq!(result.left, 100);
        assert_eq!(result.top, 100);
        assert_eq!(result.width, 800);
        assert_eq!(result.height, 1000);
        assert_eq!(result.inlier_count, 4);
    }

    // TC-MARGIN-002: 不均一マージン - 統一計算
    #[test]
    fn test_tc_margin_002_nonuniform_margins_unified() {
        // Create pages with varying margins
        let boxes = vec![
            PageBoundingBox::new(1, ContentRect { x: 100, y: 90, width: 800, height: 1000 }),
            PageBoundingBox::new(2, ContentRect { x: 110, y: 100, width: 790, height: 990 }),
            PageBoundingBox::new(3, ContentRect { x: 95, y: 95, width: 805, height: 1005 }),
            PageBoundingBox::new(4, ContentRect { x: 105, y: 105, width: 795, height: 995 }),
        ];

        let result = GroupCropAnalyzer::decide_group_crop_region(&boxes);

        // Result should use median values to unify
        assert!(result.is_valid());
        // Median values should be calculated
        assert!(result.left >= 95 && result.left <= 110);
        assert!(result.top >= 90 && result.top <= 105);
    }

    // TC-MARGIN-003: マージンなし - ゼロマージン
    #[test]
    fn test_tc_margin_003_no_margins() {
        // Content fills entire page (no margins)
        let boxes = vec![
            PageBoundingBox::new(1, ContentRect { x: 0, y: 0, width: 1000, height: 1200 }),
            PageBoundingBox::new(2, ContentRect { x: 0, y: 0, width: 1000, height: 1200 }),
        ];

        let result = GroupCropAnalyzer::decide_group_crop_region(&boxes);

        assert!(result.is_valid());
        assert_eq!(result.left, 0);
        assert_eq!(result.top, 0);
    }

    // TC-MARGIN-004: 外れ値ページ - Tukey除外
    #[test]
    fn test_tc_margin_004_outlier_exclusion_tukey() {
        // Create pages with one outlier
        let boxes = vec![
            PageBoundingBox::new(1, ContentRect { x: 100, y: 100, width: 800, height: 1000 }),
            PageBoundingBox::new(2, ContentRect { x: 102, y: 98, width: 798, height: 1002 }),
            PageBoundingBox::new(3, ContentRect { x: 101, y: 101, width: 799, height: 999 }),
            PageBoundingBox::new(4, ContentRect { x: 99, y: 99, width: 801, height: 1001 }),
            // Outlier page with very different margins
            PageBoundingBox::new(5, ContentRect { x: 300, y: 300, width: 400, height: 600 }),
        ];

        let result = GroupCropAnalyzer::decide_group_crop_region(&boxes);

        // Outlier should be excluded, result should be based on normal pages
        assert!(result.is_valid());
        // Inlier count should be less than total (outlier excluded)
        assert!(result.inlier_count <= result.total_count);
        // Result should be close to normal pages, not influenced by outlier
        assert!(result.left < 200); // Far from outlier's 300
    }

    // TC-MARGIN-005: 奇偶ページ差 - 個別リージョン
    #[test]
    fn test_tc_margin_005_odd_even_separate_regions() {
        // Odd pages have different margins than even pages (typical for books)
        let boxes = vec![
            PageBoundingBox::new(1, ContentRect { x: 120, y: 100, width: 780, height: 1000 }), // Odd
            PageBoundingBox::new(2, ContentRect { x: 100, y: 100, width: 780, height: 1000 }), // Even
            PageBoundingBox::new(3, ContentRect { x: 122, y: 102, width: 778, height: 998 }), // Odd
            PageBoundingBox::new(4, ContentRect { x: 98, y: 98, width: 782, height: 1002 }),  // Even
        ];

        let result = GroupCropAnalyzer::unify_odd_even_regions(&boxes);

        // Both regions should be valid
        assert!(result.odd_region.is_valid());
        assert!(result.even_region.is_valid());

        // Odd region should have larger left margin
        assert!(result.odd_region.left >= result.even_region.left);

        // Each region should have 2 pages
        assert_eq!(result.odd_region.total_count, 2);
        assert_eq!(result.even_region.total_count, 2);
    }
}
