"""Engram Cloud HTTP client."""

from __future__ import annotations

from typing import Any, Optional

import httpx


class EngramClient:
    """Client for the Engram Cloud REST API.

    Usage:
        client = EngramClient(
            base_url="https://your-engram-cloud.fly.dev",
            api_key="ek_...",
            tenant="my-tenant",
        )

        # Create a memory
        memory = client.create("User prefers dark mode", tags=["prefs"])

        # Search
        results = client.search("user preferences")

        # List
        memories = client.list(limit=10)
    """

    def __init__(
        self,
        base_url: str,
        api_key: str,
        tenant: str,
        timeout: float = 30.0,
    ):
        self.base_url = base_url.rstrip("/")
        self.tenant = tenant
        self._client = httpx.Client(
            base_url=self.base_url,
            headers={
                "Authorization": f"Bearer {api_key}",
                "X-Tenant-Slug": tenant,
                "Content-Type": "application/json",
            },
            timeout=timeout,
        )

    def close(self) -> None:
        self._client.close()

    def __enter__(self) -> "EngramClient":
        return self

    def __exit__(self, *args: Any) -> None:
        self.close()

    # -- MCP-over-HTTP helpers --

    def _mcp_call(self, method: str, params: dict[str, Any] | None = None) -> Any:
        """Execute an MCP tool call over HTTP."""
        payload = {
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/call",
            "params": {
                "name": method,
                "arguments": params or {},
            },
        }
        resp = self._client.post("/v1/mcp", json=payload)
        resp.raise_for_status()
        result = resp.json()
        if "error" in result:
            raise EngramError(result["error"].get("message", "Unknown error"))
        return result.get("result", {})

    # -- Memory CRUD --

    def create(
        self,
        content: str,
        *,
        memory_type: str = "note",
        tags: list[str] | None = None,
        workspace: str | None = None,
        metadata: dict[str, Any] | None = None,
        importance: float | None = None,
    ) -> dict[str, Any]:
        """Create a new memory."""
        params: dict[str, Any] = {
            "content": content,
            "memory_type": memory_type,
        }
        if tags:
            params["tags"] = tags
        if workspace:
            params["workspace"] = workspace
        if metadata:
            params["metadata"] = metadata
        if importance is not None:
            params["importance"] = importance
        return self._mcp_call("memory_create", params)

    def get(self, memory_id: int) -> dict[str, Any]:
        """Get a memory by ID."""
        return self._mcp_call("memory_get", {"id": memory_id})

    def update(
        self,
        memory_id: int,
        *,
        content: str | None = None,
        tags: list[str] | None = None,
        metadata: dict[str, Any] | None = None,
        importance: float | None = None,
    ) -> dict[str, Any]:
        """Update an existing memory."""
        params: dict[str, Any] = {"id": memory_id}
        if content is not None:
            params["content"] = content
        if tags is not None:
            params["tags"] = tags
        if metadata is not None:
            params["metadata"] = metadata
        if importance is not None:
            params["importance"] = importance
        return self._mcp_call("memory_update", params)

    def delete(self, memory_id: int) -> dict[str, Any]:
        """Delete a memory."""
        return self._mcp_call("memory_delete", {"id": memory_id})

    def list(
        self,
        *,
        limit: int = 50,
        offset: int = 0,
        workspace: str | None = None,
        memory_type: str | None = None,
        tags: list[str] | None = None,
    ) -> dict[str, Any]:
        """List memories with optional filters."""
        params: dict[str, Any] = {"limit": limit, "offset": offset}
        if workspace:
            params["workspace"] = workspace
        if memory_type:
            params["memory_type"] = memory_type
        if tags:
            params["tags"] = tags
        return self._mcp_call("memory_list", params)

    # -- Search --

    def search(
        self,
        query: str,
        *,
        limit: int = 10,
        workspace: str | None = None,
    ) -> dict[str, Any]:
        """Hybrid search (BM25 + vector + fuzzy)."""
        params: dict[str, Any] = {"query": query, "limit": limit}
        if workspace:
            params["workspace"] = workspace
        return self._mcp_call("memory_search", params)

    # -- Graph --

    def related(self, memory_id: int) -> dict[str, Any]:
        """Get related memories via knowledge graph."""
        return self._mcp_call("memory_related", {"id": memory_id})

    def link(
        self,
        from_id: int,
        to_id: int,
        edge_type: str = "related_to",
    ) -> dict[str, Any]:
        """Create a link between two memories."""
        return self._mcp_call(
            "memory_link",
            {"from_id": from_id, "to_id": to_id, "edge_type": edge_type},
        )

    # -- Daily (ephemeral) memories --

    def create_daily(
        self,
        content: str,
        *,
        tags: list[str] | None = None,
        workspace: str | None = None,
        ttl_seconds: int = 86400,
        metadata: dict[str, Any] | None = None,
    ) -> dict[str, Any]:
        """Create a daily memory that auto-expires after ``ttl_seconds``.

        Uses the ``memory_create_daily`` MCP tool which sets ``tier='daily'``
        and computes ``expires_at`` from ``ttl_seconds``.
        """
        params: dict[str, Any] = {
            "content": content,
            "ttl_seconds": ttl_seconds,
        }
        if tags:
            params["tags"] = tags
        if workspace:
            params["workspace"] = workspace
        if metadata:
            params["metadata"] = metadata
        return self._mcp_call("memory_create_daily", params)

    # -- Identity --

    def create_identity(
        self,
        canonical_id: str,
        display_name: str,
        aliases: list[str] | None = None,
        metadata: dict[str, Any] | None = None,
    ) -> dict[str, Any]:
        """Create or update an identity with optional aliases.

        Maps to the ``identity_create`` MCP tool.
        """
        params: dict[str, Any] = {
            "canonical_id": canonical_id,
            "display_name": display_name,
        }
        if aliases:
            params["aliases"] = aliases
        if metadata:
            params["metadata"] = metadata
        return self._mcp_call("identity_create", params)

    def resolve_identity(self, alias: str) -> dict[str, Any]:
        """Resolve an alias to its canonical identity.

        Maps to the ``identity_resolve`` MCP tool.
        """
        return self._mcp_call("identity_resolve", {"alias": alias})

    # -- Stats --

    def stats(self) -> dict[str, Any]:
        """Get memory statistics."""
        return self._mcp_call("memory_stats", {})

    # -- Compression --

    def compress(self, memory_id: int) -> dict[str, Any]:
        """Compress a memory to reduce token footprint."""
        return self._mcp_call("memory_compress", {"id": memory_id})

    def decompress(self, memory_id: int) -> dict[str, Any]:
        """Decompress a previously compressed memory."""
        return self._mcp_call("memory_decompress", {"id": memory_id})

    def compress_for_context(
        self,
        memory_ids: list[int],
        token_budget: int,
    ) -> dict[str, Any]:
        """Compress a set of memories to fit within a token budget."""
        return self._mcp_call(
            "memory_compress_for_context",
            {"memory_ids": memory_ids, "token_budget": token_budget},
        )

    def consolidate(
        self,
        workspace: str,
        *,
        threshold: float = 0.8,
    ) -> dict[str, Any]:
        """Consolidate similar memories in a workspace above a similarity threshold."""
        return self._mcp_call(
            "memory_consolidate",
            {"workspace": workspace, "threshold": threshold},
        )

    def synthesis(self, memory_ids: list[int]) -> dict[str, Any]:
        """Synthesize multiple memories into a single distilled memory."""
        return self._mcp_call("memory_synthesis", {"memory_ids": memory_ids})

    # -- Agentic Evolution --

    def detect_updates(self, memory_id: int) -> dict[str, Any]:
        """Detect whether a memory's content may be outdated."""
        return self._mcp_call("memory_detect_updates", {"id": memory_id})

    def utility_score(
        self,
        memory_id: int,
        *,
        signal: str | None = None,
    ) -> dict[str, Any]:
        """Compute or update the utility score for a memory."""
        params: dict[str, Any] = {"id": memory_id}
        if signal is not None:
            params["signal"] = signal
        return self._mcp_call("memory_utility_score", params)

    def sentiment_analyze(self, memory_id: int) -> dict[str, Any]:
        """Run sentiment analysis on a memory."""
        return self._mcp_call("memory_sentiment_analyze", {"id": memory_id})

    def sentiment_timeline(
        self,
        *,
        workspace: str | None = None,
        limit: int = 50,
    ) -> dict[str, Any]:
        """Retrieve sentiment scores over time for memories in a workspace."""
        params: dict[str, Any] = {"limit": limit}
        if workspace is not None:
            params["workspace"] = workspace
        return self._mcp_call("memory_sentiment_timeline", params)

    def reflect(self, memory_id: int) -> dict[str, Any]:
        """Trigger self-reflection on a memory to surface insights."""
        return self._mcp_call("memory_reflect", {"id": memory_id})

    # -- Advanced Graph --

    def detect_conflicts(
        self,
        *,
        workspace: str | None = None,
    ) -> dict[str, Any]:
        """Detect conflicting or contradictory memories."""
        params: dict[str, Any] = {}
        if workspace is not None:
            params["workspace"] = workspace
        return self._mcp_call("memory_detect_conflicts", params)

    def resolve_conflict(
        self,
        conflict_id: str,
        resolution: str,
    ) -> dict[str, Any]:
        """Resolve a detected memory conflict."""
        return self._mcp_call(
            "memory_resolve_conflict",
            {"conflict_id": conflict_id, "resolution": resolution},
        )

    def coactivation_report(
        self,
        *,
        workspace: str | None = None,
        limit: int = 50,
    ) -> dict[str, Any]:
        """Report memories that are frequently co-accessed."""
        params: dict[str, Any] = {"limit": limit}
        if workspace is not None:
            params["workspace"] = workspace
        return self._mcp_call("memory_coactivation_report", params)

    def query_triplets(
        self,
        *,
        subject: str | None = None,
        predicate: str | None = None,
        object: str | None = None,
    ) -> dict[str, Any]:
        """Query knowledge graph triplets by subject, predicate, or object."""
        params: dict[str, Any] = {}
        if subject is not None:
            params["subject"] = subject
        if predicate is not None:
            params["predicate"] = predicate
        if object is not None:
            params["object"] = object
        return self._mcp_call("memory_query_triplets", params)

    def add_knowledge(
        self,
        subject: str,
        predicate: str,
        object: str,
        *,
        confidence: float = 1.0,
    ) -> dict[str, Any]:
        """Add a knowledge triplet to the graph."""
        return self._mcp_call(
            "memory_add_knowledge",
            {
                "subject": subject,
                "predicate": predicate,
                "object": object,
                "confidence": confidence,
            },
        )

    # -- Autonomous Agent --

    def agent_start(self, *, workspace: str | None = None) -> dict[str, Any]:
        """Start the autonomous memory gardening agent."""
        params: dict[str, Any] = {}
        if workspace is not None:
            params["workspace"] = workspace
        return self._mcp_call("memory_agent_start", params)

    def agent_stop(self) -> dict[str, Any]:
        """Stop the autonomous memory gardening agent."""
        return self._mcp_call("memory_agent_stop", {})

    def agent_status(self) -> dict[str, Any]:
        """Get the current status of the autonomous agent."""
        return self._mcp_call("memory_agent_status", {})

    def agent_metrics(self) -> dict[str, Any]:
        """Get performance metrics for the autonomous agent."""
        return self._mcp_call("memory_agent_metrics", {})

    def agent_configure(self, config: dict[str, Any]) -> dict[str, Any]:
        """Configure the autonomous memory agent."""
        return self._mcp_call("memory_agent_configure", {"config": config})

    def garden(
        self,
        *,
        workspace: str | None = None,
        dry_run: bool = False,
    ) -> dict[str, Any]:
        """Run one gardening cycle: prune, merge, and promote memories."""
        params: dict[str, Any] = {"dry_run": dry_run}
        if workspace is not None:
            params["workspace"] = workspace
        return self._mcp_call("memory_garden", params)

    def garden_preview(self, *, workspace: str | None = None) -> dict[str, Any]:
        """Preview what a gardening cycle would do without applying changes."""
        params: dict[str, Any] = {}
        if workspace is not None:
            params["workspace"] = workspace
        return self._mcp_call("memory_garden_preview", params)

    def garden_undo(self, operation_id: str) -> dict[str, Any]:
        """Undo a previous gardening operation."""
        return self._mcp_call("memory_garden_undo", {"operation_id": operation_id})

    def suggest_acquisition(
        self,
        *,
        workspace: str | None = None,
    ) -> dict[str, Any]:
        """Suggest topics or entities to acquire knowledge about."""
        params: dict[str, Any] = {}
        if workspace is not None:
            params["workspace"] = workspace
        return self._mcp_call("memory_suggest_acquisition", params)

    def proactive_scan(
        self,
        *,
        workspace: str | None = None,
    ) -> dict[str, Any]:
        """Proactively scan memories for gaps, staleness, or improvement opportunities."""
        params: dict[str, Any] = {}
        if workspace is not None:
            params["workspace"] = workspace
        return self._mcp_call("memory_proactive_scan", params)

    # -- Retrieval Excellence --

    def cache_stats(self) -> dict[str, Any]:
        """Get embedding and search cache statistics."""
        return self._mcp_call("memory_cache_stats", {})

    def cache_clear(self, *, workspace: str | None = None) -> dict[str, Any]:
        """Clear the embedding and search cache."""
        params: dict[str, Any] = {}
        if workspace is not None:
            params["workspace"] = workspace
        return self._mcp_call("memory_cache_clear", params)

    def embedding_providers(self) -> dict[str, Any]:
        """List available embedding providers and their status."""
        return self._mcp_call("memory_embedding_providers", {})

    def embedding_migrate(
        self,
        *,
        from_provider: str | None = None,
        to_provider: str | None = None,
    ) -> dict[str, Any]:
        """Migrate embeddings from one provider to another."""
        params: dict[str, Any] = {}
        if from_provider is not None:
            params["from_provider"] = from_provider
        if to_provider is not None:
            params["to_provider"] = to_provider
        return self._mcp_call("memory_embedding_migrate", params)

    def explain_search(self, results: list[Any]) -> dict[str, Any]:
        """Explain why specific search results were returned."""
        return self._mcp_call("memory_explain_search", {"results": results})

    def feedback(
        self,
        query: str,
        memory_id: int,
        signal: str,
    ) -> dict[str, Any]:
        """Record relevance feedback for a search result to improve future retrieval."""
        return self._mcp_call(
            "memory_feedback",
            {"query": query, "memory_id": memory_id, "signal": signal},
        )

    def feedback_stats(self, *, workspace: str | None = None) -> dict[str, Any]:
        """Get aggregated feedback statistics."""
        params: dict[str, Any] = {}
        if workspace is not None:
            params["workspace"] = workspace
        return self._mcp_call("memory_feedback_stats", params)

    # -- Context Engineering --

    def extract_facts(self, memory_id: int) -> dict[str, Any]:
        """Extract atomic facts from a memory."""
        return self._mcp_call("memory_extract_facts", {"id": memory_id})

    def list_facts(
        self,
        *,
        memory_id: int | None = None,
        workspace: str | None = None,
        limit: int = 50,
    ) -> dict[str, Any]:
        """List extracted facts, optionally filtered by memory or workspace."""
        params: dict[str, Any] = {"limit": limit}
        if memory_id is not None:
            params["memory_id"] = memory_id
        if workspace is not None:
            params["workspace"] = workspace
        return self._mcp_call("memory_list_facts", params)

    def fact_graph(self, *, workspace: str | None = None) -> dict[str, Any]:
        """Export a graph of extracted facts and their relationships."""
        params: dict[str, Any] = {}
        if workspace is not None:
            params["workspace"] = workspace
        return self._mcp_call("memory_fact_graph", params)

    def build_context(
        self,
        query: str,
        *,
        strategy: str = "balanced",
        token_budget: int = 4096,
        workspace: str | None = None,
    ) -> dict[str, Any]:
        """Build an optimised context window for an LLM prompt."""
        params: dict[str, Any] = {
            "query": query,
            "strategy": strategy,
            "token_budget": token_budget,
        }
        if workspace is not None:
            params["workspace"] = workspace
        return self._mcp_call("memory_build_context", params)

    def prompt_template(
        self,
        template_name: str,
        *,
        memories: list[Any] | None = None,
    ) -> dict[str, Any]:
        """Render a named prompt template populated with memories."""
        params: dict[str, Any] = {"template_name": template_name}
        if memories is not None:
            params["memories"] = memories
        return self._mcp_call("memory_prompt_template", params)

    def token_estimate(self, content: str) -> dict[str, Any]:
        """Estimate the token count for the given content."""
        return self._mcp_call("memory_token_estimate", {"content": content})

    def block_get(
        self,
        block_type: str,
        label: str,
        *,
        workspace: str | None = None,
    ) -> dict[str, Any]:
        """Retrieve a named memory block by type and label."""
        params: dict[str, Any] = {"block_type": block_type, "label": label}
        if workspace is not None:
            params["workspace"] = workspace
        return self._mcp_call("memory_block_get", params)

    def block_edit(
        self,
        block_type: str,
        label: str,
        content: str,
        *,
        workspace: str | None = None,
        reason: str | None = None,
    ) -> dict[str, Any]:
        """Edit the content of an existing memory block."""
        params: dict[str, Any] = {
            "block_type": block_type,
            "label": label,
            "content": content,
        }
        if workspace is not None:
            params["workspace"] = workspace
        if reason is not None:
            params["reason"] = reason
        return self._mcp_call("memory_block_edit", params)

    def block_list(
        self,
        *,
        block_type: str | None = None,
        workspace: str | None = None,
    ) -> dict[str, Any]:
        """List memory blocks, optionally filtered by type or workspace."""
        params: dict[str, Any] = {}
        if block_type is not None:
            params["block_type"] = block_type
        if workspace is not None:
            params["workspace"] = workspace
        return self._mcp_call("memory_block_list", params)

    def block_create(
        self,
        block_type: str,
        label: str,
        content: str,
        *,
        workspace: str | None = None,
        max_tokens: int = 2048,
    ) -> dict[str, Any]:
        """Create a new named memory block."""
        params: dict[str, Any] = {
            "block_type": block_type,
            "label": label,
            "content": content,
            "max_tokens": max_tokens,
        }
        if workspace is not None:
            params["workspace"] = workspace
        return self._mcp_call("memory_block_create", params)

    # -- Temporal Graph --

    def temporal_create(
        self,
        from_entity: str,
        to_entity: str,
        relation: str,
        *,
        valid_from: str | None = None,
        confidence: float = 1.0,
    ) -> dict[str, Any]:
        """Create a time-bounded edge in the temporal knowledge graph."""
        params: dict[str, Any] = {
            "from_entity": from_entity,
            "to_entity": to_entity,
            "relation": relation,
            "confidence": confidence,
        }
        if valid_from is not None:
            params["valid_from"] = valid_from
        return self._mcp_call("memory_temporal_create", params)

    def temporal_invalidate(
        self,
        edge_id: str,
        *,
        reason: str | None = None,
    ) -> dict[str, Any]:
        """Mark a temporal graph edge as no longer valid."""
        params: dict[str, Any] = {"edge_id": edge_id}
        if reason is not None:
            params["reason"] = reason
        return self._mcp_call("memory_temporal_invalidate", params)

    def temporal_snapshot(
        self,
        *,
        timestamp: str | None = None,
        workspace: str | None = None,
    ) -> dict[str, Any]:
        """Get a snapshot of the knowledge graph at a specific point in time."""
        params: dict[str, Any] = {}
        if timestamp is not None:
            params["timestamp"] = timestamp
        if workspace is not None:
            params["workspace"] = workspace
        return self._mcp_call("memory_temporal_snapshot", params)

    def temporal_contradictions(
        self,
        *,
        workspace: str | None = None,
    ) -> dict[str, Any]:
        """Find temporal contradictions in the knowledge graph."""
        params: dict[str, Any] = {}
        if workspace is not None:
            params["workspace"] = workspace
        return self._mcp_call("memory_temporal_contradictions", params)

    def temporal_evolve(self, entity: str) -> dict[str, Any]:
        """Trace how an entity's relationships have evolved over time."""
        return self._mcp_call("memory_temporal_evolve", {"entity": entity})

    def scope_set(self, memory_id: int, scope_path: str) -> dict[str, Any]:
        """Assign a hierarchical scope path to a memory."""
        return self._mcp_call(
            "memory_scope_set",
            {"id": memory_id, "scope_path": scope_path},
        )

    def scope_get(self, memory_id: int) -> dict[str, Any]:
        """Get the scope path assigned to a memory."""
        return self._mcp_call("memory_scope_get", {"id": memory_id})

    def scope_list(
        self,
        scope_path: str,
        *,
        recursive: bool = False,
    ) -> dict[str, Any]:
        """List memories within a scope path."""
        return self._mcp_call(
            "memory_scope_list",
            {"scope_path": scope_path, "recursive": recursive},
        )

    def scope_inherit(
        self,
        scope_path: str,
        parent_path: str,
    ) -> dict[str, Any]:
        """Make a scope inherit settings and policies from a parent scope."""
        return self._mcp_call(
            "memory_scope_inherit",
            {"scope_path": scope_path, "parent_path": parent_path},
        )

    def scope_isolate(self, scope_path: str) -> dict[str, Any]:
        """Isolate a scope so it does not inherit from any parent."""
        return self._mcp_call("memory_scope_isolate", {"scope_path": scope_path})


class EngramError(Exception):
    """Error from the Engram API."""
