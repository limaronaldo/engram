"""LlamaIndex integration for Engram memory engine.

Provides three adapters that duck-type LlamaIndex's storage interfaces:

- EngramDocumentStore: BaseDocumentStore-compatible — stores and retrieves
  LlamaIndex nodes/documents as Engram memories.
- EngramLlamaIndexVectorStore: BasePydanticVectorStore-compatible — stores
  node embeddings as Engram memories and delegates similarity search to
  Engram's built-in hybrid search.
- EngramChatStore: BaseChatStore-compatible — persists chat messages as
  tagged Engram memories per session key.

Usage::

    from engram_client import EngramClient
    from engram_client.integrations.llamaindex import (
        EngramDocumentStore,
        EngramLlamaIndexVectorStore,
        EngramChatStore,
    )

    client = EngramClient(base_url="...", api_key="...", tenant="my-tenant")

    # Document store
    docstore = EngramDocumentStore(client)
    docstore.add_documents([node1, node2])
    node = docstore.get_document("some-node-id")

    # Vector store
    vector_store = EngramLlamaIndexVectorStore(client)
    ids = vector_store.add(nodes)
    result = vector_store.query(query_obj)

    # Chat store
    chat_store = EngramChatStore(client)
    chat_store.set_messages("user-123", [msg1, msg2])
    msgs = chat_store.get_messages("user-123")
"""

from __future__ import annotations

import json
from typing import Any, Dict, List, Optional

from engram_client import EngramClient

__all__ = [
    "EngramDocumentStore",
    "EngramLlamaIndexVectorStore",
    "EngramChatStore",
]


# ---------------------------------------------------------------------------
# EngramDocumentStore
# ---------------------------------------------------------------------------


