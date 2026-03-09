/**
 * Engram Cloud TypeScript SDK
 *
 * Usage:
 *   import { EngramClient } from "@engram/client";
 *
 *   const client = new EngramClient({
 *     baseUrl: "https://your-engram-cloud.fly.dev",
 *     apiKey: "ek_...",
 *     tenant: "my-tenant",
 *   });
 *
 *   const memory = await client.create("User prefers dark mode");
 *   const results = await client.search("user preferences");
 */

export interface EngramConfig {
  baseUrl: string;
  apiKey: string;
  tenant: string;
  timeout?: number;
}

export interface CreateOptions {
  memoryType?: string;
  tags?: string[];
  workspace?: string;
  metadata?: Record<string, unknown>;
  importance?: number;
}

export interface ListOptions {
  limit?: number;
  offset?: number;
  workspace?: string;
  memoryType?: string;
  tags?: string[];
}

export interface SearchOptions {
  limit?: number;
  workspace?: string;
}

export interface UpdateOptions {
  content?: string;
  tags?: string[];
  metadata?: Record<string, unknown>;
  importance?: number;
}

// -- Compression --

export interface CompressForContextOptions {
  memoryIds: number[];
  tokenBudget: number;
}

export interface ConsolidateOptions {
  threshold?: number;
}

// -- Agentic Evolution --

export interface UtilityScoreOptions {
  signal?: string;
}

export interface SentimentTimelineOptions {
  workspace?: string;
  limit?: number;
}

// -- Advanced Graph --

export interface CoactivationReportOptions {
  workspace?: string;
  limit?: number;
}

export interface QueryTripletsOptions {
  subject?: string;
  predicate?: string;
  object?: string;
}

export interface AddKnowledgeOptions {
  confidence?: number;
}

// -- Autonomous Agent --

export interface AgentStartOptions {
  workspace?: string;
}

export interface GardenOptions {
  workspace?: string;
  dryRun?: boolean;
}

export interface GardenPreviewOptions {
  workspace?: string;
}

export interface SuggestAcquisitionOptions {
  workspace?: string;
}

export interface ProactiveScanOptions {
  workspace?: string;
}

// -- Retrieval Excellence --

export interface CacheClearOptions {
  workspace?: string;
}

export interface EmbeddingMigrateOptions {
  fromProvider?: string;
  toProvider?: string;
}

export interface FeedbackStatsOptions {
  workspace?: string;
}

// -- Context Engineering --

export interface ListFactsOptions {
  memoryId?: number;
  workspace?: string;
  limit?: number;
}

export interface FactGraphOptions {
  workspace?: string;
}

export interface BuildContextOptions {
  strategy?: string;
  tokenBudget?: number;
  workspace?: string;
}

export interface PromptTemplateOptions {
  memories?: unknown[];
}

export interface BlockGetOptions {
  workspace?: string;
}

export interface BlockEditOptions {
  workspace?: string;
  reason?: string;
}

export interface BlockListOptions {
  blockType?: string;
  workspace?: string;
}

export interface BlockCreateOptions {
  workspace?: string;
  maxTokens?: number;
}

// -- Temporal Graph --

export interface TemporalCreateOptions {
  validFrom?: string;
  confidence?: number;
}

export interface TemporalInvalidateOptions {
  reason?: string;
}

export interface TemporalSnapshotOptions {
  timestamp?: string;
  workspace?: string;
}

export interface TemporalContradictionsOptions {
  workspace?: string;
}

export interface ScopeListOptions {
  recursive?: boolean;
}

export class EngramError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "EngramError";
  }
}

export class EngramClient {
  private baseUrl: string;
  private headers: Record<string, string>;
  private timeout: number;

  constructor(config: EngramConfig) {
    this.baseUrl = config.baseUrl.replace(/\/$/, "");
    this.timeout = config.timeout ?? 30000;
    this.headers = {
      Authorization: `Bearer ${config.apiKey}`,
      "X-Tenant-Slug": config.tenant,
      "Content-Type": "application/json",
    };
  }

