//! Core types for Engram

use chrono::{DateTime, Utc};
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::HashMap;

/// Unique identifier for a memory
pub type MemoryId = i64;

/// A memory entry in the database
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Memory {
    /// Unique identifier
    pub id: MemoryId,
    /// Main content of the memory
    pub content: String,
    /// Memory type (e.g., "note", "todo", "issue", "decision")
    #[serde(rename = "type")]
    pub memory_type: MemoryType,
    /// Tags for categorization
    #[serde(default)]
    pub tags: Vec<String>,
    /// Arbitrary metadata as JSON
    #[serde(default)]
    pub metadata: HashMap<String, serde_json::Value>,
    /// Importance score (0.0 - 1.0)
    #[serde(default = "default_importance")]
    pub importance: f32,
    /// Number of times accessed
    #[serde(default)]
    pub access_count: i32,
    /// When the memory was created
    pub created_at: DateTime<Utc>,
    /// When the memory was last updated
    pub updated_at: DateTime<Utc>,
    /// When the memory was last accessed
    pub last_accessed_at: Option<DateTime<Utc>>,
    /// Owner ID for multi-user support
    pub owner_id: Option<String>,
    /// Visibility level
    #[serde(default)]
    pub visibility: Visibility,
    /// Memory scope for isolation (user/session/agent/global)
    #[serde(default)]
    pub scope: MemoryScope,
    /// Workspace for project-based isolation (normalized: lowercase, [a-z0-9_-], max 64 chars)
    #[serde(default = "default_workspace")]
    pub workspace: String,
    /// Memory tier for tiered storage (permanent vs daily)
    #[serde(default)]
    pub tier: MemoryTier,
    /// Current version number
    #[serde(default = "default_version")]
    pub version: i32,
    /// Whether embedding is computed
    #[serde(default)]
    pub has_embedding: bool,
    /// When the memory expires (None = never for permanent, required for daily)
    pub expires_at: Option<DateTime<Utc>>,
    /// Content hash for deduplication (SHA256 of normalized content)
    pub content_hash: Option<String>,
    // Phase 1 - Cognitive memory fields (ENG-33)
    /// Timestamp when the event occurred (for Episodic memories)
    pub event_time: Option<DateTime<Utc>>,
    /// Duration of the event in seconds (for Episodic memories)
    pub event_duration_seconds: Option<i64>,
    /// Pattern that triggers this procedure (for Procedural memories)
    pub trigger_pattern: Option<String>,
    /// Number of times this procedure succeeded (for Procedural memories)
    #[serde(default)]
    pub procedure_success_count: i32,
    /// Number of times this procedure failed (for Procedural memories)
    #[serde(default)]
    pub procedure_failure_count: i32,
    /// ID of the memory this is a summary of (for Summary memories)
    pub summary_of_id: Option<MemoryId>,
    // Phase 5 - Lifecycle management (ENG-37)
    /// Lifecycle state for memory management (active, stale, archived)
    #[serde(default)]
    pub lifecycle_state: LifecycleState,
}

/// Lifecycle state for memory management (Phase 5 - ENG-37)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum LifecycleState {
    /// Normal state - included in search/list by default
    #[default]
    Active,
    /// Not accessed recently - included in search/list by default
    Stale,
    /// Compressed/summarized - EXCLUDED from search/list by default
    Archived,
}

impl std::fmt::Display for LifecycleState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LifecycleState::Active => write!(f, "active"),
            LifecycleState::Stale => write!(f, "stale"),
            LifecycleState::Archived => write!(f, "archived"),
        }
    }
}

impl std::str::FromStr for LifecycleState {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "active" => Ok(LifecycleState::Active),
            "stale" => Ok(LifecycleState::Stale),
            "archived" => Ok(LifecycleState::Archived),
            _ => Err(format!("Unknown lifecycle state: {}", s)),
        }
    }
}

fn default_workspace() -> String {
    "default".to_string()
}

/// Reserved workspace names that cannot be used
pub const RESERVED_WORKSPACES: &[&str] = &["_system", "_archive"];

