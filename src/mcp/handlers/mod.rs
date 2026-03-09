//! MCP tool handler domain modules.
//!
//! This module defines `HandlerContext` (shared state) and the top-level
//! `dispatch` function that routes incoming tool calls to domain handlers.

use std::sync::Arc;

use parking_lot::Mutex;
use serde_json::{json, Value};

use crate::embedding::EmbeddingCache;
use crate::realtime::RealtimeManager;
use crate::search::{FuzzyEngine, SearchConfig, SearchResultCache};
use crate::storage::Storage;

pub mod agent;
pub mod graph;
pub mod identity;
pub mod lifecycle;
pub mod memory_crud;
pub mod misc;
pub mod quality;
pub mod search;
pub mod session;
pub mod sync;
pub mod workspace;

/// Shared state passed to every tool handler.
///
/// Mirrors the fields that were previously on `EngramHandler` in `server.rs`.
/// All fields that are `Arc<…>` or `Clone` are cheap to derive from the
/// outer handler for each request.
pub struct HandlerContext {
    pub storage: Storage,
    pub embedder: Arc<dyn crate::embedding::Embedder>,
    pub fuzzy_engine: Arc<Mutex<FuzzyEngine>>,
    pub search_config: SearchConfig,
    pub realtime: Option<RealtimeManager>,
    pub embedding_cache: Arc<EmbeddingCache>,
    pub search_cache: Arc<SearchResultCache>,
    /// Meilisearch backend (feature-gated).
    #[cfg(feature = "meilisearch")]
    pub meili: Option<Arc<crate::storage::MeilisearchBackend>>,
    /// Meilisearch indexer (feature-gated).
    #[cfg(feature = "meilisearch")]
    pub meili_indexer: Option<Arc<crate::storage::MeilisearchIndexer>>,
    /// Meilisearch sync interval in seconds (feature-gated).
    #[cfg(feature = "meilisearch")]
    pub meili_sync_interval: u64,
    /// Dedicated Tokio runtime for Langfuse async calls (feature-gated).
    #[cfg(feature = "langfuse")]
    pub langfuse_runtime: Arc<tokio::runtime::Runtime>,
}