class EngramDocumentStore:
    """LlamaIndex BaseDocumentStore backed by Engram.

    Stores LlamaIndex nodes/documents as Engram memories.  Each node is
    stored with tags ``['llamaindex', 'docstore', 'node:<node_id>']`` so
    it can be retrieved, updated, or deleted by ``node_id``.

    No hard dependency on ``llama_index`` at import time — nodes are
    accepted as any object that exposes the relevant attributes/methods
    (duck typing), matching both real LlamaIndex nodes and simple mock
    objects.
    """

    def __init__(
        self,
        client: EngramClient,
        workspace: str = "llamaindex-docs",
    ) -> None:
        self.client = client
        self.workspace = workspace

    # ------------------------------------------------------------------
    # Write operations
    # ------------------------------------------------------------------

    def add_documents(self, docs: List[Any]) -> None:
        """Store a list of LlamaIndex nodes as Engram memories.

        Each node is serialised using ``node.get_content()`` as the memory
        content.  Node metadata (``node.metadata``), node type
        (``type(node).__name__``), ref doc ID (``node.ref_doc_id``), and
        hash (``node.hash``) are stored in the Engram memory's metadata
        field.

        Args:
            docs: List of LlamaIndex-compatible node objects.
        """
        for node in docs:
            node_id = _get_node_id(node)
            content = _get_node_content(node)
            meta = _build_node_metadata(node)
            self.client.create(
                content=content,
                tags=["llamaindex", "docstore", f"node:{node_id}"],
                workspace=self.workspace,
                metadata=meta,
            )

    def set_document_hash(self, doc_id: str, hash_value: str) -> None:
        """Update the hash stored in the metadata for a node.

        Finds the memory tagged ``node:<doc_id>`` and updates its metadata
        with the new hash value.

        Args:
            doc_id: Node ID of the document to update.
            hash_value: New hash string.
        """
        result = self.client.search(
            f"node:{doc_id}",
            workspace=self.workspace,
            limit=1,
        )
        memories = _extract_memories(result)
        if not memories:
            return
        mem = memories[0]
        mem_id = _extract_id_from_memory(mem)
        if mem_id is None:
            return
        existing_meta = mem.get("metadata") or {}
        updated_meta = {**existing_meta, "hash": hash_value}
        self.client.update(int(mem_id), metadata=updated_meta)

    # ------------------------------------------------------------------
    # Read operations
    # ------------------------------------------------------------------

    def get_document(self, doc_id: str) -> Optional[Dict[str, Any]]:
        """Retrieve a node by its ID.

        Searches for a memory tagged ``node:<doc_id>`` and returns the
        first match as a dict with keys ``id``, ``content``, ``metadata``,
        and ``tags``.  Returns ``None`` when not found.

        Args:
            doc_id: Node ID to look up.

        Returns:
            Memory dict or ``None``.
        """
        result = self.client.search(
            f"node:{doc_id}",
            workspace=self.workspace,
            limit=1,
        )
        memories = _extract_memories(result)
        if not memories:
            return None
        return memories[0]

    def document_exists(self, doc_id: str) -> bool:
        """Check whether a node exists in the store.

        Args:
            doc_id: Node ID to check.

        Returns:
            ``True`` if the node is found, ``False`` otherwise.
        """
        return self.get_document(doc_id) is not None

    def get_document_hash(self, doc_id: str) -> Optional[str]:
        """Get the stored hash for a node.

        Args:
            doc_id: Node ID.

        Returns:
            Hash string or ``None`` if the node or hash is not found.
        """
        doc = self.get_document(doc_id)
        if doc is None:
            return None
        meta = doc.get("metadata") or {}
        return meta.get("hash")

    def get_all_document_hashes(self) -> Dict[str, str]:
        """Return a mapping of node_id -> hash for all stored nodes.

        Iterates over all memories in the workspace and extracts those that
        carry a ``hash`` value in their metadata.

        Returns:
            Dict mapping node IDs to hash strings.
        """
        result = self.client.search(
            "llamaindex docstore",
            workspace=self.workspace,
            limit=1000,
        )
        memories = _extract_memories(result)
        hashes: Dict[str, str] = {}
        for mem in memories:
            meta = mem.get("metadata") or {}
            node_id = meta.get("node_id")
            hash_val = meta.get("hash")
            if node_id and hash_val:
                hashes[node_id] = hash_val
        return hashes

    def get_nodes(self, node_ids: List[str]) -> List[Dict[str, Any]]:
        """Retrieve multiple nodes by their IDs.

        Performs individual lookups for each ID and collects results.
        Nodes that are not found are silently omitted from the output list.

        Args:
            node_ids: List of node IDs to retrieve.

        Returns:
            List of memory dicts for the nodes that were found.
        """
        nodes: List[Dict[str, Any]] = []
        for node_id in node_ids:
            doc = self.get_document(node_id)
            if doc is not None:
                nodes.append(doc)
        return nodes

    # ------------------------------------------------------------------
    # Delete operations
    # ------------------------------------------------------------------

    def delete_document(self, doc_id: str) -> None:
        """Delete a node from the store.

        Searches for all memories tagged ``node:<doc_id>`` and deletes
        each one found.

        Args:
            doc_id: Node ID of the document to delete.
        """
        result = self.client.search(
            f"node:{doc_id}",
            workspace=self.workspace,
            limit=10,
        )
        memories = _extract_memories(result)
        for mem in memories:
            mem_id = _extract_id_from_memory(mem)
            if mem_id is not None:
                self.client.delete(int(mem_id))


# ---------------------------------------------------------------------------
# EngramLlamaIndexVectorStore
# ---------------------------------------------------------------------------