/// Maximum workspace name length
pub const MAX_WORKSPACE_LENGTH: usize = 64;

/// Workspace validation error
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkspaceError {
    Empty,
    TooLong,
    InvalidChars,
    Reserved,
}

impl std::fmt::Display for WorkspaceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WorkspaceError::Empty => write!(f, "Workspace name cannot be empty"),
            WorkspaceError::TooLong => write!(f, "Workspace name exceeds {} characters", MAX_WORKSPACE_LENGTH),
            WorkspaceError::InvalidChars => write!(f, "Workspace name can only contain lowercase letters, numbers, hyphens, and underscores"),
            WorkspaceError::Reserved => write!(f, "Workspace name is reserved"),
        }
    }
}

impl std::error::Error for WorkspaceError {}

/// Normalize and validate a workspace name
///
/// Rules:
/// - Trim whitespace and convert to lowercase
/// - Only allow [a-z0-9_-] characters
/// - Max 64 characters
/// - Cannot start with underscore (reserved for system workspaces)
/// - "default" is allowed (it's the default workspace)
pub fn normalize_workspace(s: &str) -> Result<String, WorkspaceError> {
    let normalized = s.trim().to_lowercase();

    if normalized.is_empty() {
        return Err(WorkspaceError::Empty);
    }

    if normalized.len() > MAX_WORKSPACE_LENGTH {
        return Err(WorkspaceError::TooLong);
    }

    if !normalized
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
    {
        return Err(WorkspaceError::InvalidChars);
    }

    if normalized.starts_with('_') || RESERVED_WORKSPACES.contains(&normalized.as_str()) {
        return Err(WorkspaceError::Reserved);
    }

    Ok(normalized)
}

/// Statistics for a workspace
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceStats {
    /// Workspace name
    pub workspace: String,
    /// Total number of memories
    pub memory_count: i64,
    /// Number of permanent memories
    pub permanent_count: i64,
    /// Number of daily (ephemeral) memories
    pub daily_count: i64,
    /// Timestamp of first memory
    pub first_memory_at: Option<DateTime<Utc>>,
    /// Timestamp of last memory
    pub last_memory_at: Option<DateTime<Utc>>,
    /// Top tags in this workspace (tag, count)
    #[serde(default)]
    pub top_tags: Vec<(String, i64)>,
    /// Average importance score
    pub avg_importance: Option<f32>,
}

fn default_importance() -> f32 {
    0.5
}

fn default_version() -> i32 {
    1
}

/// Memory type classification
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum MemoryType {
    #[default]
    Note,
    Todo,
    Issue,
    Decision,
    Preference,
    Learning,
    Context,
    Credential,
    Custom,
    /// Session transcript chunk (for conversation indexing)
    /// Default tier: Daily with 7-day TTL
    TranscriptChunk,
    // Cognitive memory types (Phase 1 - ENG-33)
    /// Events with temporal context (e.g., "User deployed v2.0 on Jan 15")
    /// Tracks when things happened and how long they took
    Episodic,
    /// Learned patterns and workflows (e.g., "When user asks about auth, check JWT first")
    /// Tracks success/failure counts for pattern effectiveness
    Procedural,
    /// Compressed summaries of other memories
    /// References the original via summary_of_id
    Summary,
    /// Conversation state snapshots for session resumption
    /// Replaces Context type for checkpoint-specific use
    Checkpoint,
}

/// Memory tier for tiered storage (permanent vs ephemeral)
///
/// Tiers control memory lifetime:
/// - `Permanent`: Never expires, for important knowledge and decisions
/// - `Daily`: Auto-expires after TTL, for session context and scratch notes
///
/// Invariants enforced at write-time:
/// - Permanent tier: expires_at MUST be NULL
/// - Daily tier: expires_at MUST be set (defaults to created_at + 24h)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum MemoryTier {
    /// Never expires (default)
    #[default]
    Permanent,
    /// Auto-expires after configurable TTL (default: 24 hours)
    Daily,
}

