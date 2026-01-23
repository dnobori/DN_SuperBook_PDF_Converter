//! Image Extraction module
//!
//! Provides functionality to extract page images from PDF files.

use std::path::{Path, PathBuf};
use std::process::Command;
use thiserror::Error;

/// Image extraction error types
#[derive(Debug, Error)]
pub enum ExtractError {
    #[error("PDF file not found: {0}")]
    PdfNotFound(PathBuf),

    #[error("Output directory not writable: {0}")]
    OutputNotWritable(PathBuf),

    #[error("Extraction failed for page {page}: {reason}")]
    ExtractionFailed { page: usize, reason: String },

    #[error("External tool error: {0}")]
    ExternalToolError(String),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, ExtractError>;

/// Image extraction options
pub struct ExtractOptions {
    /// Output DPI
    pub dpi: u32,
    /// Output format
    pub format: ImageFormat,
    /// Color space
    pub colorspace: ColorSpace,
    /// Background color (for transparency handling)
    pub background: Option<[u8; 3]>,
    /// Number of parallel workers
    pub parallel: usize,
    /// Progress callback
    #[allow(clippy::type_complexity)]
    pub progress_callback: Option<Box<dyn Fn(usize, usize) + Send + Sync>>,
}

impl std::fmt::Debug for ExtractOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExtractOptions")
            .field("dpi", &self.dpi)
            .field("format", &self.format)
            .field("colorspace", &self.colorspace)
            .field("background", &self.background)
            .field("parallel", &self.parallel)
            .field(
                "progress_callback",
                &self.progress_callback.as_ref().map(|_| "<callback>"),
            )
            .finish()
    }
}

impl Default for ExtractOptions {
    fn default() -> Self {
        Self {
            dpi: 300,
            format: ImageFormat::Png,
            colorspace: ColorSpace::Rgb,
            background: Some([255, 255, 255]), // White background
            parallel: num_cpus::get(),
            progress_callback: None,
        }
    }
}

impl ExtractOptions {
    /// Create a new options builder
    pub fn builder() -> ExtractOptionsBuilder {
        ExtractOptionsBuilder::default()
    }

    /// Create options for high quality extraction
    pub fn high_quality() -> Self {
        Self {
            dpi: 600,
            format: ImageFormat::Png,
            ..Default::default()
        }
    }

    /// Create options for fast extraction (lower quality)
    pub fn fast() -> Self {
        Self {
            dpi: 150,
            format: ImageFormat::Jpeg { quality: 80 },
            ..Default::default()
        }
    }

    /// Create options for grayscale documents
    pub fn grayscale() -> Self {
        Self {
            colorspace: ColorSpace::Grayscale,
            ..Default::default()
        }
    }
}

/// Builder for ExtractOptions
#[derive(Debug, Default)]
pub struct ExtractOptionsBuilder {
    options: ExtractOptions,
}

impl ExtractOptionsBuilder {
    /// Set output DPI (clamped to 72-1200)
    pub fn dpi(mut self, dpi: u32) -> Self {
        self.options.dpi = dpi.clamp(72, 1200);
        self
    }

    /// Set output format
    pub fn format(mut self, format: ImageFormat) -> Self {
        self.options.format = format;
        self
    }

    /// Set color space
    pub fn colorspace(mut self, colorspace: ColorSpace) -> Self {
        self.options.colorspace = colorspace;
        self
    }

    /// Set background color for transparency handling
    pub fn background(mut self, rgb: [u8; 3]) -> Self {
        self.options.background = Some(rgb);
        self
    }

    /// Disable background (keep transparency)
    pub fn no_background(mut self) -> Self {
        self.options.background = None;
        self
    }

    /// Set number of parallel workers
    pub fn parallel(mut self, workers: usize) -> Self {
        self.options.parallel = workers.max(1);
        self
    }

    /// Set progress callback
    pub fn progress_callback(mut self, callback: Box<dyn Fn(usize, usize) + Send + Sync>) -> Self {
        self.options.progress_callback = Some(callback);
        self
    }

    /// Build the options
    pub fn build(self) -> ExtractOptions {
        self.options
    }
}

