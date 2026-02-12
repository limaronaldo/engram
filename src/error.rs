//! Error types for Engram

use thiserror::Error;

/// Result type alias for Engram operations
pub type Result<T> = std::result::Result<T, EngramError>;

/// Main error type for Engram
#[derive(Error, Debug)]
pub enum EngramError {
    #[error("Database error: {0}")]
    Database(#[from] rusqlite::Error),

    #[error("Storage error: {0}")]
    Storage(String),

    #[error("Memory not found: {0}")]
    NotFound(i64),

    #[error("Invalid input: {0}")]
    InvalidInput(String),

    #[error("Embedding error: {0}")]
    Embedding(String),

    #[error("Search error: {0}")]
    Search(String),

    #[error("Sync error: {0}")]
    Sync(String),

    #[error("Cloud storage error: {0}")]
    CloudStorage(String),

    #[error("Encryption error: {0}")]
    Encryption(String),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("HTTP request error: {0}")]
    #[cfg(feature = "openai")]
    Http(#[from] reqwest::Error),

    #[error("HTTP request error: {0}")]
    #[cfg(not(feature = "openai"))]
    Http(String),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Conflict: {0}")]
    Conflict(String),

    #[error("Duplicate memory detected (existing_id={existing_id}): {message}")]
    Duplicate { existing_id: i64, message: String },

    #[error("Authentication error: {0}")]
    Auth(String),

    #[error("Unauthorized: {0}")]
    Unauthorized(String),

    #[error("Rate limited: retry after {0} seconds")]
    RateLimited(u64),

    #[error("Internal error: {0}")]
    Internal(String),
}

impl EngramError {
    /// Check if error is retryable
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            EngramError::Sync(_)
                | EngramError::CloudStorage(_)
                | EngramError::Http(_)
                | EngramError::RateLimited(_)
        )
    }

    /// Get error code for MCP protocol
    pub fn code(&self) -> i64 {
        match self {
            EngramError::NotFound(_) => -32001,
            EngramError::InvalidInput(_) => -32602,
            EngramError::Auth(_) => -32003,
            EngramError::Unauthorized(_) => -32003,
            EngramError::RateLimited(_) => -32004,
            EngramError::Conflict(_) => -32005,
            EngramError::Duplicate { .. } => -32006,
            _ => -32000,
        }
    }
}