impl MemoryTier {
    pub fn as_str(&self) -> &'static str {
        match self {
            MemoryTier::Permanent => "permanent",
            MemoryTier::Daily => "daily",
        }
    }

    /// Default TTL in seconds for daily tier
    pub fn default_ttl_seconds(&self) -> Option<i64> {
        match self {
            MemoryTier::Permanent => None,
            MemoryTier::Daily => Some(24 * 60 * 60), // 24 hours
        }
    }
}

impl std::str::FromStr for MemoryTier {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "permanent" => Ok(MemoryTier::Permanent),
            "daily" => Ok(MemoryTier::Daily),
            _ => Err(format!("Unknown memory tier: {}", s)),
        }
    }
}

impl MemoryType {
    pub fn as_str(&self) -> &'static str {
        match self {
            MemoryType::Note => "note",
            MemoryType::Todo => "todo",
            MemoryType::Issue => "issue",
            MemoryType::Decision => "decision",
            MemoryType::Preference => "preference",
            MemoryType::Learning => "learning",
            MemoryType::Context => "context",
            MemoryType::Credential => "credential",
            MemoryType::Custom => "custom",
            MemoryType::TranscriptChunk => "transcript_chunk",
            MemoryType::Episodic => "episodic",
            MemoryType::Procedural => "procedural",
            MemoryType::Summary => "summary",
            MemoryType::Checkpoint => "checkpoint",
        }
    }

    /// Returns true if this type should be excluded from default search
    pub fn excluded_from_default_search(&self) -> bool {
        matches!(self, MemoryType::TranscriptChunk)
    }
}

impl std::str::FromStr for MemoryType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "note" => Ok(MemoryType::Note),
            "todo" => Ok(MemoryType::Todo),
            "issue" => Ok(MemoryType::Issue),
            "decision" => Ok(MemoryType::Decision),
            "preference" => Ok(MemoryType::Preference),
            "learning" => Ok(MemoryType::Learning),
            "context" => Ok(MemoryType::Context),
            "credential" => Ok(MemoryType::Credential),
            "custom" => Ok(MemoryType::Custom),
            "transcript_chunk" => Ok(MemoryType::TranscriptChunk),
            "episodic" => Ok(MemoryType::Episodic),
            "procedural" => Ok(MemoryType::Procedural),
            "summary" => Ok(MemoryType::Summary),
            "checkpoint" => Ok(MemoryType::Checkpoint),
            _ => Err(format!("Unknown memory type: {}", s)),
        }
    }
}

/// Visibility levels for memories
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Visibility {
    #[default]
    Private,
    Shared,
    Public,
}

/// Memory scope for isolating memories by user, session, agent, or global
///
/// This enables multi-tenant memory management where:
/// - `User`: Memories belong to a specific user across all sessions
/// - `Session`: Memories are temporary and bound to a conversation session
/// - `Agent`: Memories belong to a specific AI agent instance
/// - `Global`: Memories are shared across all scopes (system-wide)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum MemoryScope {
    /// User-scoped memory, persists across sessions
    User { user_id: String },
    /// Session-scoped memory, temporary for one conversation
    Session { session_id: String },
    /// Agent-scoped memory, belongs to a specific agent instance
    Agent { agent_id: String },
    /// Global scope, accessible by all (default for backward compatibility)
    #[default]
    Global,
}

impl MemoryScope {
    /// Create a user-scoped memory scope
    pub fn user(user_id: impl Into<String>) -> Self {
        MemoryScope::User {
            user_id: user_id.into(),
        }
    }

    /// Create a session-scoped memory scope
    pub fn session(session_id: impl Into<String>) -> Self {
        MemoryScope::Session {
            session_id: session_id.into(),
        }
    }

    /// Create an agent-scoped memory scope
    pub fn agent(agent_id: impl Into<String>) -> Self {
        MemoryScope::Agent {
            agent_id: agent_id.into(),
        }
    }

