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

  // -- Stats --

  async stats(): Promise<unknown> {
    return this.mcpCall("memory_stats", {});
  }
}