class EngramLlamaIndexVectorStore:
    """LlamaIndex BasePydanticVectorStore backed by Engram's hybrid search.

    Stores LlamaIndex nodes as Engram memories and delegates similarity
    queries to Engram's built-in hybrid search (BM25 + vector).  No local
    embeddings are computed — all embedding is server-side.

    Supports two query modes (via the ``mode`` attribute on the query object):
    - ``DEFAULT`` / ``HYBRID`` — hybrid search (default)
    - ``SPARSE`` — text-only search via a ``text_only`` hint

    Nodes are tagged ``['llamaindex', 'vector-store', 'node:<node_id>']``.
    """

    def __init__(
        self,
        client: EngramClient,
        workspace: str = "llamaindex-vectors",
    ) -> None:
        self.client = client
        self.workspace = workspace

    def add(self, nodes: List[Any], **kwargs: Any) -> List[str]:
        """Add LlamaIndex nodes to Engram and return their memory IDs.

        Args:
            nodes: List of LlamaIndex-compatible node objects.
            **kwargs: Ignored (accepted for interface compatibility).

        Returns:
            List of memory IDs as strings (one per node).
        """
        ids: List[str] = []
        for node in nodes:
            node_id = _get_node_id(node)
            content = _get_node_content(node)
            meta = _build_node_metadata(node)
            result = self.client.create(
                content=content,
                tags=["llamaindex", "vector-store", f"node:{node_id}"],
                workspace=self.workspace,
                metadata=meta,
            )
            mem_id = _extract_id(result)
            ids.append(str(mem_id) if mem_id is not None else "")
        return ids

    def delete(self, node_id: str, **kwargs: Any) -> None:
        """Delete a node from the vector store by its ID.

        Args:
            node_id: The node ID to delete.
            **kwargs: Ignored (accepted for interface compatibility).
        """
        result = self.client.search(
            f"node:{node_id}",
            workspace=self.workspace,
            limit=10,
        )
        memories = _extract_memories(result)
        for mem in memories:
            mem_id = _extract_id_from_memory(mem)
            if mem_id is not None:
                self.client.delete(int(mem_id))

    def query(self, query: Any, **kwargs: Any) -> Dict[str, Any]:
        """Execute a vector store query against Engram's hybrid search.

        Translates a LlamaIndex ``VectorStoreQuery`` object (or any duck-typed
        equivalent) into an Engram search call.

        Args:
            query: Object with:
                - ``.query_str`` (str) — the natural-language query
                - ``.similarity_top_k`` (int, optional) — result limit
                - ``.mode`` (str, optional) — ``'DEFAULT'`` or ``'SPARSE'``
            **kwargs: Ignored.

        Returns:
            Dict with keys:
            - ``'nodes'`` — list of memory dicts
            - ``'similarities'`` — list of float scores (0.0 if unavailable)
            - ``'ids'`` — list of memory IDs as strings
        """
        query_str = getattr(query, "query_str", "") or ""
        limit = getattr(query, "similarity_top_k", 4) or 4
        mode = getattr(query, "mode", "DEFAULT") or "DEFAULT"

        search_kwargs: Dict[str, Any] = {
            "workspace": self.workspace,
            "limit": limit,
        }
        # For SPARSE mode signal text-only preference; Engram decides internally
        if str(mode).upper() == "SPARSE":
            search_kwargs["mode"] = "sparse"

        result = self.client.search(query_str, **search_kwargs)
        memories = _extract_memories(result)

        nodes: List[Dict[str, Any]] = []
        similarities: List[float] = []
        ids: List[str] = []

        for mem in memories:
            nodes.append(mem)
            score = mem.get("score") or mem.get("relevance_score") or 0.0
            similarities.append(float(score))
            mem_id = _extract_id_from_memory(mem)
            ids.append(str(mem_id) if mem_id is not None else "")

        return {"nodes": nodes, "similarities": similarities, "ids": ids}

    def delete_nodes(self, node_ids: List[str], **kwargs: Any) -> None:
        """Batch delete nodes by their IDs.

        Args:
            node_ids: List of node IDs to delete.
            **kwargs: Ignored (accepted for interface compatibility).
        """
        for node_id in node_ids:
            self.delete(node_id)


# ---------------------------------------------------------------------------
# EngramChatStore
# ---------------------------------------------------------------------------


