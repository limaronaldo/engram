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

    # -- Stats --

    def stats(self) -> dict[str, Any]:
        """Get memory statistics."""
        return self._mcp_call("memory_stats", {})


class EngramError(Exception):
    """Error from the Engram API."""
