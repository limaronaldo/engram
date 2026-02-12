use super::backend::{BatchCreateResult, BatchDeleteResult, HealthStatus, StorageBackend};
use crate::error::EngramError;
use crate::storage::queries::compute_content_hash;
use crate::types::{
    normalize_workspace, CreateMemoryInput, CrossReference, EdgeType, LifecycleState, ListOptions,
    MatchInfo, Memory, MemoryId, MemoryScope, MemoryTier, SearchOptions, SearchResult,
    SearchStrategy, SortField, SortOrder, StorageStats, UpdateMemoryInput, Visibility,
};

use meilisearch_sdk::client::Client;
use meilisearch_sdk::search::SearchResults;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::runtime::Runtime;

const MEMORIES_INDEX: &str = "memories";

#[derive(Serialize, Deserialize, Debug)]
pub struct MeilisearchMemory {
    pub id: i64,
    pub content: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub tags: Vec<String>,
    pub memory_type: String,
    // Add missing fields to support full reconstruction
    pub metadata: Option<HashMap<String, serde_json::Value>>,
    pub importance: f32,
    pub access_count: i32,
    pub last_accessed_at: Option<i64>,
    pub owner_id: Option<String>,
    pub visibility: String,
    pub scope: String,
    pub scope_id: Option<String>,
    pub workspace: String,
    pub tier: String,
    pub version: i32,
    pub has_embedding: bool,
    pub expires_at: Option<i64>,
    pub content_hash: Option<String>,
    // Phase 1 - Cognitive fields
    pub event_time: Option<i64>,
    pub event_duration_seconds: Option<i64>,
    pub trigger_pattern: Option<String>,
    pub procedure_success_count: i32,
    pub procedure_failure_count: i32,
    pub summary_of_id: Option<i64>,
    pub lifecycle_state: String,
}

impl From<&Memory> for MeilisearchMemory {
    fn from(m: &Memory) -> Self {
        Self {
            id: m.id,
            content: m.content.clone(),
            created_at: m.created_at.timestamp(),
            updated_at: m.updated_at.timestamp(),
            tags: m.tags.clone(),
            memory_type: m.memory_type.as_str().to_string(),
            metadata: Some(m.metadata.clone()),
            importance: m.importance,
            access_count: m.access_count,
            last_accessed_at: m.last_accessed_at.map(|t| t.timestamp()),
            owner_id: m.owner_id.clone(),
            visibility: visibility_to_str(m.visibility).to_string(),
            scope: m.scope.scope_type().to_string(),
            scope_id: m.scope.scope_id().map(|s| s.to_string()),
            workspace: m.workspace.clone(),
            tier: m.tier.as_str().to_string(),
            version: m.version,
            has_embedding: m.has_embedding,
            expires_at: m.expires_at.map(|t| t.timestamp()),
            content_hash: m.content_hash.clone(),
            event_time: m.event_time.map(|t| t.timestamp()),
            event_duration_seconds: m.event_duration_seconds,
            trigger_pattern: m.trigger_pattern.clone(),
            procedure_success_count: m.procedure_success_count,
            procedure_failure_count: m.procedure_failure_count,
            summary_of_id: m.summary_of_id,
            lifecycle_state: m.lifecycle_state.to_string(),
        }
    }
}

fn timestamp_to_datetime(timestamp: i64) -> chrono::DateTime<chrono::Utc> {
    chrono::DateTime::from_timestamp(timestamp, 0).unwrap_or_else(chrono::Utc::now)
}

fn opt_timestamp_to_datetime(timestamp: Option<i64>) -> Option<chrono::DateTime<chrono::Utc>> {
    timestamp.and_then(|t| chrono::DateTime::from_timestamp(t, 0))
}

fn scope_from_parts(scope: &str, scope_id: Option<String>) -> MemoryScope {
    match (scope, scope_id) {
        ("user", Some(id)) => MemoryScope::User { user_id: id },
        ("session", Some(id)) => MemoryScope::Session { session_id: id },
        ("agent", Some(id)) => MemoryScope::Agent { agent_id: id },
        _ => MemoryScope::Global,
    }
}

fn visibility_from_str(value: &str) -> Visibility {
    match value {
        "shared" => Visibility::Shared,
        "public" => Visibility::Public,
        _ => Visibility::Private,
    }
}

fn build_memory_from_doc(doc: MeilisearchMemory) -> Memory {
    Memory {
        id: doc.id,
        content: doc.content,
        memory_type: doc.memory_type.parse().unwrap_or_default(),
        tags: doc.tags,
        metadata: doc.metadata.unwrap_or_default(),
        created_at: timestamp_to_datetime(doc.created_at),
        updated_at: timestamp_to_datetime(doc.updated_at),
        last_accessed_at: opt_timestamp_to_datetime(doc.last_accessed_at),
        importance: doc.importance,
        access_count: doc.access_count,
        owner_id: doc.owner_id,
        visibility: visibility_from_str(&doc.visibility),
        scope: scope_from_parts(&doc.scope, doc.scope_id),
        workspace: doc.workspace,
        tier: doc.tier.parse().unwrap_or_default(),
        version: doc.version,
        has_embedding: doc.has_embedding,
        expires_at: opt_timestamp_to_datetime(doc.expires_at),
        content_hash: doc.content_hash,
        event_time: opt_timestamp_to_datetime(doc.event_time),
        event_duration_seconds: doc.event_duration_seconds,
        trigger_pattern: doc.trigger_pattern,
        procedure_success_count: doc.procedure_success_count,
        procedure_failure_count: doc.procedure_failure_count,
        summary_of_id: doc.summary_of_id,
        lifecycle_state: doc.lifecycle_state.parse().unwrap_or_default(),
    }
}

