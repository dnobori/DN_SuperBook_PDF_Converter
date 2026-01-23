//! superbook-pdf - High-quality PDF converter for scanned books
//!
//! A complete Rust implementation for converting scanned book PDFs into
//! high-quality digital books with AI enhancement.
//!
//! # Features
//!
//! - **PDF Reading** ([`pdf_reader`]) - Extract metadata, pages, and images from PDFs
//! - **PDF Writing** ([`pdf_writer`]) - Generate PDFs from images with optional OCR layer
//! - **Image Extraction** ([`image_extract`]) - Extract page images using `ImageMagick`
//! - **AI Enhancement** ([`realesrgan`]) - Upscale images using `RealESRGAN`
//! - **Deskew Correction** ([`deskew`]) - Detect and correct page skew
//! - **Margin Detection** ([`margin`]) - Detect and trim page margins
//! - **Page Number Detection** ([`page_number`]) - OCR-based page number recognition
//! - **AI Bridge** ([`ai_bridge`]) - Python subprocess bridge for AI tools
//!
//! # Quick Start
//!
//! ## Reading a PDF
//!
//! ```rust,no_run
//! use superbook_pdf::{LopdfReader, PdfWriterOptions, PrintPdfWriter};
//!
//! // Read a PDF
//! let reader = LopdfReader::new("input.pdf").unwrap();
//! println!("Pages: {}", reader.info.page_count);
//! ```
//!
//! ## Using Builder Patterns
//!
//! All option structs support fluent builder patterns:
//!
//! ```rust
//! use superbook_pdf::{PdfWriterOptions, DeskewOptions, RealEsrganOptions};
//!
//! // PDF Writer options
//! let pdf_opts = PdfWriterOptions::builder()
//!     .dpi(600)
//!     .jpeg_quality(95)
//!     .build();
//!
//! // Or use presets
//! let high_quality = PdfWriterOptions::high_quality();
//! let compact = PdfWriterOptions::compact();
//!
//! // Deskew options
//! let deskew_opts = DeskewOptions::builder()
//!     .max_angle(15.0)
//!     .build();
//!
//! // RealESRGAN options
//! let upscale_opts = RealEsrganOptions::builder()
//!     .scale(4)
//!     .tile_size(256)
//!     .build();
//! ```
//!
//! # Architecture
//!
//! The library is organized into independent modules that can be used separately:
//!
//! ```text
//! PDF Input -> Image Extraction -> Deskew -> Margin Detection
//!                                    |
//!                            AI Upscaling (RealESRGAN)
//!                                    |
//!                         Page Number Detection -> PDF Output
//! ```
//!
//! # License
//!
//! AGPL-3.0

pub mod ai_bridge;
pub mod cli;
pub mod deskew;
pub mod image_extract;
pub mod margin;
pub mod page_number;
pub mod pdf_reader;
pub mod pdf_writer;
pub mod realesrgan;
pub mod util;
pub mod yomitoku;

// Re-exports for convenience
pub use ai_bridge::{
    AiBridgeConfig, AiBridgeConfigBuilder, AiBridgeError, AiTool, SubprocessBridge,
};
pub use cli::{
    create_page_progress_bar, create_progress_bar, create_spinner, Cli, Commands, ConvertArgs,
};
pub use deskew::{
    DeskewError, DeskewOptions, DeskewOptionsBuilder, DeskewResult, ImageProcDeskewer,
};
pub use image_extract::{
    ExtractError, ExtractOptions, ExtractOptionsBuilder, ExtractedPage, MagickExtractor,
};
pub use margin::{
    ImageMarginDetector, MarginDetection, MarginError, MarginOptions, MarginOptionsBuilder, Margins,
};
pub use page_number::{
    DetectedPageNumber, PageNumberError, PageNumberOptions, PageNumberOptionsBuilder,
    TesseractPageDetector,
};
pub use pdf_reader::{LopdfReader, PdfDocument, PdfMetadata, PdfPage, PdfReaderError};
pub use pdf_writer::{PdfWriterError, PdfWriterOptions, PdfWriterOptionsBuilder, PrintPdfWriter};
pub use realesrgan::{RealEsrgan, RealEsrganError, RealEsrganOptions, RealEsrganOptionsBuilder};
pub use yomitoku::{
    BatchOcrResult, OcrResult, TextBlock, TextDirection, YomiToku, YomiTokuError, YomiTokuOptions,
    YomiTokuOptionsBuilder,
};
pub use util::{
    clamp, ensure_dir_writable, ensure_file_exists, format_duration, format_file_size, load_image,
    mm_to_pixels, mm_to_points, percentage, pixels_to_mm, points_to_mm,
};

/// Exit codes for CLI
pub mod exit_codes {
    pub const SUCCESS: i32 = 0;
    pub const GENERAL_ERROR: i32 = 1;
    pub const INVALID_ARGS: i32 = 2;
    pub const INPUT_NOT_FOUND: i32 = 3;
    pub const OUTPUT_ERROR: i32 = 4;
    pub const PROCESSING_ERROR: i32 = 5;
    pub const GPU_ERROR: i32 = 6;
    pub const EXTERNAL_TOOL_ERROR: i32 = 7;
}
