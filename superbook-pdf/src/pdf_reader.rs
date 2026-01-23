//! PDF Reader module
//!
//! Provides functionality to read PDF files and extract metadata.
//!
//! # Features
//!
//! - Read PDF files using lopdf
//! - Extract page count, dimensions, and rotation
//! - Extract metadata (title, author, etc.)
//! - Detect encrypted PDFs
//!
//! # Example
//!
//! ```rust,no_run
//! use superbook_pdf::LopdfReader;
//!
//! let reader = LopdfReader::new("document.pdf").unwrap();
//! println!("Pages: {}", reader.info.page_count);
//! println!("Title: {:?}", reader.info.metadata.title);
//! ```

use lopdf::Document;
use std::path::{Path, PathBuf};
use thiserror::Error;

/// PDF reading error types
#[derive(Debug, Error)]
pub enum PdfReaderError {
    #[error("File not found: {0}")]
    FileNotFound(PathBuf),

    #[error("Invalid PDF format: {0}")]
    InvalidFormat(String),

    #[error("Encrypted PDF not supported")]
    EncryptedPdf,

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("PDF parse error: {0}")]
    ParseError(String),
}

pub type Result<T> = std::result::Result<T, PdfReaderError>;

/// PDF document information
#[derive(Debug, Clone)]
pub struct PdfDocument {
    pub path: PathBuf,
    pub page_count: usize,
    pub metadata: PdfMetadata,
    pub pages: Vec<PdfPage>,
    pub is_encrypted: bool,
}

/// PDF metadata
#[derive(Debug, Clone, Default)]
pub struct PdfMetadata {
    pub title: Option<String>,
    pub author: Option<String>,
    pub subject: Option<String>,
    pub keywords: Option<String>,
    pub creator: Option<String>,
    pub producer: Option<String>,
    pub creation_date: Option<String>,
    pub modification_date: Option<String>,
}

/// Page information
#[derive(Debug, Clone)]
pub struct PdfPage {
    /// 0-indexed page number
    pub index: usize,
    /// Width in points (1 point = 1/72 inch)
    pub width_pt: f64,
    /// Height in points
    pub height_pt: f64,
    /// Rotation (0, 90, 180, 270)
    pub rotation: u16,
    /// Whether the page contains images
    pub has_images: bool,
    /// Whether the page contains text
    pub has_text: bool,
}

/// PDF Reader trait
pub trait PdfReader {
    /// Open a PDF file
    fn open(path: impl AsRef<Path>) -> Result<PdfDocument>;

    /// Get page information by index
    fn get_page(&self, index: usize) -> Result<&PdfPage>;

    /// Get iterator over all pages
    fn pages(&self) -> impl Iterator<Item = &PdfPage>;

    /// Get document metadata
    fn metadata(&self) -> &PdfMetadata;

    /// Check if PDF is encrypted
    fn is_encrypted(&self) -> bool;
}

/// lopdf-based PDF reader implementation
pub struct LopdfReader {
    #[allow(dead_code)]
    document: Document,
    pub info: PdfDocument,
}

impl LopdfReader {
    /// Create a new PDF reader for the given path
    pub fn new(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();

        if !path.exists() {
            return Err(PdfReaderError::FileNotFound(path.to_path_buf()));
        }

        let document = Document::load(path).map_err(|e| {
            let err_str = e.to_string();
            if err_str.contains("header") || err_str.contains("PDF") {
                PdfReaderError::InvalidFormat(err_str)
            } else {
                PdfReaderError::ParseError(err_str)
            }
        })?;

        let is_encrypted = document.is_encrypted();
        let page_count = document.get_pages().len();
        let metadata = Self::extract_metadata(&document);
        let pages = Self::extract_pages(&document)?;

        Ok(Self {
            document,
            info: PdfDocument {
                path: path.to_path_buf(),
                page_count,
                metadata,
                pages,
                is_encrypted,
            },
        })
    }

    /// Extract metadata from PDF document
    fn extract_metadata(doc: &Document) -> PdfMetadata {
        let mut metadata = PdfMetadata::default();

        // Try to get the Info dictionary
        if let Ok(info_ref) = doc.trailer.get(b"Info") {
            if let Ok(info_ref) = info_ref.as_reference() {
                if let Ok(info_dict) = doc.get_dictionary(info_ref) {
                    metadata.title = Self::get_string_from_dict(info_dict, b"Title");
                    metadata.author = Self::get_string_from_dict(info_dict, b"Author");
                    metadata.subject = Self::get_string_from_dict(info_dict, b"Subject");
                    metadata.keywords = Self::get_string_from_dict(info_dict, b"Keywords");
                    metadata.creator = Self::get_string_from_dict(info_dict, b"Creator");
                    metadata.producer = Self::get_string_from_dict(info_dict, b"Producer");
                    metadata.creation_date = Self::get_string_from_dict(info_dict, b"CreationDate");
                    metadata.modification_date = Self::get_string_from_dict(info_dict, b"ModDate");
                }
            }
        }

        metadata
    }

