//! Common types for the cleanup module

use std::path::PathBuf;
use thiserror::Error;

/// Cleanup error types
#[derive(Debug, Error)]
pub enum CleanupError {
    #[error("Image not found: {0}")]
    ImageNotFound(PathBuf),

    #[error("Invalid image: {0}")]
    InvalidImage(String),

    #[error("Processing failed: {0}")]
    ProcessingFailed(String),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, CleanupError>;