/// Output image formats
#[derive(Debug, Clone, Copy, Default)]
pub enum ImageFormat {
    #[default]
    Png,
    Jpeg {
        quality: u8,
    },
    Bmp,
    Tiff,
}

impl ImageFormat {
    /// Get file extension for format
    pub fn extension(&self) -> &str {
        match self {
            ImageFormat::Png => "png",
            ImageFormat::Jpeg { .. } => "jpg",
            ImageFormat::Bmp => "bmp",
            ImageFormat::Tiff => "tiff",
        }
    }
}

/// Color space options
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ColorSpace {
    #[default]
    Rgb,
    Grayscale,
    Cmyk,
}

/// Extracted page information
#[derive(Debug)]
pub struct ExtractedPage {
    pub page_index: usize,
    pub path: PathBuf,
    pub width: u32,
    pub height: u32,
    pub format: ImageFormat,
}

/// Image extractor trait
pub trait ImageExtractor {
    /// Extract all pages from PDF
    fn extract_all(
        pdf_path: &Path,
        output_dir: &Path,
        options: &ExtractOptions,
    ) -> Result<Vec<ExtractedPage>>;

    /// Extract a single page
    fn extract_page(
        pdf_path: &Path,
        page_index: usize,
        output_path: &Path,
        options: &ExtractOptions,
    ) -> Result<ExtractedPage>;
}

/// ImageMagick-based extractor
pub struct MagickExtractor;

impl MagickExtractor {
    /// Extract a single page from PDF using ImageMagick
    pub fn extract_page(
        pdf_path: &Path,
        page_index: usize,
        output_path: &Path,
        options: &ExtractOptions,
    ) -> Result<ExtractedPage> {
        if !pdf_path.exists() {
            return Err(ExtractError::PdfNotFound(pdf_path.to_path_buf()));
        }

        // Check if output directory is writable
        if let Some(parent) = output_path.parent() {
            if !parent.exists() {
                std::fs::create_dir_all(parent)?;
            }
            // Try to create a test file to verify writability
            let test_file = parent.join(".write_test");
            if std::fs::write(&test_file, b"test").is_err() {
                return Err(ExtractError::OutputNotWritable(parent.to_path_buf()));
            }
            let _ = std::fs::remove_file(test_file);
        }

        let mut cmd = Command::new("magick");
        cmd.arg("-density").arg(options.dpi.to_string());

        // Set background color for transparency
        if let Some(bg) = options.background {
            cmd.arg("-background")
                .arg(format!("rgb({},{},{})", bg[0], bg[1], bg[2]));
            cmd.arg("-alpha").arg("remove");
            cmd.arg("-alpha").arg("off");
        }

        // Set colorspace
        match options.colorspace {
            ColorSpace::Grayscale => {
                cmd.arg("-colorspace").arg("gray");
            }
            ColorSpace::Cmyk => {
                cmd.arg("-colorspace").arg("CMYK");
            }
            ColorSpace::Rgb => {
                cmd.arg("-colorspace").arg("sRGB");
            }
        }

        // Input file with page index
        cmd.arg(format!("{}[{}]", pdf_path.display(), page_index));

        // Set output quality for JPEG
        if let ImageFormat::Jpeg { quality } = options.format {
            cmd.arg("-quality").arg(quality.to_string());
        }

        // Output file
        cmd.arg(output_path);

        let output = cmd.output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(ExtractError::ExternalToolError(stderr.to_string()));
        }

        // Get image dimensions
        let img = image::open(output_path).map_err(|e| ExtractError::ExtractionFailed {
            page: page_index,
            reason: e.to_string(),
        })?;

