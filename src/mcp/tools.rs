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
                "memory_type": {"type": "string", "enum": ["note", "todo", "issue", "decision", "preference", "learning", "context", "credential", "episodic", "procedural", "summary", "checkpoint"], "default": "note", "description": "Memory type (preferred field; alias: type)"},
                "type": {"type": "string", "enum": ["note", "todo", "issue", "decision", "preference", "learning", "context", "credential", "episodic", "procedural", "summary", "checkpoint"], "default": "note", "description": "Deprecated alias for memory_type"},
                "tags": {"type": "array", "items": {"type": "string"}, "description": "Tags for categorization"},
                "metadata": {"type": "object", "description": "Additional metadata as key-value pairs"},
                "importance": {"type": "number", "minimum": 0, "maximum": 1, "description": "Importance score (0-1)"},
                "workspace": {"type": "string", "description": "Workspace to store the memory in (default: 'default')"},
                "tier": {"type": "string", "enum": ["permanent", "daily"], "default": "permanent", "description": "Memory tier: permanent (never expires) or daily (auto-expires)"},
                "defer_embedding": {"type": "boolean", "default": false, "description": "Defer embedding to background queue"},
                "ttl_seconds": {"type": "integer", "description": "Time-to-live in seconds. Memory will auto-expire after this duration. Omit for permanent storage. Setting this implies tier='daily'."},
                "dedup_mode": {"type": "string", "enum": ["reject", "merge", "skip", "allow"], "default": "allow", "description": "How to handle duplicate content: reject (error if exact match), merge (combine tags/metadata with existing), skip (return existing unchanged), allow (create duplicate)"},
                "dedup_threshold": {"type": "number", "minimum": 0, "maximum": 1, "description": "Similarity threshold for semantic deduplication (0.0-1.0). When set with dedup_mode != 'allow', memories with cosine similarity >= threshold are treated as duplicates. Requires embeddings. If not set, only exact content hash matching is used."},
                "event_time": {"type": "string", "format": "date-time", "description": "ISO8601 timestamp for episodic memories (when the event occurred)"},
                "event_duration_seconds": {"type": "integer", "description": "Duration of the event in seconds (for episodic memories)"},
                "trigger_pattern": {"type": "string", "description": "Pattern that triggers this procedure (for procedural memories)"},
                "summary_of_id": {"type": "integer", "description": "ID of the memory this summarizes (for summary memories)"}
            },
            "required": ["content"]
        }"#,
    ),
    (
        "context_seed",
        "Injects initial context (premises, persona assumptions, or structured facts) about an entity to avoid cold start. Seeded memories are tagged as origin:seed and status:unverified, and should be treated as revisable assumptions.",
        r#"{
            "type": "object",
            "properties": {
                "entity_context": {
                    "type": "string",
                    "maxLength": 200,
                    "description": "Name or ID of the entity (e.g., 'Client: Roberto', 'Account: ACME', 'Project: Alpha')"
                },
                "workspace": {"type": "string", "description": "Workspace to store the memories in (default: 'default')"},
                "base_tags": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Tags applied to all facts (e.g., ['vip', 'prospect'])"
                },
                "ttl_seconds": {
                    "type": "integer",
                    "description": "Override TTL for all facts in seconds (0 = disable TTL). If omitted, TTL is derived from confidence."
                },
                "disable_ttl": {
                    "type": "boolean",
                    "default": false,
                    "description": "Disable TTL and keep seeded memories permanent regardless of confidence."
                },
                "facts": {
                    "type": "array",
                    "minItems": 1,
                    "items": {
                        "type": "object",
                        "properties": {
                            "content": {"type": "string", "minLength": 1},
                            "category": {
                                "type": "string",
                                "enum": ["fact", "behavior_instruction", "interest", "persona", "preference"],
                                "description": "Structured category for filtering and ranking"
                            },
                            "confidence": {
                                "type": "number",
                                "minimum": 0.0,
                                "maximum": 1.0,
                                "description": "0.0 to 1.0 (defaults to 0.7 for seeds). TTL derived by confidence if ttl_seconds not provided."
                            }
                        },
                        "required": ["content"]
                    }
                }
            },
            "required": ["facts"]
        }"#,
    ),
    (
        "memory_seed",
        "Deprecated alias for context_seed. Use context_seed instead.",
        r#"{
            "type": "object",
            "properties": {
                "entity_context": {
                    "type": "string",
                    "maxLength": 200,
                    "description": "Name or ID of the entity (e.g., 'Client: Roberto', 'Account: ACME', 'Project: Alpha')"
                },
                "workspace": {"type": "string", "description": "Workspace to store the memories in (default: 'default')"},
                "base_tags": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Tags applied to all facts (e.g., ['vip', 'prospect'])"
                },
                "ttl_seconds": {
                    "type": "integer",
                    "description": "Override TTL for all facts in seconds (0 = disable TTL). If omitted, TTL is derived from confidence."
                },
                "disable_ttl": {
                    "type": "boolean",
                    "default": false,
                    "description": "Disable TTL and keep seeded memories permanent regardless of confidence."
                },
                "facts": {
                    "type": "array",
                    "minItems": 1,
                    "items": {
                        "type": "object",
                        "properties": {
                            "content": {"type": "string", "minLength": 1},
                            "category": {
                                "type": "string",
                                "enum": ["fact", "behavior_instruction", "interest", "persona", "preference"],
                                "description": "Structured category for filtering and ranking"
                            },
                            "confidence": {
                                "type": "number",
                                "minimum": 0.0,
                                "maximum": 1.0,
                                "description": "0.0 to 1.0 (defaults to 0.7 for seeds). TTL derived by confidence if ttl_seconds not provided."
                            }
                        },
                        "required": ["content"]
                    }
                }
            },
            "required": ["facts"]
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
                "memory_type": {"type": "string", "enum": ["note", "todo", "issue", "decision", "preference", "learning", "context", "credential", "episodic", "procedural", "summary", "checkpoint"], "description": "Memory type (preferred field; alias: type)"},
                "type": {"type": "string", "enum": ["note", "todo", "issue", "decision", "preference", "learning", "context", "credential", "episodic", "procedural", "summary", "checkpoint"], "description": "Deprecated alias for memory_type"},
                "tags": {"type": "array", "items": {"type": "string"}},
                "metadata": {"type": "object"},
                "importance": {"type": "number", "minimum": 0, "maximum": 1},
                "ttl_seconds": {"type": "integer", "description": "Time-to-live in seconds (0 = remove expiration, positive = set new expiration)"},
                "event_time": {"type": ["string", "null"], "format": "date-time", "description": "ISO8601 timestamp for episodic memories (null to clear)"},
                "trigger_pattern": {"type": ["string", "null"], "description": "Pattern that triggers this procedure (null to clear)"}
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
        "List memories with filtering and pagination. Supports workspace isolation, tier filtering, and advanced filter syntax with AND/OR and comparison operators.",
        r#"{
            "type": "object",
            "properties": {
                "limit": {"type": "integer", "default": 20},
                "offset": {"type": "integer", "default": 0},
                "tags": {"type": "array", "items": {"type": "string"}},
                "memory_type": {"type": "string", "description": "Filter by memory type (preferred field; alias: type)"},
                "type": {"type": "string", "description": "Deprecated alias for memory_type"},
                "workspace": {"type": "string", "description": "Filter by single workspace"},
                "workspaces": {"type": "array", "items": {"type": "string"}, "description": "Filter by multiple workspaces"},
                "tier": {"type": "string", "enum": ["permanent", "daily"], "description": "Filter by memory tier"},
                "sort_by": {"type": "string", "enum": ["created_at", "updated_at", "last_accessed_at", "importance", "access_count"]},
                "sort_order": {"type": "string", "enum": ["asc", "desc"], "default": "desc"},
                "filter": {
                    "type": "object",
                    "description": "Advanced filter with AND/OR logic and comparison operators. Supports workspace, tier, and metadata fields. Example: {\"AND\": [{\"metadata.project\": {\"eq\": \"engram\"}}, {\"importance\": {\"gte\": 0.5}}]}. Supported operators: eq, neq, gt, gte, lt, lte, contains, not_contains, exists. Fields: content, memory_type, importance, tags, workspace, tier, created_at, updated_at, metadata.*"
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
        "Search memories using hybrid search (keyword + semantic). Automatically selects optimal strategy with optional reranking. Supports workspace isolation, tier filtering, and advanced filters.",
        r#"{
            "type": "object",
            "properties": {
                "query": {"type": "string", "description": "Search query"},
                "limit": {"type": "integer", "default": 10},
                "min_score": {"type": "number", "default": 0.1},
                "tags": {"type": "array", "items": {"type": "string"}},
                "memory_type": {"type": "string", "description": "Filter by memory type (preferred field; alias: type)"},
                "type": {"type": "string", "description": "Deprecated alias for memory_type"},
                "workspace": {"type": "string", "description": "Filter by single workspace"},
                "workspaces": {"type": "array", "items": {"type": "string"}, "description": "Filter by multiple workspaces"},
                "tier": {"type": "string", "enum": ["permanent", "daily"], "description": "Filter by memory tier"},
                "include_transcripts": {"type": "boolean", "default": false, "description": "Include transcript chunk memories (excluded by default)"},
                "strategy": {"type": "string", "enum": ["auto", "keyword", "keyword_only", "semantic", "semantic_only", "hybrid"], "description": "Force specific strategy (auto selects based on query; keyword/semantic are aliases for keyword_only/semantic_only)"},
                "explain": {"type": "boolean", "default": false, "description": "Include match explanations"},
                "rerank": {"type": "boolean", "default": true, "description": "Apply reranking to improve result quality"},
                "rerank_strategy": {"type": "string", "enum": ["none", "heuristic", "multi_signal"], "default": "heuristic", "description": "Reranking strategy to use"},
                "filter": {
                    "type": "object",
                    "description": "Advanced filter with AND/OR logic. Supports workspace, tier, and metadata fields. Example: {\"AND\": [{\"workspace\": {\"eq\": \"my-project\"}}, {\"importance\": {\"gte\": 0.5}}]}"
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
    (
        "memory_list_instruction_files",
        "List AI instruction files (CLAUDE.md, AGENTS.md, .cursorrules, etc.) in a directory without ingesting them. Returns file paths, types, and sizes for discovery purposes.",
        r#"{
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "Directory to scan (defaults to current working directory)"},
                "scan_parents": {"type": "boolean", "default": false, "description": "Also scan parent directories for instruction files"}
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
    // Workspace Management
    (
        "workspace_list",
        "List all workspaces with their statistics (memory count, tier breakdown, etc.)",
        r#"{
            "type": "object",
            "properties": {}
        }"#,
    ),
    (
        "workspace_stats",
        "Get detailed statistics for a specific workspace",
        r#"{
            "type": "object",
            "properties": {
                "workspace": {"type": "string", "description": "Workspace name"}
            },
            "required": ["workspace"]
        }"#,
    ),
    (
        "workspace_move",
        "Move a memory to a different workspace",
        r#"{
            "type": "object",
            "properties": {
                "id": {"type": "integer", "description": "Memory ID to move"},
                "workspace": {"type": "string", "description": "Target workspace name"}
            },
            "required": ["id", "workspace"]
        }"#,
    ),
    (
        "workspace_delete",
        "Delete a workspace. Can either move all memories to 'default' workspace or hard delete them.",
        r#"{
            "type": "object",
            "properties": {
                "workspace": {"type": "string", "description": "Workspace to delete"},
                "move_to_default": {"type": "boolean", "default": true, "description": "If true, moves memories to 'default' workspace. If false, deletes all memories in the workspace."}
            },
            "required": ["workspace"]
        }"#,
    ),
    // Memory Tiering
    (
        "memory_create_daily",
        "Create a daily (ephemeral) memory that auto-expires after the specified TTL. Useful for session context and scratch notes.",
        r#"{
            "type": "object",
            "properties": {
                "content": {"type": "string", "description": "The content to remember"},
                "type": {"type": "string", "enum": ["note", "todo", "issue", "decision", "preference", "learning", "context", "credential"], "default": "note"},
                "tags": {"type": "array", "items": {"type": "string"}, "description": "Tags for categorization"},
                "metadata": {"type": "object", "description": "Additional metadata as key-value pairs"},
                "importance": {"type": "number", "minimum": 0, "maximum": 1, "description": "Importance score (0-1)"},
                "ttl_seconds": {"type": "integer", "default": 86400, "description": "Time-to-live in seconds (default: 24 hours)"},
                "workspace": {"type": "string", "description": "Workspace to store the memory in (default: 'default')"}
            },
            "required": ["content"]
        }"#,
    ),
    (
        "memory_promote_to_permanent",
        "Promote a daily memory to permanent tier. Clears the expiration and makes the memory permanent.",
        r#"{
            "type": "object",
            "properties": {
                "id": {"type": "integer", "description": "Memory ID to promote"}
            },
            "required": ["id"]
        }"#,
    ),
    // Embedding Cache
    (
        "embedding_cache_stats",
        "Get statistics about the embedding cache (hits, misses, entries, bytes used, hit rate)",
        r#"{
            "type": "object",
            "properties": {}
        }"#,
    ),
    (
        "embedding_cache_clear",
        "Clear all entries from the embedding cache",
        r#"{
            "type": "object",
            "properties": {}
        }"#,
    ),
    // Session Transcript Indexing
    (
        "session_index",
        "Index a conversation into searchable memory chunks. Uses dual-limiter chunking (messages + characters) with overlap.",
        r#"{
            "type": "object",
            "properties": {
                "session_id": {"type": "string", "description": "Unique session identifier"},
                "messages": {
                    "type": "array",
                    "description": "Array of conversation messages",
                    "items": {
                        "type": "object",
                        "properties": {
                            "role": {"type": "string", "description": "Message role (user, assistant, system)"},
                            "content": {"type": "string", "description": "Message content"},
                            "timestamp": {"type": "string", "description": "ISO 8601 timestamp"},
                            "id": {"type": "string", "description": "Optional message ID"}
                        },
                        "required": ["role", "content"]
                    }
                },
                "title": {"type": "string", "description": "Optional session title"},
                "workspace": {"type": "string", "description": "Workspace to store chunks in (default: 'default')"},
                "agent_id": {"type": "string", "description": "Optional agent identifier"},
                "max_messages": {"type": "integer", "default": 10, "description": "Max messages per chunk"},
                "max_chars": {"type": "integer", "default": 8000, "description": "Max characters per chunk"},
                "overlap": {"type": "integer", "default": 2, "description": "Overlap messages between chunks"},
                "ttl_days": {"type": "integer", "default": 7, "description": "TTL for transcript chunks in days"}
            },
            "required": ["session_id", "messages"]
        }"#,
    ),
    (
        "session_index_delta",
        "Incrementally index new messages to an existing session. More efficient than full reindex.",
        r#"{
            "type": "object",
            "properties": {
                "session_id": {"type": "string", "description": "Session to update"},
                "messages": {
                    "type": "array",
                    "description": "New messages to add",
                    "items": {
                        "type": "object",
                        "properties": {
                            "role": {"type": "string"},
                            "content": {"type": "string"},
                            "timestamp": {"type": "string"},
                            "id": {"type": "string"}
                        },
                        "required": ["role", "content"]
                    }
                }
            },
            "required": ["session_id", "messages"]
        }"#,
    ),
    (
        "session_get",
        "Get information about an indexed session",
        r#"{
            "type": "object",
            "properties": {
                "session_id": {"type": "string", "description": "Session ID to retrieve"}
            },
            "required": ["session_id"]
        }"#,
    ),
    (
        "session_list",
        "List indexed sessions with optional workspace filter",
        r#"{
            "type": "object",
            "properties": {
                "workspace": {"type": "string", "description": "Filter by workspace"},
                "limit": {"type": "integer", "default": 20, "description": "Maximum sessions to return"}
            }
        }"#,
    ),
    (
        "session_delete",
        "Delete a session and all its indexed chunks",
        r#"{
            "type": "object",
            "properties": {
                "session_id": {"type": "string", "description": "Session to delete"}
            },
            "required": ["session_id"]
        }"#,
    ),
    // Identity Management
    (
        "identity_create",
        "Create a new identity with canonical ID, display name, and optional aliases",
        r#"{
            "type": "object",
            "properties": {
                "canonical_id": {"type": "string", "description": "Unique canonical identifier (e.g., 'user:ronaldo', 'org:acme')"},
                "display_name": {"type": "string", "description": "Human-readable display name"},
                "entity_type": {"type": "string", "enum": ["person", "organization", "project", "tool", "concept", "other"], "default": "person"},
                "description": {"type": "string", "description": "Optional description"},
                "aliases": {"type": "array", "items": {"type": "string"}, "description": "Initial aliases for this identity"},
                "metadata": {"type": "object", "description": "Additional metadata"}
            },
            "required": ["canonical_id", "display_name"]
        }"#,
    ),
    (
        "identity_get",
        "Get an identity by its canonical ID",
        r#"{
            "type": "object",
            "properties": {
                "canonical_id": {"type": "string", "description": "Canonical identifier"}
            },
            "required": ["canonical_id"]
        }"#,
    ),
    (
        "identity_update",
        "Update an identity's display name, description, or type",
        r#"{
            "type": "object",
            "properties": {
                "canonical_id": {"type": "string", "description": "Canonical identifier"},
                "display_name": {"type": "string", "description": "New display name"},
                "description": {"type": "string", "description": "New description"},
                "entity_type": {"type": "string", "enum": ["person", "organization", "project", "tool", "concept", "other"]}
            },
            "required": ["canonical_id"]
        }"#,
    ),
    (
        "identity_delete",
        "Delete an identity and all its aliases",
        r#"{
            "type": "object",
            "properties": {
                "canonical_id": {"type": "string", "description": "Canonical identifier to delete"}
            },
            "required": ["canonical_id"]
        }"#,
    ),
    (
        "identity_add_alias",
        "Add an alias to an identity. Aliases are normalized (lowercase, trimmed). Conflicts with existing aliases are rejected.",
        r#"{
            "type": "object",
            "properties": {
                "canonical_id": {"type": "string", "description": "Canonical identifier"},
                "alias": {"type": "string", "description": "Alias to add"},
                "source": {"type": "string", "description": "Optional source of the alias (e.g., 'manual', 'extracted')"}
            },
            "required": ["canonical_id", "alias"]
        }"#,
    ),
    (
        "identity_remove_alias",
        "Remove an alias from any identity",
        r#"{
            "type": "object",
            "properties": {
                "alias": {"type": "string", "description": "Alias to remove"}
            },
            "required": ["alias"]
        }"#,
    ),
    (
        "identity_resolve",
        "Resolve an alias to its canonical identity. Returns the identity if found, null otherwise.",
        r#"{
            "type": "object",
            "properties": {
                "alias": {"type": "string", "description": "Alias to resolve"}
            },
            "required": ["alias"]
        }"#,
    ),
    (
        "identity_list",
        "List all identities with optional type filter",
        r#"{
            "type": "object",
            "properties": {
                "entity_type": {"type": "string", "enum": ["person", "organization", "project", "tool", "concept", "other"]},
                "limit": {"type": "integer", "default": 50}
            }
        }"#,
    ),
    (
        "identity_search",
        "Search identities by alias or display name",
        r#"{
            "type": "object",
            "properties": {
                "query": {"type": "string", "description": "Search query"},
                "limit": {"type": "integer", "default": 20}
            },
            "required": ["query"]
        }"#,
    ),
    (
        "identity_link",
        "Link an identity to a memory (mark that the identity is mentioned in the memory)",
        r#"{
            "type": "object",
            "properties": {
                "memory_id": {"type": "integer", "description": "Memory ID"},
                "canonical_id": {"type": "string", "description": "Identity canonical ID"},
                "mention_text": {"type": "string", "description": "The text that mentions this identity"}
            },
            "required": ["memory_id", "canonical_id"]
        }"#,
    ),
    (
        "identity_unlink",
        "Remove the link between an identity and a memory",
        r#"{
            "type": "object",
            "properties": {
                "memory_id": {"type": "integer", "description": "Memory ID"},
                "canonical_id": {"type": "string", "description": "Identity canonical ID"}
            },
            "required": ["memory_id", "canonical_id"]
        }"#,
    ),
    (
        "memory_get_identities",
        "Get all identities (persons, organizations, projects, etc.) linked to a memory. Returns identity details including display name, type, aliases, and mention information.",
        r#"{
            "type": "object",
            "properties": {
                "id": {"type": "integer", "description": "Memory ID"}
            },
            "required": ["id"]
        }"#,
    ),
    // Content Utilities
    (
        "memory_soft_trim",
        "Intelligently trim memory content while preserving context. Keeps the beginning (head) and end (tail) of content with an ellipsis in the middle. Useful for displaying long content in limited space while keeping important context from both ends.",
        r#"{
            "type": "object",
            "properties": {
                "id": {"type": "integer", "description": "Memory ID to trim"},
                "max_chars": {"type": "integer", "default": 500, "description": "Maximum characters for trimmed output"},
                "head_percent": {"type": "integer", "default": 60, "description": "Percentage of space for the head (0-100)"},
                "tail_percent": {"type": "integer", "default": 30, "description": "Percentage of space for the tail (0-100)"},
                "ellipsis": {"type": "string", "default": "\n...\n", "description": "Text to insert between head and tail"},
                "preserve_words": {"type": "boolean", "default": true, "description": "Avoid breaking in the middle of words"}
            },
            "required": ["id"]
        }"#,
    ),
    (
        "memory_list_compact",
        "List memories with compact preview instead of full content. More efficient for browsing/listing UIs. Returns only essential fields and a truncated content preview with metadata about original content length.",
        r#"{
            "type": "object",
            "properties": {
                "limit": {"type": "integer", "default": 20, "description": "Maximum memories to return"},
                "offset": {"type": "integer", "default": 0, "description": "Pagination offset"},
                "tags": {"type": "array", "items": {"type": "string"}, "description": "Filter by tags"},
                "memory_type": {"type": "string", "description": "Filter by memory type (preferred field; alias: type)"},
                "type": {"type": "string", "description": "Deprecated alias for memory_type"},
                "workspace": {"type": "string", "description": "Filter by workspace"},
                "tier": {"type": "string", "enum": ["permanent", "daily"], "description": "Filter by tier"},
                "sort_by": {"type": "string", "enum": ["created_at", "updated_at", "last_accessed_at", "importance", "access_count"], "default": "created_at"},
                "sort_order": {"type": "string", "enum": ["asc", "desc"], "default": "desc"},
                "preview_chars": {"type": "integer", "default": 100, "description": "Maximum characters for content preview"}
            }
        }"#,
    ),
    (
        "memory_content_stats",
        "Get content statistics for a memory (character count, word count, line count, sentence count, paragraph count)",
        r#"{
            "type": "object",
            "properties": {
                "id": {"type": "integer", "description": "Memory ID"}
            },
            "required": ["id"]
        }"#,
    ),
    // Batch Operations
    (
        "memory_create_batch",
        "Create multiple memories in a single operation. More efficient than individual creates for bulk imports.",
        r#"{
            "type": "object",
            "properties": {
                "memories": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "content": {"type": "string"},
                            "type": {"type": "string", "enum": ["note", "todo", "issue", "decision", "preference", "learning", "context", "credential"]},
                            "tags": {"type": "array", "items": {"type": "string"}},
                            "metadata": {"type": "object"},
                            "importance": {"type": "number", "minimum": 0, "maximum": 1},
                            "workspace": {"type": "string"}
                        },
                        "required": ["content"]
                    },
                    "description": "Array of memories to create"
                }
            },
            "required": ["memories"]
        }"#,
    ),
    (
        "memory_delete_batch",
        "Delete multiple memories in a single operation.",
        r#"{
            "type": "object",
            "properties": {
                "ids": {
                    "type": "array",
                    "items": {"type": "integer"},
                    "description": "Array of memory IDs to delete"
                }
            },
            "required": ["ids"]
        }"#,
    ),
    // Tag Utilities
    (
        "memory_tags",
        "List all tags with usage counts and most recent usage timestamps.",
        r#"{
            "type": "object",
            "properties": {}
        }"#,
    ),
    (
        "memory_tag_hierarchy",
        "Get tags organized in a hierarchical tree structure. Tags with slashes are treated as paths (e.g., 'project/engram/core').",
        r#"{
            "type": "object",
            "properties": {}
        }"#,
    ),
    (
        "memory_validate_tags",
        "Validate tag consistency across memories. Reports orphaned tags, unused tags, and suggested normalizations.",
        r#"{
            "type": "object",
            "properties": {}
        }"#,
    ),
    // Import/Export
    (
        "memory_export",
        "Export all memories to a JSON-serializable format for backup or migration.",
        r#"{
            "type": "object",
            "properties": {
                "workspace": {"type": "string", "description": "Optional: export only from specific workspace"},
                "include_embeddings": {"type": "boolean", "default": false, "description": "Include embedding vectors in export (larger file size)"}
            }
        }"#,
    ),
    (
        "memory_import",
        "Import memories from a previously exported JSON format.",
        r#"{
            "type": "object",
            "properties": {
                "data": {"type": "object", "description": "The exported data object"},
                "skip_duplicates": {"type": "boolean", "default": true, "description": "Skip memories with matching content hash"}
            },
            "required": ["data"]
        }"#,
    ),
    // Maintenance
    (
        "memory_rebuild_embeddings",
        "Rebuild embeddings for all memories that are missing them. Useful after model changes or data recovery.",
        r#"{
            "type": "object",
            "properties": {}
        }"#,
    ),
    (
        "memory_rebuild_crossrefs",
        "Rebuild cross-reference links between memories. Re-analyzes all memories to find and create links.",
        r#"{
            "type": "object",
            "properties": {}
        }"#,
    ),
    // Special Memory Types
    (
        "memory_create_section",
        "Create a section memory for organizing content hierarchically. Sections can have parent sections for nested organization.",
        r#"{
            "type": "object",
            "properties": {
                "title": {"type": "string", "description": "Section title"},
                "content": {"type": "string", "description": "Section description or content"},
                "parent_id": {"type": "integer", "description": "Optional parent section ID for nesting"},
                "level": {"type": "integer", "default": 1, "description": "Heading level (1-6)"},
                "workspace": {"type": "string", "description": "Workspace for the section"}
            },
            "required": ["title"]
        }"#,
    ),
    (
        "memory_checkpoint",
        "Create a checkpoint memory marking a significant point in a session. Useful for session resumption and context restoration.",
        r#"{
            "type": "object",
            "properties": {
                "session_id": {"type": "string", "description": "Session identifier"},
                "summary": {"type": "string", "description": "Summary of session state at checkpoint"},
                "context": {"type": "object", "description": "Additional context data to preserve"},
                "workspace": {"type": "string", "description": "Workspace for the checkpoint"}
            },
            "required": ["session_id", "summary"]
        }"#,
    ),
    // Phase 1: Cognitive Memory Types (ENG-33)
    (
        "memory_create_episodic",
        "Create an episodic memory representing an event with temporal context. Use for tracking when things happened and their duration.",
        r#"{
            "type": "object",
            "properties": {
                "content": {"type": "string", "description": "Description of the event"},
                "event_time": {"type": "string", "format": "date-time", "description": "ISO8601 timestamp when the event occurred"},
                "event_duration_seconds": {"type": "integer", "description": "Duration of the event in seconds"},
                "tags": {"type": "array", "items": {"type": "string"}, "description": "Tags for categorization"},
                "metadata": {"type": "object", "description": "Additional metadata"},
                "importance": {"type": "number", "minimum": 0, "maximum": 1, "description": "Importance score (0-1)"},
                "workspace": {"type": "string", "description": "Workspace (default: 'default')"}
            },
            "required": ["content", "event_time"]
        }"#,
    ),
    (
        "memory_create_procedural",
        "Create a procedural memory representing a learned pattern or workflow. Tracks success/failure to measure effectiveness.",
        r#"{
            "type": "object",
            "properties": {
                "content": {"type": "string", "description": "Description of the procedure/workflow"},
                "trigger_pattern": {"type": "string", "description": "Pattern that triggers this procedure (e.g., 'When user asks about auth')"},
                "tags": {"type": "array", "items": {"type": "string"}, "description": "Tags for categorization"},
                "metadata": {"type": "object", "description": "Additional metadata"},
                "importance": {"type": "number", "minimum": 0, "maximum": 1, "description": "Importance score (0-1)"},
                "workspace": {"type": "string", "description": "Workspace (default: 'default')"}
            },
            "required": ["content", "trigger_pattern"]
        }"#,
    ),
    (
        "memory_get_timeline",
        "Query episodic memories by time range. Returns events ordered by event_time.",
        r#"{
            "type": "object",
            "properties": {
                "start_time": {"type": "string", "format": "date-time", "description": "Start of time range (ISO8601)"},
                "end_time": {"type": "string", "format": "date-time", "description": "End of time range (ISO8601)"},
                "workspace": {"type": "string", "description": "Filter by workspace"},
                "tags": {"type": "array", "items": {"type": "string"}, "description": "Filter by tags"},
                "limit": {"type": "integer", "default": 50, "description": "Maximum results to return"}
            }
        }"#,
    ),
    (
        "memory_get_procedures",
        "List procedural memories (learned patterns/workflows). Optionally filter by trigger pattern.",
        r#"{
            "type": "object",
            "properties": {
                "trigger_pattern": {"type": "string", "description": "Filter by trigger pattern (partial match)"},
                "workspace": {"type": "string", "description": "Filter by workspace"},
                "min_success_rate": {"type": "number", "minimum": 0, "maximum": 1, "description": "Minimum success rate (successes / (successes + failures))"},
                "limit": {"type": "integer", "default": 50, "description": "Maximum results to return"}
            }
        }"#,
    ),
    (
        "memory_boost",
        "Temporarily boost a memory's importance score. The boost can optionally decay over time.",
        r#"{
            "type": "object",
            "properties": {
                "id": {"type": "integer", "description": "Memory ID to boost"},
                "boost_amount": {"type": "number", "default": 0.2, "description": "Amount to increase importance (0-1)"},
                "duration_seconds": {"type": "integer", "description": "Optional: duration before boost decays (omit for permanent boost)"}
            },
            "required": ["id"]
        }"#,
    ),
    // Phase 2: Context Compression Engine
    (
        "memory_summarize",
        "Create a summary of one or more memories. Returns a new Summary-type memory with summary_of_id set.",
        r#"{
            "type": "object",
            "properties": {
                "memory_ids": {
                    "type": "array",
                    "items": {"type": "integer"},
                    "description": "IDs of memories to summarize"
                },
                "summary": {"type": "string", "description": "The summary text (provide this or let the system generate one)"},
                "max_length": {"type": "integer", "default": 500, "description": "Maximum length for auto-generated summary"},
                "workspace": {"type": "string", "description": "Workspace for the summary memory"},
                "tags": {"type": "array", "items": {"type": "string"}, "description": "Tags for the summary memory"}
            },
            "required": ["memory_ids"]
        }"#,
    ),
    (
        "memory_get_full",
        "Get the full/original content of a memory. If the memory is a Summary, returns the original content from summary_of_id.",
        r#"{
            "type": "object",
            "properties": {
                "id": {"type": "integer", "description": "Memory ID to get full content for"}
            },
            "required": ["id"]
        }"#,
    ),
    (
        "context_budget_check",
        "Check token usage of memories against a budget. Returns token counts and suggestions if over budget.",
        r#"{
            "type": "object",
            "properties": {
                "memory_ids": {
                    "type": "array",
                    "items": {"type": "integer"},
                    "description": "IDs of memories to check"
                },
                "model": {
                    "type": "string",
                    "description": "Model name for tokenization (gpt-4, gpt-4o, gpt-4o-mini, claude-3-opus, etc.)"
                },
                "encoding": {
                    "type": "string",
                    "description": "Override encoding (cl100k_base, o200k_base). Optional if model is known."
                },
                "budget": {"type": "integer", "description": "Token budget to check against"}
            },
            "required": ["memory_ids", "model", "budget"]
        }"#,
    ),
    (
        "memory_archive_old",
        "Archive old, low-importance memories by creating summaries. Moves originals to archived state.",
        r#"{
            "type": "object",
            "properties": {
                "max_age_days": {"type": "integer", "default": 90, "description": "Archive memories older than this many days"},
                "max_importance": {"type": "number", "default": 0.5, "description": "Only archive memories with importance below this"},
                "min_access_count": {"type": "integer", "default": 5, "description": "Skip memories accessed more than this many times"},
                "workspace": {"type": "string", "description": "Limit to specific workspace"},
                "dry_run": {"type": "boolean", "default": true, "description": "If true, only report what would be archived"}
            }
        }"#,
    ),
    // Phase 3: Langfuse Integration (ENG-35) - feature-gated
    #[cfg(feature = "langfuse")]
    (
        "langfuse_connect",
        "Configure Langfuse connection for observability integration. Stores config in metadata.",
        r#"{
            "type": "object",
            "properties": {
                "public_key": {"type": "string", "description": "Langfuse public key (or use LANGFUSE_PUBLIC_KEY env var)"},
                "secret_key": {"type": "string", "description": "Langfuse secret key (or use LANGFUSE_SECRET_KEY env var)"},
                "base_url": {"type": "string", "default": "https://cloud.langfuse.com", "description": "Langfuse API base URL"}
            }
        }"#,
    ),
    #[cfg(feature = "langfuse")]
    (
        "langfuse_sync",
        "Start background sync from Langfuse traces to memories. Returns task_id for status checking.",
        r#"{
            "type": "object",
            "properties": {
                "since": {"type": "string", "format": "date-time", "description": "Sync traces since this timestamp (default: 24h ago)"},
                "limit": {"type": "integer", "default": 100, "description": "Maximum traces to sync"},
                "workspace": {"type": "string", "description": "Workspace to create memories in"},
                "dry_run": {"type": "boolean", "default": false, "description": "Preview what would be synced without creating memories"}
            }
        }"#,
    ),
    #[cfg(feature = "langfuse")]
    (
        "langfuse_sync_status",
        "Check the status of a Langfuse sync task.",
        r#"{
            "type": "object",
            "properties": {
                "task_id": {"type": "string", "description": "Task ID returned from langfuse_sync"}
            },
            "required": ["task_id"]
        }"#,
    ),
    #[cfg(feature = "langfuse")]
    (
        "langfuse_extract_patterns",
        "Extract patterns from Langfuse traces without saving. Preview mode for pattern discovery.",
        r#"{
            "type": "object",
            "properties": {
                "since": {"type": "string", "format": "date-time", "description": "Analyze traces since this timestamp"},
                "limit": {"type": "integer", "default": 50, "description": "Maximum traces to analyze"},
                "min_confidence": {"type": "number", "default": 0.7, "description": "Minimum confidence for patterns"}
            }
        }"#,
    ),
    #[cfg(feature = "langfuse")]
    (
        "memory_from_trace",
        "Create a memory from a specific Langfuse trace ID.",
        r#"{
            "type": "object",
            "properties": {
                "trace_id": {"type": "string", "description": "Langfuse trace ID"},
                "memory_type": {"type": "string", "enum": ["note", "episodic", "procedural", "learning"], "default": "episodic", "description": "Type of memory to create"},
                "workspace": {"type": "string", "description": "Workspace for the memory"},
                "tags": {"type": "array", "items": {"type": "string"}, "description": "Additional tags"}
            },
            "required": ["trace_id"]
        }"#,
    ),
    // Phase 4: Search Result Caching (ENG-36)
    (
        "search_cache_feedback",
        "Report feedback on search results quality. Helps tune the adaptive cache threshold.",
        r#"{
            "type": "object",
            "properties": {
                "query": {"type": "string", "description": "The search query"},
                "positive": {"type": "boolean", "description": "True if results were helpful, false otherwise"},
                "workspace": {"type": "string", "description": "Workspace filter used (if any)"}
            },
            "required": ["query", "positive"]
        }"#,
    ),
    (
        "search_cache_stats",
        "Get search result cache statistics including hit rate, entry count, and current threshold.",
        r#"{
            "type": "object",
            "properties": {}
        }"#,
    ),
    (
        "search_cache_clear",
        "Clear the search result cache. Useful after bulk operations.",
        r#"{
            "type": "object",
            "properties": {
                "workspace": {"type": "string", "description": "Only clear cache for this workspace (optional)"}
            }
        }"#,
    ),
    // Phase 5: Memory Lifecycle Management (ENG-37)
    (
        "lifecycle_status",
        "Get lifecycle statistics (active/stale/archived counts by workspace).",
        r#"{
            "type": "object",
            "properties": {
                "workspace": {"type": "string", "description": "Filter by workspace (optional)"}
            }
        }"#,
    ),
    (
        "lifecycle_run",
        "Manually trigger a lifecycle cycle (mark stale, archive old). Dry run by default.",
        r#"{
            "type": "object",
            "properties": {
                "dry_run": {"type": "boolean", "default": true, "description": "Preview changes without applying"},
                "workspace": {"type": "string", "description": "Limit to specific workspace"},
                "stale_days": {"type": "integer", "default": 30, "description": "Mark memories older than this as stale"},
                "archive_days": {"type": "integer", "default": 90, "description": "Archive memories older than this"},
                "min_importance": {"type": "number", "default": 0.5, "description": "Only process memories below this importance"}
            }
        }"#,
    ),
    (
        "memory_set_lifecycle",
        "Manually set the lifecycle state of a memory.",
        r#"{
            "type": "object",
            "properties": {
                "id": {"type": "integer", "description": "Memory ID"},
                "state": {"type": "string", "enum": ["active", "stale", "archived"], "description": "New lifecycle state"}
            },
            "required": ["id", "state"]
        }"#,
    ),
    (
        "lifecycle_config",
        "Get or set lifecycle configuration (intervals, thresholds).",
        r#"{
            "type": "object",
            "properties": {
                "stale_days": {"type": "integer", "description": "Days before marking as stale"},
                "archive_days": {"type": "integer", "description": "Days before auto-archiving"},
                "min_importance": {"type": "number", "description": "Importance threshold for lifecycle"},
                "min_access_count": {"type": "integer", "description": "Access count threshold"}
            }
        }"#,
    ),
    // Event System
    (
        "memory_events_poll",
        "Poll for memory events (create, update, delete, etc.) since a given point. Useful for syncing and monitoring.",
        r#"{
            "type": "object",
            "properties": {
                "since_id": {"type": "integer", "description": "Return events after this event ID"},
                "since_time": {"type": "string", "format": "date-time", "description": "Return events after this timestamp (RFC3339)"},
                "agent_id": {"type": "string", "description": "Filter events for specific agent"},
                "limit": {"type": "integer", "default": 100, "description": "Maximum events to return"}
            }
        }"#,
    ),
    (
        "memory_events_clear",
        "Clear old events from the event log. Helps manage storage for long-running systems.",
        r#"{
            "type": "object",
            "properties": {
                "before_id": {"type": "integer", "description": "Delete events before this ID"},
                "before_time": {"type": "string", "format": "date-time", "description": "Delete events before this timestamp"},
                "keep_recent": {"type": "integer", "description": "Keep only the N most recent events"}
            }
        }"#,
    ),
    // Advanced Sync
    (
        "sync_version",
        "Get the current sync version and metadata. Used to check if local data is up-to-date.",
        r#"{
            "type": "object",
            "properties": {}
        }"#,
    ),
    (
        "sync_delta",
        "Get changes (delta) since a specific version. Returns created, updated, and deleted memories.",
        r#"{
            "type": "object",
            "properties": {
                "since_version": {"type": "integer", "description": "Version to get changes from"}
            },
            "required": ["since_version"]
        }"#,
    ),
    (
        "sync_state",
        "Get or update sync state for a specific agent. Tracks what each agent has synced.",
        r#"{
            "type": "object",
            "properties": {
                "agent_id": {"type": "string", "description": "Agent identifier"},
                "update_version": {"type": "integer", "description": "If provided, updates the agent's last synced version"}
            },
            "required": ["agent_id"]
        }"#,
    ),
    (
        "sync_cleanup",
        "Clean up old sync data (events, etc.) older than specified days.",
        r#"{
            "type": "object",
            "properties": {
                "older_than_days": {"type": "integer", "default": 30, "description": "Delete sync data older than this many days"}
            }
        }"#,
    ),
    // Multi-Agent Sharing
    (
        "memory_share",
        "Share a memory with another agent. The target agent can poll for shared memories.",
        r#"{
            "type": "object",
            "properties": {
                "memory_id": {"type": "integer", "description": "ID of memory to share"},
                "from_agent": {"type": "string", "description": "Sender agent identifier"},
                "to_agent": {"type": "string", "description": "Recipient agent identifier"},
                "message": {"type": "string", "description": "Optional message to include with share"}
            },
            "required": ["memory_id", "from_agent", "to_agent"]
        }"#,
    ),
    (
        "memory_shared_poll",
        "Poll for memories shared with this agent.",
        r#"{
            "type": "object",
            "properties": {
                "agent_id": {"type": "string", "description": "Agent identifier to check shares for"},
                "include_acknowledged": {"type": "boolean", "default": false, "description": "Include already acknowledged shares"}
            },
            "required": ["agent_id"]
        }"#,
    ),
    (
        "memory_share_ack",
        "Acknowledge receipt of a shared memory.",
        r#"{
            "type": "object",
            "properties": {
                "share_id": {"type": "integer", "description": "Share ID to acknowledge"},
                "agent_id": {"type": "string", "description": "Agent acknowledging the share"}
            },
            "required": ["share_id", "agent_id"]
        }"#,
    ),
    // Search Variants
    (
        "memory_search_by_identity",
        "Search memories by identity (person, entity, or alias). Finds all mentions of a specific identity across memories.",
        r#"{
            "type": "object",
            "properties": {
                "identity": {"type": "string", "description": "Identity name or alias to search for"},
                "workspace": {"type": "string", "description": "Optional: limit search to specific workspace"},
                "limit": {"type": "integer", "default": 50, "description": "Maximum results to return"}
            },
            "required": ["identity"]
        }"#,
    ),
    (
        "memory_session_search",
        "Search within session transcript chunks. Useful for finding content from past conversations.",
        r#"{
            "type": "object",
            "properties": {
                "query": {"type": "string", "description": "Search query"},
                "session_id": {"type": "string", "description": "Optional: limit to specific session"},
                "workspace": {"type": "string", "description": "Optional: limit to specific workspace"},
                "limit": {"type": "integer", "default": 20, "description": "Maximum results to return"}
            },
            "required": ["query"]
        }"#,
    ),
    // Image Handling
    (
        "memory_upload_image",
        "Upload an image file and attach it to a memory. The image will be stored locally and linked to the memory's metadata.",
        r#"{
            "type": "object",
            "properties": {
                "memory_id": {"type": "integer", "description": "ID of the memory to attach the image to"},
                "file_path": {"type": "string", "description": "Path to the image file to upload"},
                "image_index": {"type": "integer", "default": 0, "description": "Index for ordering multiple images (0-based)"},
                "caption": {"type": "string", "description": "Optional caption for the image"}
            },
            "required": ["memory_id", "file_path"]
        }"#,
    ),
    (
        "memory_migrate_images",
        "Migrate existing base64-encoded images in memories to file storage. Scans all memories and uploads any embedded data URIs to storage, replacing them with file references.",
        r#"{
            "type": "object",
            "properties": {
                "dry_run": {"type": "boolean", "default": false, "description": "If true, only report what would be migrated without making changes"}
            }
        }"#,
    ),
    // Auto-Tagging
    (
        "memory_suggest_tags",
        "Suggest tags for a memory based on AI content analysis. Uses pattern matching, keyword extraction, and structure detection to suggest relevant tags with confidence scores.",
        r#"{
            "type": "object",
            "properties": {
                "id": {"type": "integer", "description": "Memory ID to analyze (alternative to content)"},
                "memory_id": {"type": "integer", "description": "Memory ID to analyze (alias for id)"},
                "content": {"type": "string", "description": "Content to analyze (alternative to id/memory_id)"},
                "type": {"type": "string", "enum": ["note", "todo", "issue", "decision", "preference", "learning", "context", "credential"], "description": "Memory type (used when providing content directly)"},
                "existing_tags": {"type": "array", "items": {"type": "string"}, "description": "Tags already on the memory (excluded from suggestions)"},
                "min_confidence": {"type": "number", "minimum": 0, "maximum": 1, "default": 0.5, "description": "Minimum confidence threshold for suggestions"},
                "max_tags": {"type": "integer", "default": 5, "description": "Maximum number of tags to suggest"},
                "enable_patterns": {"type": "boolean", "default": true, "description": "Use pattern-based tagging"},
                "enable_keywords": {"type": "boolean", "default": true, "description": "Use keyword-based tagging"},
                "enable_entities": {"type": "boolean", "default": true, "description": "Use entity-based tagging"},
                "enable_type_tags": {"type": "boolean", "default": true, "description": "Add tags based on memory type"},
                "keyword_mappings": {"type": "object", "description": "Custom keyword-to-tag mappings (e.g., {\"ibvi\": \"project/ibvi\"})"}
            }
        }"#,
    ),
    (
        "memory_auto_tag",
        "Automatically suggest and optionally apply tags to a memory. Analyzes content using AI heuristics and can merge suggested tags with existing ones.",
        r#"{
            "type": "object",
            "properties": {
                "id": {"type": "integer", "description": "Memory ID to auto-tag"},
                "memory_id": {"type": "integer", "description": "Memory ID (alias for id)"},
                "apply": {"type": "boolean", "default": false, "description": "If true, apply the suggested tags to the memory. If false, only return suggestions."},
                "merge": {"type": "boolean", "default": true, "description": "If true and apply=true, merge with existing tags. If false, replace existing tags."},
                "min_confidence": {"type": "number", "minimum": 0, "maximum": 1, "default": 0.5, "description": "Minimum confidence threshold"},
                "max_tags": {"type": "integer", "default": 5, "description": "Maximum tags to suggest/apply"},
                "keyword_mappings": {"type": "object", "description": "Custom keyword-to-tag mappings"}
            },
            "required": ["id"]
        }"#,
    ),
    // Phase 8: Salience & Sessions (ENG-66 to ENG-77)
    (
        "salience_get",
        "Get the salience score for a memory. Returns recency, frequency, importance, and feedback components with the combined score and lifecycle state.",
        r#"{
            "type": "object",
            "properties": {
                "id": {"type": "integer", "description": "Memory ID to get salience for"},
                "feedback_signal": {"type": "number", "minimum": -1, "maximum": 1, "default": 0, "description": "Optional feedback signal (-1 to 1) to include in calculation"}
            },
            "required": ["id"]
        }"#,
    ),
    (
        "salience_set_importance",
        "Set the importance score for a memory. This is the static importance component of salience.",
        r#"{
            "type": "object",
            "properties": {
                "id": {"type": "integer", "description": "Memory ID"},
                "importance": {"type": "number", "minimum": 0, "maximum": 1, "description": "Importance score (0-1)"}
            },
            "required": ["id", "importance"]
        }"#,
    ),
    (
        "salience_boost",
        "Boost a memory's salience score temporarily or permanently. Useful for marking memories as contextually relevant.",
        r#"{
            "type": "object",
            "properties": {
                "id": {"type": "integer", "description": "Memory ID to boost"},
                "boost_amount": {"type": "number", "minimum": 0, "maximum": 1, "default": 0.2, "description": "Amount to boost (0-1)"},
                "reason": {"type": "string", "description": "Optional reason for boosting"}
            },
            "required": ["id"]
        }"#,
    ),
    (
        "salience_demote",
        "Demote a memory's salience score. Useful for marking memories as less relevant.",
        r#"{
            "type": "object",
            "properties": {
                "id": {"type": "integer", "description": "Memory ID to demote"},
                "demote_amount": {"type": "number", "minimum": 0, "maximum": 1, "default": 0.2, "description": "Amount to demote (0-1)"},
                "reason": {"type": "string", "description": "Optional reason for demoting"}
            },
            "required": ["id"]
        }"#,
    ),
    (
        "salience_decay_run",
        "Run temporal decay on all memories. Updates lifecycle states (Active → Stale → Archived) based on salience scores.",
        r#"{
            "type": "object",
            "properties": {
                "dry_run": {"type": "boolean", "default": false, "description": "If true, compute changes without persisting updates"},
                "record_history": {"type": "boolean", "default": true, "description": "Record salience history entries while updating"},
                "workspace": {"type": "string", "description": "Limit to specific workspace"},
                "stale_threshold_days": {"type": "integer", "minimum": 1, "description": "Days of inactivity before marking stale"},
                "archive_threshold_days": {"type": "integer", "minimum": 1, "description": "Days of inactivity before suggesting archive"}
            }
        }"#,
    ),
    (
        "salience_stats",
        "Get salience statistics across all memories. Returns distribution, percentiles, and state counts.",
        r#"{
            "type": "object",
            "properties": {
                "workspace": {"type": "string", "description": "Limit to specific workspace"}
            }
        }"#,
    ),
    (
        "salience_history",
        "Get salience score history for a memory. Shows how salience has changed over time.",
        r#"{
            "type": "object",
            "properties": {
                "id": {"type": "integer", "description": "Memory ID"},
                "limit": {"type": "integer", "default": 50, "description": "Maximum history entries to return"}
            },
            "required": ["id"]
        }"#,
    ),
    (
        "salience_top",
        "Get top memories by salience score. Useful for context injection.",
        r#"{
            "type": "object",
            "properties": {
                "limit": {"type": "integer", "default": 20, "description": "Maximum memories to return"},
                "workspace": {"type": "string", "description": "Limit to specific workspace"},
                "min_score": {"type": "number", "minimum": 0, "maximum": 1, "description": "Minimum salience score"},
                "memory_type": {"type": "string", "description": "Filter by memory type"}
            }
        }"#,
    ),
    // Session Context Tools (ENG-70, ENG-71)
    (
        "session_context_create",
        "Create a new session context for tracking related memories during a conversation or task.",
        r#"{
            "type": "object",
            "properties": {
                "name": {"type": "string", "description": "Session name"},
                "description": {"type": "string", "description": "Session description"},
                "workspace": {"type": "string", "description": "Workspace for the session"},
                "metadata": {"type": "object", "description": "Additional session metadata"}
            },
            "required": ["name"]
        }"#,
    ),
    (
        "session_context_add_memory",
        "Add a memory to a session context with relevance score and role.",
        r#"{
            "type": "object",
            "properties": {
                "session_id": {"type": "string", "description": "Session ID"},
                "memory_id": {"type": "integer", "description": "Memory ID to add"},
                "relevance_score": {"type": "number", "minimum": 0, "maximum": 1, "default": 1.0, "description": "How relevant this memory is to the session"},
                "context_role": {"type": "string", "enum": ["referenced", "created", "updated", "pinned"], "default": "referenced", "description": "Role of the memory in the session"}
            },
            "required": ["session_id", "memory_id"]
        }"#,
    ),
    (
        "session_context_remove_memory",
        "Remove a memory from a session context.",
        r#"{
            "type": "object",
            "properties": {
                "session_id": {"type": "string", "description": "Session ID"},
                "memory_id": {"type": "integer", "description": "Memory ID to remove"}
            },
            "required": ["session_id", "memory_id"]
        }"#,
    ),
    (
        "session_context_get",
        "Get a session context with its linked memories.",
        r#"{
            "type": "object",
            "properties": {
                "session_id": {"type": "string", "description": "Session ID"}
            },
            "required": ["session_id"]
        }"#,
    ),
    (
        "session_context_list",
        "List all session contexts with optional filtering.",
        r#"{
            "type": "object",
            "properties": {
                "workspace": {"type": "string", "description": "Filter by workspace"},
                "active_only": {"type": "boolean", "default": false, "description": "Only return active sessions"},
                "limit": {"type": "integer", "default": 50, "description": "Maximum sessions to return"},
                "offset": {"type": "integer", "default": 0, "description": "Offset for pagination"}
            }
        }"#,
    ),
    (
        "session_context_search",
        "Search memories within a specific session context.",
        r#"{
            "type": "object",
            "properties": {
                "session_id": {"type": "string", "description": "Session ID to search within"},
                "query": {"type": "string", "description": "Search query"},
                "limit": {"type": "integer", "default": 20, "description": "Maximum results"}
            },
            "required": ["session_id", "query"]
        }"#,
    ),
    (
        "session_context_update_summary",
        "Update the summary of a session context.",
        r#"{
            "type": "object",
            "properties": {
                "session_id": {"type": "string", "description": "Session ID"},
                "summary": {"type": "string", "description": "New session summary"}
            },
            "required": ["session_id", "summary"]
        }"#,
    ),
    (
        "session_context_end",
        "End a session context, marking it as inactive.",
        r#"{
            "type": "object",
            "properties": {
                "session_id": {"type": "string", "description": "Session ID to end"},
                "summary": {"type": "string", "description": "Optional final summary"}
            },
            "required": ["session_id"]
        }"#,
    ),
    (
        "session_context_export",
        "Export a session context with all its memories for archival or sharing.",
        r#"{
            "type": "object",
            "properties": {
                "session_id": {"type": "string", "description": "Session ID to export"},
                "include_content": {"type": "boolean", "default": true, "description": "Include full memory content"},
                "format": {"type": "string", "enum": ["json", "markdown"], "default": "json", "description": "Export format"}
            },
            "required": ["session_id"]
        }"#,
    ),
    // Phase 9: Context Quality (ENG-48 to ENG-66)
    (
        "quality_score",
        "Get the quality score for a memory with detailed breakdown of clarity, completeness, freshness, consistency, and source trust components.",
        r#"{
            "type": "object",
            "properties": {
                "id": {"type": "integer", "description": "Memory ID to score"}
            },
            "required": ["id"]
        }"#,
    ),
    (
        "quality_report",
        "Generate a comprehensive quality report for a workspace. Includes quality distribution, top issues, conflict and duplicate counts.",
        r#"{
            "type": "object",
            "properties": {
                "workspace": {"type": "string", "description": "Workspace to analyze (default: 'default')"}
            }
        }"#,
    ),
    (
        "quality_find_duplicates",
        "Find near-duplicate memories using text similarity. Returns pairs of similar memories above the threshold.",
        r#"{
            "type": "object",
            "properties": {
                "threshold": {"type": "number", "minimum": 0, "maximum": 1, "default": 0.85, "description": "Similarity threshold (0-1)"},
                "limit": {"type": "integer", "default": 100, "description": "Maximum memories to compare"}
            }
        }"#,
    ),
    (
        "quality_get_duplicates",
        "Get pending duplicate candidates that need review.",
        r#"{
            "type": "object",
            "properties": {
                "limit": {"type": "integer", "default": 50, "description": "Maximum duplicates to return"}
            }
        }"#,
    ),
    (
        "quality_find_conflicts",
        "Detect conflicts for a memory against existing memories. Finds contradictions, staleness, and semantic overlaps.",
        r#"{
            "type": "object",
            "properties": {
                "id": {"type": "integer", "description": "Memory ID to check for conflicts"}
            },
            "required": ["id"]
        }"#,
    ),
    (
        "quality_get_conflicts",
        "Get unresolved conflicts that need attention.",
        r#"{
            "type": "object",
            "properties": {
                "limit": {"type": "integer", "default": 50, "description": "Maximum conflicts to return"}
            }
        }"#,
    ),
    (
        "quality_resolve_conflict",
        "Resolve a conflict between memories. Options: keep_a, keep_b, merge, keep_both, delete_both, false_positive.",
        r#"{
            "type": "object",
            "properties": {
                "conflict_id": {"type": "integer", "description": "Conflict ID to resolve"},
                "resolution": {"type": "string", "enum": ["keep_a", "keep_b", "merge", "keep_both", "delete_both", "false_positive"], "description": "How to resolve the conflict"},
                "notes": {"type": "string", "description": "Optional notes about the resolution"}
            },
            "required": ["conflict_id", "resolution"]
        }"#,
    ),
    (
        "quality_source_trust",
        "Get or update trust score for a source type. Higher trust means memories from this source are weighted more in quality calculations.",
        r#"{
            "type": "object",
            "properties": {
                "source_type": {"type": "string", "description": "Source type (user, seed, extraction, inference, external)"},
                "source_identifier": {"type": "string", "description": "Optional specific source identifier"},
                "trust_score": {"type": "number", "minimum": 0, "maximum": 1, "description": "New trust score (omit to just get current score)"},
                "notes": {"type": "string", "description": "Notes about this source"}
            },
            "required": ["source_type"]
        }"#,
    ),
    (
        "quality_improve",
        "Get suggestions for improving a memory's quality. Returns actionable recommendations.",
        r#"{
            "type": "object",
            "properties": {
                "id": {"type": "integer", "description": "Memory ID to analyze"}
            },
            "required": ["id"]
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