  private async mcpCall(
    method: string,
    params: Record<string, unknown> = {}
  ): Promise<unknown> {
    const payload = {
      jsonrpc: "2.0",
      id: 1,
      method: "tools/call",
      params: { name: method, arguments: params },
    };

    const controller = new AbortController();
    const timer = setTimeout(() => controller.abort(), this.timeout);

    try {
      const resp = await fetch(`${this.baseUrl}/v1/mcp`, {
        method: "POST",
        headers: this.headers,
        body: JSON.stringify(payload),
        signal: controller.signal,
      });

      if (!resp.ok) {
        throw new EngramError(`HTTP ${resp.status}: ${resp.statusText}`);
      }

      const result = (await resp.json()) as {
        error?: { message?: string };
        result?: unknown;
      };

      if (result.error) {
        throw new EngramError(result.error.message ?? "Unknown error");
      }

      return result.result;
    } finally {
      clearTimeout(timer);
    }
  }

  // -- Memory CRUD --

  async create(content: string, options?: CreateOptions): Promise<unknown> {
    const params: Record<string, unknown> = {
      content,
      memory_type: options?.memoryType ?? "note",
    };
    if (options?.tags) params.tags = options.tags;
    if (options?.workspace) params.workspace = options.workspace;
    if (options?.metadata) params.metadata = options.metadata;
    if (options?.importance !== undefined)
      params.importance = options.importance;
    return this.mcpCall("memory_create", params);
  }

  async get(memoryId: number): Promise<unknown> {
    return this.mcpCall("memory_get", { id: memoryId });
  }

  async update(memoryId: number, options: UpdateOptions): Promise<unknown> {
    const params: Record<string, unknown> = { id: memoryId };
    if (options.content !== undefined) params.content = options.content;
    if (options.tags !== undefined) params.tags = options.tags;
    if (options.metadata !== undefined) params.metadata = options.metadata;
    if (options.importance !== undefined)
      params.importance = options.importance;
    return this.mcpCall("memory_update", params);
  }

  async delete(memoryId: number): Promise<unknown> {
    return this.mcpCall("memory_delete", { id: memoryId });
  }

  async list(options?: ListOptions): Promise<unknown> {
    const params: Record<string, unknown> = {
      limit: options?.limit ?? 50,
      offset: options?.offset ?? 0,
    };
    if (options?.workspace) params.workspace = options.workspace;
    if (options?.memoryType) params.memory_type = options.memoryType;
    if (options?.tags) params.tags = options.tags;
    return this.mcpCall("memory_list", params);
  }

  // -- Search --

  async search(query: string, options?: SearchOptions): Promise<unknown> {
    const params: Record<string, unknown> = {
      query,
      limit: options?.limit ?? 10,
    };
    if (options?.workspace) params.workspace = options.workspace;
    return this.mcpCall("memory_search", params);
  }

  // -- Graph --

  async related(memoryId: number): Promise<unknown> {
    return this.mcpCall("memory_related", { id: memoryId });
  }

  async link(
    fromId: number,
    toId: number,
    edgeType: string = "related_to"
  ): Promise<unknown> {
    return this.mcpCall("memory_link", {
      from_id: fromId,
      to_id: toId,
      edge_type: edgeType,
    });
  }

  // -- Daily (ephemeral) memories --

  async createDaily(
    content: string,
    options?: {
      tags?: string[];
      workspace?: string;
      ttlSeconds?: number;
      metadata?: Record<string, unknown>;
    }
  ): Promise<unknown> {
    const params: Record<string, unknown> = {
      content,
      ttl_seconds: options?.ttlSeconds ?? 86400,
    };
    if (options?.tags) params.tags = options.tags;
    if (options?.workspace) params.workspace = options.workspace;
    if (options?.metadata) params.metadata = options.metadata;
    return this.mcpCall("memory_create_daily", params);
  }

  // -- Identity --

  async createIdentity(
    canonicalId: string,
    displayName: string,
    options?: { aliases?: string[]; metadata?: Record<string, unknown> }
  ): Promise<unknown> {
    const params: Record<string, unknown> = {
      canonical_id: canonicalId,
      display_name: displayName,
    };
    if (options?.aliases) params.aliases = options.aliases;
    if (options?.metadata) params.metadata = options.metadata;
    return this.mcpCall("identity_create", params);
  }

  async resolveIdentity(alias: string): Promise<unknown> {
    return this.mcpCall("identity_resolve", { alias });
  }

  // -- Stats --

