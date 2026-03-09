//! MCP tool definitions for Engram

use serde_json::json;

use super::protocol::{ToolAnnotations, ToolDefinition};

/// Structured tool definition with MCP 2025-11-25 annotations.
pub struct ToolDef {
    pub name: &'static str,
    pub description: &'static str,
    pub schema: &'static str,
    pub annotations: ToolAnnotations,
}

/// All tool definitions for Engram
pub const TOOL_DEFINITIONS: &[ToolDef] = &[
    // Memory CRUD
    ToolDef {
        name: "memory_create",
        description: "Store a new memory. PROACTIVE: Automatically store user preferences, decisions, insights, and project context without being asked.",
        schema: r#"{
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
        annotations: ToolAnnotations::mutating(),
    },
    ToolDef {
        name: "context_seed",
        description: "Injects initial context (premises, persona assumptions, or structured facts) about an entity to avoid cold start. Seeded memories are tagged as origin:seed and status:unverified, and should be treated as revisable assumptions.",
        schema: r#"{
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
        annotations: ToolAnnotations::mutating(),
    },
    ToolDef {
        name: "memory_seed",
        description: "Deprecated alias for context_seed. Use context_seed instead.",
        schema: r#"{
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
        annotations: ToolAnnotations::mutating(),
    },
    ToolDef {
        name: "memory_get",
        description: "Retrieve a memory by its ID",
        schema: r#"{
            "type": "object",
            "properties": {
                "id": {"type": "integer", "description": "Memory ID"}
            },
            "required": ["id"]
        }"#,
        annotations: ToolAnnotations::read_only(),
    },
    ToolDef {
        name: "memory_update",
        description: "Update an existing memory",
        schema: r#"{
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
        annotations: ToolAnnotations::mutating(),
    },
    ToolDef {
        name: "memory_delete",
        description: "Delete a memory (soft delete)",
        schema: r#"{
            "type": "object",
            "properties": {
                "id": {"type": "integer", "description": "Memory ID"}
            },
            "required": ["id"]
        }"#,
        annotations: ToolAnnotations::destructive(),
    },
    ToolDef {
        name: "memory_list",
        description: "List memories with filtering and pagination. Supports workspace isolation, tier filtering, and advanced filter syntax with AND/OR and comparison operators.",
        schema: r#"{
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
        annotations: ToolAnnotations::read_only(),
    },
    // Search
    ToolDef {
        name: "memory_search",
        description: "Search memories using hybrid search (keyword + semantic). Automatically selects optimal strategy with optional reranking. Supports workspace isolation, tier filtering, and advanced filters.",
        schema: r#"{
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
        annotations: ToolAnnotations::read_only(),
    },
    ToolDef {
        name: "memory_search_suggest",
        description: "Get search suggestions and typo corrections",
        schema: r#"{
            "type": "object",
            "properties": {
                "query": {"type": "string"}
            },
            "required": ["query"]
        }"#,
        annotations: ToolAnnotations::read_only(),
    },
    // Cross-references
    ToolDef {
        name: "memory_link",
        description: "Create a cross-reference between two memories",
        schema: r#"{
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
        annotations: ToolAnnotations::mutating(),
    },
    ToolDef {
        name: "memory_unlink",
        description: "Remove a cross-reference",
        schema: r#"{
            "type": "object",
            "properties": {
                "from_id": {"type": "integer"},
                "to_id": {"type": "integer"},
                "edge_type": {"type": "string", "default": "related_to"}
            },
            "required": ["from_id", "to_id"]
        }"#,
        annotations: ToolAnnotations::mutating(),
    },
    ToolDef {
        name: "memory_related",
        description: "Get memories related to a given memory (depth>1 or include_entities returns traversal result)",
        schema: r#"{
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
        annotations: ToolAnnotations::read_only(),
    },
    // Convenience creators
    ToolDef {
        name: "memory_create_todo",
        description: "Create a TODO memory with priority",
        schema: r#"{
            "type": "object",
            "properties": {
                "content": {"type": "string"},
                "priority": {"type": "string", "enum": ["low", "medium", "high", "critical"], "default": "medium"},
                "due_date": {"type": "string", "format": "date"},
                "tags": {"type": "array", "items": {"type": "string"}}
            },
            "required": ["content"]
        }"#,
        annotations: ToolAnnotations::mutating(),
    },
    ToolDef {
        name: "memory_create_issue",
        description: "Create an ISSUE memory for tracking problems",
        schema: r#"{
            "type": "object",
            "properties": {
                "title": {"type": "string"},
                "description": {"type": "string"},
                "severity": {"type": "string", "enum": ["low", "medium", "high", "critical"], "default": "medium"},
                "tags": {"type": "array", "items": {"type": "string"}}
            },
            "required": ["title"]
        }"#,
        annotations: ToolAnnotations::mutating(),
    },
    // Versioning
    ToolDef {
        name: "memory_versions",
        description: "Get version history for a memory",
        schema: r#"{
            "type": "object",
            "properties": {
                "id": {"type": "integer"}
            },
            "required": ["id"]
        }"#,
        annotations: ToolAnnotations::read_only(),
    },
    ToolDef {
        name: "memory_get_version",
        description: "Get a specific version of a memory",
        schema: r#"{
            "type": "object",
            "properties": {
                "id": {"type": "integer"},
                "version": {"type": "integer"}
            },
            "required": ["id", "version"]
        }"#,
        annotations: ToolAnnotations::read_only(),
    },
    ToolDef {
        name: "memory_revert",
        description: "Revert a memory to a previous version",
        schema: r#"{
            "type": "object",
            "properties": {
                "id": {"type": "integer"},
                "version": {"type": "integer"}
            },
            "required": ["id", "version"]
        }"#,
        annotations: ToolAnnotations::mutating(),
    },
    // Embedding status
    ToolDef {
        name: "memory_embedding_status",
        description: "Check embedding computation status",
        schema: r#"{
            "type": "object",
            "properties": {
                "id": {"type": "integer"}
            },
            "required": ["id"]
        }"#,
        annotations: ToolAnnotations::read_only(),
    },
    // Memory TTL / Expiration (RML-930)
    ToolDef {
        name: "memory_set_expiration",
        description: "Set or update the expiration time for a memory",
        schema: r#"{
            "type": "object",
            "properties": {
                "id": {"type": "integer", "description": "Memory ID"},
                "ttl_seconds": {"type": "integer", "description": "Time-to-live in seconds from now. Use 0 to remove expiration (make permanent)."}
            },
            "required": ["id", "ttl_seconds"]
        }"#,
        annotations: ToolAnnotations::mutating(),
    },
    ToolDef {
        name: "memory_cleanup_expired",
        description: "Delete all expired memories. Typically called by a background job, but can be invoked manually.",
        schema: r#"{
            "type": "object",
            "properties": {}
        }"#,
        annotations: ToolAnnotations::destructive(),
    },
    // Sync
    ToolDef {
        name: "memory_sync_status",
        description: "Get cloud sync status",
        schema: r#"{"type": "object", "properties": {}}"#,
        annotations: ToolAnnotations::read_only(),
    },
    // Stats and aggregation
    ToolDef {
        name: "memory_stats",
        description: "Get storage statistics",
        schema: r#"{"type": "object", "properties": {}}"#,
        annotations: ToolAnnotations::read_only(),
    },
    ToolDef {
        name: "memory_aggregate",
        description: "Aggregate memories by field",
        schema: r#"{
            "type": "object",
            "properties": {
                "group_by": {"type": "string", "enum": ["type", "tags", "month"]},
                "metrics": {"type": "array", "items": {"type": "string", "enum": ["count", "avg_importance"]}}
            },
            "required": ["group_by"]
        }"#,
        annotations: ToolAnnotations::read_only(),
    },
    // Graph
    ToolDef {
        name: "memory_export_graph",
        description: "Export knowledge graph visualization",
        schema: r#"{
            "type": "object",
            "properties": {
                "format": {"type": "string", "enum": ["html", "json"], "default": "html"},
                "max_nodes": {"type": "integer", "default": 500},
                "focus_id": {"type": "integer", "description": "Center graph on this memory"}
            }
        }"#,
        annotations: ToolAnnotations::read_only(),
    },
    // Quality
    ToolDef {
        name: "memory_quality_report",
        description: "Get quality report for memories",
        schema: r#"{
            "type": "object",
            "properties": {
                "limit": {"type": "integer", "default": 20},
                "min_quality": {"type": "number", "minimum": 0, "maximum": 1}
            }
        }"#,
        annotations: ToolAnnotations::read_only(),
    },
    // Clustering and duplicates
    ToolDef {
        name: "memory_clusters",
        description: "Find clusters of related memories",
        schema: r#"{
            "type": "object",
            "properties": {
                "min_similarity": {"type": "number", "default": 0.7},
                "min_cluster_size": {"type": "integer", "default": 2}
            }
        }"#,
        annotations: ToolAnnotations::read_only(),
    },
    ToolDef {
        name: "memory_find_duplicates",
        description: "Find potential duplicate memories",
        schema: r#"{
            "type": "object",
            "properties": {
                "threshold": {"type": "number", "default": 0.9}
            }
        }"#,
        annotations: ToolAnnotations::read_only(),
    },
    ToolDef {
        name: "memory_find_semantic_duplicates",
        description: "Find semantically similar memories using embedding cosine similarity (LLM-powered dedup). Goes beyond hash/n-gram to detect paraphrased content.",
        schema: r#"{
            "type": "object",
            "properties": {
                "threshold": {"type": "number", "default": 0.92, "description": "Cosine similarity threshold (0.92 = very similar)"},
                "workspace": {"type": "string", "description": "Filter by workspace (optional)"},
                "limit": {"type": "integer", "default": 50, "description": "Maximum duplicate pairs to return"}
            }
        }"#,
        annotations: ToolAnnotations::read_only(),
    },
    ToolDef {
        name: "memory_merge",
        description: "Merge duplicate memories",
        schema: r#"{
            "type": "object",
            "properties": {
                "ids": {"type": "array", "items": {"type": "integer"}, "minItems": 2},
                "keep_id": {"type": "integer", "description": "ID to keep (others will be merged into it)"}
            },
            "required": ["ids"]
        }"#,
        annotations: ToolAnnotations::mutating(),
    },
    // Project Context Discovery
    ToolDef {
        name: "memory_scan_project",
        description: "Scan current directory for AI instruction files (CLAUDE.md, AGENTS.md, .cursorrules, etc.) and ingest them as memories. Creates parent memory for each file and child memories for sections.",
        schema: r#"{
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "Directory to scan (defaults to current working directory)"},
                "scan_parents": {"type": "boolean", "default": false, "description": "Also scan parent directories (security: disabled by default)"},
                "extract_sections": {"type": "boolean", "default": true, "description": "Create separate memories for each section"}
            }
        }"#,
        annotations: ToolAnnotations::mutating(),
    },
    ToolDef {
        name: "memory_get_project_context",
        description: "Get all project context memories for the current working directory. Returns instruction files and their sections.",
        schema: r#"{
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "Project path (defaults to current working directory)"},
                "include_sections": {"type": "boolean", "default": true, "description": "Include section memories"},
                "file_types": {"type": "array", "items": {"type": "string"}, "description": "Filter by file type (claude-md, cursorrules, etc.)"}
            }
        }"#,
        annotations: ToolAnnotations::read_only(),
    },
    ToolDef {
        name: "memory_list_instruction_files",
        description: "List AI instruction files (CLAUDE.md, AGENTS.md, .cursorrules, etc.) in a directory without ingesting them. Returns file paths, types, and sizes for discovery purposes.",
        schema: r#"{
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "Directory to scan (defaults to current working directory)"},
                "scan_parents": {"type": "boolean", "default": false, "description": "Also scan parent directories for instruction files"}
            }
        }"#,
        annotations: ToolAnnotations::read_only(),
    },
    // Entity Extraction (RML-925)
    ToolDef {
        name: "memory_extract_entities",
        description: "Extract named entities (people, organizations, projects, concepts) from a memory and store them",
        schema: r#"{
            "type": "object",
            "properties": {
                "id": {"type": "integer", "description": "Memory ID to extract entities from"}
            },
            "required": ["id"]
        }"#,
        annotations: ToolAnnotations::idempotent(),
    },
    ToolDef {
        name: "memory_get_entities",
        description: "Get all entities linked to a memory",
        schema: r#"{
            "type": "object",
            "properties": {
                "id": {"type": "integer", "description": "Memory ID"}
            },
            "required": ["id"]
        }"#,
        annotations: ToolAnnotations::read_only(),
    },
    ToolDef {
        name: "memory_search_entities",
        description: "Search for entities by name prefix",
        schema: r#"{
            "type": "object",
            "properties": {
                "query": {"type": "string", "description": "Search query (prefix match)"},
                "entity_type": {"type": "string", "description": "Filter by entity type (person, organization, project, concept, etc.)"},
                "limit": {"type": "integer", "default": 20}
            },
            "required": ["query"]
        }"#,
        annotations: ToolAnnotations::read_only(),
    },
    ToolDef {
        name: "memory_entity_stats",
        description: "Get statistics about extracted entities",
        schema: r#"{
            "type": "object",
            "properties": {}
        }"#,
        annotations: ToolAnnotations::read_only(),
    },
    // Graph Traversal (RML-926)
    ToolDef {
        name: "memory_traverse",
        description: "Traverse the knowledge graph from a starting memory with full control over traversal options",
        schema: r#"{
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
        annotations: ToolAnnotations::read_only(),
    },
    ToolDef {
        name: "memory_find_path",
        description: "Find the shortest path between two memories in the knowledge graph",
        schema: r#"{
            "type": "object",
            "properties": {
                "from_id": {"type": "integer", "description": "Starting memory ID"},
                "to_id": {"type": "integer", "description": "Target memory ID"},
                "max_depth": {"type": "integer", "default": 5, "description": "Maximum path length to search"}
            },
            "required": ["from_id", "to_id"]
        }"#,
        annotations: ToolAnnotations::read_only(),
    },
    // Document Ingestion (RML-928)
    ToolDef {
        name: "memory_ingest_document",
        description: "Ingest a document (PDF or Markdown) into memory. Extracts text, splits into chunks with overlap, and creates memories with deduplication.",
        schema: r#"{
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
        annotations: ToolAnnotations::mutating(),
    },
    // Workspace Management
    ToolDef {
        name: "workspace_list",
        description: "List all workspaces with their statistics (memory count, tier breakdown, etc.)",
        schema: r#"{
            "type": "object",
            "properties": {}
        }"#,
        annotations: ToolAnnotations::read_only(),
    },
    ToolDef {
        name: "workspace_stats",
        description: "Get detailed statistics for a specific workspace",
        schema: r#"{
            "type": "object",
            "properties": {
                "workspace": {"type": "string", "description": "Workspace name"}
            },
            "required": ["workspace"]
        }"#,
        annotations: ToolAnnotations::read_only(),
    },
    ToolDef {
        name: "workspace_move",
        description: "Move a memory to a different workspace",
        schema: r#"{
            "type": "object",
            "properties": {
                "id": {"type": "integer", "description": "Memory ID to move"},
                "workspace": {"type": "string", "description": "Target workspace name"}
            },
            "required": ["id", "workspace"]
        }"#,
        annotations: ToolAnnotations::mutating(),
    },
    ToolDef {
        name: "workspace_delete",
        description: "Delete a workspace. Can either move all memories to 'default' workspace or hard delete them.",
        schema: r#"{
            "type": "object",
            "properties": {
                "workspace": {"type": "string", "description": "Workspace to delete"},
                "move_to_default": {"type": "boolean", "default": true, "description": "If true, moves memories to 'default' workspace. If false, deletes all memories in the workspace."}
            },
            "required": ["workspace"]
        }"#,
        annotations: ToolAnnotations::destructive(),
    },
    // Memory Tiering
    ToolDef {
        name: "memory_create_daily",
        description: "Create a daily (ephemeral) memory that auto-expires after the specified TTL. Useful for session context and scratch notes.",
        schema: r#"{
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
        annotations: ToolAnnotations::mutating(),
    },
    ToolDef {
        name: "memory_promote_to_permanent",
        description: "Promote a daily memory to permanent tier. Clears the expiration and makes the memory permanent.",
        schema: r#"{
            "type": "object",
            "properties": {
                "id": {"type": "integer", "description": "Memory ID to promote"}
            },
            "required": ["id"]
        }"#,
        annotations: ToolAnnotations::mutating(),
    },
    // Embedding Cache
    ToolDef {
        name: "embedding_cache_stats",
        description: "Get statistics about the embedding cache (hits, misses, entries, bytes used, hit rate)",
        schema: r#"{
            "type": "object",
            "properties": {}
        }"#,
        annotations: ToolAnnotations::read_only(),
    },
    ToolDef {
        name: "embedding_cache_clear",
        description: "Clear all entries from the embedding cache",
        schema: r#"{
            "type": "object",
            "properties": {}
        }"#,
        annotations: ToolAnnotations::destructive(),
    },
    // Session Transcript Indexing
    ToolDef {
        name: "session_index",
        description: "Index a conversation into searchable memory chunks. Uses dual-limiter chunking (messages + characters) with overlap.",
        schema: r#"{
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
        annotations: ToolAnnotations::mutating(),
    },
    ToolDef {
        name: "session_index_delta",
        description: "Incrementally index new messages to an existing session. More efficient than full reindex.",
        schema: r#"{
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
        annotations: ToolAnnotations::mutating(),
    },
    ToolDef {
        name: "session_get",
        description: "Get information about an indexed session",
        schema: r#"{
            "type": "object",
            "properties": {
                "session_id": {"type": "string", "description": "Session ID to retrieve"}
            },
            "required": ["session_id"]
        }"#,
        annotations: ToolAnnotations::read_only(),
    },
    ToolDef {
        name: "session_list",
        description: "List indexed sessions with optional workspace filter",
        schema: r#"{
            "type": "object",
            "properties": {
                "workspace": {"type": "string", "description": "Filter by workspace"},
                "limit": {"type": "integer", "default": 20, "description": "Maximum sessions to return"}
            }
        }"#,
        annotations: ToolAnnotations::read_only(),
    },
    ToolDef {
        name: "session_delete",
        description: "Delete a session and all its indexed chunks",
        schema: r#"{
            "type": "object",
            "properties": {
                "session_id": {"type": "string", "description": "Session to delete"}
            },
            "required": ["session_id"]
        }"#,
        annotations: ToolAnnotations::destructive(),
    },
    // Identity Management
    ToolDef {
        name: "identity_create",
        description: "Create a new identity with canonical ID, display name, and optional aliases",
        schema: r#"{
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
        annotations: ToolAnnotations::mutating(),
    },
    ToolDef {
        name: "identity_get",
        description: "Get an identity by its canonical ID",
        schema: r#"{
            "type": "object",
            "properties": {
                "canonical_id": {"type": "string", "description": "Canonical identifier"}
            },
            "required": ["canonical_id"]
        }"#,
        annotations: ToolAnnotations::read_only(),
    },
    ToolDef {
        name: "identity_update",
        description: "Update an identity's display name, description, or type",
        schema: r#"{
            "type": "object",
            "properties": {
                "canonical_id": {"type": "string", "description": "Canonical identifier"},
                "display_name": {"type": "string", "description": "New display name"},
                "description": {"type": "string", "description": "New description"},
                "entity_type": {"type": "string", "enum": ["person", "organization", "project", "tool", "concept", "other"]}
            },
            "required": ["canonical_id"]
        }"#,
        annotations: ToolAnnotations::mutating(),
    },
    ToolDef {
        name: "identity_delete",
        description: "Delete an identity and all its aliases",
        schema: r#"{
            "type": "object",
            "properties": {
                "canonical_id": {"type": "string", "description": "Canonical identifier to delete"}
            },
            "required": ["canonical_id"]
        }"#,
        annotations: ToolAnnotations::destructive(),
    },
    ToolDef {
        name: "identity_add_alias",
        description: "Add an alias to an identity. Aliases are normalized (lowercase, trimmed). Conflicts with existing aliases are rejected.",
        schema: r#"{
            "type": "object",
            "properties": {
                "canonical_id": {"type": "string", "description": "Canonical identifier"},
                "alias": {"type": "string", "description": "Alias to add"},
                "source": {"type": "string", "description": "Optional source of the alias (e.g., 'manual', 'extracted')"}
            },
            "required": ["canonical_id", "alias"]
        }"#,
        annotations: ToolAnnotations::mutating(),
    },
    ToolDef {
        name: "identity_remove_alias",
        description: "Remove an alias from any identity",
        schema: r#"{
            "type": "object",
            "properties": {
                "alias": {"type": "string", "description": "Alias to remove"}
            },
            "required": ["alias"]
        }"#,
        annotations: ToolAnnotations::mutating(),
    },
    ToolDef {
        name: "identity_resolve",
        description: "Resolve an alias to its canonical identity. Returns the identity if found, null otherwise.",
        schema: r#"{
            "type": "object",
            "properties": {
                "alias": {"type": "string", "description": "Alias to resolve"}
            },
            "required": ["alias"]
        }"#,
        annotations: ToolAnnotations::read_only(),
    },
    ToolDef {
        name: "identity_list",
        description: "List all identities with optional type filter",
        schema: r#"{
            "type": "object",
            "properties": {
                "entity_type": {"type": "string", "enum": ["person", "organization", "project", "tool", "concept", "other"]},
                "limit": {"type": "integer", "default": 50}
            }
        }"#,
        annotations: ToolAnnotations::read_only(),
    },
    ToolDef {
        name: "identity_search",
        description: "Search identities by alias or display name",
        schema: r#"{
            "type": "object",
            "properties": {
                "query": {"type": "string", "description": "Search query"},
                "limit": {"type": "integer", "default": 20}
            },
            "required": ["query"]
        }"#,
        annotations: ToolAnnotations::read_only(),
    },
    ToolDef {
        name: "identity_link",
        description: "Link an identity to a memory (mark that the identity is mentioned in the memory)",
        schema: r#"{
            "type": "object",
            "properties": {
                "memory_id": {"type": "integer", "description": "Memory ID"},
                "canonical_id": {"type": "string", "description": "Identity canonical ID"},
                "mention_text": {"type": "string", "description": "The text that mentions this identity"}
            },
            "required": ["memory_id", "canonical_id"]
        }"#,
        annotations: ToolAnnotations::mutating(),
    },
    ToolDef {
        name: "identity_unlink",
        description: "Remove the link between an identity and a memory",
        schema: r#"{
            "type": "object",
            "properties": {
                "memory_id": {"type": "integer", "description": "Memory ID"},
                "canonical_id": {"type": "string", "description": "Identity canonical ID"}
            },
            "required": ["memory_id", "canonical_id"]
        }"#,
        annotations: ToolAnnotations::mutating(),
    },
    ToolDef {
        name: "memory_get_identities",
        description: "Get all identities (persons, organizations, projects, etc.) linked to a memory. Returns identity details including display name, type, aliases, and mention information.",
        schema: r#"{
            "type": "object",
            "properties": {
                "id": {"type": "integer", "description": "Memory ID"}
            },
            "required": ["id"]
        }"#,
        annotations: ToolAnnotations::read_only(),
    },
    // Content Utilities
    ToolDef {
        name: "memory_soft_trim",
        description: "Intelligently trim memory content while preserving context. Keeps the beginning (head) and end (tail) of content with an ellipsis in the middle. Useful for displaying long content in limited space while keeping important context from both ends.",
        schema: r#"{
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
        annotations: ToolAnnotations::read_only(),
    },
    ToolDef {
        name: "memory_list_compact",
        description: "List memories with compact preview instead of full content. More efficient for browsing/listing UIs. Returns only essential fields and a truncated content preview with metadata about original content length.",
        schema: r#"{
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
        annotations: ToolAnnotations::read_only(),
    },
    ToolDef {
        name: "memory_content_stats",
        description: "Get content statistics for a memory (character count, word count, line count, sentence count, paragraph count)",
        schema: r#"{
            "type": "object",
            "properties": {
                "id": {"type": "integer", "description": "Memory ID"}
            },
            "required": ["id"]
        }"#,
        annotations: ToolAnnotations::read_only(),
    },
    // Batch Operations
    ToolDef {
        name: "memory_create_batch",
        description: "Create multiple memories in a single operation. More efficient than individual creates for bulk imports.",
        schema: r#"{
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
        annotations: ToolAnnotations::mutating(),
    },
    ToolDef {
        name: "memory_delete_batch",
        description: "Delete multiple memories in a single operation.",
        schema: r#"{
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
        annotations: ToolAnnotations::destructive(),
    },
    // Tag Utilities
    ToolDef {
        name: "memory_tags",
        description: "List all tags with usage counts and most recent usage timestamps.",
        schema: r#"{
            "type": "object",
            "properties": {}
        }"#,
        annotations: ToolAnnotations::read_only(),
    },
    ToolDef {
        name: "memory_tag_hierarchy",
        description: "Get tags organized in a hierarchical tree structure. Tags with slashes are treated as paths (e.g., 'project/engram/core').",
        schema: r#"{
            "type": "object",
            "properties": {}
        }"#,
        annotations: ToolAnnotations::read_only(),
    },
    ToolDef {
        name: "memory_validate_tags",
        description: "Validate tag consistency across memories. Reports orphaned tags, unused tags, and suggested normalizations.",
        schema: r#"{
            "type": "object",
            "properties": {}
        }"#,
        annotations: ToolAnnotations::read_only(),
    },
    // Import/Export
    ToolDef {
        name: "memory_export",
        description: "Export all memories to a JSON-serializable format for backup or migration.",
        schema: r#"{
            "type": "object",
            "properties": {
                "workspace": {"type": "string", "description": "Optional: export only from specific workspace"},
                "include_embeddings": {"type": "boolean", "default": false, "description": "Include embedding vectors in export (larger file size)"}
            }
        }"#,
        annotations: ToolAnnotations::read_only(),
    },
    ToolDef {
        name: "memory_import",
        description: "Import memories from a previously exported JSON format.",
        schema: r#"{
            "type": "object",
            "properties": {
                "data": {"type": "object", "description": "The exported data object"},
                "skip_duplicates": {"type": "boolean", "default": true, "description": "Skip memories with matching content hash"}
            },
            "required": ["data"]
        }"#,
        annotations: ToolAnnotations::mutating(),
    },
    // Maintenance
    ToolDef {
        name: "memory_rebuild_embeddings",
        description: "Rebuild embeddings for all memories that are missing them. Useful after model changes or data recovery.",
        schema: r#"{
            "type": "object",
            "properties": {}
        }"#,
        annotations: ToolAnnotations::idempotent(),
    },
    ToolDef {
        name: "memory_rebuild_crossrefs",
        description: "Rebuild cross-reference links between memories. Re-analyzes all memories to find and create links.",
        schema: r#"{
            "type": "object",
            "properties": {}
        }"#,
        annotations: ToolAnnotations::idempotent(),
    },
    // Special Memory Types
    ToolDef {
        name: "memory_create_section",
        description: "Create a section memory for organizing content hierarchically. Sections can have parent sections for nested organization.",
        schema: r#"{
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
        annotations: ToolAnnotations::mutating(),
    },
    ToolDef {
        name: "memory_checkpoint",
        description: "Create a checkpoint memory marking a significant point in a session. Useful for session resumption and context restoration.",
        schema: r#"{
            "type": "object",
            "properties": {
                "session_id": {"type": "string", "description": "Session identifier"},
                "summary": {"type": "string", "description": "Summary of session state at checkpoint"},
                "context": {"type": "object", "description": "Additional context data to preserve"},
                "workspace": {"type": "string", "description": "Workspace for the checkpoint"}
            },
            "required": ["session_id", "summary"]
        }"#,
        annotations: ToolAnnotations::mutating(),
    },
    // Phase 1: Cognitive Memory Types (ENG-33)
    ToolDef {
        name: "memory_create_episodic",
        description: "Create an episodic memory representing an event with temporal context. Use for tracking when things happened and their duration.",
        schema: r#"{
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
        annotations: ToolAnnotations::mutating(),
    },
    ToolDef {
        name: "memory_create_procedural",
        description: "Create a procedural memory representing a learned pattern or workflow. Tracks success/failure to measure effectiveness.",
        schema: r#"{
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
        annotations: ToolAnnotations::mutating(),
    },
    ToolDef {
        name: "memory_get_timeline",
        description: "Query episodic memories by time range. Returns events ordered by event_time.",
        schema: r#"{
            "type": "object",
            "properties": {
                "start_time": {"type": "string", "format": "date-time", "description": "Start of time range (ISO8601)"},
                "end_time": {"type": "string", "format": "date-time", "description": "End of time range (ISO8601)"},
                "workspace": {"type": "string", "description": "Filter by workspace"},
                "tags": {"type": "array", "items": {"type": "string"}, "description": "Filter by tags"},
                "limit": {"type": "integer", "default": 50, "description": "Maximum results to return"}
            }
        }"#,
        annotations: ToolAnnotations::read_only(),
    },
    ToolDef {
        name: "memory_get_procedures",
        description: "List procedural memories (learned patterns/workflows). Optionally filter by trigger pattern.",
        schema: r#"{
            "type": "object",
            "properties": {
                "trigger_pattern": {"type": "string", "description": "Filter by trigger pattern (partial match)"},
                "workspace": {"type": "string", "description": "Filter by workspace"},
                "min_success_rate": {"type": "number", "minimum": 0, "maximum": 1, "description": "Minimum success rate (successes / (successes + failures))"},
                "limit": {"type": "integer", "default": 50, "description": "Maximum results to return"}
            }
        }"#,
        annotations: ToolAnnotations::read_only(),
    },
    ToolDef {
        name: "memory_record_procedure_outcome",
        description: "Record a success or failure for a procedural memory. Increments the corresponding counter.",
        schema: r#"{
            "type": "object",
            "properties": {
                "id": {"type": "integer", "description": "Procedural memory ID"},
                "success": {"type": "boolean", "description": "true = success, false = failure"}
            },
            "required": ["id", "success"]
        }"#,
        annotations: ToolAnnotations::mutating(),
    },
    ToolDef {
        name: "memory_boost",
        description: "Temporarily boost a memory's importance score. The boost can optionally decay over time.",
        schema: r#"{
            "type": "object",
            "properties": {
                "id": {"type": "integer", "description": "Memory ID to boost"},
                "boost_amount": {"type": "number", "default": 0.2, "description": "Amount to increase importance (0-1)"},
                "duration_seconds": {"type": "integer", "description": "Optional: duration before boost decays (omit for permanent boost)"}
            },
            "required": ["id"]
        }"#,
        annotations: ToolAnnotations::mutating(),
    },
    // Phase 2: Context Compression Engine
    ToolDef {
        name: "memory_summarize",
        description: "Create a summary of one or more memories. Returns a new Summary-type memory with summary_of_id set.",
        schema: r#"{
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
        annotations: ToolAnnotations::mutating(),
    },
    ToolDef {
        name: "memory_get_full",
        description: "Get the full/original content of a memory. If the memory is a Summary, returns the original content from summary_of_id.",
        schema: r#"{
            "type": "object",
            "properties": {
                "id": {"type": "integer", "description": "Memory ID to get full content for"}
            },
            "required": ["id"]
        }"#,
        annotations: ToolAnnotations::read_only(),
    },
    ToolDef {
        name: "context_budget_check",
        description: "Check token usage of memories against a budget. Returns token counts and suggestions if over budget.",
        schema: r#"{
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
        annotations: ToolAnnotations::read_only(),
    },
    ToolDef {
        name: "memory_archive_old",
        description: "Archive old, low-importance memories by creating summaries. Moves originals to archived state.",
        schema: r#"{
            "type": "object",
            "properties": {
                "max_age_days": {"type": "integer", "default": 90, "description": "Archive memories older than this many days"},
                "max_importance": {"type": "number", "default": 0.5, "description": "Only archive memories with importance below this"},
                "min_access_count": {"type": "integer", "default": 5, "description": "Skip memories accessed more than this many times"},
                "workspace": {"type": "string", "description": "Limit to specific workspace"},
                "dry_run": {"type": "boolean", "default": true, "description": "If true, only report what would be archived"}
            }
        }"#,
        annotations: ToolAnnotations::destructive(),
    },
    // Phase 3: Langfuse Integration (ENG-35) - feature-gated
    #[cfg(feature = "langfuse")]
    ToolDef {
        name: "langfuse_connect",
        description: "Configure Langfuse connection for observability integration. Stores config in metadata.",
        schema: r#"{
            "type": "object",
            "properties": {
                "public_key": {"type": "string", "description": "Langfuse public key (or use LANGFUSE_PUBLIC_KEY env var)"},
                "secret_key": {"type": "string", "description": "Langfuse secret key (or use LANGFUSE_SECRET_KEY env var)"},
                "base_url": {"type": "string", "default": "https://cloud.langfuse.com", "description": "Langfuse API base URL"}
            }
        }"#,
        annotations: ToolAnnotations::mutating(),
    },
    #[cfg(feature = "langfuse")]
    ToolDef {
        name: "langfuse_sync",
        description: "Start background sync from Langfuse traces to memories. Returns task_id for status checking.",
        schema: r#"{
            "type": "object",
            "properties": {
                "since": {"type": "string", "format": "date-time", "description": "Sync traces since this timestamp (default: 24h ago)"},
                "limit": {"type": "integer", "default": 100, "description": "Maximum traces to sync"},
                "workspace": {"type": "string", "description": "Workspace to create memories in"},
                "dry_run": {"type": "boolean", "default": false, "description": "Preview what would be synced without creating memories"}
            }
        }"#,
        annotations: ToolAnnotations::mutating(),
    },
    #[cfg(feature = "langfuse")]
    ToolDef {
        name: "langfuse_sync_status",
        description: "Check the status of a Langfuse sync task.",
        schema: r#"{
            "type": "object",
            "properties": {
                "task_id": {"type": "string", "description": "Task ID returned from langfuse_sync"}
            },
            "required": ["task_id"]
        }"#,
        annotations: ToolAnnotations::read_only(),
    },
    #[cfg(feature = "langfuse")]
    ToolDef {
        name: "langfuse_extract_patterns",
        description: "Extract patterns from Langfuse traces without saving. Preview mode for pattern discovery.",
        schema: r#"{
            "type": "object",
            "properties": {
                "since": {"type": "string", "format": "date-time", "description": "Analyze traces since this timestamp"},
                "limit": {"type": "integer", "default": 50, "description": "Maximum traces to analyze"},
                "min_confidence": {"type": "number", "default": 0.7, "description": "Minimum confidence for patterns"}
            }
        }"#,
        annotations: ToolAnnotations::read_only(),
    },
    #[cfg(feature = "langfuse")]
    ToolDef {
        name: "memory_from_trace",
        description: "Create a memory from a specific Langfuse trace ID.",
        schema: r#"{
            "type": "object",
            "properties": {
                "trace_id": {"type": "string", "description": "Langfuse trace ID"},
                "memory_type": {"type": "string", "enum": ["note", "episodic", "procedural", "learning"], "default": "episodic", "description": "Type of memory to create"},
                "workspace": {"type": "string", "description": "Workspace for the memory"},
                "tags": {"type": "array", "items": {"type": "string"}, "description": "Additional tags"}
            },
            "required": ["trace_id"]
        }"#,
        annotations: ToolAnnotations::mutating(),
    },
    // Phase 4: Search Result Caching (ENG-36)
    ToolDef {
        name: "search_cache_feedback",
        description: "Report feedback on search results quality. Helps tune the adaptive cache threshold.",
        schema: r#"{
            "type": "object",
            "properties": {
                "query": {"type": "string", "description": "The search query"},
                "positive": {"type": "boolean", "description": "True if results were helpful, false otherwise"},
                "workspace": {"type": "string", "description": "Workspace filter used (if any)"}
            },
            "required": ["query", "positive"]
        }"#,
        annotations: ToolAnnotations::mutating(),
    },
    ToolDef {
        name: "search_cache_stats",
        description: "Get search result cache statistics including hit rate, entry count, and current threshold.",
        schema: r#"{
            "type": "object",
            "properties": {}
        }"#,
        annotations: ToolAnnotations::read_only(),
    },
    ToolDef {
        name: "search_cache_clear",
        description: "Clear the search result cache. Useful after bulk operations.",
        schema: r#"{
            "type": "object",
            "properties": {
                "workspace": {"type": "string", "description": "Only clear cache for this workspace (optional)"}
            }
        }"#,
        annotations: ToolAnnotations::destructive(),
    },
    // Phase 5: Memory Lifecycle Management (ENG-37)
    ToolDef {
        name: "lifecycle_status",
        description: "Get lifecycle statistics (active/stale/archived counts by workspace).",
        schema: r#"{
            "type": "object",
            "properties": {
                "workspace": {"type": "string", "description": "Filter by workspace (optional)"}
            }
        }"#,
        annotations: ToolAnnotations::read_only(),
    },
    ToolDef {
        name: "lifecycle_run",
        description: "Manually trigger a lifecycle cycle (mark stale, archive old). Dry run by default.",
        schema: r#"{
            "type": "object",
            "properties": {
                "dry_run": {"type": "boolean", "default": true, "description": "Preview changes without applying"},
                "workspace": {"type": "string", "description": "Limit to specific workspace"},
                "stale_days": {"type": "integer", "default": 30, "description": "Mark memories older than this as stale"},
                "archive_days": {"type": "integer", "default": 90, "description": "Archive memories older than this"},
                "min_importance": {"type": "number", "default": 0.5, "description": "Only process memories below this importance"}
            }
        }"#,
        annotations: ToolAnnotations::idempotent(),
    },
    ToolDef {
        name: "memory_set_lifecycle",
        description: "Manually set the lifecycle state of a memory.",
        schema: r#"{
            "type": "object",
            "properties": {
                "id": {"type": "integer", "description": "Memory ID"},
                "state": {"type": "string", "enum": ["active", "stale", "archived"], "description": "New lifecycle state"}
            },
            "required": ["id", "state"]
        }"#,
        annotations: ToolAnnotations::mutating(),
    },
    ToolDef {
        name: "lifecycle_config",
        description: "Get or set lifecycle configuration (intervals, thresholds).",
        schema: r#"{
            "type": "object",
            "properties": {
                "stale_days": {"type": "integer", "description": "Days before marking as stale"},
                "archive_days": {"type": "integer", "description": "Days before auto-archiving"},
                "min_importance": {"type": "number", "description": "Importance threshold for lifecycle"},
                "min_access_count": {"type": "integer", "description": "Access count threshold"}
            }
        }"#,
        annotations: ToolAnnotations::read_only(),
    },
    // Retention Policies
    ToolDef {
        name: "retention_policy_set",
        description: "Set a retention policy for a workspace. Controls auto-compression, max memory count, and auto-deletion.",
        schema: r#"{
            "type": "object",
            "properties": {
                "workspace": {"type": "string", "description": "Workspace name"},
                "max_age_days": {"type": "integer", "description": "Hard age limit — auto-delete after this many days"},
                "max_memories": {"type": "integer", "description": "Maximum active memories in this workspace"},
                "compress_after_days": {"type": "integer", "description": "Auto-compress memories older than this"},
                "compress_max_importance": {"type": "number", "description": "Only compress memories with importance <= this (default 0.3)"},
                "compress_min_access": {"type": "integer", "description": "Skip compression if access_count >= this (default 3)"},
                "auto_delete_after_days": {"type": "integer", "description": "Auto-delete archived memories older than this"},
                "exclude_types": {"type": "array", "items": {"type": "string"}, "description": "Memory types exempt from policy (e.g. [\"decision\", \"checkpoint\"])"}
            },
            "required": ["workspace"]
        }"#,
        annotations: ToolAnnotations::mutating(),
    },
    ToolDef {
        name: "retention_policy_get",
        description: "Get the retention policy for a workspace.",
        schema: r#"{
            "type": "object",
            "properties": {
                "workspace": {"type": "string", "description": "Workspace name"}
            },
            "required": ["workspace"]
        }"#,
        annotations: ToolAnnotations::read_only(),
    },
    ToolDef {
        name: "retention_policy_list",
        description: "List all retention policies across all workspaces.",
        schema: r#"{
            "type": "object",
            "properties": {}
        }"#,
        annotations: ToolAnnotations::read_only(),
    },
    ToolDef {
        name: "retention_policy_delete",
        description: "Delete a retention policy for a workspace.",
        schema: r#"{
            "type": "object",
            "properties": {
                "workspace": {"type": "string", "description": "Workspace name"}
            },
            "required": ["workspace"]
        }"#,
        annotations: ToolAnnotations::destructive(),
    },
    ToolDef {
        name: "retention_policy_apply",
        description: "Apply all retention policies now. Compresses, caps, and deletes per workspace rules.",
        schema: r#"{
            "type": "object",
            "properties": {
                "dry_run": {"type": "boolean", "default": false, "description": "Preview what would happen without making changes"}
            }
        }"#,
        annotations: ToolAnnotations::idempotent(),
    },
    // Event System
    ToolDef {
        name: "memory_events_poll",
        description: "Poll for memory events (create, update, delete, etc.) since a given point. Useful for syncing and monitoring.",
        schema: r#"{
            "type": "object",
            "properties": {
                "since_id": {"type": "integer", "description": "Return events after this event ID"},
                "since_time": {"type": "string", "format": "date-time", "description": "Return events after this timestamp (RFC3339)"},
                "agent_id": {"type": "string", "description": "Filter events for specific agent"},
                "limit": {"type": "integer", "default": 100, "description": "Maximum events to return"}
            }
        }"#,
        annotations: ToolAnnotations::read_only(),
    },
    ToolDef {
        name: "memory_events_clear",
        description: "Clear old events from the event log. Helps manage storage for long-running systems.",
        schema: r#"{
            "type": "object",
            "properties": {
                "before_id": {"type": "integer", "description": "Delete events before this ID"},
                "before_time": {"type": "string", "format": "date-time", "description": "Delete events before this timestamp"},
                "keep_recent": {"type": "integer", "description": "Keep only the N most recent events"}
            }
        }"#,
        annotations: ToolAnnotations::destructive(),
    },
    // Advanced Sync
    ToolDef {
        name: "sync_version",
        description: "Get the current sync version and metadata. Used to check if local data is up-to-date.",
        schema: r#"{
            "type": "object",
            "properties": {}
        }"#,
        annotations: ToolAnnotations::read_only(),
    },
    ToolDef {
        name: "sync_delta",
        description: "Get changes (delta) since a specific version. Returns created, updated, and deleted memories.",
        schema: r#"{
            "type": "object",
            "properties": {
                "since_version": {"type": "integer", "description": "Version to get changes from"}
            },
            "required": ["since_version"]
        }"#,
        annotations: ToolAnnotations::read_only(),
    },
    ToolDef {
        name: "sync_state",
        description: "Get or update sync state for a specific agent. Tracks what each agent has synced.",
        schema: r#"{
            "type": "object",
            "properties": {
                "agent_id": {"type": "string", "description": "Agent identifier"},
                "update_version": {"type": "integer", "description": "If provided, updates the agent's last synced version"}
            },
            "required": ["agent_id"]
        }"#,
        annotations: ToolAnnotations::read_only(),
    },
    ToolDef {
        name: "sync_cleanup",
        description: "Clean up old sync data (events, etc.) older than specified days.",
        schema: r#"{
            "type": "object",
            "properties": {
                "older_than_days": {"type": "integer", "default": 30, "description": "Delete sync data older than this many days"}
            }
        }"#,
        annotations: ToolAnnotations::destructive(),
    },
    // Multi-Agent Sharing
    ToolDef {
        name: "memory_share",
        description: "Share a memory with another agent. The target agent can poll for shared memories.",
        schema: r#"{
            "type": "object",
            "properties": {
                "memory_id": {"type": "integer", "description": "ID of memory to share"},
                "from_agent": {"type": "string", "description": "Sender agent identifier"},
                "to_agent": {"type": "string", "description": "Recipient agent identifier"},
                "message": {"type": "string", "description": "Optional message to include with share"}
            },
            "required": ["memory_id", "from_agent", "to_agent"]
        }"#,
        annotations: ToolAnnotations::mutating(),
    },
    ToolDef {
        name: "memory_shared_poll",
        description: "Poll for memories shared with this agent.",
        schema: r#"{
            "type": "object",
            "properties": {
                "agent_id": {"type": "string", "description": "Agent identifier to check shares for"},
                "include_acknowledged": {"type": "boolean", "default": false, "description": "Include already acknowledged shares"}
            },
            "required": ["agent_id"]
        }"#,
        annotations: ToolAnnotations::read_only(),
    },
    ToolDef {
        name: "memory_share_ack",
        description: "Acknowledge receipt of a shared memory.",
        schema: r#"{
            "type": "object",
            "properties": {
                "share_id": {"type": "integer", "description": "Share ID to acknowledge"},
                "agent_id": {"type": "string", "description": "Agent acknowledging the share"}
            },
            "required": ["share_id", "agent_id"]
        }"#,
        annotations: ToolAnnotations::mutating(),
    },
    // Scope-based access grants for multi-agent memory sharing
    ToolDef {
        name: "memory_grant_access",
        description: "Grant an agent access to a scope path. Supports read, write, and admin permissions. Access also applies to all descendant scopes.",
        schema: r#"{
            "type": "object",
            "properties": {
                "agent_id": {"type": "string", "description": "Agent ID to grant access to"},
                "scope_path": {"type": "string", "description": "Scope path to grant access to (e.g. 'global/org:acme')"},
                "permissions": {"type": "string", "enum": ["read", "write", "admin"], "default": "read", "description": "Permission level"},
                "granted_by": {"type": "string", "description": "Optional: ID of the granting agent"}
            },
            "required": ["agent_id", "scope_path"]
        }"#,
        annotations: ToolAnnotations::mutating(),
    },
    ToolDef {
        name: "memory_revoke_access",
        description: "Revoke an agent's access to a specific scope path.",
        schema: r#"{
            "type": "object",
            "properties": {
                "agent_id": {"type": "string", "description": "Agent ID to revoke access from"},
                "scope_path": {"type": "string", "description": "Scope path to revoke access from"}
            },
            "required": ["agent_id", "scope_path"]
        }"#,
        annotations: ToolAnnotations::destructive(),
    },
    ToolDef {
        name: "memory_list_grants",
        description: "List all scope access grants for a given agent.",
        schema: r#"{
            "type": "object",
            "properties": {
                "agent_id": {"type": "string", "description": "Agent ID to list grants for"}
            },
            "required": ["agent_id"]
        }"#,
        annotations: ToolAnnotations::read_only(),
    },
    ToolDef {
        name: "memory_check_access",
        description: "Check whether an agent has a required permission level on a scope path (including ancestor grants).",
        schema: r#"{
            "type": "object",
            "properties": {
                "agent_id": {"type": "string", "description": "Agent ID to check"},
                "scope_path": {"type": "string", "description": "Scope path to check access for"},
                "permissions": {"type": "string", "enum": ["read", "write", "admin"], "default": "read", "description": "Required permission level"}
            },
            "required": ["agent_id", "scope_path"]
        }"#,
        annotations: ToolAnnotations::read_only(),
    },
    // Search Variants
    ToolDef {
        name: "memory_search_by_identity",
        description: "Search memories by identity (person, entity, or alias). Finds all mentions of a specific identity across memories.",
        schema: r#"{
            "type": "object",
            "properties": {
                "identity": {"type": "string", "description": "Identity name or alias to search for"},
                "workspace": {"type": "string", "description": "Optional: limit search to specific workspace"},
                "limit": {"type": "integer", "default": 50, "description": "Maximum results to return"}
            },
            "required": ["identity"]
        }"#,
        annotations: ToolAnnotations::mutating(),
    },
    ToolDef {
        name: "memory_session_search",
        description: "Search within session transcript chunks. Useful for finding content from past conversations.",
        schema: r#"{
            "type": "object",
            "properties": {
                "query": {"type": "string", "description": "Search query"},
                "session_id": {"type": "string", "description": "Optional: limit to specific session"},
                "workspace": {"type": "string", "description": "Optional: limit to specific workspace"},
                "limit": {"type": "integer", "default": 20, "description": "Maximum results to return"}
            },
            "required": ["query"]
        }"#,
        annotations: ToolAnnotations::mutating(),
    },
    // Image Handling
    ToolDef {
        name: "memory_upload_image",
        description: "Upload an image file and attach it to a memory. The image will be stored locally and linked to the memory's metadata.",
        schema: r#"{
            "type": "object",
            "properties": {
                "memory_id": {"type": "integer", "description": "ID of the memory to attach the image to"},
                "file_path": {"type": "string", "description": "Path to the image file to upload"},
                "image_index": {"type": "integer", "default": 0, "description": "Index for ordering multiple images (0-based)"},
                "caption": {"type": "string", "description": "Optional caption for the image"}
            },
            "required": ["memory_id", "file_path"]
        }"#,
        annotations: ToolAnnotations::mutating(),
    },
    ToolDef {
        name: "memory_migrate_images",
        description: "Migrate existing base64-encoded images in memories to file storage. Scans all memories and uploads any embedded data URIs to storage, replacing them with file references.",
        schema: r#"{
            "type": "object",
            "properties": {
                "dry_run": {"type": "boolean", "default": false, "description": "If true, only report what would be migrated without making changes"}
            }
        }"#,
        annotations: ToolAnnotations::idempotent(),
    },
    // Auto-Tagging
    ToolDef {
        name: "memory_suggest_tags",
        description: "Suggest tags for a memory based on AI content analysis. Uses pattern matching, keyword extraction, and structure detection to suggest relevant tags with confidence scores.",
        schema: r#"{
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
        annotations: ToolAnnotations::read_only(),
    },
    ToolDef {
        name: "memory_auto_tag",
        description: "Automatically suggest and optionally apply tags to a memory. Analyzes content using AI heuristics and can merge suggested tags with existing ones.",
        schema: r#"{
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
        annotations: ToolAnnotations::mutating(),
    },
    // Phase 8: Salience & Sessions (ENG-66 to ENG-77)
    ToolDef {
        name: "salience_get",
        description: "Get the salience score for a memory. Returns recency, frequency, importance, and feedback components with the combined score and lifecycle state.",
        schema: r#"{
            "type": "object",
            "properties": {
                "id": {"type": "integer", "description": "Memory ID to get salience for"},
                "feedback_signal": {"type": "number", "minimum": -1, "maximum": 1, "default": 0, "description": "Optional feedback signal (-1 to 1) to include in calculation"}
            },
            "required": ["id"]
        }"#,
        annotations: ToolAnnotations::read_only(),
    },
    ToolDef {
        name: "salience_set_importance",
        description: "Set the importance score for a memory. This is the static importance component of salience.",
        schema: r#"{
            "type": "object",
            "properties": {
                "id": {"type": "integer", "description": "Memory ID"},
                "importance": {"type": "number", "minimum": 0, "maximum": 1, "description": "Importance score (0-1)"}
            },
            "required": ["id", "importance"]
        }"#,
        annotations: ToolAnnotations::mutating(),
    },
    ToolDef {
        name: "salience_boost",
        description: "Boost a memory's salience score temporarily or permanently. Useful for marking memories as contextually relevant.",
        schema: r#"{
            "type": "object",
            "properties": {
                "id": {"type": "integer", "description": "Memory ID to boost"},
                "boost_amount": {"type": "number", "minimum": 0, "maximum": 1, "default": 0.2, "description": "Amount to boost (0-1)"},
                "reason": {"type": "string", "description": "Optional reason for boosting"}
            },
            "required": ["id"]
        }"#,
        annotations: ToolAnnotations::mutating(),
    },
    ToolDef {
        name: "salience_demote",
        description: "Demote a memory's salience score. Useful for marking memories as less relevant.",
        schema: r#"{
            "type": "object",
            "properties": {
                "id": {"type": "integer", "description": "Memory ID to demote"},
                "demote_amount": {"type": "number", "minimum": 0, "maximum": 1, "default": 0.2, "description": "Amount to demote (0-1)"},
                "reason": {"type": "string", "description": "Optional reason for demoting"}
            },
            "required": ["id"]
        }"#,
        annotations: ToolAnnotations::mutating(),
    },
    ToolDef {
        name: "salience_decay_run",
        description: "Run temporal decay on all memories. Updates lifecycle states (Active → Stale → Archived) based on salience scores.",
        schema: r#"{
            "type": "object",
            "properties": {
                "dry_run": {"type": "boolean", "default": false, "description": "If true, compute changes without persisting updates"},
                "record_history": {"type": "boolean", "default": true, "description": "Record salience history entries while updating"},
                "workspace": {"type": "string", "description": "Limit to specific workspace"},
                "stale_threshold_days": {"type": "integer", "minimum": 1, "description": "Days of inactivity before marking stale"},
                "archive_threshold_days": {"type": "integer", "minimum": 1, "description": "Days of inactivity before suggesting archive"}
            }
        }"#,
        annotations: ToolAnnotations::destructive(),
    },
    ToolDef {
        name: "salience_stats",
        description: "Get salience statistics across all memories. Returns distribution, percentiles, and state counts.",
        schema: r#"{
            "type": "object",
            "properties": {
                "workspace": {"type": "string", "description": "Limit to specific workspace"}
            }
        }"#,
        annotations: ToolAnnotations::read_only(),
    },
    ToolDef {
        name: "salience_history",
        description: "Get salience score history for a memory. Shows how salience has changed over time.",
        schema: r#"{
            "type": "object",
            "properties": {
                "id": {"type": "integer", "description": "Memory ID"},
                "limit": {"type": "integer", "default": 50, "description": "Maximum history entries to return"}
            },
            "required": ["id"]
        }"#,
        annotations: ToolAnnotations::read_only(),
    },
    ToolDef {
        name: "salience_top",
        description: "Get top memories by salience score. Useful for context injection.",
        schema: r#"{
            "type": "object",
            "properties": {
                "limit": {"type": "integer", "default": 20, "description": "Maximum memories to return"},
                "workspace": {"type": "string", "description": "Limit to specific workspace"},
                "min_score": {"type": "number", "minimum": 0, "maximum": 1, "description": "Minimum salience score"},
                "memory_type": {"type": "string", "description": "Filter by memory type"}
            }
        }"#,
        annotations: ToolAnnotations::read_only(),
    },
    // Session Context Tools (ENG-70, ENG-71)
    ToolDef {
        name: "session_context_create",
        description: "Create a new session context for tracking related memories during a conversation or task.",
        schema: r#"{
            "type": "object",
            "properties": {
                "name": {"type": "string", "description": "Session name"},
                "description": {"type": "string", "description": "Session description"},
                "workspace": {"type": "string", "description": "Workspace for the session"},
                "metadata": {"type": "object", "description": "Additional session metadata"}
            },
            "required": ["name"]
        }"#,
        annotations: ToolAnnotations::mutating(),
    },
    ToolDef {
        name: "session_context_add_memory",
        description: "Add a memory to a session context with relevance score and role.",
        schema: r#"{
            "type": "object",
            "properties": {
                "session_id": {"type": "string", "description": "Session ID"},
                "memory_id": {"type": "integer", "description": "Memory ID to add"},
                "relevance_score": {"type": "number", "minimum": 0, "maximum": 1, "default": 1.0, "description": "How relevant this memory is to the session"},
                "context_role": {"type": "string", "enum": ["referenced", "created", "updated", "pinned"], "default": "referenced", "description": "Role of the memory in the session"}
            },
            "required": ["session_id", "memory_id"]
        }"#,
        annotations: ToolAnnotations::mutating(),
    },
    ToolDef {
        name: "session_context_remove_memory",
        description: "Remove a memory from a session context.",
        schema: r#"{
            "type": "object",
            "properties": {
                "session_id": {"type": "string", "description": "Session ID"},
                "memory_id": {"type": "integer", "description": "Memory ID to remove"}
            },
            "required": ["session_id", "memory_id"]
        }"#,
        annotations: ToolAnnotations::mutating(),
    },
    ToolDef {
        name: "session_context_get",
        description: "Get a session context with its linked memories.",
        schema: r#"{
            "type": "object",
            "properties": {
                "session_id": {"type": "string", "description": "Session ID"}
            },
            "required": ["session_id"]
        }"#,
        annotations: ToolAnnotations::read_only(),
    },
    ToolDef {
        name: "session_context_list",
        description: "List all session contexts with optional filtering.",
        schema: r#"{
            "type": "object",
            "properties": {
                "workspace": {"type": "string", "description": "Filter by workspace"},
                "active_only": {"type": "boolean", "default": false, "description": "Only return active sessions"},
                "limit": {"type": "integer", "default": 50, "description": "Maximum sessions to return"},
                "offset": {"type": "integer", "default": 0, "description": "Offset for pagination"}
            }
        }"#,
        annotations: ToolAnnotations::read_only(),
    },
    ToolDef {
        name: "session_context_search",
        description: "Search memories within a specific session context.",
        schema: r#"{
            "type": "object",
            "properties": {
                "session_id": {"type": "string", "description": "Session ID to search within"},
                "query": {"type": "string", "description": "Search query"},
                "limit": {"type": "integer", "default": 20, "description": "Maximum results"}
            },
            "required": ["session_id", "query"]
        }"#,
        annotations: ToolAnnotations::read_only(),
    },
    ToolDef {
        name: "session_context_update_summary",
        description: "Update the summary of a session context.",
        schema: r#"{
            "type": "object",
            "properties": {
                "session_id": {"type": "string", "description": "Session ID"},
                "summary": {"type": "string", "description": "New session summary"}
            },
            "required": ["session_id", "summary"]
        }"#,
        annotations: ToolAnnotations::mutating(),
    },
    ToolDef {
        name: "session_context_end",
        description: "End a session context, marking it as inactive.",
        schema: r#"{
            "type": "object",
            "properties": {
                "session_id": {"type": "string", "description": "Session ID to end"},
                "summary": {"type": "string", "description": "Optional final summary"}
            },
            "required": ["session_id"]
        }"#,
        annotations: ToolAnnotations::mutating(),
    },
    ToolDef {
        name: "session_context_export",
        description: "Export a session context with all its memories for archival or sharing.",
        schema: r#"{
            "type": "object",
            "properties": {
                "session_id": {"type": "string", "description": "Session ID to export"},
                "include_content": {"type": "boolean", "default": true, "description": "Include full memory content"},
                "format": {"type": "string", "enum": ["json", "markdown"], "default": "json", "description": "Export format"}
            },
            "required": ["session_id"]
        }"#,
        annotations: ToolAnnotations::read_only(),
    },
    // Phase 9: Context Quality (ENG-48 to ENG-66)
    ToolDef {
        name: "quality_score",
        description: "Get the quality score for a memory with detailed breakdown of clarity, completeness, freshness, consistency, and source trust components.",
        schema: r#"{
            "type": "object",
            "properties": {
                "id": {"type": "integer", "description": "Memory ID to score"}
            },
            "required": ["id"]
        }"#,
        annotations: ToolAnnotations::read_only(),
    },
    ToolDef {
        name: "quality_report",
        description: "Generate a comprehensive quality report for a workspace. Includes quality distribution, top issues, conflict and duplicate counts.",
        schema: r#"{
            "type": "object",
            "properties": {
                "workspace": {"type": "string", "description": "Workspace to analyze (default: 'default')"}
            }
        }"#,
        annotations: ToolAnnotations::read_only(),
    },
    ToolDef {
        name: "quality_find_duplicates",
        description: "Find near-duplicate memories using text similarity. Returns pairs of similar memories above the threshold.",
        schema: r#"{
            "type": "object",
            "properties": {
                "threshold": {"type": "number", "minimum": 0, "maximum": 1, "default": 0.85, "description": "Similarity threshold (0-1)"},
                "limit": {"type": "integer", "default": 100, "description": "Maximum memories to compare"}
            }
        }"#,
        annotations: ToolAnnotations::read_only(),
    },
    ToolDef {
        name: "quality_get_duplicates",
        description: "Get pending duplicate candidates that need review.",
        schema: r#"{
            "type": "object",
            "properties": {
                "limit": {"type": "integer", "default": 50, "description": "Maximum duplicates to return"}
            }
        }"#,
        annotations: ToolAnnotations::read_only(),
    },
    ToolDef {
        name: "quality_find_conflicts",
        description: "Detect conflicts for a memory against existing memories. Finds contradictions, staleness, and semantic overlaps.",
        schema: r#"{
            "type": "object",
            "properties": {
                "id": {"type": "integer", "description": "Memory ID to check for conflicts"}
            },
            "required": ["id"]
        }"#,
        annotations: ToolAnnotations::read_only(),
    },
    ToolDef {
        name: "quality_get_conflicts",
        description: "Get unresolved conflicts that need attention.",
        schema: r#"{
            "type": "object",
            "properties": {
                "limit": {"type": "integer", "default": 50, "description": "Maximum conflicts to return"}
            }
        }"#,
        annotations: ToolAnnotations::read_only(),
    },
    ToolDef {
        name: "quality_resolve_conflict",
        description: "Resolve a conflict between memories. Options: keep_a, keep_b, merge, keep_both, delete_both, false_positive.",
        schema: r#"{
            "type": "object",
            "properties": {
                "conflict_id": {"type": "integer", "description": "Conflict ID to resolve"},
                "resolution": {"type": "string", "enum": ["keep_a", "keep_b", "merge", "keep_both", "delete_both", "false_positive"], "description": "How to resolve the conflict"},
                "notes": {"type": "string", "description": "Optional notes about the resolution"}
            },
            "required": ["conflict_id", "resolution"]
        }"#,
        annotations: ToolAnnotations::destructive(),
    },
    ToolDef {
        name: "quality_source_trust",
        description: "Get or update trust score for a source type. Higher trust means memories from this source are weighted more in quality calculations.",
        schema: r#"{
            "type": "object",
            "properties": {
                "source_type": {"type": "string", "description": "Source type (user, seed, extraction, inference, external)"},
                "source_identifier": {"type": "string", "description": "Optional specific source identifier"},
                "trust_score": {"type": "number", "minimum": 0, "maximum": 1, "description": "New trust score (omit to just get current score)"},
                "notes": {"type": "string", "description": "Notes about this source"}
            },
            "required": ["source_type"]
        }"#,
        annotations: ToolAnnotations::read_only(),
    },
    ToolDef {
        name: "quality_improve",
        description: "Get suggestions for improving a memory's quality. Returns actionable recommendations.",
        schema: r#"{
            "type": "object",
            "properties": {
                "id": {"type": "integer", "description": "Memory ID to analyze"}
            },
            "required": ["id"]
        }"#,
        annotations: ToolAnnotations::mutating(),
    },
    // Phase 7: Meilisearch Integration (ENG-58) - feature-gated
    #[cfg(feature = "meilisearch")]
    ToolDef {
        name: "meilisearch_search",
        description: "Search memories using Meilisearch (typo-tolerant, fast full-text). Requires Meilisearch to be configured. Falls back to hybrid search if unavailable.",
        schema: r#"{
            "type": "object",
            "properties": {
                "query": {"type": "string", "description": "Search query text"},
                "limit": {"type": "integer", "default": 20, "description": "Maximum results to return"},
                "offset": {"type": "integer", "default": 0, "description": "Number of results to skip"},
                "workspace": {"type": "string", "description": "Filter by workspace"},
                "tags": {"type": "array", "items": {"type": "string"}, "description": "Filter by tags (AND logic)"},
                "memory_type": {"type": "string", "description": "Filter by memory type"}
            },
            "required": ["query"]
        }"#,
        annotations: ToolAnnotations::read_only(),
    },
    #[cfg(feature = "meilisearch")]
    ToolDef {
        name: "meilisearch_reindex",
        description: "Trigger a full re-sync from SQLite to Meilisearch. Use after bulk imports or if the index is out of sync.",
        schema: r#"{
            "type": "object",
            "properties": {}
        }"#,
        annotations: ToolAnnotations::idempotent(),
    },
    #[cfg(feature = "meilisearch")]
    ToolDef {
        name: "meilisearch_status",
        description: "Get Meilisearch index status including document count, indexing state, and health.",
        schema: r#"{
            "type": "object",
            "properties": {}
        }"#,
        annotations: ToolAnnotations::read_only(),
    },
    #[cfg(feature = "meilisearch")]
    ToolDef {
        name: "meilisearch_config",
        description: "Show current Meilisearch configuration (URL, sync interval, enabled status).",
        schema: r#"{
            "type": "object",
            "properties": {}
        }"#,
        annotations: ToolAnnotations::read_only(),
    },
    // ── Agent Registry ────────────────────────────────────────────────────
    ToolDef {
        name: "agent_register",
        description: "Register an AI agent with capabilities and namespace isolation. Upserts if agent_id already exists.",
        schema: r#"{
            "type": "object",
            "properties": {
                "agent_id": {"type": "string", "description": "Unique identifier for the agent"},
                "display_name": {"type": "string", "description": "Human-readable name (defaults to agent_id)"},
                "capabilities": {"type": "array", "items": {"type": "string"}, "description": "List of capabilities (e.g., 'search', 'create', 'analyze')"},
                "namespaces": {"type": "array", "items": {"type": "string"}, "description": "Namespaces the agent operates in (default: ['default'])"},
                "metadata": {"type": "object", "description": "Additional metadata as key-value pairs"}
            },
            "required": ["agent_id"]
        }"#,
        annotations: ToolAnnotations::mutating(),
    },
    ToolDef {
        name: "agent_deregister",
        description: "Deregister an AI agent (soft delete — sets status to 'inactive').",
        schema: r#"{
            "type": "object",
            "properties": {
                "agent_id": {"type": "string", "description": "ID of the agent to deregister"}
            },
            "required": ["agent_id"]
        }"#,
        annotations: ToolAnnotations::destructive(),
    },
    ToolDef {
        name: "agent_heartbeat",
        description: "Update an agent's heartbeat timestamp to indicate it is still alive.",
        schema: r#"{
            "type": "object",
            "properties": {
                "agent_id": {"type": "string", "description": "ID of the agent sending heartbeat"}
            },
            "required": ["agent_id"]
        }"#,
        annotations: ToolAnnotations::mutating(),
    },
    ToolDef {
        name: "agent_list",
        description: "List registered agents, optionally filtered by status or namespace.",
        schema: r#"{
            "type": "object",
            "properties": {
                "status": {"type": "string", "enum": ["active", "inactive"], "description": "Filter by agent status"},
                "namespace": {"type": "string", "description": "Filter by namespace (returns agents that include this namespace)"}
            }
        }"#,
        annotations: ToolAnnotations::read_only(),
    },
    ToolDef {
        name: "agent_get",
        description: "Get details of a specific registered agent by ID.",
        schema: r#"{
            "type": "object",
            "properties": {
                "agent_id": {"type": "string", "description": "ID of the agent to retrieve"}
            },
            "required": ["agent_id"]
        }"#,
        annotations: ToolAnnotations::read_only(),
    },
    ToolDef {
        name: "agent_capabilities",
        description: "Update the capabilities list of a registered agent.",
        schema: r#"{
            "type": "object",
            "properties": {
                "agent_id": {"type": "string", "description": "ID of the agent to update"},
                "capabilities": {"type": "array", "items": {"type": "string"}, "description": "New capabilities list (replaces existing)"}
            },
            "required": ["agent_id", "capabilities"]
        }"#,
        annotations: ToolAnnotations::mutating(),
    },

    // ── Snapshot Tools (agent-portability) ────────────────────────────────────
    #[cfg(feature = "agent-portability")]
    ToolDef {
        name: "snapshot_create",
        description: "Create a portable .egm snapshot of memories filtered by workspace, tags, date range, or importance. Optionally encrypt with AES-256-GCM or sign with Ed25519.",
        schema: r#"{
            "type": "object",
            "properties": {
                "output_path": {"type": "string", "description": "File path for the .egm snapshot"},
                "workspace": {"type": "string", "description": "Filter by workspace"},
                "tags": {"type": "array", "items": {"type": "string"}, "description": "Filter by tags"},
                "importance_min": {"type": "number", "description": "Minimum importance score"},
                "memory_types": {"type": "array", "items": {"type": "string"}, "description": "Filter by memory types"},
                "description": {"type": "string", "description": "Human-readable description"},
                "creator": {"type": "string", "description": "Creator name"},
                "encrypt_key": {"type": "string", "description": "Hex-encoded 32-byte AES key"},
                "sign_key": {"type": "string", "description": "Hex-encoded 32-byte Ed25519 secret key"}
            },
            "required": ["output_path"]
        }"#,
        annotations: ToolAnnotations::mutating(),
    },
    #[cfg(feature = "agent-portability")]
    ToolDef {
        name: "snapshot_load",
        description: "Load a .egm snapshot into the memory store. Strategies: merge (skip duplicates), replace (clear workspace first), isolate (new workspace), dry_run (preview only).",
        schema: r#"{
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "Path to .egm file"},
                "strategy": {"type": "string", "enum": ["merge", "replace", "isolate", "dry_run"], "description": "Load strategy"},
                "target_workspace": {"type": "string", "description": "Target workspace (defaults to snapshot's workspace)"},
                "decrypt_key": {"type": "string", "description": "Hex-encoded 32-byte AES key for encrypted snapshots"}
            },
            "required": ["path", "strategy"]
        }"#,
        annotations: ToolAnnotations::mutating(),
    },
    #[cfg(feature = "agent-portability")]
    ToolDef {
        name: "snapshot_inspect",
        description: "Inspect a .egm snapshot without loading it. Returns manifest, file list, and size.",
        schema: r#"{
            "type": "object",
            "properties": {
                "path": {"type": "string", "description": "Path to .egm file"}
            },
            "required": ["path"]
        }"#,
        annotations: ToolAnnotations::read_only(),
    },

    // ── Attestation Tools (agent-portability) ──────────────────────────────────
    #[cfg(feature = "agent-portability")]
    ToolDef {
        name: "attestation_log",
        description: "Log a document ingestion with cryptographic attestation. Creates a chained record proving the document was processed.",
        schema: r#"{
            "type": "object",
            "properties": {
                "content": {"type": "string", "description": "Document content to attest"},
                "document_name": {"type": "string", "description": "Name of the document"},
                "agent_id": {"type": "string", "description": "ID of the attesting agent"},
                "memory_ids": {"type": "array", "items": {"type": "integer"}, "description": "IDs of memories created from this document"},
                "sign_key": {"type": "string", "description": "Hex-encoded 32-byte Ed25519 secret key"}
            },
            "required": ["content", "document_name"]
        }"#,
        annotations: ToolAnnotations::mutating(),
    },
    #[cfg(feature = "agent-portability")]
    ToolDef {
        name: "attestation_verify",
        description: "Verify whether a document has been attested (ingested and recorded).",
        schema: r#"{
            "type": "object",
            "properties": {
                "content": {"type": "string", "description": "Document content to verify"}
            },
            "required": ["content"]
        }"#,
        annotations: ToolAnnotations::read_only(),
    },
    #[cfg(feature = "agent-portability")]
    ToolDef {
        name: "attestation_chain_verify",
        description: "Verify the integrity of the entire attestation chain. Returns valid, broken (with location), or empty.",
        schema: r#"{
            "type": "object",
            "properties": {}
        }"#,
        annotations: ToolAnnotations::read_only(),
    },
    #[cfg(feature = "agent-portability")]
    ToolDef {
        name: "attestation_list",
        description: "List attestation records with optional filters. Supports JSON, CSV, and Merkle proof export formats.",
        schema: r#"{
            "type": "object",
            "properties": {
                "limit": {"type": "integer", "description": "Maximum records to return", "default": 50},
                "offset": {"type": "integer", "description": "Number of records to skip", "default": 0},
                "agent_id": {"type": "string", "description": "Filter by agent ID"},
                "document_name": {"type": "string", "description": "Filter by document name"},
                "export_format": {"type": "string", "enum": ["json", "csv", "merkle_proof"], "description": "Export format"}
            }
        }"#,
        annotations: ToolAnnotations::read_only(),
    },
];