class EngramChatStore:
    """LlamaIndex BaseChatStore backed by Engram.

    Persists chat messages as Engram memories.  Each message is stored with
    tags ``['llamaindex', 'chat-store', 'session:<key>', 'role:<role>']``
    and content formatted as ``'[<role>] <message_content>'``.

    No hard dependency on ``llama_index`` at import time — messages are
    accepted as any object with ``.role`` and ``.content`` attributes.
    """

    def __init__(
        self,
        client: EngramClient,
        workspace: str = "llamaindex-chat",
    ) -> None:
        self.client = client
        self.workspace = workspace

    def set_messages(self, key: str, messages: List[Any]) -> None:
        """Replace the message list for a session key.

        Deletes any existing messages for the key, then stores the supplied
        list in order.

        Args:
            key: Session identifier.
            messages: List of message objects with ``.role`` and ``.content``.
        """
        self.delete_messages(key)
        for msg in messages:
            self.add_message(key, msg)

    def get_messages(self, key: str) -> List[Dict[str, Any]]:
        """Retrieve all messages for a session key.

        Returns messages ordered by creation time (oldest first, as Engram
        returns them).

        Args:
            key: Session identifier.

        Returns:
            List of dicts with keys ``role`` and ``content``.
        """
        result = self.client.search(
            f"session:{key}",
            workspace=self.workspace,
            limit=500,
        )
        memories = _extract_memories(result)
        output: List[Dict[str, Any]] = []
        for mem in memories:
            content_str = mem.get("content", "")
            role, text = _parse_message_content(content_str)
            output.append({"role": role, "content": text})
        return output

    def add_message(self, key: str, message: Any) -> None:
        """Append a single message to the session.

        Args:
            key: Session identifier.
            message: Object with ``.role`` (str) and ``.content`` (str).
        """
        role = message.role
        content = message.content
        self.client.create(
            content=f"[{role}] {content}",
            tags=["llamaindex", "chat-store", f"session:{key}", f"role:{role}"],
            workspace=self.workspace,
            metadata={"session_key": key, "role": role},
        )

    def delete_messages(self, key: str) -> Optional[List[Dict[str, Any]]]:
        """Delete all messages for a session key and return the deleted list.

        Args:
            key: Session identifier.

        Returns:
            List of deleted message dicts (role, content), or ``None`` if
            no messages were found.
        """
        result = self.client.search(
            f"session:{key}",
            workspace=self.workspace,
            limit=500,
        )
        memories = _extract_memories(result)
        if not memories:
            return None
        deleted: List[Dict[str, Any]] = []
        for mem in memories:
            content_str = mem.get("content", "")
            role, text = _parse_message_content(content_str)
            deleted.append({"role": role, "content": text})
            mem_id = _extract_id_from_memory(mem)
            if mem_id is not None:
                self.client.delete(int(mem_id))
        return deleted

    def delete_message(self, key: str, idx: int) -> Optional[Dict[str, Any]]:
        """Delete the message at a specific index within a session.

        Args:
            key: Session identifier.
            idx: Zero-based index of the message to delete.

        Returns:
            The deleted message dict, or ``None`` if the index is out of range.
        """
        result = self.client.search(
            f"session:{key}",
            workspace=self.workspace,
            limit=500,
        )
        memories = _extract_memories(result)
        if idx < 0 or idx >= len(memories):
            return None
        mem = memories[idx]
        content_str = mem.get("content", "")
        role, text = _parse_message_content(content_str)
        mem_id = _extract_id_from_memory(mem)
        if mem_id is not None:
            self.client.delete(int(mem_id))
        return {"role": role, "content": text}

    def delete_last_message(self, key: str) -> Optional[Dict[str, Any]]:
        """Delete the last message in a session.

        Args:
            key: Session identifier.

        Returns:
            The deleted message dict, or ``None`` if the session is empty.
        """
        result = self.client.search(
            f"session:{key}",
            workspace=self.workspace,
            limit=500,
        )
        memories = _extract_memories(result)
        if not memories:
            return None
        mem = memories[-1]
        content_str = mem.get("content", "")
        role, text = _parse_message_content(content_str)
        mem_id = _extract_id_from_memory(mem)
        if mem_id is not None:
            self.client.delete(int(mem_id))
        return {"role": role, "content": text}

    def get_keys(self) -> List[str]:
        """Return all unique session keys known to this chat store.

        Scans all memories in the workspace and extracts unique session keys
        from their ``session:<key>`` tags.

        Returns:
            Sorted list of unique session key strings.
        """
        result = self.client.search(
            "llamaindex chat-store",
            workspace=self.workspace,
            limit=1000,
        )
        memories = _extract_memories(result)
        keys: List[str] = []
        for mem in memories:
            tags = mem.get("tags") or []
            for tag in tags:
                if isinstance(tag, str) and tag.startswith("session:"):
                    session_key = tag[len("session:"):]
                    if session_key not in keys:
                        keys.append(session_key)
        return sorted(keys)


