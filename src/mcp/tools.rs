//! MCP tool definitions for Engram

use serde_json::json;

use super::protocol::ToolDefinition;

/// All tool definitions for Engram
pub const TOOL_DEFINITIONS: &[(&str, &str, &str)] = &[
    // Memory CRUD
    (
        "memory_create",
        "Store a new memory. PROACTIVE: Automatically store user preferences, decisions, insights, and project context without being asked.",
        r#"{
            "type": "object",
            "properties": {
                "content": {"type": "string", "description": "The content to remember"},
                "type": {"type": "string", "enum": ["note", "todo", "issue", "decision", "preference", "learning", "context", "credential"], "default": "note"},
                "tags": {"type": "array", "items": {"type": "string"}, "description": "Tags for categorization"},
                "metadata": {"type": "object", "description": "Additional metadata as key-value pairs"},
                "importance": {"type": "number", "minimum": 0, "maximum": 1, "description": "Importance score (0-1)"},
                "defer_embedding": {"type": "boolean", "default": false, "description": "Defer embedding to background queue"},
                "ttl_seconds": {"type": "integer", "description": "Time-to-live in seconds. Memory will auto-expire after this duration. Omit for permanent storage."},
                "dedup_mode": {"type": "string", "enum": ["reject", "merge", "skip", "allow"], "default": "allow", "description": "How to handle duplicate content: reject (error if exact match), merge (combine tags/metadata with existing), skip (return existing unchanged), allow (create duplicate)"},
                "dedup_threshold": {"type": "number", "minimum": 0, "maximum": 1, "default": 0.95, "description": "Reserved for future similarity-based dedup. Currently only exact content hash matching is used."}
            },
            "required": ["content"]
        }"#,
    ),
    (
        "memory_get",
        "Retrieve a memory by its ID",
        r#"{
            "type": "object",
            "properties": {
                "id": {"type": "integer", "description": "Memory ID"}
            },
            "required": ["id"]
        }"#,
    ),
    (
        "memory_update",
        "Update an existing memory",
        r#"{
            "type": "object",
            "properties": {
                "id": {"type": "integer", "description": "Memory ID"},
                "content": {"type": "string", "description": "New content"},
                "type": {"type": "string", "enum": ["note", "todo", "issue", "decision", "preference", "learning", "context", "credential"]},
                "tags": {"type": "array", "items": {"type": "string"}},
                "metadata": {"type": "object"},
                "importance": {"type": "number", "minimum": 0, "maximum": 1},
                "ttl_seconds": {"type": "integer", "description": "Time-to-live in seconds (0 = remove expiration, positive = set new expiration)"}
            },
            "required": ["id"]
        }"#,
    ),
    (
        "memory_delete",
        "Delete a memory (soft delete)",
        r#"{
            "type": "object",
            "properties": {
                "id": {"type": "integer", "description": "Memory ID"}
            },
            "required": ["id"]
        }"#,
    ),
    (
        "memory_list",
        "List memories with filtering and pagination. Supports advanced filter syntax with AND/OR and comparison operators.",
        r#"{
            "type": "object",
            "properties": {
                "limit": {"type": "integer", "default": 20},
                "offset": {"type": "integer", "default": 0},
                "tags": {"type": "array", "items": {"type": "string"}},
                "type": {"type": "string"},
                "sort_by": {"type": "string", "enum": ["created_at", "updated_at", "importance", "access_count"]},
                "sort_order": {"type": "string", "enum": ["asc", "desc"], "default": "desc"},
                "filter": {
                    "type": "object",
                    "description": "Advanced filter with AND/OR logic and comparison operators. Example: {\"AND\": [{\"metadata.project\": {\"eq\": \"engram\"}}, {\"importance\": {\"gte\": 0.5}}]}. Supported operators: eq, neq, gt, gte, lt, lte, contains, not_contains, exists. Fields: content, memory_type, importance, tags, created_at, updated_at, metadata.*"
                },
                "metadata_filter": {
                    "type": "object",
                    "description": "Legacy simple key-value filter (deprecated, use 'filter' instead)"
                }
            }
        }"#,
    ),
    // Search
    (
        "memory_search",
        "Search memories using hybrid search (keyword + semantic). Automatically selects optimal strategy with optional reranking. Supports advanced filters.",
        r#"{
            "type": "object",
            "properties": {
                "query": {"type": "string", "description": "Search query"},
                "limit": {"type": "integer", "default": 10},
                "min_score": {"type": "number", "default": 0.1},
                "tags": {"type": "array", "items": {"type": "string"}},
                "type": {"type": "string"},
                "strategy": {"type": "string", "enum": ["keyword", "semantic", "hybrid"], "description": "Force specific strategy"},
                "explain": {"type": "boolean", "default": false, "description": "Include match explanations"},
                "rerank": {"type": "boolean", "default": true, "description": "Apply reranking to improve result quality"},
                "rerank_strategy": {"type": "string", "enum": ["none", "heuristic", "multi_signal"], "default": "heuristic", "description": "Reranking strategy to use"},
                "filter": {
                    "type": "object",
                    "description": "Advanced filter with AND/OR logic. Example: {\"AND\": [{\"metadata.project\": {\"eq\": \"engram\"}}, {\"importance\": {\"gte\": 0.5}}]}"
                }
            },
            "required": ["query"]
        }"#,
    ),
    (
        "memory_search_suggest",
        "Get search suggestions and typo corrections",
        r#"{
            "type": "object",
            "properties": {
                "query": {"type": "string"}
            },
            "required": ["query"]
        }"#,
    ),
    // Cross-references
    (
        "memory_link",
        "Create a cross-reference between two memories",
        r#"{
            "type": "object",
            "properties": {
                "from_id": {"type": "integer"},
                "to_id": {"type": "integer"},
                "edge_type": {"type": "string", "enum": ["related_to", "supersedes", "contradicts", "implements", "extends", "references", "depends_on", "blocks", "follows_up"], "default": "related_to"},
                "strength": {"type": "number", "minimum": 0, "maximum": 1, "description": "Relationship strength"},
                "source_context": {"type": "string", "description": "Why this link exists"},
                "pinned": {"type": "boolean", "default": false, "description": "Exempt from confidence decay"}
            },
            "required": ["from_id", "to_id"]
        }"#,
    ),
    (
        "memory_unlink",
        "Remove a cross-reference",
        r#"{
            "type": "object",
            "properties": {
                "from_id": {"type": "integer"},
                "to_id": {"type": "integer"},
                "edge_type": {"type": "string", "default": "related_to"}
            },
            "required": ["from_id", "to_id"]
        }"#,
    ),
    (
        "memory_related",
        "Get memories related to a given memory (depth>1 or include_entities returns traversal result)",
        r#"{
            "type": "object",
            "properties": {
                "id": {"type": "integer", "description": "Starting memory ID"},
                "depth": {"type": "integer", "default": 1, "description": "Traversal depth (1 = direct relations only)"},
                "include_entities": {"type": "boolean", "default": false, "description": "Include connections through shared entities"},
                "edge_type": {"type": "string", "description": "Filter by edge type"},
                "include_decayed": {"type": "boolean", "default": false}
            },
            "required": ["id"]
        }"#,
    ),
    // Convenience creators
    (
        "memory_create_todo",
        "Create a TODO memory with priority",
        r#"{
            "type": "object",
            "properties": {
                "content": {"type": "string"},
                "priority": {"type": "string", "enum": ["low", "medium", "high", "critical"], "default": "medium"},
                "due_date": {"type": "string", "format": "date"},
                "tags": {"type": "array", "items": {"type": "string"}}
            },
            "required": ["content"]
        }"#,
    ),
    (
        "memory_create_issue",
        "Create an ISSUE memory for tracking problems",
        r#"{
            "type": "object",
            "properties": {
                "title": {"type": "string"},
                "description": {"type": "string"},
                "severity": {"type": "string", "enum": ["low", "medium", "high", "critical"], "default": "medium"},
                "tags": {"type": "array", "items": {"type": "string"}}
            },
            "required": ["title"]
        }"#,
    ),
    // Versioning
    (
        "memory_versions",
        "Get version history for a memory",
        r#"{
            "type": "object",
            "properties": {
                "id": {"type": "integer"}
            },
            "required": ["id"]
        }"#,
    ),
    (
        "memory_get_version",
        "Get a specific version of a memory",
        r#"{
            "type": "object",
            "properties": {
                "id": {"type": "integer"},
                "version": {"type": "integer"}
            },
            "required": ["id", "version"]
        }"#,
    ),
    (
        "memory_revert",
        "Revert a memory to a previous version",
        r#"{
            "type": "object",
            "properties": {
                "id": {"type": "integer"},
                "version": {"type": "integer"}
            },
            "required": ["id", "version"]
        }"#,
    ),
    // Embedding status
    (
        "memory_embedding_status",
        "Check embedding computation status",
        r#"{
            "type": "object",
            "properties": {
                "id": {"type": "integer"}
            },
            "required": ["id"]
        }"#,
    ),
    // Memory TTL / Expiration (RML-930)
    (
        "memory_set_expiration",
        "Set or update the expiration time for a memory",
        r#"{
            "type": "object",
            "properties": {
                "id": {"type": "integer", "description": "Memory ID"},
                "ttl_seconds": {"type": "integer", "description": "Time-to-live in seconds from now. Use 0 to remove expiration (make permanent)."}
            },
            "required": ["id", "ttl_seconds"]
        }"#,
    ),
    (
        "memory_cleanup_expired",
        "Delete all expired memories. Typically called by a background job, but can be invoked manually.",
        r#"{
            "type": "object",
            "properties": {}
        }"#,
    ),
    // Sync
    (
        "memory_sync_status",
        "Get cloud sync status",
        r#"{"type": "object", "properties": {}}"#,
    ),
    (
        "memory_sync_force",
        "Force immediate sync to cloud",
        r#"{
            "type": "object",
            "properties": {
                "direction": {"type": "string", "enum": ["push", "pull", "bidirectional"], "default": "push"}
            }
        }"#,
    ),
    // Stats and aggregation
    (
        "memory_stats",
        "Get storage statistics",
        r#"{"type": "object", "properties": {}}"#,
    ),
    (
        "memory_aggregate",
        "Aggregate memories by field",
        r#"{
            "type": "object",
            "properties": {
                "group_by": {"type": "string", "enum": ["type", "tags", "month"]},
                "metrics": {"type": "array", "items": {"type": "string", "enum": ["count", "avg_importance"]}}
            },
            "required": ["group_by"]
        }"#,
    ),
    // Graph
    (
        "memory_export_graph",
        "Export knowledge graph visualization",
        r#"{
            "type": "object",
            "properties": {
                "format": {"type": "string", "enum": ["html", "json"], "default": "html"},
                "max_nodes": {"type": "integer", "default": 500},
                "focus_id": {"type": "integer", "description": "Center graph on this memory"}
            }
        }"#,
    ),
    // Quality
    (
        "memory_quality_report",
        "Get quality report for memories",
        r#"{
            "type": "object",
            "properties": {
                "limit": {"type": "integer", "default": 20},
                "min_quality": {"type": "number", "minimum": 0, "maximum": 1}
            }
        }"#,
    ),
    // Clustering and duplicates
    (
        "memory_clusters",
        "Find clusters of related memories",
        r#"{
            "type": "object",
            "properties": {
                "min_similarity": {"type": "number", "default": 0.7},
                "min_cluster_size": {"type": "integer", "default": 2}
            }
        }"#,
    ),
    (
        "memory_find_duplicates",
        "Find potential duplicate memories",
        r#"{
            "type": "object",
            "properties": {
                "threshold": {"type": "number", "default": 0.9}
            }
        }"#,
    ),
    (
        "memory_merge",
        "Merge duplicate memories",
        r#"{
            "type": "object",
            "properties": {
                "ids": {"type": "array", "items": {"type": "integer"}, "minItems": 2},
                "keep_id": {"type": "integer", "description": "ID to keep (others will be merged into it)"}
            },
            "required": ["ids"]
        }"#,
    ),
    // Project Context Discovery
    (
        "memory_scan_project",
        "Scan current directory for AI instruction files (CLAUDE.md, AGENTS.md, .cursorrules, etc.) and ingest them as memories. Creates parent memory for each file and child memories for sections.",
        r#"{
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "Directory to scan (defaults to current working directory)"},
                "scan_parents": {"type": "boolean", "default": false, "description": "Also scan parent directories (security: disabled by default)"},
                "extract_sections": {"type": "boolean", "default": true, "description": "Create separate memories for each section"}
            }
        }"#,
    ),
    (
        "memory_get_project_context",
        "Get all project context memories for the current working directory. Returns instruction files and their sections.",
        r#"{
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "Project path (defaults to current working directory)"},
                "include_sections": {"type": "boolean", "default": true, "description": "Include section memories"},
                "file_types": {"type": "array", "items": {"type": "string"}, "description": "Filter by file type (claude-md, cursorrules, etc.)"}
            }
        }"#,
    ),
    // Entity Extraction (RML-925)
    (
        "memory_extract_entities",
        "Extract named entities (people, organizations, projects, concepts) from a memory and store them",
        r#"{
            "type": "object",
            "properties": {
                "id": {"type": "integer", "description": "Memory ID to extract entities from"}
            },
            "required": ["id"]
        }"#,
    ),
    (
        "memory_get_entities",
        "Get all entities linked to a memory",
        r#"{
            "type": "object",
            "properties": {
                "id": {"type": "integer", "description": "Memory ID"}
            },
            "required": ["id"]
        }"#,
    ),
    (
        "memory_search_entities",
        "Search for entities by name prefix",
        r#"{
            "type": "object",
            "properties": {
                "query": {"type": "string", "description": "Search query (prefix match)"},
                "entity_type": {"type": "string", "description": "Filter by entity type (person, organization, project, concept, etc.)"},
                "limit": {"type": "integer", "default": 20}
            },
            "required": ["query"]
        }"#,
    ),
    (
        "memory_entity_stats",
        "Get statistics about extracted entities",
        r#"{
            "type": "object",
            "properties": {}
        }"#,
    ),
    // Graph Traversal (RML-926)
    (
        "memory_traverse",
        "Traverse the knowledge graph from a starting memory with full control over traversal options",
        r#"{
            "type": "object",
            "properties": {
                "id": {"type": "integer", "description": "Starting memory ID"},
                "depth": {"type": "integer", "default": 2, "description": "Maximum traversal depth"},
                "direction": {"type": "string", "enum": ["outgoing", "incoming", "both"], "default": "both"},
                "edge_types": {"type": "array", "items": {"type": "string"}, "description": "Filter by edge types (related_to, depends_on, etc.)"},
                "min_score": {"type": "number", "default": 0, "description": "Minimum edge score threshold"},
                "min_confidence": {"type": "number", "default": 0, "description": "Minimum confidence threshold"},
                "limit_per_hop": {"type": "integer", "default": 50, "description": "Max results per hop"},
                "include_entities": {"type": "boolean", "default": true, "description": "Include entity-based connections"}
            },
            "required": ["id"]
        }"#,
    ),
    (
        "memory_find_path",
        "Find the shortest path between two memories in the knowledge graph",
        r#"{
            "type": "object",
            "properties": {
                "from_id": {"type": "integer", "description": "Starting memory ID"},
                "to_id": {"type": "integer", "description": "Target memory ID"},
                "max_depth": {"type": "integer", "default": 5, "description": "Maximum path length to search"}
            },
            "required": ["from_id", "to_id"]
        }"#,
    ),
    // Document Ingestion (RML-928)
    (
        "memory_ingest_document",
        "Ingest a document (PDF or Markdown) into memory. Extracts text, splits into chunks with overlap, and creates memories with deduplication.",
        r#"{
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "Local file path to the document"},
                "format": {"type": "string", "enum": ["auto", "md", "pdf"], "default": "auto", "description": "Document format (auto-detect from extension if not specified)"},
                "chunk_size": {"type": "integer", "default": 1200, "description": "Maximum characters per chunk"},
                "overlap": {"type": "integer", "default": 200, "description": "Overlap between chunks in characters"},
                "max_file_size": {"type": "integer", "default": 10485760, "description": "Maximum file size in bytes (default 10MB)"},
                "tags": {"type": "array", "items": {"type": "string"}, "description": "Additional tags to add to all chunks"}
            },
            "required": ["path"]
        }"#,
    ),
];

/// Get all tool definitions as ToolDefinition structs
pub fn get_tool_definitions() -> Vec<ToolDefinition> {
    TOOL_DEFINITIONS
        .iter()
        .map(|(name, description, schema)| ToolDefinition {
            name: name.to_string(),
            description: description.to_string(),
            input_schema: serde_json::from_str(schema).unwrap_or(json!({})),
        })
        .collect()
}