/// Get all tool definitions as ToolDefinition structs
pub fn get_tool_definitions() -> Vec<ToolDefinition> {
    TOOL_DEFINITIONS
        .iter()
        .map(|def| ToolDefinition {
            name: def.name.to_string(),
            description: def.description.to_string(),
            input_schema: serde_json::from_str(def.schema).unwrap_or(json!({})),
            annotations: Some(def.annotations.clone()),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_definitions_all_parseable() {
        let tools = get_tool_definitions();
        assert!(!tools.is_empty(), "TOOL_DEFINITIONS must not be empty");
        for tool in &tools {
            assert!(!tool.name.is_empty(), "tool name must not be empty");
            assert!(
                !tool.description.is_empty(),
                "tool description must not be empty"
            );
            assert!(
                tool.input_schema.is_object(),
                "tool '{}' schema must be a JSON object",
                tool.name
            );
        }
    }

    #[test]
    fn test_read_only_tools_have_annotation() {
        let tools = get_tool_definitions();
        let read_only_names = ["memory_get", "memory_list", "memory_search", "memory_stats"];
        for name in read_only_names {
            let tool = tools
                .iter()
                .find(|t| t.name == name)
                .unwrap_or_else(|| panic!("tool '{}' not found", name));
            let ann = tool
                .annotations
                .as_ref()
                .expect("annotations must be present");
            assert_eq!(
                ann.read_only_hint,
                Some(true),
                "tool '{}' should have readOnlyHint=true",
                name
            );
        }
    }

    #[test]
    fn test_destructive_tools_have_annotation() {
        let tools = get_tool_definitions();
        let destructive_names = [
            "memory_delete",
            "memory_cleanup_expired",
            "embedding_cache_clear",
        ];
        for name in destructive_names {
            let tool = tools
                .iter()
                .find(|t| t.name == name)
                .unwrap_or_else(|| panic!("tool '{}' not found", name));
            let ann = tool
                .annotations
                .as_ref()
                .expect("annotations must be present");
            assert_eq!(
                ann.destructive_hint,
                Some(true),
                "tool '{}' should have destructiveHint=true",
                name
            );
        }
    }

    #[test]
    fn test_idempotent_tools_have_annotation() {
        let tools = get_tool_definitions();
        let idempotent_names = [
            "memory_extract_entities",
            "memory_rebuild_embeddings",
            "memory_rebuild_crossrefs",
            "lifecycle_run",
            "retention_policy_apply",
        ];
        for name in idempotent_names {
            let tool = tools
                .iter()
                .find(|t| t.name == name)
                .unwrap_or_else(|| panic!("tool '{}' not found", name));
            let ann = tool
                .annotations
                .as_ref()
                .expect("annotations must be present");
            assert_eq!(
                ann.idempotent_hint,
                Some(true),
                "tool '{}' should have idempotentHint=true",
                name
            );
        }
    }

    #[test]
    fn test_annotations_serialize_with_camel_case_keys() {
        let tools = get_tool_definitions();
        let memory_get = tools.iter().find(|t| t.name == "memory_get").unwrap();
        let json = serde_json::to_string(memory_get).expect("serialization must succeed");
        assert!(
            json.contains("readOnlyHint"),
            "should serialize as readOnlyHint"
        );
        assert!(
            !json.contains("read_only_hint"),
            "must not use snake_case key"
        );
    }

    #[test]
    fn test_mutating_tool_has_no_hints() {
        let tools = get_tool_definitions();
        let memory_create = tools.iter().find(|t| t.name == "memory_create").unwrap();
        let ann = memory_create
            .annotations
            .as_ref()
            .expect("annotations must be present");
        assert!(ann.read_only_hint.is_none());
        assert!(ann.destructive_hint.is_none());
        assert!(ann.idempotent_hint.is_none());
        // Serialized form should omit None fields
        let json = serde_json::to_string(ann).expect("serialization must succeed");
        // mutating annotations serialize as empty object since all fields are None
        assert_eq!(json, "{}");
    }
}