# ---------------------------------------------------------------------------
# Internal helpers
# ---------------------------------------------------------------------------


def _extract_memories(result: Any) -> List[Dict[str, Any]]:
    """Pull the list of memory dicts from an Engram MCP response.

    Handles the shapes returned by different MCP tools:
    - ``{"memories": [...]}``
    - ``{"results": [...]}``
    - ``{"items": [...]}``
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


def _extract_id(result: Any) -> Optional[Any]:
    """Extract memory ID from a create-call response.

    Tries the common shapes:
    - ``{"id": ...}``
    - ``{"memory_id": ...}``
    - ``{"memory": {"id": ...}}``
    """
    if isinstance(result, dict):
        for key in ("id", "memory_id"):
            if key in result:
                return result[key]
        mem = result.get("memory")
        if isinstance(mem, dict):
            return mem.get("id")
    return None


def _extract_id_from_memory(mem: Dict[str, Any]) -> Optional[Any]:
    """Extract an ID from a memory dict already retrieved from Engram."""
    for key in ("id", "memory_id"):
        if key in mem:
            return mem[key]
    return None


def _get_node_id(node: Any) -> str:
    """Extract the node/doc ID from a LlamaIndex node (duck typed)."""
    # Try common attribute names in order of preference
    for attr in ("node_id", "id_", "id", "doc_id"):
        val = getattr(node, attr, None)
        if val is not None:
            return str(val)
    # Fallback: use str(node)
    return str(id(node))


def _get_node_content(node: Any) -> str:
    """Extract text content from a LlamaIndex node (duck typed)."""
    # LlamaIndex nodes expose get_content()
    get_content = getattr(node, "get_content", None)
    if callable(get_content):
        return str(get_content())
    # Fallback to text attribute
    text = getattr(node, "text", None)
    if text is not None:
        return str(text)
    return str(node)


def _build_node_metadata(node: Any) -> Dict[str, Any]:
    """Build a metadata dict from a LlamaIndex node's attributes."""
    meta: Dict[str, Any] = {}
    node_id = _get_node_id(node)
    meta["node_id"] = node_id
    # Node-level metadata dict
    node_meta = getattr(node, "metadata", None)
    if isinstance(node_meta, dict):
        meta["node_metadata"] = node_meta
    # Node type
    meta["node_type"] = type(node).__name__
    # ref_doc_id (LlamaIndex concept: the source document)
    ref_doc_id = getattr(node, "ref_doc_id", None)
    if ref_doc_id is not None:
        meta["ref_doc_id"] = str(ref_doc_id)
    # Hash
    hash_val = getattr(node, "hash", None)
    if hash_val is not None:
        meta["hash"] = str(hash_val)
    return meta


def _parse_message_content(content: str) -> tuple:
    """Parse ``'[role] text'`` format back into ``(role, text)`` tuple.

    If the format is not recognised, returns ``('unknown', content)``.
    """
    if content.startswith("[") and "] " in content:
        bracket_end = content.index("] ")
        role = content[1:bracket_end]
        text = content[bracket_end + 2:]
        return role, text
    return "unknown", content