    /// Get the scope type as a string
    pub fn scope_type(&self) -> &'static str {
        match self {
            MemoryScope::User { .. } => "user",
            MemoryScope::Session { .. } => "session",
            MemoryScope::Agent { .. } => "agent",
            MemoryScope::Global => "global",
        }
    }

    /// Get the scope ID (user_id, session_id, agent_id, or None for global)
    pub fn scope_id(&self) -> Option<&str> {
        match self {
            MemoryScope::User { user_id } => Some(user_id.as_str()),
            MemoryScope::Session { session_id } => Some(session_id.as_str()),
            MemoryScope::Agent { agent_id } => Some(agent_id.as_str()),
            MemoryScope::Global => None,
        }
    }

    /// Check if this scope matches or is accessible from another scope
    /// Global scope can access everything, specific scopes can only access their own
    pub fn can_access(&self, other: &MemoryScope) -> bool {
        match (self, other) {
            // Global can access everything
            (MemoryScope::Global, _) => true,
            // Same scope type and ID
            (MemoryScope::User { user_id: a }, MemoryScope::User { user_id: b }) => a == b,
            (MemoryScope::Session { session_id: a }, MemoryScope::Session { session_id: b }) => {
                a == b
            }
            (MemoryScope::Agent { agent_id: a }, MemoryScope::Agent { agent_id: b }) => a == b,
            // Anyone can access global memories
            (_, MemoryScope::Global) => true,
            // Different scope types cannot access each other
            _ => false,
        }
    }
}

/// Cross-reference (relation) between memories
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrossReference {
    /// Source memory ID
    pub from_id: MemoryId,
    /// Target memory ID
    pub to_id: MemoryId,
    /// Type of relationship
    pub edge_type: EdgeType,
    /// Similarity/relevance score (0.0 - 1.0)
    pub score: f32,
    /// Confidence level (decays over time)
    #[serde(default = "default_confidence")]
    pub confidence: f32,
    /// User-adjustable importance
    #[serde(default = "default_strength")]
    pub strength: f32,
    /// How the relation was created
    #[serde(default)]
    pub source: RelationSource,
    /// Context explaining why the relation exists
    pub source_context: Option<String>,
    /// When the relation was created
    pub created_at: DateTime<Utc>,
    /// When the relation became valid
    pub valid_from: DateTime<Utc>,
    /// When the relation stopped being valid (None = still valid)
    pub valid_to: Option<DateTime<Utc>>,
    /// Exempt from confidence decay
    #[serde(default)]
    pub pinned: bool,
    /// Additional metadata
    #[serde(default)]
    pub metadata: HashMap<String, serde_json::Value>,
}

fn default_confidence() -> f32 {
    1.0
}

fn default_strength() -> f32 {
    1.0
}

/// Types of edges/relationships between memories
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum EdgeType {
    #[default]
    RelatedTo,
    Supersedes,
    Contradicts,
    Implements,
    Extends,
    References,
    DependsOn,
    Blocks,
    FollowsUp,
}

impl EdgeType {
    pub fn as_str(&self) -> &'static str {
        match self {
            EdgeType::RelatedTo => "related_to",
            EdgeType::Supersedes => "supersedes",
            EdgeType::Contradicts => "contradicts",
            EdgeType::Implements => "implements",
            EdgeType::Extends => "extends",
            EdgeType::References => "references",
            EdgeType::DependsOn => "depends_on",
            EdgeType::Blocks => "blocks",
            EdgeType::FollowsUp => "follows_up",
        }
    }

    pub fn all() -> &'static [EdgeType] {
        &[
            EdgeType::RelatedTo,
            EdgeType::Supersedes,
            EdgeType::Contradicts,
            EdgeType::Implements,
            EdgeType::Extends,
            EdgeType::References,
            EdgeType::DependsOn,
            EdgeType::Blocks,
            EdgeType::FollowsUp,
        ]
    }
}

impl std::str::FromStr for EdgeType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "related_to" | "related" => Ok(EdgeType::RelatedTo),
            "supersedes" => Ok(EdgeType::Supersedes),
            "contradicts" => Ok(EdgeType::Contradicts),
            "implements" => Ok(EdgeType::Implements),
            "extends" => Ok(EdgeType::Extends),
            "references" => Ok(EdgeType::References),
            "depends_on" => Ok(EdgeType::DependsOn),
            "blocks" => Ok(EdgeType::Blocks),
            "follows_up" => Ok(EdgeType::FollowsUp),
            _ => Err(format!("Unknown edge type: {}", s)),
        }
    }
}

