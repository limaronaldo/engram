//! Intelligence module for AI-powered features (Phase 4, 8, 9)
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
//! - Salience scoring and temporal decay (Phase 8 - ENG-66 to ENG-68)
//! - Session context tracking (Phase 8 - ENG-70, ENG-71)
//! - Context quality and deduplication (Phase 9 - ENG-48 to ENG-66)

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
pub mod salience;
pub mod session_context;
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
pub use salience::{
    boost_memory_salience, demote_memory_salience, get_memory_salience, get_salience_history,
    get_salience_stats, run_salience_decay, set_memory_importance, DecayResult, SalienceCalculator,
    SalienceConfig, SalienceHistoryEntry, SaliencePercentiles, SalienceScore, SalienceStats,
    ScoredMemory, StateDistribution,
};
pub use session_context::{
    add_memory_to_session, create_session, end_session, export_session, get_session_context,
    get_session_memories, get_sessions_for_memory, list_sessions_extended,
    remove_memory_from_session, search_session_memories, update_session_context,
    update_session_summary, ContextRole, CreateSessionInput, SessionContext, SessionExport,
    SessionMemoryLink, SessionSearchResult,
};
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

// Phase 9: Context Quality (ENG-48 to ENG-66)
pub use context_quality::{
    calculate_quality_score, calculate_text_similarity, detect_conflicts, find_near_duplicates,
    find_semantic_duplicates, generate_quality_report, get_pending_duplicates, get_source_trust,
    get_unresolved_conflicts, resolve_conflict, update_source_trust, ConflictSeverity,
    ConflictType, ContextQualityConfig, DuplicateCandidate, EnhancedQualityScore, MemoryConflict,
    QualityIssue, QualityReport, QualitySuggestion, ResolutionType, SourceTrustScore,
    ValidationStatus,
};