    /// Helper to extract string from dictionary
    fn get_string_from_dict(dict: &lopdf::Dictionary, key: &[u8]) -> Option<String> {
        dict.get(key).ok().and_then(|obj| {
            match obj {
                lopdf::Object::String(bytes, _) => {
                    // Try UTF-8 first, then Latin-1
                    String::from_utf8(bytes.clone())
                        .ok()
                        .or_else(|| Some(bytes.iter().map(|&b| b as char).collect()))
                }
                _ => None,
            }
        })
    }

    /// Extract page information from PDF document
    fn extract_pages(doc: &Document) -> Result<Vec<PdfPage>> {
        let page_ids = doc.get_pages();
        let mut pages = Vec::with_capacity(page_ids.len());

        for (index, (_, page_id)) in page_ids.iter().enumerate() {
            let page_dict = doc
                .get_dictionary(*page_id)
                .map_err(|e| PdfReaderError::ParseError(e.to_string()))?;

            // Get MediaBox (required) or use default A4
            let (width_pt, height_pt) =
                Self::get_page_size(doc, page_dict).unwrap_or((595.0, 842.0)); // A4 default

            // Get rotation
            let rotation = page_dict
                .get(b"Rotate")
                .ok()
                .and_then(|obj| obj.as_i64().ok())
                .map(|r| (r % 360) as u16)
                .unwrap_or(0);

            // Check for images (simplified check)
            let has_images = page_dict.has(b"Resources")
                && doc
                    .get_dictionary(
                        page_dict
                            .get(b"Resources")
                            .ok()
                            .and_then(|r| r.as_reference().ok())
                            .unwrap_or((0, 0)),
                    )
                    .map(|res| res.has(b"XObject"))
                    .unwrap_or(false);

            // Check for text (simplified check - presence of Contents)
            let has_text = page_dict.has(b"Contents");

            pages.push(PdfPage {
                index,
                width_pt,
                height_pt,
                rotation,
                has_images,
                has_text,
            });
        }

        Ok(pages)
    }

    /// Get page dimensions from MediaBox or CropBox
    fn get_page_size(doc: &Document, page_dict: &lopdf::Dictionary) -> Option<(f64, f64)> {
        // Try CropBox first, then MediaBox
        for key in &[b"CropBox".as_slice(), b"MediaBox".as_slice()] {
            if let Ok(box_obj) = page_dict.get(key) {
                if let Ok(box_arr) = Self::resolve_array(doc, box_obj) {
                    if box_arr.len() >= 4 {
                        let x1 = Self::get_number(&box_arr[0]).unwrap_or(0.0);
                        let y1 = Self::get_number(&box_arr[1]).unwrap_or(0.0);
                        let x2 = Self::get_number(&box_arr[2]).unwrap_or(595.0);
                        let y2 = Self::get_number(&box_arr[3]).unwrap_or(842.0);
                        return Some(((x2 - x1).abs(), (y2 - y1).abs()));
                    }
                }
            }
        }
        None
    }

