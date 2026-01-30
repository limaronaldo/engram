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
//! - Session transcript indexing with dual-limiter chunking
//! - AI auto-tagging for memories
//! - Context compression and token counting (ENG-34)

pub mod auto_capture;
pub mod auto_tagging;
pub mod compression;
pub mod consolidation;
pub mod content_utils;
pub mod document_ingest;
pub mod entities;
pub mod entity_extraction;
pub mod natural_language;
pub mod project_context;
pub mod quality;
pub mod session_indexing;
pub mod suggestions;

pub use auto_capture::{
    AutoCaptureConfig, AutoCaptureEngine, CaptureCandidate, CaptureType, ConversationTracker,
};
pub use auto_tagging::{AutoTagConfig, AutoTagResult, AutoTagger, TagSource, TagSuggestion};
pub use consolidation::{ConsolidationEngine, ConsolidationResult, ConsolidationStrategy};
pub use content_utils::{
    compact_preview, content_stats, soft_trim, CompactMemory, ContentStats, SoftTrimConfig,
    SoftTrimResult,
};
pub use document_ingest::{
    DocumentChunk, DocumentFormat, DocumentIngestor, DocumentSection, IngestConfig, IngestResult,
    DEFAULT_CHUNK_SIZE, DEFAULT_MAX_FILE_SIZE, DEFAULT_OVERLAP,
};
pub use entities::{
    Entity, EntityExtractionConfig, EntityExtractor, EntityRelation, EntityType, ExtractedEntity,
    ExtractionResult, MemoryEntity,
};
pub use entity_extraction::{
    auto_link_memory, extract_entities, ExtractedEntity as NerExtractedEntity, ExtractedEntityType,
    ExtractionConfig, ExtractionResult as NerExtractionResult,
};
pub use natural_language::{CommandType, NaturalLanguageParser, ParsedCommand};
pub use project_context::{
    DiscoveredFile, InstructionFileParser, InstructionFileType, ParsedInstructions, ParsedSection,
    ProjectContextConfig, ProjectContextEngine, ScanResult, CORE_INSTRUCTION_FILES,
};
pub use quality::{QualityMetrics, QualityScore, QualityScorer};
pub use session_indexing::{
    chunk_conversation, delete_session, get_session, index_conversation, index_conversation_delta,
    list_sessions, ChunkingConfig, ConversationChunk, Message, Session,
};
pub use suggestions::{Suggestion, SuggestionEngine, SuggestionType};

// Phase 2: Context Compression Engine (ENG-34)
pub use compression::{
    check_context_budget, count_tokens, detect_encoding, parse_encoding, CompressionStrategy,
    ContextBudgetInput, ContextBudgetResult, MemoryTokenCount, TokenEncoding,
};