/// How a relation was created
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum RelationSource {
    #[default]
    Auto,
    Manual,
    Llm,
}

/// Search result with metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    /// The matched memory
    pub memory: Memory,
    /// Overall relevance score
    pub score: f32,
    /// How the result matched
    pub match_info: MatchInfo,
}

/// Information about how a search result matched
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatchInfo {
    /// Which search strategy was used
    pub strategy: SearchStrategy,
    /// Terms that matched (for keyword search)
    #[serde(default)]
    pub matched_terms: Vec<String>,
    /// Highlighted snippets
    #[serde(default)]
    pub highlights: Vec<String>,
    /// Semantic similarity score (if used)
    pub semantic_score: Option<f32>,
    /// Keyword/BM25 score (if used)
    pub keyword_score: Option<f32>,
}

/// Search strategy used
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SearchStrategy {
    #[serde(alias = "keyword")]
    KeywordOnly,
    #[serde(alias = "semantic")]
    SemanticOnly,
    #[default]
    Hybrid,
}

impl SearchStrategy {
    pub fn parse_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "keyword" | "keyword_only" => Some(SearchStrategy::KeywordOnly),
            "semantic" | "semantic_only" => Some(SearchStrategy::SemanticOnly),
            "hybrid" => Some(SearchStrategy::Hybrid),
            _ => None,
        }
    }
}

fn deserialize_search_strategy_opt<'de, D>(
    deserializer: D,
) -> Result<Option<SearchStrategy>, D::Error>
where
    D: Deserializer<'de>,
{
    let opt = Option::<String>::deserialize(deserializer)?;
    match opt.as_deref() {
        None => Ok(None),
        Some("auto") => Ok(None),
        Some(other) => SearchStrategy::parse_str(other).map(Some).ok_or_else(|| {
            <D::Error as serde::de::Error>::custom(format!("Invalid search strategy: {}", other))
        }),
    }
}

/// Memory version for history tracking
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryVersion {
    /// Version number (1, 2, 3, ...)
    pub version: i32,
    /// Content at this version
    pub content: String,
    /// Tags at this version
    pub tags: Vec<String>,
    /// Metadata at this version
    pub metadata: HashMap<String, serde_json::Value>,
    /// When this version was created
    pub created_at: DateTime<Utc>,
    /// Who created this version
    pub created_by: Option<String>,
    /// Summary of changes
    pub change_summary: Option<String>,
}

/// Statistics about the memory store
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct StorageStats {
    pub total_memories: i64,
    pub total_tags: i64,
    pub total_crossrefs: i64,
    pub total_versions: i64,
    pub total_identities: i64,
    pub total_entities: i64,
    pub db_size_bytes: i64,
    pub memories_with_embeddings: i64,
    pub memories_pending_embedding: i64,
    pub last_sync: Option<DateTime<Utc>>,
    pub sync_pending: bool,
    pub storage_mode: String,
    pub schema_version: i32,
    pub workspaces: HashMap<String, i64>,
    pub type_counts: HashMap<String, i64>,
    pub tier_counts: HashMap<String, i64>,
}

/// Configuration for the storage engine
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageConfig {
    /// Path to SQLite database
    pub db_path: String,
    /// Storage mode (local or cloud-safe)
    #[serde(default)]
    pub storage_mode: StorageMode,
    /// Cloud storage URI (s3://bucket/path)
    pub cloud_uri: Option<String>,
    /// Enable encryption for cloud storage
    #[serde(default)]
    pub encrypt_cloud: bool,
    /// Confidence decay half-life in days
    #[serde(default = "default_half_life")]
    pub confidence_half_life_days: f32,
    /// Auto-sync after writes
    #[serde(default = "default_true")]
    pub auto_sync: bool,
    /// Sync debounce delay in milliseconds
    #[serde(default = "default_sync_debounce")]
    pub sync_debounce_ms: u64,
}