  async stats(): Promise<unknown> {
    return this.mcpCall("memory_stats", {});
  }

  // -- Compression --

  async compress(memoryId: number): Promise<unknown> {
    return this.mcpCall("memory_compress", { id: memoryId });
  }

  async decompress(memoryId: number): Promise<unknown> {
    return this.mcpCall("memory_decompress", { id: memoryId });
  }

  async compressForContext(
    memoryIds: number[],
    tokenBudget: number
  ): Promise<unknown> {
    return this.mcpCall("memory_compress_for_context", {
      memory_ids: memoryIds,
      token_budget: tokenBudget,
    });
  }

  async consolidate(
    workspace: string,
    options?: ConsolidateOptions
  ): Promise<unknown> {
    return this.mcpCall("memory_consolidate", {
      workspace,
      threshold: options?.threshold ?? 0.8,
    });
  }

  async synthesis(memoryIds: number[]): Promise<unknown> {
    return this.mcpCall("memory_synthesis", { memory_ids: memoryIds });
  }

  // -- Agentic Evolution --

  async detectUpdates(memoryId: number): Promise<unknown> {
    return this.mcpCall("memory_detect_updates", { id: memoryId });
  }

  async utilityScore(
    memoryId: number,
    options?: UtilityScoreOptions
  ): Promise<unknown> {
    const params: Record<string, unknown> = { id: memoryId };
    if (options?.signal !== undefined) params.signal = options.signal;
    return this.mcpCall("memory_utility_score", params);
  }

  async sentimentAnalyze(memoryId: number): Promise<unknown> {
    return this.mcpCall("memory_sentiment_analyze", { id: memoryId });
  }

  async sentimentTimeline(
    options?: SentimentTimelineOptions
  ): Promise<unknown> {
    const params: Record<string, unknown> = {
      limit: options?.limit ?? 50,
    };
    if (options?.workspace !== undefined) params.workspace = options.workspace;
    return this.mcpCall("memory_sentiment_timeline", params);
  }

  async reflect(memoryId: number): Promise<unknown> {
    return this.mcpCall("memory_reflect", { id: memoryId });
  }

  // -- Advanced Graph --

  async detectConflicts(workspace?: string): Promise<unknown> {
    const params: Record<string, unknown> = {};
    if (workspace !== undefined) params.workspace = workspace;
    return this.mcpCall("memory_detect_conflicts", params);
  }

  async resolveConflict(
    conflictId: string,
    resolution: string
  ): Promise<unknown> {
    return this.mcpCall("memory_resolve_conflict", {
      conflict_id: conflictId,
      resolution,
    });
  }

  async coactivationReport(
    options?: CoactivationReportOptions
  ): Promise<unknown> {
    const params: Record<string, unknown> = {
      limit: options?.limit ?? 50,
    };
    if (options?.workspace !== undefined) params.workspace = options.workspace;
    return this.mcpCall("memory_coactivation_report", params);
  }

  async queryTriplets(options?: QueryTripletsOptions): Promise<unknown> {
    const params: Record<string, unknown> = {};
    if (options?.subject !== undefined) params.subject = options.subject;
    if (options?.predicate !== undefined) params.predicate = options.predicate;
    if (options?.object !== undefined) params.object = options.object;
    return this.mcpCall("memory_query_triplets", params);
  }

  async addKnowledge(
    subject: string,
    predicate: string,
    object: string,
    options?: AddKnowledgeOptions
  ): Promise<unknown> {
    return this.mcpCall("memory_add_knowledge", {
      subject,
      predicate,
      object,
      confidence: options?.confidence ?? 1.0,
    });
  }

  // -- Autonomous Agent --

  async agentStart(options?: AgentStartOptions): Promise<unknown> {
    const params: Record<string, unknown> = {};
    if (options?.workspace !== undefined) params.workspace = options.workspace;
    return this.mcpCall("memory_agent_start", params);
  }

  async agentStop(): Promise<unknown> {
    return this.mcpCall("memory_agent_stop", {});
  }

  async agentStatus(): Promise<unknown> {
    return this.mcpCall("memory_agent_status", {});
  }

  async agentMetrics(): Promise<unknown> {
    return this.mcpCall("memory_agent_metrics", {});
  }

  async agentConfigure(config: Record<string, unknown>): Promise<unknown> {
    return this.mcpCall("memory_agent_configure", { config });
  }

