//! Cleanup module for image post-processing
//!
//! Provides functionality for removing artifacts from scanned images:
//!
//! # Features
//!
//! - **Marker Removal** ([`marker_removal`]) - Remove highlighter marks and annotations
//! - **Deblur** ([`deblur`]) - Correct focus blur using unsharp mask or AI
//!
//! # Issue Coverage
//!
//! - Issue #34: マーカー・書き込み除去
//! - Issue #35: ピントボケ補正

pub mod deblur;
pub mod marker_removal;
mod types;

// Re-export public API
pub use deblur::{
    BlurDetector, DeblurAlgorithm, DeblurOptions, DeblurOptionsBuilder, DeblurResult,
};

pub use marker_removal::{
    HighlighterColor, MarkerDetectionResult, MarkerRemovalOptions, MarkerRemovalOptionsBuilder,
    MarkerRemover,
};

pub use types::CleanupError;