fn default_half_life() -> f32 {
    30.0
}

fn default_true() -> bool {
    true
}

fn default_sync_debounce() -> u64 {
    5000
}

/// Storage mode for SQLite
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum StorageMode {
    #[default]
    Local,
    CloudSafe,
}

/// Embedding model configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingConfig {
    /// Model to use: "openai", "local", "tfidf"
    pub model: String,
    /// OpenAI API key (for openai model)
    pub api_key: Option<String>,
    /// OpenAI-compatible API base URL (for OpenRouter, Azure, etc.)
    /// Default: https://api.openai.com/v1
    pub base_url: Option<String>,
    /// Embedding model name override (e.g., "text-embedding-3-small", "openai/text-embedding-3-small")
    pub embedding_model: Option<String>,
    /// Local model path (for local model)
    pub model_path: Option<String>,
    /// Embedding dimensions (must match model output)
    /// Default: 384 for TF-IDF, 1536 for text-embedding-3-small
    pub dimensions: usize,
    /// Batch size for async queue
    #[serde(default = "default_batch_size")]
    pub batch_size: usize,
}

fn default_batch_size() -> usize {
    100
}

impl Default for EmbeddingConfig {
    fn default() -> Self {
        Self {
            model: "tfidf".to_string(),
            api_key: None,
            base_url: None,
            embedding_model: None,
            model_path: None,
            dimensions: 384,
            batch_size: 100,
        }
    }
}

/// Deduplication mode when creating memories
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum DedupMode {
    /// Return error if duplicate found
    Reject,
    /// Merge with existing memory (update metadata, tags)
    Merge,
    /// Silently skip creation, return existing memory
    Skip,
    /// Allow duplicate creation (default, current behavior)
    #[default]
    Allow,
}

/// Input for creating a new memory
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CreateMemoryInput {
    pub content: String,
    #[serde(default, alias = "type")]
    pub memory_type: MemoryType,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub metadata: HashMap<String, serde_json::Value>,
    pub importance: Option<f32>,
    /// Memory scope for isolation (user/session/agent/global)
    #[serde(default)]
    pub scope: MemoryScope,
    /// Workspace for project-based isolation (will be normalized)
    pub workspace: Option<String>,
    /// Memory tier (permanent or daily)
    #[serde(default)]
    pub tier: MemoryTier,
    /// Defer embedding computation to background queue
    #[serde(default)]
    pub defer_embedding: bool,
    /// Time-to-live in seconds (None = use tier default, Some(0) = never expires)
    /// For daily tier: defaults to 24 hours if not specified
    /// For permanent tier: must be None (enforced at write-time)
    pub ttl_seconds: Option<i64>,
    /// Deduplication mode (default: allow)
    #[serde(default)]
    pub dedup_mode: DedupMode,
    /// Similarity threshold for semantic deduplication (0.0-1.0, default: 0.95)
    pub dedup_threshold: Option<f32>,
    // Phase 1 - Cognitive memory fields (ENG-33)
    /// Timestamp when the event occurred (for Episodic memories)
    pub event_time: Option<DateTime<Utc>>,
    /// Duration of the event in seconds (for Episodic memories)
    pub event_duration_seconds: Option<i64>,
    /// Pattern that triggers this procedure (for Procedural memories)
    pub trigger_pattern: Option<String>,
    /// ID of the memory this is a summary of (for Summary memories)
    pub summary_of_id: Option<MemoryId>,
}

