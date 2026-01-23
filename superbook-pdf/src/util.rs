//! Common utilities for superbook-pdf
//!
//! Provides shared functionality across modules to reduce code duplication.

use image::DynamicImage;
use std::path::Path;

/// Load an image from path with consistent error handling
pub fn load_image<P: AsRef<Path>>(path: P) -> Result<DynamicImage, String> {
    let path = path.as_ref();
    if !path.exists() {
        return Err(format!("Image not found: {}", path.display()));
    }
    image::open(path).map_err(|e| format!("Failed to load image: {}", e))
}

/// Check if a path exists and is a file
pub fn ensure_file_exists<P: AsRef<Path>>(path: P) -> Result<(), String> {
    let path = path.as_ref();
    if !path.exists() {
        return Err(format!("File not found: {}", path.display()));
    }
    if !path.is_file() {
        return Err(format!("Path is not a file: {}", path.display()));
    }
    Ok(())
}

/// Check if a directory exists and is writable
pub fn ensure_dir_writable<P: AsRef<Path>>(path: P) -> Result<(), String> {
    let path = path.as_ref();
    if !path.exists() {
        std::fs::create_dir_all(path).map_err(|e| format!("Failed to create directory: {}", e))?;
    }

    // Test writability
    let test_file = path.join(".write_test");
    std::fs::write(&test_file, b"test")
        .map_err(|_| format!("Directory not writable: {}", path.display()))?;
    let _ = std::fs::remove_file(test_file);

    Ok(())
}

/// Convert pixels to millimeters at given DPI
#[inline]
pub fn pixels_to_mm(pixels: u32, dpi: u32) -> f32 {
    (pixels as f32 / dpi as f32) * 25.4
}

/// Convert millimeters to pixels at given DPI
#[inline]
pub fn mm_to_pixels(mm: f32, dpi: u32) -> u32 {
    (mm * dpi as f32 / 25.4) as u32
}

/// Convert points to millimeters
#[inline]
pub fn points_to_mm(points: f64) -> f32 {
    (points / 72.0 * 25.4) as f32
}

/// Convert millimeters to points
#[inline]
pub fn mm_to_points(mm: f32) -> f64 {
    (mm as f64 / 25.4) * 72.0
}

/// Format file size in human-readable format
pub fn format_file_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if bytes >= GB {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.2} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.2} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}

/// Format duration in human-readable format
pub fn format_duration(duration: std::time::Duration) -> String {
    let secs = duration.as_secs();
    let millis = duration.subsec_millis();

    if secs >= 3600 {
        let hours = secs / 3600;
        let mins = (secs % 3600) / 60;
        format!("{}h {}m", hours, mins)
    } else if secs >= 60 {
        let mins = secs / 60;
        let remaining_secs = secs % 60;
        format!("{}m {}s", mins, remaining_secs)
    } else if secs > 0 {
        format!("{}.{:03}s", secs, millis)
    } else {
        format!("{}ms", millis)
    }
}

/// Clamp a value to a range
#[inline]
pub fn clamp<T: PartialOrd>(value: T, min: T, max: T) -> T {
    if value < min {
        min
    } else if value > max {
        max
    } else {
        value
    }
}

