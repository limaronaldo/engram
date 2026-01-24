//! Intelligence module for AI-powered features (Phase 4)
//!
//! Provides:
//! - Smart memory suggestions (RML-890)
//! - Automatic memory consolidation (RML-891)
//! - Memory quality scoring (RML-892)
//! - Natural language commands (RML-893)
//! - Auto-capture mode for proactive memory (RML-903)
//! - Project context discovery (AI instruction files)
//! - Entity extraction / NER (RML-925)
//! - Document ingestion (RML-928)

pub mod auto_capture;
pub mod consolidation;
pub mod document_ingest;
pub mod entities;
pub mod natural_language;
pub mod project_context;
pub mod quality;
pub mod suggestions;

pub use auto_capture::{
    AutoCaptureConfig, AutoCaptureEngine, CaptureCandidate, CaptureType, ConversationTracker,
};
pub use consolidation::{ConsolidationEngine, ConsolidationResult, ConsolidationStrategy};
pub use document_ingest::{
    DocumentChunk, DocumentFormat, DocumentIngestor, DocumentSection, IngestConfig, IngestResult,
    DEFAULT_CHUNK_SIZE, DEFAULT_MAX_FILE_SIZE, DEFAULT_OVERLAP,
};
pub use entities::{
    Entity, EntityExtractionConfig, EntityExtractor, EntityRelation, EntityType, ExtractedEntity,
    ExtractionResult, MemoryEntity,
};
pub use natural_language::{CommandType, NaturalLanguageParser, ParsedCommand};
pub use project_context::{
    DiscoveredFile, InstructionFileParser, InstructionFileType, ParsedInstructions, ParsedSection,
    ProjectContextConfig, ProjectContextEngine, ScanResult, CORE_INSTRUCTION_FILES,
};
pub use quality::{QualityMetrics, QualityScore, QualityScorer};
pub use suggestions::{Suggestion, SuggestionEngine, SuggestionType};