  async garden(options?: GardenOptions): Promise<unknown> {
    const params: Record<string, unknown> = {
      dry_run: options?.dryRun ?? false,
    };
    if (options?.workspace !== undefined) params.workspace = options.workspace;
    return this.mcpCall("memory_garden", params);
  }

  async gardenPreview(options?: GardenPreviewOptions): Promise<unknown> {
    const params: Record<string, unknown> = {};
    if (options?.workspace !== undefined) params.workspace = options.workspace;
    return this.mcpCall("memory_garden_preview", params);
  }

  async gardenUndo(operationId: string): Promise<unknown> {
    return this.mcpCall("memory_garden_undo", { operation_id: operationId });
  }

  async suggestAcquisition(
    options?: SuggestAcquisitionOptions
  ): Promise<unknown> {
    const params: Record<string, unknown> = {};
    if (options?.workspace !== undefined) params.workspace = options.workspace;
    return this.mcpCall("memory_suggest_acquisition", params);
  }

  async proactiveScan(options?: ProactiveScanOptions): Promise<unknown> {
    const params: Record<string, unknown> = {};
    if (options?.workspace !== undefined) params.workspace = options.workspace;
    return this.mcpCall("memory_proactive_scan", params);
  }

  // -- Retrieval Excellence --

  async cacheStats(): Promise<unknown> {
    return this.mcpCall("memory_cache_stats", {});
  }

  async cacheClear(options?: CacheClearOptions): Promise<unknown> {
    const params: Record<string, unknown> = {};
    if (options?.workspace !== undefined) params.workspace = options.workspace;
    return this.mcpCall("memory_cache_clear", params);
  }

  async embeddingProviders(): Promise<unknown> {
    return this.mcpCall("memory_embedding_providers", {});
  }

  async embeddingMigrate(options?: EmbeddingMigrateOptions): Promise<unknown> {
    const params: Record<string, unknown> = {};
    if (options?.fromProvider !== undefined)
      params.from_provider = options.fromProvider;
    if (options?.toProvider !== undefined)
      params.to_provider = options.toProvider;
    return this.mcpCall("memory_embedding_migrate", params);
  }

  async explainSearch(results: unknown[]): Promise<unknown> {
    return this.mcpCall("memory_explain_search", { results });
  }

  async feedback(
    query: string,
    memoryId: number,
    signal: string
  ): Promise<unknown> {
    return this.mcpCall("memory_feedback", {
      query,
      memory_id: memoryId,
      signal,
    });
  }

  async feedbackStats(options?: FeedbackStatsOptions): Promise<unknown> {
    const params: Record<string, unknown> = {};
    if (options?.workspace !== undefined) params.workspace = options.workspace;
    return this.mcpCall("memory_feedback_stats", params);
  }

  // -- Context Engineering --

  async extractFacts(memoryId: number): Promise<unknown> {
    return this.mcpCall("memory_extract_facts", { id: memoryId });
  }

  async listFacts(options?: ListFactsOptions): Promise<unknown> {
    const params: Record<string, unknown> = {
      limit: options?.limit ?? 50,
    };
    if (options?.memoryId !== undefined) params.memory_id = options.memoryId;
    if (options?.workspace !== undefined) params.workspace = options.workspace;
    return this.mcpCall("memory_list_facts", params);
  }

  async factGraph(options?: FactGraphOptions): Promise<unknown> {
    const params: Record<string, unknown> = {};
    if (options?.workspace !== undefined) params.workspace = options.workspace;
    return this.mcpCall("memory_fact_graph", params);
  }

  async buildContext(
    query: string,
    options?: BuildContextOptions
  ): Promise<unknown> {
    const params: Record<string, unknown> = {
      query,
      strategy: options?.strategy ?? "balanced",
      token_budget: options?.tokenBudget ?? 4096,
    };
    if (options?.workspace !== undefined) params.workspace = options.workspace;
    return this.mcpCall("memory_build_context", params);
  }

  async promptTemplate(
    templateName: string,
    options?: PromptTemplateOptions
  ): Promise<unknown> {
    const params: Record<string, unknown> = { template_name: templateName };
    if (options?.memories !== undefined) params.memories = options.memories;
    return this.mcpCall("memory_prompt_template", params);
  }