/// Input for updating a memory
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateMemoryInput {
    pub content: Option<String>,
    #[serde(alias = "type")]
    pub memory_type: Option<MemoryType>,
    pub tags: Option<Vec<String>>,
    pub metadata: Option<HashMap<String, serde_json::Value>>,
    pub importance: Option<f32>,
    /// Memory scope for isolation (user/session/agent/global)
    pub scope: Option<MemoryScope>,
    /// Time-to-live in seconds (None = no change, Some(0) = remove expiration, Some(n) = set to n seconds from now)
    pub ttl_seconds: Option<i64>,
    // Phase 1 - Cognitive memory fields (ENG-33)
    /// Timestamp when the event occurred (for Episodic memories)
    /// Use Some(None) to clear the value
    pub event_time: Option<Option<DateTime<Utc>>>,
    /// Pattern that triggers this procedure (for Procedural memories)
    /// Use Some(None) to clear the value
    pub trigger_pattern: Option<Option<String>>,
}

/// Input for creating a cross-reference
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateCrossRefInput {
    pub from_id: MemoryId,
    pub to_id: MemoryId,
    #[serde(default)]
    pub edge_type: EdgeType,
    pub strength: Option<f32>,
    pub source_context: Option<String>,
    #[serde(default)]
    pub pinned: bool,
}

/// Options for listing memories
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ListOptions {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
    pub tags: Option<Vec<String>>,
    #[serde(alias = "type")]
    pub memory_type: Option<MemoryType>,
    pub sort_by: Option<SortField>,
    pub sort_order: Option<SortOrder>,
    /// Legacy metadata filter (simple key-value equality)
    /// Deprecated: Use `filter` for advanced queries
    pub metadata_filter: Option<HashMap<String, serde_json::Value>>,
    /// Filter by memory scope
    pub scope: Option<MemoryScope>,
    /// Filter by workspace (single workspace)
    pub workspace: Option<String>,
    /// Filter by multiple workspaces (OR logic)
    pub workspaces: Option<Vec<String>>,
    /// Filter by memory tier
    pub tier: Option<MemoryTier>,
    /// Advanced filter expression with AND/OR/comparison operators (RML-932)
    /// Example: {"AND": [{"metadata.project": {"eq": "engram"}}, {"importance": {"gte": 0.5}}]}
    pub filter: Option<serde_json::Value>,
    // Phase 5 - Lifecycle management (ENG-37)
    /// Include archived memories in results (default: false)
    #[serde(default)]
    pub include_archived: bool,
}

/// Fields to sort by
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SortField {
    #[default]
    CreatedAt,
    UpdatedAt,
    LastAccessedAt,
    Importance,
    AccessCount,
}

/// Sort order
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum SortOrder {
    Asc,
    #[default]
    Desc,
}

/// Options for search operations
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SearchOptions {
    pub limit: Option<i64>,
    pub min_score: Option<f32>,
    pub tags: Option<Vec<String>>,
    #[serde(alias = "type")]
    pub memory_type: Option<MemoryType>,
    /// Force a specific search strategy
    #[serde(default, deserialize_with = "deserialize_search_strategy_opt")]
    pub strategy: Option<SearchStrategy>,
    /// Include match explanations
    #[serde(default)]
    pub explain: bool,
    /// Filter by memory scope
    pub scope: Option<MemoryScope>,
    /// Filter by workspace (single workspace)
    pub workspace: Option<String>,
    /// Filter by multiple workspaces (OR logic)
    pub workspaces: Option<Vec<String>>,
    /// Filter by memory tier
    pub tier: Option<MemoryTier>,
    /// Include transcript chunks in search (default: false)
    /// By default, transcript_chunk memories are excluded from search
    #[serde(default)]
    pub include_transcripts: bool,
    /// Advanced filter expression with AND/OR/comparison operators (RML-932)
    /// Takes precedence over `tags` and `memory_type` if specified
    pub filter: Option<serde_json::Value>,
    // Phase 5 - Lifecycle management (ENG-37)
    /// Include archived memories in search results (default: false)
    #[serde(default)]
    pub include_archived: bool,
}

/// Sync status information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncStatus {
    pub pending_changes: i64,
    pub last_sync: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
    pub is_syncing: bool,
}

/// Embedding queue status
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingStatus {
    pub memory_id: MemoryId,
    pub status: EmbeddingState,
    pub queued_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub error: Option<String>,
}

/// State of embedding computation
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EmbeddingState {
    Pending,
    Processing,
    Complete,
    Failed,
}