fn escape_filter_value(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

fn build_tags_filter(tags: &[String]) -> Option<String> {
    if tags.is_empty() {
        return None;
    }
    let clauses: Vec<String> = tags
        .iter()
        .map(|tag| format!("tags = \"{}\"", escape_filter_value(tag)))
        .collect();
    Some(clauses.join(" AND "))
}

fn build_workspace_filter(workspaces: &[String]) -> Option<String> {
    if workspaces.is_empty() {
        return None;
    }
    if workspaces.len() == 1 {
        return Some(format!(
            "workspace = \"{}\"",
            escape_filter_value(&workspaces[0])
        ));
    }
    let values: Vec<String> = workspaces
        .iter()
        .map(|w| format!("\"{}\"", escape_filter_value(w)))
        .collect();
    Some(format!("workspace IN [{}]", values.join(", ")))
}

fn build_scope_filter(scope: &MemoryScope) -> Vec<String> {
    let mut parts = Vec::new();
    parts.push(format!("scope = \"{}\"", scope.scope_type()));
    match scope.scope_id() {
        Some(id) => parts.push(format!("scope_id = \"{}\"", escape_filter_value(id))),
        None => parts.push("scope_id IS NULL".to_string()),
    }
    parts
}

fn build_filter_from_search_options(
    options: &SearchOptions,
) -> Result<Option<String>, EngramError> {
    if options.filter.is_some() {
        return Err(EngramError::InvalidInput(
            "Advanced filter expressions are not supported by the Meilisearch backend.".to_string(),
        ));
    }

    let mut clauses = Vec::new();

    if let Some(scope) = &options.scope {
        clauses.extend(build_scope_filter(scope));
    }

    if let Some(memory_type) = &options.memory_type {
        clauses.push(format!(
            "memory_type = \"{}\"",
            escape_filter_value(memory_type.as_str())
        ));
    } else if !options.include_transcripts {
        clauses.push("memory_type != \"transcript_chunk\"".to_string());
    }

    if let Some(tier) = &options.tier {
        clauses.push(format!("tier = \"{}\"", escape_filter_value(tier.as_str())));
    }

    if !options.include_archived {
        clauses.push("lifecycle_state != \"archived\"".to_string());
    }

    if let Some(tags) = &options.tags {
        if let Some(tag_clause) = build_tags_filter(tags) {
            clauses.push(tag_clause);
        }
    }

    let workspaces = if let Some(workspace) = &options.workspace {
        vec![workspace.clone()]
    } else {
        options.workspaces.clone().unwrap_or_default()
    };
    if let Some(workspace_clause) = build_workspace_filter(&workspaces) {
        clauses.push(workspace_clause);
    }

    Ok(if clauses.is_empty() {
        None
    } else {
        Some(clauses.join(" AND "))
    })
}

fn build_filter_from_list_options(options: &ListOptions) -> Result<Option<String>, EngramError> {
    if options.filter.is_some() || options.metadata_filter.is_some() {
        return Err(EngramError::InvalidInput(
            "Metadata/advanced filters are not supported by the Meilisearch backend.".to_string(),
        ));
    }

    let mut clauses = Vec::new();

    if let Some(scope) = &options.scope {
        clauses.extend(build_scope_filter(scope));
    }

    if let Some(memory_type) = &options.memory_type {
        clauses.push(format!(
            "memory_type = \"{}\"",
            escape_filter_value(memory_type.as_str())
        ));
    }

    if let Some(tier) = &options.tier {
        clauses.push(format!("tier = \"{}\"", escape_filter_value(tier.as_str())));
    }

    if !options.include_archived {
        clauses.push("lifecycle_state != \"archived\"".to_string());
    }

    if let Some(tags) = &options.tags {
        if let Some(tag_clause) = build_tags_filter(tags) {
            clauses.push(tag_clause);
        }
    }

    let workspaces = if let Some(workspace) = &options.workspace {
        vec![workspace.clone()]
    } else {
        options.workspaces.clone().unwrap_or_default()
    };
    if let Some(workspace_clause) = build_workspace_filter(&workspaces) {
        clauses.push(workspace_clause);
    }

    Ok(if clauses.is_empty() {
        None
    } else {
        Some(clauses.join(" AND "))
    })
}

fn sort_to_meili(sort_by: SortField, sort_order: SortOrder) -> String {
    let field = match sort_by {
        SortField::CreatedAt => "created_at",
        SortField::UpdatedAt => "updated_at",
        SortField::LastAccessedAt => "last_accessed_at",
        SortField::Importance => "importance",
        SortField::AccessCount => "access_count",
    };
    let order = match sort_order {
        SortOrder::Asc => "asc",
        SortOrder::Desc => "desc",
    };
    format!("{}:{}", field, order)
}

fn visibility_to_str(visibility: Visibility) -> &'static str {
    match visibility {
        Visibility::Private => "private",
        Visibility::Shared => "shared",
        Visibility::Public => "public",
    }
}