/// Calculate percentage
#[inline]
pub fn percentage(current: usize, total: usize) -> f32 {
    if total == 0 {
        0.0
    } else {
        (current as f32 / total as f32) * 100.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pixels_to_mm() {
        // At 72 DPI, 72 pixels = 1 inch = 25.4 mm
        let mm = pixels_to_mm(72, 72);
        assert!((mm - 25.4).abs() < 0.01);

        // At 300 DPI, 300 pixels = 1 inch = 25.4 mm
        let mm = pixels_to_mm(300, 300);
        assert!((mm - 25.4).abs() < 0.01);
    }

    #[test]
    fn test_mm_to_pixels() {
        // 25.4 mm at 72 DPI = 72 pixels
        let px = mm_to_pixels(25.4, 72);
        assert_eq!(px, 72);

        // 25.4 mm at 300 DPI = 300 pixels
        let px = mm_to_pixels(25.4, 300);
        assert_eq!(px, 300);
    }

    #[test]
    fn test_points_to_mm() {
        // 72 points = 1 inch = 25.4 mm
        let mm = points_to_mm(72.0);
        assert!((mm - 25.4).abs() < 0.01);
    }

    #[test]
    fn test_mm_to_points() {
        // 25.4 mm = 1 inch = 72 points
        let pt = mm_to_points(25.4);
        assert!((pt - 72.0).abs() < 0.01);
    }

    #[test]
    fn test_ensure_file_exists_nonexistent() {
        let result = ensure_file_exists("/nonexistent/file.txt");
        assert!(result.is_err());
    }

    #[test]
    fn test_load_image_nonexistent() {
        let result = load_image("/nonexistent/image.png");
        assert!(result.is_err());
    }

    #[test]
    fn test_format_file_size() {
        assert_eq!(format_file_size(0), "0 B");
        assert_eq!(format_file_size(500), "500 B");
        assert_eq!(format_file_size(1024), "1.00 KB");
        assert_eq!(format_file_size(1536), "1.50 KB");
        assert_eq!(format_file_size(1048576), "1.00 MB");
        assert_eq!(format_file_size(1073741824), "1.00 GB");
    }

    #[test]
    fn test_format_duration() {
        use std::time::Duration;

        assert_eq!(format_duration(Duration::from_millis(500)), "500ms");
        assert_eq!(format_duration(Duration::from_secs(5)), "5.000s");
        assert_eq!(format_duration(Duration::from_secs(65)), "1m 5s");
        assert_eq!(format_duration(Duration::from_secs(3665)), "1h 1m");
    }

    #[test]
    fn test_clamp() {
        assert_eq!(clamp(5, 0, 10), 5);
        assert_eq!(clamp(-5, 0, 10), 0);
        assert_eq!(clamp(15, 0, 10), 10);
        assert_eq!(clamp(0.5f32, 0.0, 1.0), 0.5);
    }

    #[test]
    fn test_percentage() {
        assert_eq!(percentage(50, 100), 50.0);
        assert_eq!(percentage(0, 100), 0.0);
        assert_eq!(percentage(100, 100), 100.0);
        assert_eq!(percentage(0, 0), 0.0); // Edge case: no div by zero
    }

    #[test]
    fn test_ensure_dir_writable() {
        let temp_dir = tempfile::tempdir().unwrap();
        let result = ensure_dir_writable(temp_dir.path());
        assert!(result.is_ok());
    }

    #[test]
    fn test_ensure_dir_writable_creates_dir() {
        let temp_dir = tempfile::tempdir().unwrap();
        let new_dir = temp_dir.path().join("new_subdir");
        let result = ensure_dir_writable(&new_dir);
        assert!(result.is_ok());
        assert!(new_dir.exists());
    }

    #[test]
    fn test_roundtrip_mm_pixels() {
        // Test roundtrip conversion
        let original_mm = 100.0f32;
        let dpi = 300u32;
        let pixels = mm_to_pixels(original_mm, dpi);
        let back_to_mm = pixels_to_mm(pixels, dpi);
        assert!((original_mm - back_to_mm).abs() < 0.1);
    }

    #[test]
    fn test_roundtrip_mm_points() {
        // Test roundtrip conversion
        let original_mm = 100.0f32;
        let points = mm_to_points(original_mm);
        let back_to_mm = points_to_mm(points);
        assert!((original_mm - back_to_mm).abs() < 0.1);
    }

    #[test]
    fn test_format_file_size_edge_cases() {
        // Just under KB
        assert_eq!(format_file_size(1023), "1023 B");
        // Exact boundaries
        assert_eq!(format_file_size(1024 * 1024 - 1), "1024.00 KB");
    }

    #[test]
    fn test_clamp_boundary_values() {
        // Exact boundary values
        assert_eq!(clamp(0, 0, 10), 0);
        assert_eq!(clamp(10, 0, 10), 10);
        // Same min and max
        assert_eq!(clamp(5, 5, 5), 5);
    }
}
