//! Markdown Conversion module
//!
//! Provides functionality to convert PDF documents to Markdown format
//! with support for Japanese vertical text, figures, tables, and more.
//!
//! # Issue #36 Implementation
//!
//! This module provides complete PDF to Markdown conversion with:
//!
//! - OCR integration (YomiToku)
//! - Reading order detection (vertical/horizontal)
//! - Figure and table extraction
//! - Heading level estimation
//! - Optional external API validation

mod converter;
mod element_detect;
mod reading_order;
mod renderer;
mod types;

pub mod api_validate;

// Re-export public API
pub use converter::{MarkdownConverter, MarkdownConversionResult};
pub use element_detect::{ElementDetector, DetectedElement, ElementType, TableStructure};
pub use reading_order::{ReadingOrderSorter, TextDirection, ReadingOrderOptions};
pub use renderer::{MarkdownRenderer, MarkdownRenderOptions};
pub use types::{
    MarkdownError, MarkdownOptions, MarkdownOptionsBuilder, PageContent, TextBlock, BoundingBox,
    TextDirectionOption,
};
pub use api_validate::{ValidationProvider, ValidationResult, ApiValidator};
