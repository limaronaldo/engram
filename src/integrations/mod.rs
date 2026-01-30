//! External integrations module (Phase 3 - ENG-35)
//!
//! Provides integration with external observability and monitoring platforms.
//!
//! Currently supported:
//! - Langfuse (feature-gated behind `langfuse` feature)

#[cfg(feature = "langfuse")]
pub mod langfuse;

#[cfg(feature = "langfuse")]
pub use langfuse::{
    LangfuseClient, LangfuseConfig, LangfuseError, PatternExtraction, SyncProgress, SyncTask,
    Trace, TraceGeneration,
};
