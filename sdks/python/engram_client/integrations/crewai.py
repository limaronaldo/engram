"""CrewAI integration for Engram memory engine.

Provides three memory classes that mirror CrewAI's memory interface:

- EngramShortTermMemory: daily (auto-expiring) memories backed by Engram.
- EngramLongTermMemory: permanent memories isolated per crew workspace.
- EngramEntityMemory: entity records backed by Engram's identity system.

Usage::

    from engram_client import EngramClient
    from engram_client.integrations.crewai import (
        EngramShortTermMemory,
        EngramLongTermMemory,
        EngramEntityMemory,
    )

    client = EngramClient(base_url="...", api_key="...", tenant="my-tenant")

    # Short-term memory (expires after 1 hour by default)
    stm = EngramShortTermMemory(client)
    stm.save("last_result", "Task completed successfully")
    results = stm.search("task result")

    # Long-term memory (permanent, per-crew workspace)
    ltm = EngramLongTermMemory(client, crew_name="research-crew")
    ltm.save("finding_1", "Paris is the capital of France")
    results = ltm.search("capital cities")

    # Entity memory (identity-backed)
    em = EngramEntityMemory(client)
    em.save_entity("Alice", "person", "Lead researcher", aliases=["alice@example.com"])
    entity = em.get_entity("Alice", entity_type="person")
"""

from __future__ import annotations

from typing import Any, Dict, List, Optional

from engram_client import EngramClient


class EngramShortTermMemory:
    """CrewAI-compatible short-term memory backed by Engram daily memories.

    Memories auto-expire using ``memory_create_daily`` with a configurable TTL.
    All entries are stored in a dedicated workspace and tagged with
    ``['crewai', 'short-term', 'key:<key>']`` for efficient retrieval.
    """

    def __init__(
        self,
        client: EngramClient,
        workspace: str = "crewai-stm",
        ttl_seconds: int = 3600,
    ) -> None:
        self.client = client
        self.workspace = workspace
        self.ttl_seconds = ttl_seconds

    def save(
        self,
        key: str,
        value: str,
        metadata: Optional[Dict[str, Any]] = None,
    ) -> Dict[str, Any]:
        """Save a short-term memory that expires after ``ttl_seconds``.

        Args:
            key: Logical key identifying this memory entry.
            value: Content to store.
            metadata: Optional extra metadata attached to the memory.

        Returns:
            The Engram API response dict.
        """
        return self.client.create_daily(
            content=f"[{key}] {value}",
            tags=["crewai", "short-term", f"key:{key}"],
            workspace=self.workspace,
            ttl_seconds=self.ttl_seconds,
            metadata=metadata or {},
        )

    def search(self, query: str, limit: int = 5) -> Dict[str, Any]:
        """Search short-term memories with hybrid search.

        Args:
            query: Search query string.
            limit: Maximum number of results to return.

        Returns:
            The Engram API search response dict.
        """
        return self.client.search(query, workspace=self.workspace, limit=limit)

    def reset(self) -> None:
        """Clear all short-term memories in this workspace (best effort).

        Searches for up to 100 memories and deletes each one.  Because
        Engram memories also expire automatically via TTL, any failures
        here are non-fatal.
        """
        result = self.client.search("crewai", workspace=self.workspace, limit=100)
        memories = _extract_memories(result)
        for mem in memories:
            mem_id = mem.get("id")
            if mem_id is not None:
                try:
                    self.client.delete(int(mem_id))
                except Exception:
                    pass