fn generate_memory_id() -> i64 {
    (rand::random::<u64>() & i64::MAX as u64) as i64
}

fn build_memory_from_input(
    id: i64,
    input: CreateMemoryInput,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<Memory, EngramError> {
    let workspace = normalize_workspace(input.workspace.as_deref().unwrap_or("default"))
        .map_err(|e| EngramError::InvalidInput(e.to_string()))?;

    let expires_at = match input.tier {
        MemoryTier::Permanent => {
            if input.ttl_seconds.is_some() && input.ttl_seconds != Some(0) {
                return Err(EngramError::InvalidInput(
                    "Permanent tier memories cannot have a TTL. Use Daily tier for expiring memories.".to_string(),
                ));
            }
            None
        }
        MemoryTier::Daily => {
            let ttl = input.ttl_seconds.filter(|&t| t > 0).unwrap_or(86400);
            Some(now + chrono::Duration::seconds(ttl))
        }
    };

    let content_hash = Some(compute_content_hash(&input.content));

    Ok(Memory {
        id,
        content: input.content,
        memory_type: input.memory_type,
        tags: input.tags,
        metadata: input.metadata,
        created_at: now,
        updated_at: now,
        last_accessed_at: None,
        importance: input.importance.unwrap_or(0.5),
        access_count: 0,
        owner_id: None,
        visibility: Visibility::Private,
        scope: input.scope,
        workspace,
        tier: input.tier,
        version: 1,
        has_embedding: false,
        expires_at,
        content_hash,
        event_time: input.event_time,
        event_duration_seconds: input.event_duration_seconds,
        trigger_pattern: input.trigger_pattern,
        procedure_success_count: 0,
        procedure_failure_count: 0,
        summary_of_id: input.summary_of_id,
        lifecycle_state: LifecycleState::Active,
    })
}

pub struct MeilisearchBackend {
    client: Client,
    rt: Arc<Runtime>,
    url: String,
    api_key: Option<String>,
}

impl MeilisearchBackend {
    pub fn new(url: &str, api_key: Option<&str>) -> Result<Self, EngramError> {
        let client = Client::new(url, api_key)
            .map_err(|e| EngramError::Storage(format!("Failed to create client: {}", e)))?;

        let rt = Runtime::new().map_err(|e| EngramError::Storage(e.to_string()))?;

        let backend = Self {
            client,
            rt: Arc::new(rt),
            url: url.to_string(),
            api_key: api_key.map(|key| key.to_string()),
        };

        backend.init_schema()?;

        Ok(backend)
    }

    fn init_schema(&self) -> Result<(), EngramError> {
        self.rt.block_on(async {
            let index = self.client.index(MEMORIES_INDEX);
            // Ensure index exists
            let task = self.client.create_index(MEMORIES_INDEX, Some("id")).await;
            if let Ok(task) = task {
                let _ = self.client.wait_for_task(task, None, None).await;
            }

            // Configure filterable attributes
            let filterable_task = index
                .set_filterable_attributes(&[
                    "tags",
                    "memory_type",
                    "created_at",
                    "updated_at",
                    "importance",
                    "access_count",
                    "workspace",
                    "tier",
                    "scope",
                    "scope_id",
                    "visibility",
                    "lifecycle_state",
                ])
                .await;
            if let Ok(task) = filterable_task {
                let _ = index.wait_for_task(task, None, None).await;
            }

            // Configure sortable attributes
            let sortable_task = index
                .set_sortable_attributes(&[
                    "created_at",
                    "updated_at",
                    "importance",
                    "access_count",
                    "last_accessed_at",
                ])
                .await;
            if let Ok(task) = sortable_task {
                let _ = index.wait_for_task(task, None, None).await;
            }

            Ok(())
        })
    }

    pub fn index_memory(&self, memory: &Memory) -> Result<(), EngramError> {
        let doc = MeilisearchMemory::from(memory);
        self.rt.block_on(async {
            let task = self
                .client
                .index(MEMORIES_INDEX)
                .add_documents(&[doc], Some("id"))
                .await
                .map_err(|e| EngramError::Storage(e.to_string()))?;
            self.client
                .index(MEMORIES_INDEX)
                .wait_for_task(task, None, None)
                .await
                .map_err(|e| EngramError::Storage(e.to_string()))?;
            Ok(())
        })
    }

    pub fn index_memories(&self, memories: &[Memory]) -> Result<(), EngramError> {
        if memories.is_empty() {
            return Ok(());
        }
        let docs: Vec<MeilisearchMemory> = memories.iter().map(MeilisearchMemory::from).collect();
        self.rt.block_on(async {
            let task = self
                .client
                .index(MEMORIES_INDEX)
                .add_documents(&docs, Some("id"))
                .await
                .map_err(|e| EngramError::Storage(e.to_string()))?;
            self.client
                .index(MEMORIES_INDEX)
                .wait_for_task(task, None, None)
                .await
                .map_err(|e| EngramError::Storage(e.to_string()))?;
            Ok(())
        })
    }

    /// Get the configured Meilisearch URL
    pub fn url(&self) -> &str {
        &self.url
    }

    /// Whether an API key is configured
    pub fn has_api_key(&self) -> bool {
        self.api_key.is_some()
    }

    /// Get index statistics from Meilisearch
    pub fn get_index_stats(&self) -> Result<serde_json::Value, EngramError> {
        self.rt.block_on(async {
            let stats = self
                .client
                .index(MEMORIES_INDEX)
                .get_stats()
                .await
                .map_err(|e| EngramError::Storage(e.to_string()))?;
            Ok(serde_json::json!({
                "number_of_documents": stats.number_of_documents,
                "is_indexing": stats.is_indexing,
            }))
        })
    }

    /// Get facet distribution for a field (used for tag/workspace listing)
    fn get_facet_distribution(
        &self,
        field: &str,
        filter: Option<&str>,
    ) -> Result<HashMap<String, usize>, EngramError> {
        self.rt.block_on(async {
            let index = self.client.index(MEMORIES_INDEX);
            let mut search = index.search();
            search.with_query("");
            search.with_limit(0);
            let facet_fields = [field];
            search.with_facets(meilisearch_sdk::search::Selectors::Some(&facet_fields));
            if let Some(f) = filter {
                search.with_filter(f);
            }

            let results: SearchResults<MeilisearchMemory> = search
                .execute()
                .await
                .map_err(|e| EngramError::Storage(e.to_string()))?;

            let distribution = results
                .facet_distribution
                .and_then(|fd| fd.get(field).cloned())
                .unwrap_or_default();

            Ok(distribution)
        })
    }
}

impl StorageBackend for MeilisearchBackend {
    fn create_memory(&self, input: CreateMemoryInput) -> Result<Memory, EngramError> {
        let id = generate_memory_id();
        let now = chrono::Utc::now();
        let memory = build_memory_from_input(id, input, now)?;

        self.index_memory(&memory)?;
        Ok(memory)
    }

    fn get_memory(&self, id: MemoryId) -> Result<Option<Memory>, EngramError> {
        self.rt.block_on(async {
            match self
                .client
                .index(MEMORIES_INDEX)
                .get_document::<MeilisearchMemory>(&id.to_string())
                .await
            {
                Ok(doc) => Ok(Some(build_memory_from_doc(doc))),
                Err(meilisearch_sdk::errors::Error::Meilisearch(e))
                    if e.error_code == meilisearch_sdk::errors::ErrorCode::DocumentNotFound =>
                {
                    Ok(None)
                }
                Err(e) => Err(EngramError::Storage(e.to_string())),
            }
        })
    }

    fn update_memory(&self, id: MemoryId, input: UpdateMemoryInput) -> Result<Memory, EngramError> {
        let mut memory = self.get_memory(id)?.ok_or(EngramError::NotFound(id))?;
        let mut changed = false;
        let now = chrono::Utc::now();

        if let Some(content) = input.content {
            memory.content = content;
            memory.content_hash = Some(compute_content_hash(&memory.content));
            changed = true;
        }
        if let Some(memory_type) = input.memory_type {
            memory.memory_type = memory_type;
            changed = true;
        }
        if let Some(tags) = input.tags {
            memory.tags = tags;
            changed = true;
        }
        if let Some(metadata) = input.metadata {
            memory.metadata = metadata;
            changed = true;
        }
        if let Some(importance) = input.importance {
            memory.importance = importance;
            changed = true;
        }
        if let Some(scope) = input.scope {
            memory.scope = scope;
            changed = true;
        }
        if let Some(event_time) = input.event_time {
            memory.event_time = event_time;
            changed = true;
        }
        if let Some(trigger_pattern) = input.trigger_pattern {
            memory.trigger_pattern = trigger_pattern;
            changed = true;
        }
        if let Some(ttl) = input.ttl_seconds {
            if ttl <= 0 {
                if memory.tier == MemoryTier::Daily {
                    return Err(EngramError::InvalidInput(
                        "Cannot remove expiration from a Daily tier memory. Use promote_to_permanent first.".to_string(),
                    ));
                }
                memory.expires_at = None;
            } else {
                if memory.tier == MemoryTier::Permanent {
                    return Err(EngramError::InvalidInput(
                        "Cannot set expiration on a Permanent tier memory. Permanent memories cannot expire.".to_string(),
                    ));
                }
                memory.expires_at = Some(now + chrono::Duration::seconds(ttl));
            }
            changed = true;
        }

        if changed {
            memory.updated_at = now;
            memory.version += 1;
        }

        self.index_memory(&memory)?;
        Ok(memory)
    }

    fn delete_memory(&self, id: MemoryId) -> Result<(), EngramError> {
        self.rt.block_on(async {
            let task = self
                .client
                .index(MEMORIES_INDEX)
                .delete_document(&id.to_string())
                .await
                .map_err(|e| EngramError::Storage(e.to_string()))?;
            self.client
                .index(MEMORIES_INDEX)
                .wait_for_task(task, None, None)
                .await
                .map_err(|e| EngramError::Storage(e.to_string()))?;
            Ok(())
        })
    }

    // --- Batch Operations ---

    fn create_memories_batch(
        &self,
        inputs: Vec<CreateMemoryInput>,
    ) -> Result<BatchCreateResult, EngramError> {
        let start = std::time::Instant::now();
        let mut created = Vec::new();
        let mut docs = Vec::new();
        let mut failed = Vec::new();
        let now = chrono::Utc::now();

        for (idx, input) in inputs.into_iter().enumerate() {
            let id = generate_memory_id();
            match build_memory_from_input(id, input, now) {
                Ok(memory) => {
                    created.push(memory.clone());
                    docs.push(MeilisearchMemory::from(&memory));
                }
                Err(e) => failed.push((idx, e.to_string())),
            }
        }

        if !docs.is_empty() {
            self.rt.block_on(async {
                let task = self
                    .client
                    .index(MEMORIES_INDEX)
                    .add_documents(&docs, Some("id"))
                    .await
                    .map_err(|e| EngramError::Storage(e.to_string()))?;
                self.client
                    .index(MEMORIES_INDEX)
                    .wait_for_task(task, None, None)
                    .await
                    .map_err(|e| EngramError::Storage(e.to_string()))?;
                Ok::<(), EngramError>(())
            })?;
        }

        Ok(BatchCreateResult {
            created,
            failed,
            elapsed_ms: start.elapsed().as_secs_f64() * 1000.0,
        })
    }

    fn delete_memories_batch(&self, ids: Vec<MemoryId>) -> Result<BatchDeleteResult, EngramError> {
        self.rt.block_on(async {
            let task = self
                .client
                .index(MEMORIES_INDEX)
                .delete_documents(&ids)
                .await
                .map_err(|e| EngramError::Storage(e.to_string()))?;
            self.client
                .index(MEMORIES_INDEX)
                .wait_for_task(task, None, None)
                .await
                .map_err(|e| EngramError::Storage(e.to_string()))
        })?;

        Ok(BatchDeleteResult {
            deleted_count: ids.len(),
            not_found: vec![],
            failed: vec![],
        })
    }

    // --- Query Operations ---

    fn list_memories(&self, options: ListOptions) -> Result<Vec<Memory>, EngramError> {
        let filter = build_filter_from_list_options(&options)?;
        let sort = sort_to_meili(
            options.sort_by.unwrap_or(SortField::CreatedAt),
            options.sort_order.unwrap_or(SortOrder::Desc),
        );
        let sort_refs = vec![sort.as_str()];

        self.rt.block_on(async {
            let index = self.client.index(MEMORIES_INDEX);
            let mut search = index.search();
            search.with_query("");
            search.with_limit(options.limit.unwrap_or(50) as usize);
            search.with_sort(&sort_refs);
            if let Some(ref filter) = filter {
                search.with_filter(filter);
            }

            let results: SearchResults<MeilisearchMemory> = search
                .execute()
                .await
                .map_err(|e| EngramError::Storage(e.to_string()))?;

            Ok(results
                .hits
                .into_iter()
                .map(|hit| build_memory_from_doc(hit.result))
                .collect())
        })
    }

    fn count_memories(&self, _options: ListOptions) -> Result<i64, EngramError> {
        self.rt.block_on(async {
            let stats = self
                .client
                .index(MEMORIES_INDEX)
                .get_stats()
                .await
                .map_err(|e| EngramError::Storage(e.to_string()))?;
            Ok(stats.number_of_documents as i64)
        })
    }

    fn search_memories(
        &self,
        query: &str,
        options: SearchOptions,
    ) -> Result<Vec<SearchResult>, EngramError> {
        self.rt.block_on(async {
            let index = self.client.index(MEMORIES_INDEX);
            let mut search = index.search();

            search.with_query(query);
            search.with_limit(options.limit.unwrap_or(50) as usize);

            let filter = build_filter_from_search_options(&options)?;
            if let Some(ref filter) = filter {
                search.with_filter(filter);
            }

            let results: SearchResults<MeilisearchMemory> = search
                .execute()
                .await
                .map_err(|e| EngramError::Storage(e.to_string()))?;

            Ok(results
                .hits
                .into_iter()
                .map(|hit| {
                    let memory = build_memory_from_doc(hit.result);
                    let score = hit.ranking_score.unwrap_or(0.0) as f32;

                    SearchResult {
                        memory,
                        score,
                        match_info: MatchInfo {
                            strategy: SearchStrategy::KeywordOnly, // Meilisearch is primarily keyword/typo-tolerant
                            matched_terms: vec![], // Would need parsing of hit._formatted or similar
                            highlights: vec![],
                            semantic_score: None,
                            keyword_score: Some(score),
                        },
                    }
                })
                .collect())
        })
    }

    // --- Graph Operations (Not supported in plain Meilisearch) ---

    fn create_crossref(
        &self,
        _from_id: MemoryId,
        _to_id: MemoryId,
        _edge_type: EdgeType,
        _score: f32,
    ) -> Result<CrossReference, EngramError> {
        Err(EngramError::Storage(
            "Graph operations not supported in Meilisearch backend".to_string(),
        ))
    }

    fn get_crossrefs(&self, _memory_id: MemoryId) -> Result<Vec<CrossReference>, EngramError> {
        Ok(vec![])
    }

    fn delete_crossref(&self, _from_id: MemoryId, _to_id: MemoryId) -> Result<(), EngramError> {
        Ok(())
    }

    // --- Tag Operations ---

    fn list_tags(&self) -> Result<Vec<(String, i64)>, EngramError> {
        let distribution = self.get_facet_distribution("tags", None)?;
        let mut tags: Vec<(String, i64)> = distribution
            .into_iter()
            .map(|(tag, count)| (tag, count as i64))
            .collect();
        tags.sort_by(|a, b| b.1.cmp(&a.1));
        Ok(tags)
    }

    fn get_memories_by_tag(
        &self,
        tag: &str,
        limit: Option<usize>,
    ) -> Result<Vec<Memory>, EngramError> {
        let options = SearchOptions {
            limit: Some(limit.unwrap_or(50) as i64),
            ..Default::default()
        };
        self.search_memories(tag, options)
            .map(|results| results.into_iter().map(|r| r.memory).collect())
    }

    // --- Workspace Operations ---

    fn list_workspaces(&self) -> Result<Vec<(String, i64)>, EngramError> {
        let distribution = self.get_facet_distribution("workspace", None)?;
        let mut workspaces: Vec<(String, i64)> = distribution
            .into_iter()
            .map(|(ws, count)| (ws, count as i64))
            .collect();
        workspaces.sort_by(|a, b| b.1.cmp(&a.1));
        Ok(workspaces)
    }

    fn get_workspace_stats(&self, workspace: &str) -> Result<HashMap<String, i64>, EngramError> {
        let filter = format!("workspace = \"{}\"", escape_filter_value(workspace));
        let type_dist = self.get_facet_distribution("memory_type", Some(&filter))?;
        let mut stats: HashMap<String, i64> =
            type_dist.into_iter().map(|(k, v)| (k, v as i64)).collect();
        let total: i64 = stats.values().sum();
        stats.insert("total".to_string(), total);
        Ok(stats)
    }

    fn move_to_workspace(&self, ids: Vec<MemoryId>, workspace: &str) -> Result<usize, EngramError> {
        let mut moved = 0;
        for id in &ids {
            if let Some(mut memory) = self.get_memory(*id)? {
                memory.workspace = workspace.to_string();
                memory.updated_at = chrono::Utc::now();
                self.index_memory(&memory)?;
                moved += 1;
            }
        }
        Ok(moved)
    }

    // --- Maintenance & Metadata ---

    fn get_stats(&self) -> Result<StorageStats, EngramError> {
        let count = self.count_memories(ListOptions::default())?;
        Ok(StorageStats {
            total_memories: count,
            storage_mode: "meilisearch".to_string(),
            ..Default::default()
        })
    }

    fn health_check(&self) -> Result<HealthStatus, EngramError> {
        self.rt.block_on(async {
            match self.client.health().await {
                Ok(_) => Ok(HealthStatus {
                    healthy: true,
                    latency_ms: 0.0,
                    error: None,
                    details: HashMap::new(),
                }),
                Err(e) => Ok(HealthStatus {
                    healthy: false,
                    latency_ms: 0.0,
                    error: Some(e.to_string()),
                    details: HashMap::new(),
                }),
            }
        })
    }

    fn optimize(&self) -> Result<(), EngramError> {
        Ok(())
    }

    fn backend_name(&self) -> &'static str {
        "meilisearch"
    }

    fn schema_version(&self) -> Result<i32, EngramError> {
        Ok(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MemoryType;

    // --- escape_filter_value ---

    #[test]
    fn test_escape_filter_value_plain() {
        assert_eq!(escape_filter_value("hello"), "hello");
    }

    #[test]
    fn test_escape_filter_value_quotes() {
        assert_eq!(escape_filter_value(r#"say "hi""#), r#"say \"hi\""#);
    }

    #[test]
    fn test_escape_filter_value_backslashes() {
        assert_eq!(escape_filter_value(r"path\to"), r"path\\to");
    }

    #[test]
    fn test_escape_filter_value_mixed() {
        assert_eq!(escape_filter_value(r#"a\"b"#), r#"a\\\"b"#);
    }

    // --- build_tags_filter ---

    #[test]
    fn test_build_tags_filter_empty() {
        assert_eq!(build_tags_filter(&[]), None);
    }

    #[test]
    fn test_build_tags_filter_single() {
        let tags = vec!["rust".to_string()];
        assert_eq!(
            build_tags_filter(&tags),
            Some(r#"tags = "rust""#.to_string())
        );
    }

    #[test]
    fn test_build_tags_filter_multiple() {
        let tags = vec!["rust".to_string(), "async".to_string()];
        assert_eq!(
            build_tags_filter(&tags),
            Some(r#"tags = "rust" AND tags = "async""#.to_string())
        );
    }

    #[test]
    fn test_build_tags_filter_special_chars() {
        let tags = vec![r#"say "hi""#.to_string()];
        assert_eq!(
            build_tags_filter(&tags),
            Some(r#"tags = "say \"hi\"""#.to_string())
        );
    }

    // --- build_workspace_filter ---

    #[test]
    fn test_build_workspace_filter_empty() {
        assert_eq!(build_workspace_filter(&[]), None);
    }

    #[test]
    fn test_build_workspace_filter_single() {
        let ws = vec!["default".to_string()];
        assert_eq!(
            build_workspace_filter(&ws),
            Some(r#"workspace = "default""#.to_string())
        );
    }

    #[test]
    fn test_build_workspace_filter_multiple() {
        let ws = vec!["proj-a".to_string(), "proj-b".to_string()];
        assert_eq!(
            build_workspace_filter(&ws),
            Some(r#"workspace IN ["proj-a", "proj-b"]"#.to_string())
        );
    }

    // --- build_scope_filter ---

    #[test]
    fn test_build_scope_filter_global() {
        let parts = build_scope_filter(&MemoryScope::Global);
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0], r#"scope = "global""#);
        assert_eq!(parts[1], "scope_id IS NULL");
    }

    #[test]
    fn test_build_scope_filter_user() {
        let parts = build_scope_filter(&MemoryScope::User {
            user_id: "u123".to_string(),
        });
        assert_eq!(parts[0], r#"scope = "user""#);
        assert_eq!(parts[1], r#"scope_id = "u123""#);
    }

    #[test]
    fn test_build_scope_filter_session() {
        let parts = build_scope_filter(&MemoryScope::Session {
            session_id: "s-abc".to_string(),
        });
        assert_eq!(parts[0], r#"scope = "session""#);
        assert_eq!(parts[1], r#"scope_id = "s-abc""#);
    }

    #[test]
    fn test_build_scope_filter_agent() {
        let parts = build_scope_filter(&MemoryScope::Agent {
            agent_id: "agent-1".to_string(),
        });
        assert_eq!(parts[0], r#"scope = "agent""#);
        assert_eq!(parts[1], r#"scope_id = "agent-1""#);
    }

    // --- build_filter_from_search_options ---

    #[test]
    fn test_search_filter_defaults() {
        let opts = SearchOptions::default();
        let filter = build_filter_from_search_options(&opts).unwrap();
        // Default excludes transcript_chunk and archived
        let f = filter.unwrap();
        assert!(f.contains(r#"memory_type != "transcript_chunk""#));
        assert!(f.contains(r#"lifecycle_state != "archived""#));
    }

    #[test]
    fn test_search_filter_with_workspace() {
        let opts = SearchOptions {
            workspace: Some("my-proj".to_string()),
            ..Default::default()
        };
        let filter = build_filter_from_search_options(&opts).unwrap().unwrap();
        assert!(filter.contains(r#"workspace = "my-proj""#));
    }

    #[test]
    fn test_search_filter_with_tags_and_type() {
        let opts = SearchOptions {
            tags: Some(vec!["rust".to_string()]),
            memory_type: Some(MemoryType::Note),
            ..Default::default()
        };
        let filter = build_filter_from_search_options(&opts).unwrap().unwrap();
        assert!(filter.contains(r#"memory_type = "note""#));
        assert!(filter.contains(r#"tags = "rust""#));
        // When memory_type is set, transcript_chunk exclusion is NOT added
        assert!(!filter.contains("transcript_chunk"));
    }

    #[test]
    fn test_search_filter_rejects_advanced_filter() {
        let opts = SearchOptions {
            filter: Some(serde_json::json!({"and": []})),
            ..Default::default()
        };
        assert!(build_filter_from_search_options(&opts).is_err());
    }

    #[test]
    fn test_search_filter_include_transcripts() {
        let opts = SearchOptions {
            include_transcripts: true,
            ..Default::default()
        };
        let filter = build_filter_from_search_options(&opts).unwrap();
        let f = filter.unwrap();
        // Should NOT exclude transcript_chunk when include_transcripts is true
        assert!(!f.contains("transcript_chunk"));
    }

    #[test]
    fn test_search_filter_include_archived() {
        let opts = SearchOptions {
            include_archived: true,
            ..Default::default()
        };
        let filter = build_filter_from_search_options(&opts).unwrap();
        let f = filter.unwrap();
        assert!(!f.contains("archived"));
    }

    // --- build_filter_from_list_options ---

    #[test]
    fn test_list_filter_defaults() {
        let opts = ListOptions::default();
        let filter = build_filter_from_list_options(&opts).unwrap();
        // Default only excludes archived
        let f = filter.unwrap();
        assert!(f.contains(r#"lifecycle_state != "archived""#));
    }

    #[test]
    fn test_list_filter_with_workspace_and_tier() {
        let opts = ListOptions {
            workspace: Some("eng".to_string()),
            tier: Some(MemoryTier::Permanent),
            ..Default::default()
        };
        let filter = build_filter_from_list_options(&opts).unwrap().unwrap();
        assert!(filter.contains(r#"workspace = "eng""#));
        assert!(filter.contains(r#"tier = "permanent""#));
    }

    #[test]
    fn test_list_filter_rejects_metadata_filter() {
        let opts = ListOptions {
            metadata_filter: Some(HashMap::from([(
                "key".to_string(),
                serde_json::json!("val"),
            )])),
            ..Default::default()
        };
        assert!(build_filter_from_list_options(&opts).is_err());
    }

    // --- sort_to_meili ---

    #[test]
    fn test_sort_created_at_desc() {
        assert_eq!(
            sort_to_meili(SortField::CreatedAt, SortOrder::Desc),
            "created_at:desc"
        );
    }

    #[test]
    fn test_sort_importance_asc() {
        assert_eq!(
            sort_to_meili(SortField::Importance, SortOrder::Asc),
            "importance:asc"
        );
    }

    #[test]
    fn test_sort_all_fields() {
        // Verify all sort fields produce valid output
        let fields = [
            SortField::CreatedAt,
            SortField::UpdatedAt,
            SortField::LastAccessedAt,
            SortField::Importance,
            SortField::AccessCount,
        ];
        for field in fields {
            let result = sort_to_meili(field, SortOrder::Desc);
            assert!(result.ends_with(":desc"));
            assert!(!result.starts_with(':'));
        }
    }

    // --- scope_from_parts ---

    #[test]
    fn test_scope_from_parts_user() {
        let scope = scope_from_parts("user", Some("u1".to_string()));
        assert!(matches!(scope, MemoryScope::User { user_id } if user_id == "u1"));
    }

    #[test]
    fn test_scope_from_parts_global_fallback() {
        let scope = scope_from_parts("unknown", None);
        assert!(matches!(scope, MemoryScope::Global));
    }

    #[test]
    fn test_scope_from_parts_missing_id_falls_back() {
        // "user" without an ID falls back to Global
        let scope = scope_from_parts("user", None);
        assert!(matches!(scope, MemoryScope::Global));
    }

    // --- visibility_from_str / visibility_to_str roundtrip ---

    #[test]
    fn test_visibility_roundtrip() {
        for vis in [Visibility::Private, Visibility::Shared, Visibility::Public] {
            let s = visibility_to_str(vis);
            let back = visibility_from_str(s);
            assert_eq!(back, vis);
        }
    }

    #[test]
    fn test_visibility_unknown_defaults_private() {
        assert_eq!(visibility_from_str("unknown"), Visibility::Private);
    }

    // --- build_memory_from_doc ---

    #[test]
    fn test_build_memory_from_doc_roundtrip() {
        let doc = MeilisearchMemory {
            id: 42,
            content: "test memory".to_string(),
            memory_type: "note".to_string(),
            tags: vec!["tag1".to_string()],
            metadata: Some(HashMap::new()),
            created_at: 1700000000,
            updated_at: 1700001000,
            last_accessed_at: Some(1700002000),
            importance: 0.8,
            access_count: 5,
            owner_id: None,
            visibility: "private".to_string(),
            scope: "global".to_string(),
            scope_id: None,
            workspace: "default".to_string(),
            tier: "permanent".to_string(),
            version: 1,
            has_embedding: true,
            expires_at: None,
            content_hash: Some("abc123".to_string()),
            event_time: None,
            event_duration_seconds: None,
            trigger_pattern: None,
            procedure_success_count: 0,
            procedure_failure_count: 0,
            summary_of_id: None,
            lifecycle_state: "active".to_string(),
        };

        let memory = build_memory_from_doc(doc);

        assert_eq!(memory.id, 42);
        assert_eq!(memory.content, "test memory");
        assert_eq!(memory.memory_type, MemoryType::Note);
        assert_eq!(memory.tags, vec!["tag1".to_string()]);
        assert_eq!(memory.importance, 0.8);
        assert_eq!(memory.access_count, 5);
        assert_eq!(memory.workspace, "default");
        assert_eq!(memory.tier, MemoryTier::Permanent);
        assert!(memory.has_embedding);
        assert_eq!(memory.content_hash, Some("abc123".to_string()));
        assert!(matches!(memory.scope, MemoryScope::Global));
        assert_eq!(memory.visibility, Visibility::Private);
        assert_eq!(memory.lifecycle_state, LifecycleState::Active);
    }
}