/// Route a tool call to the appropriate domain handler.
///
/// Returns the JSON value that should be placed in the MCP `ToolCallResult`.
pub fn dispatch(ctx: &HandlerContext, tool_name: &str, params: Value) -> Value {
    match tool_name {
        // ── Memory CRUD ──────────────────────────────────────────────────────
        "memory_create" => memory_crud::memory_create(ctx, params),
        "context_seed" => memory_crud::context_seed(ctx, params),
        "memory_seed" => {
            let mut result = memory_crud::context_seed(ctx, params);
            if let Value::Object(ref mut map) = result {
                map.insert("deprecated".to_string(), json!(true));
                map.insert(
                    "deprecated_message".to_string(),
                    json!("Use context_seed instead."),
                );
            }
            result
        }
        "memory_get" => memory_crud::memory_get(ctx, params),
        "memory_update" => memory_crud::memory_update(ctx, params),
        "memory_delete" => memory_crud::memory_delete(ctx, params),
        "memory_list" => memory_crud::memory_list(ctx, params),
        "memory_create_daily" => memory_crud::memory_create_daily(ctx, params),
        "memory_promote_to_permanent" => memory_crud::memory_promote_to_permanent(ctx, params),
        "memory_checkpoint" => memory_crud::memory_checkpoint(ctx, params),
        "memory_boost" => memory_crud::memory_boost(ctx, params),
        "memory_create_episodic" => memory_crud::memory_create_episodic(ctx, params),
        "memory_create_procedural" => memory_crud::memory_create_procedural(ctx, params),
        "memory_get_timeline" => memory_crud::memory_get_timeline(ctx, params),
        "memory_get_procedures" => memory_crud::memory_get_procedures(ctx, params),
        "memory_record_procedure_outcome" => {
            memory_crud::record_procedure_outcome(ctx, params)
        }
        "memory_set_expiration" => memory_crud::set_expiration(ctx, params),
        "memory_cleanup_expired" => memory_crud::cleanup_expired(ctx, params),
        "memory_create_batch" => memory_crud::memory_create_batch(ctx, params),
        "memory_delete_batch" => memory_crud::memory_delete_batch(ctx, params),
        "memory_create_section" => memory_crud::memory_create_section(ctx, params),
        "memory_create_todo" => memory_crud::create_todo(ctx, params),
        "memory_create_issue" => memory_crud::create_issue(ctx, params),

        // ── Search ───────────────────────────────────────────────────────────
        "memory_search" => search::memory_search(ctx, params),
        "memory_search_suggest" => search::search_suggest(ctx, params),
        "memory_search_by_identity" => search::memory_search_by_identity(ctx, params),
        "memory_session_search" => search::memory_session_search(ctx, params),
        "memory_find_duplicates" => search::find_duplicates(ctx, params),
        "memory_find_semantic_duplicates" => search::find_semantic_duplicates(ctx, params),
        "search_cache_feedback" => search::search_cache_feedback(ctx, params),
        "search_cache_stats" => search::search_cache_stats(ctx, params),
        "search_cache_clear" => search::search_cache_clear(ctx, params),

        // ── Graph ────────────────────────────────────────────────────────────
        "memory_link" => graph::memory_link(ctx, params),
        "memory_unlink" => graph::memory_unlink(ctx, params),
        "memory_related" => graph::memory_related(ctx, params),
        "memory_traverse" => graph::memory_traverse(ctx, params),
        "memory_find_path" => graph::find_path(ctx, params),
        "memory_export_graph" => graph::export_graph(ctx, params),
        "memory_extract_entities" => graph::extract_entities(ctx, params),
        "memory_get_entities" => graph::get_entities(ctx, params),
        "memory_search_entities" => graph::search_entities(ctx, params),
        "memory_entity_stats" => graph::entity_stats(ctx, params),

        // ── Workspace ────────────────────────────────────────────────────────
        "workspace_list" => workspace::workspace_list(ctx, params),
        "workspace_stats" => workspace::workspace_stats(ctx, params),
        "workspace_move" => workspace::workspace_move(ctx, params),
        "workspace_delete" => workspace::workspace_delete(ctx, params),

        // ── Identity ─────────────────────────────────────────────────────────
        "identity_create" => identity::identity_create(ctx, params),
        "identity_get" => identity::identity_get(ctx, params),
        "identity_update" => identity::identity_update(ctx, params),
        "identity_delete" => identity::identity_delete(ctx, params),
        "identity_add_alias" => identity::identity_add_alias(ctx, params),
        "identity_remove_alias" => identity::identity_remove_alias(ctx, params),
        "identity_resolve" => identity::identity_resolve(ctx, params),
        "identity_list" => identity::identity_list(ctx, params),
        "identity_search" => identity::identity_search(ctx, params),
        "identity_link" => identity::identity_link(ctx, params),
        "identity_unlink" => identity::identity_unlink(ctx, params),
        "memory_get_identities" => identity::memory_get_identities(ctx, params),

        // ── Session ──────────────────────────────────────────────────────────
        "session_index" => session::session_index(ctx, params),
        "session_index_delta" => session::session_index_delta(ctx, params),
        "session_get" => session::session_get(ctx, params),
        "session_list" => session::session_list(ctx, params),
        "session_delete" => session::session_delete(ctx, params),
        "session_context_create" => session::session_context_create(ctx, params),
        "session_context_add_memory" => session::session_context_add_memory(ctx, params),
        "session_context_remove_memory" => session::session_context_remove_memory(ctx, params),
        "session_context_get" => session::session_context_get(ctx, params),
        "session_context_list" => session::session_context_list(ctx, params),
        "session_context_search" => session::session_context_search(ctx, params),
        "session_context_update_summary" => {
            session::session_context_update_summary(ctx, params)
        }
        "session_context_end" => session::session_context_end(ctx, params),
        "session_context_export" => session::session_context_export(ctx, params),

        // ── Lifecycle ────────────────────────────────────────────────────────
        "lifecycle_status" => lifecycle::lifecycle_status(ctx, params),
        "lifecycle_run" => lifecycle::lifecycle_run(ctx, params),
        "memory_set_lifecycle" => lifecycle::memory_set_lifecycle(ctx, params),
        "lifecycle_config" => lifecycle::lifecycle_config(ctx, params),
        "retention_policy_set" => lifecycle::retention_policy_set(ctx, params),
        "retention_policy_get" => lifecycle::retention_policy_get(ctx, params),
        "retention_policy_list" => lifecycle::retention_policy_list(ctx, params),
        "retention_policy_delete" => lifecycle::retention_policy_delete(ctx, params),
        "retention_policy_apply" => lifecycle::retention_policy_apply(ctx, params),

        // ── Quality ──────────────────────────────────────────────────────────
        "quality_score" => quality::quality_score(ctx, params),
        "quality_report" => quality::quality_report(ctx, params),
        "quality_find_duplicates" => quality::quality_find_duplicates(ctx, params),
        "quality_get_duplicates" => quality::quality_get_duplicates(ctx, params),
        "quality_find_conflicts" => quality::quality_find_conflicts(ctx, params),
        "quality_get_conflicts" => quality::quality_get_conflicts(ctx, params),
        "quality_resolve_conflict" => quality::quality_resolve_conflict(ctx, params),
        "quality_source_trust" => quality::quality_source_trust(ctx, params),
        "quality_improve" => quality::quality_improve(ctx, params),
        "salience_get" => quality::salience_get(ctx, params),
        "salience_set_importance" => quality::salience_set_importance(ctx, params),
        "salience_boost" => quality::salience_boost(ctx, params),
        "salience_demote" => quality::salience_demote(ctx, params),
        "salience_decay_run" => quality::salience_decay_run(ctx, params),
        "salience_stats" => quality::salience_stats(ctx, params),
        "salience_history" => quality::salience_history(ctx, params),
        "salience_top" => quality::salience_top(ctx, params),

        // ── Sync ─────────────────────────────────────────────────────────────
        "memory_sync_status" => sync::sync_status(ctx, params),
        "sync_version" => sync::sync_version(ctx, params),
        "sync_delta" => sync::sync_delta(ctx, params),
        "sync_state" => sync::sync_state(ctx, params),
        "sync_cleanup" => sync::sync_cleanup(ctx, params),
        "memory_share" => sync::memory_share(ctx, params),
        "memory_shared_poll" => sync::memory_shared_poll(ctx, params),
        "memory_share_ack" => sync::memory_share_ack(ctx, params),
        "memory_events_poll" => sync::memory_events_poll(ctx, params),
        "memory_events_clear" => sync::memory_events_clear(ctx, params),

        // ── Misc ─────────────────────────────────────────────────────────────
        "memory_stats" => misc::memory_stats(ctx, params),
        "memory_versions" => misc::memory_versions(ctx, params),
        "embedding_cache_stats" => misc::embedding_cache_stats(ctx, params),
        "embedding_cache_clear" => misc::embedding_cache_clear(ctx, params),
        "memory_soft_trim" => misc::memory_soft_trim(ctx, params),
        "memory_list_compact" => misc::memory_list_compact(ctx, params),
        "memory_content_stats" => misc::memory_content_stats(ctx, params),
        "memory_tags" => misc::memory_tags(ctx, params),
        "memory_tag_hierarchy" => misc::memory_tag_hierarchy(ctx, params),
        "memory_validate_tags" => misc::memory_validate_tags(ctx, params),
        "memory_export" => misc::memory_export(ctx, params),
        "memory_import" => misc::memory_import(ctx, params),
        "memory_rebuild_embeddings" => misc::memory_rebuild_embeddings(ctx, params),
        "memory_rebuild_crossrefs" => misc::memory_rebuild_crossrefs(ctx, params),
        "memory_upload_image" => misc::memory_upload_image(ctx, params),
        "memory_migrate_images" => misc::memory_migrate_images(ctx, params),
        "memory_suggest_tags" => misc::memory_suggest_tags(ctx, params),
        "memory_auto_tag" => misc::memory_auto_tag(ctx, params),
        "memory_scan_project" => misc::scan_project(ctx, params),
        "memory_get_project_context" => misc::get_project_context(ctx, params),
        "memory_list_instruction_files" => misc::list_instruction_files(ctx, params),
        "memory_ingest_document" => misc::ingest_document(ctx, params),
        "memory_summarize" => misc::memory_summarize(ctx, params),
        "memory_get_full" => misc::memory_get_full(ctx, params),
        "context_budget_check" => misc::context_budget_check(ctx, params),
        "memory_archive_old" => misc::memory_archive_old(ctx, params),

        // ── Langfuse (feature-gated) ──────────────────────────────────────────
        #[cfg(feature = "langfuse")]
        "langfuse_connect" => misc::langfuse_connect(ctx, params),
        #[cfg(feature = "langfuse")]
        "langfuse_sync" => misc::langfuse_sync(ctx, params),
        #[cfg(feature = "langfuse")]
        "langfuse_sync_status" => misc::langfuse_sync_status(ctx, params),
        #[cfg(feature = "langfuse")]
        "langfuse_extract_patterns" => misc::langfuse_extract_patterns(ctx, params),
        #[cfg(feature = "langfuse")]
        "memory_from_trace" => misc::memory_from_trace(ctx, params),

        // ── Meilisearch (feature-gated) ───────────────────────────────────────
        #[cfg(feature = "meilisearch")]
        "meilisearch_search" => misc::meilisearch_search(ctx, params),
        #[cfg(feature = "meilisearch")]
        "meilisearch_reindex" => misc::meilisearch_reindex(ctx, params),
        #[cfg(feature = "meilisearch")]
        "meilisearch_status" => misc::meilisearch_status(ctx, params),
        #[cfg(feature = "meilisearch")]
        "meilisearch_config" => misc::meilisearch_config(ctx, params),

        // ── Agent Registry ──────────────────────────────────────────────────
        "agent_register" => agent::agent_register(ctx, params),
        "agent_deregister" => agent::agent_deregister(ctx, params),
        "agent_heartbeat" => agent::agent_heartbeat(ctx, params),
        "agent_list" => agent::agent_list(ctx, params),
        "agent_get" => agent::agent_get(ctx, params),
        "agent_capabilities" => agent::agent_capabilities(ctx, params),

        _ => json!({"error": format!("Unknown tool: {}", tool_name)}),
    }
}