  async tokenEstimate(content: string): Promise<unknown> {
    return this.mcpCall("memory_token_estimate", { content });
  }

  async blockGet(
    blockType: string,
    label: string,
    options?: BlockGetOptions
  ): Promise<unknown> {
    const params: Record<string, unknown> = {
      block_type: blockType,
      label,
    };
    if (options?.workspace !== undefined) params.workspace = options.workspace;
    return this.mcpCall("memory_block_get", params);
  }

  async blockEdit(
    blockType: string,
    label: string,
    content: string,
    options?: BlockEditOptions
  ): Promise<unknown> {
    const params: Record<string, unknown> = {
      block_type: blockType,
      label,
      content,
    };
    if (options?.workspace !== undefined) params.workspace = options.workspace;
    if (options?.reason !== undefined) params.reason = options.reason;
    return this.mcpCall("memory_block_edit", params);
  }

  async blockList(options?: BlockListOptions): Promise<unknown> {
    const params: Record<string, unknown> = {};
    if (options?.blockType !== undefined)
      params.block_type = options.blockType;
    if (options?.workspace !== undefined) params.workspace = options.workspace;
    return this.mcpCall("memory_block_list", params);
  }

  async blockCreate(
    blockType: string,
    label: string,
    content: string,
    options?: BlockCreateOptions
  ): Promise<unknown> {
    const params: Record<string, unknown> = {
      block_type: blockType,
      label,
      content,
      max_tokens: options?.maxTokens ?? 2048,
    };
    if (options?.workspace !== undefined) params.workspace = options.workspace;
    return this.mcpCall("memory_block_create", params);
  }

  // -- Temporal Graph --

  async temporalCreate(
    fromEntity: string,
    toEntity: string,
    relation: string,
    options?: TemporalCreateOptions
  ): Promise<unknown> {
    const params: Record<string, unknown> = {
      from_entity: fromEntity,
      to_entity: toEntity,
      relation,
      confidence: options?.confidence ?? 1.0,
    };
    if (options?.validFrom !== undefined) params.valid_from = options.validFrom;
    return this.mcpCall("memory_temporal_create", params);
  }

  async temporalInvalidate(
    edgeId: string,
    options?: TemporalInvalidateOptions
  ): Promise<unknown> {
    const params: Record<string, unknown> = { edge_id: edgeId };
    if (options?.reason !== undefined) params.reason = options.reason;
    return this.mcpCall("memory_temporal_invalidate", params);
  }

  async temporalSnapshot(
    options?: TemporalSnapshotOptions
  ): Promise<unknown> {
    const params: Record<string, unknown> = {};
    if (options?.timestamp !== undefined) params.timestamp = options.timestamp;
    if (options?.workspace !== undefined) params.workspace = options.workspace;
    return this.mcpCall("memory_temporal_snapshot", params);
  }

  async temporalContradictions(
    options?: TemporalContradictionsOptions
  ): Promise<unknown> {
    const params: Record<string, unknown> = {};
    if (options?.workspace !== undefined) params.workspace = options.workspace;
    return this.mcpCall("memory_temporal_contradictions", params);
  }

  async temporalEvolve(entity: string): Promise<unknown> {
    return this.mcpCall("memory_temporal_evolve", { entity });
  }

  async scopeSet(memoryId: number, scopePath: string): Promise<unknown> {
    return this.mcpCall("memory_scope_set", {
      id: memoryId,
      scope_path: scopePath,
    });
  }

  async scopeGet(memoryId: number): Promise<unknown> {
    return this.mcpCall("memory_scope_get", { id: memoryId });
  }

  async scopeList(
    scopePath: string,
    options?: ScopeListOptions
  ): Promise<unknown> {
    return this.mcpCall("memory_scope_list", {
      scope_path: scopePath,
      recursive: options?.recursive ?? false,
    });
  }

  async scopeInherit(
    scopePath: string,
    parentPath: string
  ): Promise<unknown> {
    return this.mcpCall("memory_scope_inherit", {
      scope_path: scopePath,
      parent_path: parentPath,
    });
  }

  async scopeIsolate(scopePath: string): Promise<unknown> {
    return this.mcpCall("memory_scope_isolate", { scope_path: scopePath });
  }
}