        Ok(ExtractedPage {
            page_index,
            path: output_path.to_path_buf(),
            width: img.width(),
            height: img.height(),
            format: options.format,
        })
    }

    /// Extract all pages from PDF
    pub fn extract_all(
        pdf_path: &Path,
        output_dir: &Path,
        options: &ExtractOptions,
    ) -> Result<Vec<ExtractedPage>> {
        if !pdf_path.exists() {
            return Err(ExtractError::PdfNotFound(pdf_path.to_path_buf()));
        }

        // Create output directory if it doesn't exist
        if !output_dir.exists() {
            std::fs::create_dir_all(output_dir)?;
        }

        // Check writability
        let test_file = output_dir.join(".write_test");
        if std::fs::write(&test_file, b"test").is_err() {
            return Err(ExtractError::OutputNotWritable(output_dir.to_path_buf()));
        }
        let _ = std::fs::remove_file(test_file);

        // Get page count using pdfinfo or similar
        let page_count = Self::get_page_count(pdf_path)?;

        // Extract pages (optionally in parallel)
        let extension = options.format.extension();
        let mut results = Vec::with_capacity(page_count);

        // Sequential extraction for now (parallel would require more complex handling)
        for i in 0..page_count {
            let output_path = output_dir.join(format!("page_{:05}.{}", i, extension));

            let result = Self::extract_page(pdf_path, i, &output_path, options)?;
            results.push(result);

            // Call progress callback if provided
            if let Some(ref callback) = options.progress_callback {
                callback(i + 1, page_count);
            }
        }

        Ok(results)
    }

    /// Get the number of pages in a PDF
    fn get_page_count(pdf_path: &Path) -> Result<usize> {
        // Try using pdfinfo first
        if let Ok(output) = Command::new("pdfinfo").arg(pdf_path).output() {
            if output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                for line in stdout.lines() {
                    if line.starts_with("Pages:") {
                        if let Some(count_str) = line.split(':').nth(1) {
                            if let Ok(count) = count_str.trim().parse() {
                                return Ok(count);
                            }
                        }
                    }
                }
            }
        }

        // Fallback: use ImageMagick identify
        let output = Command::new("magick")
            .args(["identify", "-format", "%n\n"])
            .arg(pdf_path)
            .output()?;

        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            if let Some(line) = stdout.lines().next() {
                if let Ok(count) = line.trim().parse() {
                    return Ok(count);
                }
            }
        }

        // Last resort: try lopdf
        let doc = lopdf::Document::load(pdf_path).map_err(|e| ExtractError::ExtractionFailed {
            page: 0,
            reason: e.to_string(),
        })?;

        Ok(doc.get_pages().len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_nonexistent_pdf_error() {
        let temp_dir = tempdir().unwrap();

        let result = MagickExtractor::extract_all(
            Path::new("/nonexistent/file.pdf"),
            temp_dir.path(),
            &ExtractOptions::default(),
        );

        assert!(matches!(result, Err(ExtractError::PdfNotFound(_))));
    }

    #[test]
    fn test_default_options() {
        let opts = ExtractOptions::default();

        assert_eq!(opts.dpi, 300);
        assert!(matches!(opts.format, ImageFormat::Png));
        assert!(matches!(opts.colorspace, ColorSpace::Rgb));
        assert_eq!(opts.background, Some([255, 255, 255]));
        assert!(opts.parallel > 0);
    }

    #[test]
    fn test_image_format_extension() {
        assert_eq!(ImageFormat::Png.extension(), "png");
        assert_eq!(ImageFormat::Jpeg { quality: 90 }.extension(), "jpg");
        assert_eq!(ImageFormat::Bmp.extension(), "bmp");
        assert_eq!(ImageFormat::Tiff.extension(), "tiff");
    }

    #[test]
    fn test_builder_pattern() {
        let options = ExtractOptions::builder()
            .dpi(600)
            .format(ImageFormat::Jpeg { quality: 95 })
            .colorspace(ColorSpace::Grayscale)
            .background([0, 0, 0])
            .parallel(4)
            .build();

        assert_eq!(options.dpi, 600);
        assert!(matches!(options.format, ImageFormat::Jpeg { quality: 95 }));
        assert!(matches!(options.colorspace, ColorSpace::Grayscale));
        assert_eq!(options.background, Some([0, 0, 0]));
        assert_eq!(options.parallel, 4);
    }

    #[test]
    fn test_builder_dpi_clamping() {
        // DPI should be clamped to 72-1200
        let options = ExtractOptions::builder().dpi(50).build();
        assert_eq!(options.dpi, 72);

        let options = ExtractOptions::builder().dpi(2000).build();
        assert_eq!(options.dpi, 1200);

        let options = ExtractOptions::builder().dpi(300).build();
        assert_eq!(options.dpi, 300);
    }

    #[test]
    fn test_builder_parallel_minimum() {
        // Parallel workers should be at least 1
        let options = ExtractOptions::builder().parallel(0).build();
        assert_eq!(options.parallel, 1);
    }

    #[test]
    fn test_builder_no_background() {
        let options = ExtractOptions::builder().no_background().build();
        assert!(options.background.is_none());
    }

    #[test]
    fn test_high_quality_preset() {
        let options = ExtractOptions::high_quality();

        assert_eq!(options.dpi, 600);
        assert!(matches!(options.format, ImageFormat::Png));
    }

    #[test]
    fn test_fast_preset() {
        let options = ExtractOptions::fast();

        assert_eq!(options.dpi, 150);
        assert!(matches!(options.format, ImageFormat::Jpeg { quality: 80 }));
    }

    #[test]
    fn test_grayscale_preset() {
        let options = ExtractOptions::grayscale();

        assert!(matches!(options.colorspace, ColorSpace::Grayscale));
    }

    // Note: The following tests require ImageMagick and actual PDF fixtures
    // They are marked with #[ignore] until fixtures are available

    #[test]
    #[ignore = "requires external tool"]
    fn test_extract_single_page() {
        let temp_dir = tempdir().unwrap();
        let output = temp_dir.path().join("page_0.png");

        let result = MagickExtractor::extract_page(
            Path::new("tests/fixtures/sample.pdf"),
            0,
            &output,
            &ExtractOptions::default(),
        )
        .unwrap();

        assert!(output.exists());
        assert_eq!(result.page_index, 0);
        assert!(result.width > 0);
        assert!(result.height > 0);
    }

    #[test]
    #[ignore = "requires external tool"]
    fn test_extract_all_pages() {
        let temp_dir = tempdir().unwrap();

        let results = MagickExtractor::extract_all(
            Path::new("tests/fixtures/10pages.pdf"),
            temp_dir.path(),
            &ExtractOptions::default(),
        )
        .unwrap();

        assert_eq!(results.len(), 10);
        for (i, result) in results.iter().enumerate() {
            assert_eq!(result.page_index, i);
            assert!(result.path.exists());
        }
    }

    #[test]
    #[ignore = "requires external tool"]
    fn test_dpi_setting() {
        let temp_dir = tempdir().unwrap();

        // 72 DPI
        let output_72 = temp_dir.path().join("72dpi.png");
        let result_72 = MagickExtractor::extract_page(
            Path::new("tests/fixtures/a4.pdf"),
            0,
            &output_72,
            &ExtractOptions {
                dpi: 72,
                ..Default::default()
            },
        )
        .unwrap();

        // 300 DPI
        let output_300 = temp_dir.path().join("300dpi.png");
        let result_300 = MagickExtractor::extract_page(
            Path::new("tests/fixtures/a4.pdf"),
            0,
            &output_300,
            &ExtractOptions {
                dpi: 300,
                ..Default::default()
            },
        )
        .unwrap();

        // 300 DPI image should be ~4x larger in each dimension
        assert!(result_300.width > result_72.width * 3);
        assert!(result_300.height > result_72.height * 3);
    }

    #[test]
    #[ignore = "requires external tool"]
    fn test_jpeg_output() {
        let temp_dir = tempdir().unwrap();
        let output = temp_dir.path().join("page_0.jpg");

        MagickExtractor::extract_page(
            Path::new("tests/fixtures/sample.pdf"),
            0,
            &output,
            &ExtractOptions {
                format: ImageFormat::Jpeg { quality: 85 },
                ..Default::default()
            },
        )
        .unwrap();

        assert!(output.exists());

        // Check JPEG magic bytes
        let bytes = std::fs::read(&output).unwrap();
        assert_eq!(&bytes[0..2], &[0xFF, 0xD8]);
    }

    #[test]
    #[ignore = "requires external tool"]
    fn test_grayscale_extraction() {
        let temp_dir = tempdir().unwrap();
        let output = temp_dir.path().join("gray.png");

        MagickExtractor::extract_page(
            Path::new("tests/fixtures/color.pdf"),
            0,
            &output,
            &ExtractOptions {
                colorspace: ColorSpace::Grayscale,
                ..Default::default()
            },
        )
        .unwrap();

        // Verify image is grayscale
        let img = image::open(&output).unwrap();
        let rgb = img.to_rgb8();

        // Check that R=G=B for each pixel (grayscale property)
        for pixel in rgb.pixels() {
            assert_eq!(pixel[0], pixel[1]);
            assert_eq!(pixel[1], pixel[2]);
        }
    }

    // Additional structure tests

    #[test]
    fn test_extracted_page_construction() {
        let page = ExtractedPage {
            page_index: 5,
            path: PathBuf::from("/test/page_5.png"),
            width: 2480,
            height: 3508,
            format: ImageFormat::Png,
        };

        assert_eq!(page.page_index, 5);
        assert_eq!(page.path, PathBuf::from("/test/page_5.png"));
        assert_eq!(page.width, 2480);
        assert_eq!(page.height, 3508);
        assert!(matches!(page.format, ImageFormat::Png));
    }

    #[test]
    fn test_all_image_formats() {
        let formats = vec![
            ImageFormat::Png,
            ImageFormat::Jpeg { quality: 90 },
            ImageFormat::Bmp,
            ImageFormat::Tiff,
        ];

        let expected_ext = vec!["png", "jpg", "bmp", "tiff"];

        for (format, ext) in formats.iter().zip(expected_ext.iter()) {
            assert_eq!(format.extension(), *ext);
        }
    }

    #[test]
    fn test_all_colorspaces() {
        let colorspaces = vec![ColorSpace::Rgb, ColorSpace::Grayscale, ColorSpace::Cmyk];

        // Verify all colorspaces can be constructed and roundtrip through builder
        for cs in colorspaces {
            let options = ExtractOptions::builder().colorspace(cs).build();
            match (cs, options.colorspace) {
                (ColorSpace::Rgb, ColorSpace::Rgb) => {}
                (ColorSpace::Grayscale, ColorSpace::Grayscale) => {}
                (ColorSpace::Cmyk, ColorSpace::Cmyk) => {}
                _ => panic!("Colorspace mismatch"),
            }
        }
    }

    #[test]
    fn test_error_types() {
        // Test all error variants can be constructed
        let _err1 = ExtractError::PdfNotFound(PathBuf::from("/test/path"));
        let _err2 = ExtractError::OutputNotWritable(PathBuf::from("/readonly/dir"));
        let _err3 = ExtractError::ExternalToolError("ImageMagick not found".to_string());
        let _err4 = ExtractError::ExtractionFailed {
            page: 3,
            reason: "Test error".to_string(),
        };
        let _err5: ExtractError = std::io::Error::new(std::io::ErrorKind::NotFound, "test").into();
    }

    #[test]
    fn test_output_not_writable_error_message() {
        let err = ExtractError::OutputNotWritable(PathBuf::from("/readonly/output"));

        let msg = err.to_string();
        assert!(msg.contains("/readonly/output"));
        assert!(msg.contains("not writable"));
    }

    #[test]
    fn test_extraction_failed_error_message() {
        let err = ExtractError::ExtractionFailed {
            page: 5,
            reason: "ImageMagick crashed".to_string(),
        };

        let msg = err.to_string();
        assert!(msg.contains("5"));
        assert!(msg.contains("ImageMagick"));
    }

    #[test]
    fn test_format_builder() {
        // Test different JPEG qualities
        let options_low = ExtractOptions::builder()
            .format(ImageFormat::Jpeg { quality: 50 })
            .build();
        let options_high = ExtractOptions::builder()
            .format(ImageFormat::Jpeg { quality: 95 })
            .build();

        match (options_low.format, options_high.format) {
            (ImageFormat::Jpeg { quality: q1 }, ImageFormat::Jpeg { quality: q2 }) => {
                assert_eq!(q1, 50);
                assert_eq!(q2, 95);
            }
            _ => panic!("Expected JPEG format"),
        }
    }

    #[test]
    fn test_background_color_setting() {
        // White background (default)
        let options = ExtractOptions::builder()
            .background([255, 255, 255])
            .build();
        assert_eq!(options.background, Some([255, 255, 255]));

        // Black background
        let options = ExtractOptions::builder().background([0, 0, 0]).build();
        assert_eq!(options.background, Some([0, 0, 0]));

        // Transparent (no background)
        let options = ExtractOptions::builder().no_background().build();
        assert!(options.background.is_none());
    }

    // TC-EXT-007: 並列抽出
    #[test]
    fn test_parallel_extraction_config() {
        // Test parallel thread configuration
        let options_single = ExtractOptions::builder().parallel(1).build();
        assert_eq!(options_single.parallel, 1);

        let options_multi = ExtractOptions::builder().parallel(4).build();
        assert_eq!(options_multi.parallel, 4);

        let options_max = ExtractOptions::builder().parallel(16).build();
        assert_eq!(options_max.parallel, 16);

        // parallel(0) should be clamped to 1
        let options_zero = ExtractOptions::builder().parallel(0).build();
        assert_eq!(options_zero.parallel, 1);
    }

    // TC-EXT-008: 進捗コールバック
    #[test]
    fn test_progress_callback_structure() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;

        // Test that progress callback can be set
        let progress_count = Arc::new(AtomicUsize::new(0));
        let progress_clone = progress_count.clone();

        let options = ExtractOptions::builder()
            .progress_callback(Box::new(move |current, total| {
                progress_clone.fetch_add(1, Ordering::SeqCst);
                assert!(current <= total);
            }))
            .build();

        // Verify callback is set
        assert!(options.progress_callback.is_some());

        // Call the callback to verify it works
        if let Some(callback) = &options.progress_callback {
            callback(1, 10);
            callback(5, 10);
            callback(10, 10);
        }

        assert_eq!(progress_count.load(Ordering::SeqCst), 3);
    }

    // TC-EXT-010: 書き込み不可ディレクトリエラー追加テスト
    #[test]
    fn test_output_not_writable_error_display() {
        let err = ExtractError::OutputNotWritable(std::path::PathBuf::from("/root/protected"));
        let display = format!("{}", err);
        assert!(display.contains("/root/protected"));
        assert!(display.contains("not writable") || display.contains("writable"));
    }

    #[test]
    fn test_extracted_page_display() {
        let page = ExtractedPage {
            page_index: 0,
            path: std::path::PathBuf::from("/tmp/page_001.png"),
            width: 2480,
            height: 3508,
            format: ImageFormat::Png,
        };

        assert_eq!(page.page_index, 0);
        assert_eq!(page.width, 2480);
        assert_eq!(page.height, 3508);
        assert!(page.path.to_string_lossy().contains("page_001"));
    }

    #[test]
    fn test_colorspace_all_variants() {
        // Test all colorspace variants exist and can be used
        let colorspaces = [ColorSpace::Rgb, ColorSpace::Grayscale, ColorSpace::Cmyk];

        for cs in &colorspaces {
            let options = ExtractOptions::builder().colorspace(*cs).build();
            assert_eq!(options.colorspace, *cs);
        }
    }

    #[test]
    fn test_image_format_all_variants() {
        // Test all image format variants
        let formats = [
            ImageFormat::Png,
            ImageFormat::Jpeg { quality: 85 },
            ImageFormat::Bmp,
            ImageFormat::Tiff,
        ];

        for fmt in &formats {
            let options = ExtractOptions::builder().format(fmt.clone()).build();
            match (&options.format, fmt) {
                (ImageFormat::Png, ImageFormat::Png) => {}
                (ImageFormat::Tiff, ImageFormat::Tiff) => {}
                (ImageFormat::Bmp, ImageFormat::Bmp) => {}
                (ImageFormat::Jpeg { quality: q1 }, ImageFormat::Jpeg { quality: q2 }) => {
                    assert_eq!(q1, q2);
                }
                _ => panic!("Format mismatch"),
            }
        }
    }
}
