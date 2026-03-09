"""LangChain integration for Engram memory engine.

Provides:
- EngramChatMessageHistory: BaseChatMessageHistory-compatible class backed by Engram.
- EngramVectorStore: VectorStore-compatible class backed by Engram's hybrid search.

Usage::

    from engram_client import EngramClient
    from engram_client.integrations.langchain import EngramChatMessageHistory, EngramVectorStore

    client = EngramClient(base_url="...", api_key="...", tenant="my-tenant")

    # Chat history
    history = EngramChatMessageHistory(client, session_id="user-123")
    history.add_message(HumanMessage(content="Hello!"))
    msgs = history.messages

    # Vector store
    store = EngramVectorStore(client)
    store.add_texts(["Paris is the capital of France."])
    docs = store.similarity_search("capital of France")
"""

from __future__ import annotations

from typing import Any, Dict, List, Optional

from engram_client import EngramClient


class EngramChatMessageHistory:
    """LangChain BaseChatMessageHistory backed by Engram.

    Stores chat messages as memories with tags ``['langchain', 'chat-history',
    'session:<session_id>', 'role:<role>']``.

    No hard dependency on ``langchain-core`` at import time — messages are
    accepted as any object with ``.type`` and ``.content`` attributes, which
    matches both LangChain ``BaseMessage`` objects and simple mock objects.
    """

    def __init__(
        self,
        client: EngramClient,
        session_id: str,
        workspace: str = "langchain",
    ) -> None:
        self.client = client
        self.session_id = session_id
        self.workspace = workspace

    @property
    def messages(self) -> List[Dict[str, Any]]:
        """Retrieve messages from Engram for the current session.

        Returns a list of dicts with keys ``type`` and ``content``, ordered
        by creation time (oldest first, as Engram returns them).
        """
        result = self.client.search(
            f"session:{self.session_id}",
            workspace=self.workspace,
            limit=100,
        )
        memories = _extract_memories(result)
        output: List[Dict[str, Any]] = []
        for mem in memories:
            content_str = mem.get("content", "")
            role, text = _parse_message_content(content_str)
            output.append({"type": role, "content": text})
        return output

    def add_message(self, message: Any) -> None:
        """Add a message to Engram.

        Args:
            message: Any object with ``.type`` (str) and ``.content`` (str)
                     attributes.  LangChain ``HumanMessage``, ``AIMessage``,
                     ``SystemMessage`` all satisfy this contract.
        """
        role = message.type
        content = message.content
        self.client.create(
            content=f"[{role}] {content}",
            tags=[
                "langchain",
                "chat-history",
                f"session:{self.session_id}",
                f"role:{role}",
            ],
            workspace=self.workspace,
            metadata={"session_id": self.session_id, "role": role},
        )

    def add_messages(self, messages: List[Any]) -> None:
        """Add multiple messages to Engram."""
        for msg in messages:
            self.add_message(msg)

    def clear(self) -> None:
        """Delete all messages for the current session from Engram.

        Searches for all memories tagged with this session and deletes them.
        """
        result = self.client.search(
            f"session:{self.session_id}",
            workspace=self.workspace,
            limit=100,
        )
        memories = _extract_memories(result)
        for mem in memories:
            mem_id = mem.get("id")
            if mem_id is not None:
                self.client.delete(int(mem_id))


class EngramVectorStore:
    """LangChain VectorStore backed by Engram's hybrid search.

    Uses Engram's built-in embedding (TF-IDF or OpenAI, depending on server
    config) and hybrid BM25 + vector search.  No local embeddings are computed.

    Documents are stored as memories with tags ``['langchain', 'vector-store']``
    and metadata attached to each memory.
    """

    def __init__(
        self,
        client: EngramClient,
        workspace: str = "langchain-vectors",
    ) -> None:
        self.client = client
        self.workspace = workspace

    def add_texts(
        self,
        texts: List[str],
        metadatas: Optional[List[Dict[str, Any]]] = None,
        **kwargs: Any,
    ) -> List[str]:
        """Add texts to Engram and return their memory IDs.

        Args:
            texts: List of text strings to store.
            metadatas: Optional list of metadata dicts, one per text.
            **kwargs: Ignored (accepted for interface compatibility).

        Returns:
            List of memory IDs as strings.
        """
        ids: List[str] = []
        for i, text in enumerate(texts):
            meta = metadatas[i] if metadatas and i < len(metadatas) else {}
            result = self.client.create(
                content=text,
                workspace=self.workspace,
                tags=["langchain", "vector-store"],
                metadata=meta,
            )
            mem_id = _extract_id(result)
            ids.append(str(mem_id) if mem_id is not None else "")
        return ids

    def similarity_search(
        self,
        query: str,
        k: int = 4,
        **kwargs: Any,
    ) -> List[Dict[str, Any]]:
        """Search Engram using hybrid search and return Document-like dicts.

        Args:
            query: Search query string.
            k: Maximum number of results to return.
            **kwargs: Ignored (accepted for interface compatibility).

        Returns:
            List of dicts with keys ``page_content`` (str) and
            ``metadata`` (dict), mirroring the LangChain ``Document`` interface.
        """
        result = self.client.search(
            query=query,
            workspace=self.workspace,
            limit=k,
        )
        memories = _extract_memories(result)
        docs: List[Dict[str, Any]] = []
        for mem in memories:
            docs.append(
                {
                    "page_content": mem.get("content", ""),
                    "metadata": mem.get("metadata") or {},
                }
            )
        return docs


# ---------------------------------------------------------------------------
# Internal helpers
# ---------------------------------------------------------------------------


def _extract_memories(result: Any) -> List[Dict[str, Any]]:
    """Pull the list of memory dicts from an Engram MCP response.

    Engram returns results in a few shapes depending on the tool:
    - ``{"memories": [...]}``
    - ``{"results": [...]}``
    - A bare list ``[...]``
    - A single dict (treat as single-element list)
    """
    if isinstance(result, list):
        return result
    if isinstance(result, dict):
        for key in ("memories", "results", "items"):
            if key in result and isinstance(result[key], list):
                return result[key]
    return []


def _extract_id(result: Any) -> Optional[Any]:
    """Extract memory ID from a create-call response."""
    if isinstance(result, dict):
        for key in ("id", "memory_id"):
            if key in result:
                return result[key]
        # Nested under 'memory' key
        mem = result.get("memory")
        if isinstance(mem, dict):
            return mem.get("id")
    return None


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