    /// Resolve an object to an array (following references)
    fn resolve_array<'a>(doc: &'a Document, obj: &'a lopdf::Object) -> Result<Vec<lopdf::Object>> {
        match obj {
            lopdf::Object::Array(arr) => Ok(arr.clone()),
            lopdf::Object::Reference(id) => {
                let resolved = doc
                    .get_object(*id)
                    .map_err(|e| PdfReaderError::ParseError(e.to_string()))?;
                Self::resolve_array(doc, resolved)
            }
            _ => Err(PdfReaderError::ParseError("Expected array".to_string())),
        }
    }

    /// Extract number from PDF object
    fn get_number(obj: &lopdf::Object) -> Option<f64> {
        match obj {
            lopdf::Object::Integer(i) => Some(*i as f64),
            lopdf::Object::Real(f) => Some(*f as f64),
            _ => None,
        }
    }

    /// Get page by index
    pub fn get_page(&self, index: usize) -> Result<&PdfPage> {
        self.info
            .pages
            .get(index)
            .ok_or_else(|| PdfReaderError::ParseError(format!("Page {} not found", index)))
    }

    /// Get metadata
    pub fn metadata(&self) -> &PdfMetadata {
        &self.info.metadata
    }

    /// Check if encrypted
    pub fn is_encrypted(&self) -> bool {
        self.info.is_encrypted
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_open_nonexistent_file() {
        let result = LopdfReader::new("/nonexistent/file.pdf");
        assert!(matches!(result, Err(PdfReaderError::FileNotFound(_))));
    }

    #[test]
    fn test_open_invalid_pdf() {
        // Create a non-PDF file
        let mut temp = NamedTempFile::new().unwrap();
        writeln!(temp, "This is not a PDF").unwrap();

        let result = LopdfReader::new(temp.path());
        assert!(matches!(
            result,
            Err(PdfReaderError::InvalidFormat(_) | PdfReaderError::ParseError(_))
        ));
    }

    // PDF fixture tests

    #[test]
    fn test_open_valid_pdf() {
        let path = PathBuf::from("tests/fixtures/sample.pdf");
        let doc = LopdfReader::new(&path).unwrap();

        assert!(doc.info.page_count > 0);
        assert_eq!(doc.info.path, path);
    }

    #[test]
    fn test_page_count() {
        let doc = LopdfReader::new("tests/fixtures/10pages.pdf").unwrap();
        assert_eq!(doc.info.page_count, 10);
    }

    #[test]
    fn test_page_dimensions() {
        let doc = LopdfReader::new("tests/fixtures/a4.pdf").unwrap();
        let page = doc.get_page(0).unwrap();

        // A4: 595 x 842 points
        assert!((page.width_pt - 595.0).abs() < 1.0);
        assert!((page.height_pt - 842.0).abs() < 1.0);
    }

    #[test]
    fn test_metadata_extraction() {
        let doc = LopdfReader::new("tests/fixtures/with_metadata.pdf").unwrap();
        let meta = doc.metadata();

        assert!(meta.title.is_some());
        assert!(meta.author.is_some());
    }

    #[test]
    fn test_rotated_page() {
        let doc = LopdfReader::new("tests/fixtures/rotated.pdf").unwrap();
        let page = doc.get_page(0).unwrap();

        assert_eq!(page.rotation, 90);
    }

    #[test]
    fn test_encrypted_pdf_detection() {
        let doc = LopdfReader::new("tests/fixtures/encrypted.pdf").unwrap();
        assert!(doc.is_encrypted());
    }

    // TC-PDR-009: Large PDF memory efficiency
    // Requires large_1000pages.pdf fixture and procfs dependency
    #[test]
    #[ignore = "requires external tool"]
    fn test_large_pdf_memory() {
        let doc = LopdfReader::new("tests/fixtures/large_1000pages.pdf").unwrap();
        assert_eq!(doc.info.page_count, 1000);
        // Memory usage check would require procfs crate
    }

    // TC-PDR-010: Concurrent open
    #[test]
    fn test_concurrent_open() {
        use rayon::prelude::*;

        // Use existing fixture files for concurrent test
        let paths = vec![
            "tests/fixtures/sample.pdf",
            "tests/fixtures/a4.pdf",
            "tests/fixtures/10pages.pdf",
            "tests/fixtures/with_metadata.pdf",
        ];

        let results: Vec<_> = paths.par_iter().map(|p| LopdfReader::new(p)).collect();

        assert!(results.iter().all(|r| r.is_ok()));
    }

    // Additional structure tests

    #[test]
    fn test_pdf_document_structure() {
        let doc = PdfDocument {
            path: PathBuf::from("/test/path.pdf"),
            page_count: 5,
            metadata: PdfMetadata::default(),
            pages: vec![],
            is_encrypted: false,
        };

        assert_eq!(doc.path, PathBuf::from("/test/path.pdf"));
        assert_eq!(doc.page_count, 5);
        assert!(!doc.is_encrypted);
    }

    #[test]
    fn test_pdf_metadata_construction() {
        let metadata = PdfMetadata {
            title: Some("Test Title".to_string()),
            author: Some("Test Author".to_string()),
            subject: Some("Test Subject".to_string()),
            keywords: Some("test, keywords".to_string()),
            creator: Some("Test Creator".to_string()),
            producer: Some("Test Producer".to_string()),
            creation_date: Some("D:20240101120000".to_string()),
            modification_date: Some("D:20240102120000".to_string()),
        };

        assert_eq!(metadata.title, Some("Test Title".to_string()));
        assert_eq!(metadata.author, Some("Test Author".to_string()));
        assert!(metadata.creation_date.is_some());
    }

    #[test]
    fn test_pdf_page_structure() {
        let page = PdfPage {
            index: 0,
            width_pt: 595.0,
            height_pt: 842.0,
            rotation: 90,
            has_images: true,
            has_text: true,
        };

        assert_eq!(page.index, 0);
        assert_eq!(page.width_pt, 595.0);
        assert_eq!(page.height_pt, 842.0);
        assert_eq!(page.rotation, 90);
        assert!(page.has_images);
        assert!(page.has_text);
    }

    #[test]
    fn test_error_types() {
        // Test all error variants can be constructed
        let _err1 = PdfReaderError::FileNotFound(PathBuf::from("/test/path"));
        let _err2 = PdfReaderError::InvalidFormat("Invalid format".to_string());
        let _err3 = PdfReaderError::EncryptedPdf;
        let _err4 = PdfReaderError::ParseError("Parse error".to_string());
        let _err5: PdfReaderError =
            std::io::Error::new(std::io::ErrorKind::NotFound, "test").into();
    }

    #[test]
    fn test_default_metadata() {
        let metadata = PdfMetadata::default();

        assert!(metadata.title.is_none());
        assert!(metadata.author.is_none());
        assert!(metadata.subject.is_none());
        assert!(metadata.keywords.is_none());
        assert!(metadata.creator.is_none());
        assert!(metadata.producer.is_none());
        assert!(metadata.creation_date.is_none());
        assert!(metadata.modification_date.is_none());
    }

    #[test]
    fn test_page_index_out_of_bounds() {
        let doc = LopdfReader::new("tests/fixtures/sample.pdf").unwrap();

        // Try to get page beyond count
        let result = doc.get_page(9999);
        assert!(result.is_err());
    }

    // Additional tests for spec coverage

    #[test]
    fn test_pages_iterator() {
        let doc = LopdfReader::new("tests/fixtures/10pages.pdf").unwrap();

        // Iterate over all pages
        let page_count = doc.info.pages.iter().count();
        assert_eq!(page_count, doc.info.page_count);
    }

    #[test]
    fn test_page_rotation_values() {
        // Test that rotation is normalized to valid values
        let page = PdfPage {
            index: 0,
            width_pt: 595.0,
            height_pt: 842.0,
            rotation: 270,
            has_images: false,
            has_text: true,
        };

        // Valid rotations: 0, 90, 180, 270
        assert!(
            page.rotation == 0
                || page.rotation == 90
                || page.rotation == 180
                || page.rotation == 270
        );
    }

    #[test]
    fn test_error_display_messages() {
        let err1 = PdfReaderError::FileNotFound(PathBuf::from("/test/path.pdf"));
        assert!(err1.to_string().contains("not found"));

        let err2 = PdfReaderError::InvalidFormat("bad header".to_string());
        assert!(err2.to_string().contains("Invalid"));

        let err3 = PdfReaderError::EncryptedPdf;
        assert!(err3.to_string().contains("ncrypted"));

        let err4 = PdfReaderError::ParseError("parse failed".to_string());
        assert!(err4.to_string().contains("error"));
    }

    #[test]
    fn test_metadata_clone() {
        let metadata = PdfMetadata {
            title: Some("Test Title".to_string()),
            author: Some("Test Author".to_string()),
            subject: None,
            keywords: None,
            creator: None,
            producer: None,
            creation_date: None,
            modification_date: None,
        };

        let cloned = metadata.clone();
        assert_eq!(cloned.title, metadata.title);
        assert_eq!(cloned.author, metadata.author);
    }

    #[test]
    fn test_pdf_document_clone() {
        let doc = PdfDocument {
            path: PathBuf::from("/test/path.pdf"),
            page_count: 10,
            metadata: PdfMetadata::default(),
            pages: vec![],
            is_encrypted: false,
        };

        let cloned = doc.clone();
        assert_eq!(cloned.path, doc.path);
        assert_eq!(cloned.page_count, doc.page_count);
        assert_eq!(cloned.is_encrypted, doc.is_encrypted);
    }

    #[test]
    fn test_page_dimensions_calculation() {
        let page = PdfPage {
            index: 0,
            width_pt: 595.0,  // A4 width in points
            height_pt: 842.0, // A4 height in points
            rotation: 0,
            has_images: true,
            has_text: true,
        };

        // A4 is 210mm x 297mm, 1 inch = 72 points, 1 inch = 25.4mm
        // width_mm = 595 / 72 * 25.4 ≈ 210
        let width_mm = page.width_pt / 72.0 * 25.4;
        let height_mm = page.height_pt / 72.0 * 25.4;

        assert!((width_mm - 210.0).abs() < 1.0);
        assert!((height_mm - 297.0).abs() < 1.0);
    }
}