class EngramLongTermMemory:
    """CrewAI-compatible long-term memory backed by permanent Engram memories.

    Each crew gets its own isolated workspace (``crewai-<crew_name>`` by
    default) so multiple crews can coexist without data leakage.  Memories
    are tagged with ``['crewai', 'long-term', 'key:<key>']``.
    """

    def __init__(
        self,
        client: EngramClient,
        crew_name: str = "default",
        workspace: Optional[str] = None,
    ) -> None:
        self.client = client
        self.workspace = workspace or f"crewai-{crew_name}"

    def save(
        self,
        key: str,
        value: str,
        metadata: Optional[Dict[str, Any]] = None,
    ) -> Dict[str, Any]:
        """Save a permanent memory for the crew.

        Args:
            key: Logical key identifying this memory entry.
            value: Content to store.
            metadata: Optional extra metadata attached to the memory.

        Returns:
            The Engram API response dict.
        """
        return self.client.create(
            content=f"[{key}] {value}",
            tags=["crewai", "long-term", f"key:{key}"],
            workspace=self.workspace,
            metadata=metadata or {},
        )

    def search(self, query: str, limit: int = 10) -> Dict[str, Any]:
        """Search long-term memories with hybrid search.

        Args:
            query: Search query string.
            limit: Maximum number of results to return.

        Returns:
            The Engram API search response dict.
        """
        return self.client.search(query, workspace=self.workspace, limit=limit)


class EngramEntityMemory:
    """CrewAI-compatible entity memory backed by Engram's identity system.

    Entities are represented as Engram identities with a canonical ID of
    the form ``<entity_type>:<normalised_name>`` (e.g. ``person:alice``).
    Entity search falls back to Engram hybrid search within the entity
    workspace when a direct identity lookup is not needed.
    """

    def __init__(
        self,
        client: EngramClient,
        workspace: str = "crewai-entities",
    ) -> None:
        self.client = client
        self.workspace = workspace

    def save_entity(
        self,
        entity_name: str,
        entity_type: str,
        description: str,
        aliases: Optional[List[str]] = None,
    ) -> Dict[str, Any]:
        """Create or update an entity in the Engram identity system.

        Args:
            entity_name: Human-readable name for the entity.
            entity_type: Category string, e.g. ``'person'``, ``'company'``.
            description: Brief description (stored as display_name context).
            aliases: Optional list of alternative identifiers (email, id, …).

        Returns:
            The Engram API response dict.
        """
        canonical_id = _canonical_id(entity_type, entity_name)
        return self.client.create_identity(
            canonical_id=canonical_id,
            display_name=f"{entity_name} ({description})",
            aliases=aliases or [],
        )

    def get_entity(
        self,
        entity_name: str,
        entity_type: str = "person",
    ) -> Dict[str, Any]:
        """Resolve an entity by name and type.

        Args:
            entity_name: Human-readable entity name.
            entity_type: Category string matching the one used in
                         :meth:`save_entity`.

        Returns:
            The Engram API identity resolution response dict.
        """
        canonical_id = _canonical_id(entity_type, entity_name)
        return self.client.resolve_identity(canonical_id)

    def search_entities(self, query: str, limit: int = 5) -> Dict[str, Any]:
        """Search for entities using Engram hybrid search.

        Args:
            query: Search query string.
            limit: Maximum number of results to return.

        Returns:
            The Engram API search response dict.
        """
        return self.client.search(query, workspace=self.workspace, limit=limit)


# ---------------------------------------------------------------------------
# Internal helpers
# ---------------------------------------------------------------------------


def _canonical_id(entity_type: str, entity_name: str) -> str:
    """Build a canonical identity ID from type and name.

    Example: ``('person', 'Alice Smith')`` -> ``'person:alice_smith'``.
    """
    normalised = entity_name.lower().replace(" ", "_")
    return f"{entity_type}:{normalised}"


def _extract_memories(result: Any) -> List[Dict[str, Any]]:
    """Pull the list of memory dicts from an Engram MCP response.

    Handles the shapes returned by different MCP tools:
    - ``{"memories": [...]}``
    - ``{"results": [...]}``
    - A bare list ``[...]``
    - A single dict (treated as single-element list)
    """
    if isinstance(result, list):
        return result
    if isinstance(result, dict):
        for key in ("memories", "results", "items"):
            if key in result and isinstance(result[key], list):
                return result[key]
    return []
