"""OpenAI Assistants API threads adapter for Engram memory engine.

Provides:
- EngramThreadStore: Syncs OpenAI Assistants API threads/messages into Engram
  session memories, with dedup, per-run scoping, and cross-thread search.

Usage::

    import openai
    from engram_client import EngramClient
    from engram_client.integrations.openai_threads import EngramThreadStore

    openai_client = openai.OpenAI(api_key="sk-...")
    engram_client = EngramClient(base_url="...", api_key="...", tenant="my-tenant")

    store = EngramThreadStore(engram_client, openai_client=openai_client)

    # Sync all messages from a thread
    synced = store.sync_thread("thread_abc123")

    # Sync only messages produced during a specific run
    synced = store.sync_run("thread_abc123", run_id="run_xyz789")

    # Search across all synced thread memories
    results = store.search_threads("user asked about billing")

No hard dependency on the ``openai`` package at import time — the OpenAI
client is accepted via duck typing.  The only contract is that it exposes:
    ``client.beta.threads.messages.list(thread_id, ...)``
    returning an object whose ``.data`` attribute is a list of message objects,
    each with ``.id``, ``.role``, ``.content``, ``.run_id``,
    ``.assistant_id``, and ``.created_at``.
"""

from __future__ import annotations

from typing import Any, Dict, List, Optional

from engram_client import EngramClient


class EngramThreadStore:
    """Sync OpenAI Assistants API threads into Engram session memories.

    Each message is stored as a single Engram memory within the configured
    workspace.  The message's OpenAI ID is stored as part of the metadata so
    that re-syncing a thread does **not** create duplicate memories (dedup).

    Tags applied to every synced message::

        ["openai", "thread:<thread_id>", "role:<role>"]

    When ``run_id`` is known, an additional tag is applied::

        "run:<run_id>"

    Metadata stored per memory::

        {
            "thread_id": "<thread_id>",
            "message_id": "<openai_message_id>",
            "role": "<user|assistant|...>",
            "run_id": "<run_id or null>",
            "assistant_id": "<assistant_id or null>",
            "created_at": <unix_timestamp_int>,
        }
    """

    def __init__(
        self,
        client: EngramClient,
        openai_client: Any = None,
        workspace: str = "openai-threads",
    ) -> None:
        """Initialise the thread store.

        Args:
            client: An :class:`~engram_client.EngramClient` instance.
            openai_client: An ``openai.OpenAI`` (or ``AsyncOpenAI``) client
                           instance.  Accepted via duck typing — no hard import
                           of the ``openai`` package is performed at module
                           level.  If ``None``, you must pass the client later
                           or use :meth:`sync_thread` with explicit message
                           data via a subclass.
            workspace: Engram workspace name for all synced thread memories.
                       Defaults to ``"openai-threads"``.
        """
        self.client = client
        self.openai_client = openai_client
        self.workspace = workspace

    # ------------------------------------------------------------------
    # Public API
    # ------------------------------------------------------------------

    def sync_thread(self, thread_id: str, limit: int = 100) -> List[Dict[str, Any]]:
        """Fetch all messages from an OpenAI thread and store them in Engram.

        Already-synced messages are skipped (dedup by ``message_id`` tag
        search before inserting).

        Args:
            thread_id: The OpenAI thread ID (e.g. ``"thread_abc123"``).
            limit: Maximum number of messages to fetch from the OpenAI API
                   in a single list call.  Defaults to 100.

        Returns:
            List of Engram API response dicts — one per **newly** stored
            message (already-synced messages are excluded).

        Raises:
            RuntimeError: If no OpenAI client was provided at construction
                          time.
        """
        if self.openai_client is None:
            raise RuntimeError(
                "An OpenAI client must be provided to EngramThreadStore before calling "
                "sync_thread().  Pass it as the openai_client= argument."
            )
        messages = self._fetch_messages(thread_id, limit=limit)
        return self._store_messages(thread_id, messages)

    def sync_run(self, thread_id: str, run_id: str, limit: int = 100) -> List[Dict[str, Any]]:
        """Sync messages from a specific assistant run within a thread.

        Only messages whose ``run_id`` matches the supplied value are stored.
        Dedup logic applies: already-synced messages are skipped.

        Args:
            thread_id: The OpenAI thread ID.
            run_id: The OpenAI run ID to filter messages by.
            limit: Maximum number of messages to fetch.

        Returns:
            List of Engram API response dicts for newly stored messages.

        Raises:
            RuntimeError: If no OpenAI client was provided at construction
                          time.
        """
        if self.openai_client is None:
            raise RuntimeError(
                "An OpenAI client must be provided to EngramThreadStore before calling "
                "sync_run().  Pass it as the openai_client= argument."
            )
        all_messages = self._fetch_messages(thread_id, limit=limit)
        run_messages = [m for m in all_messages if _get_attr(m, "run_id") == run_id]
        return self._store_messages(thread_id, run_messages, run_id=run_id)

    def search_threads(self, query: str, limit: int = 10) -> Dict[str, Any]:
        """Search across all synced thread memories using Engram hybrid search.

        Args:
            query: Free-text search query.
            limit: Maximum number of results to return.

        Returns:
            The Engram API search response dict (shape: ``{"memories": [...]}``)
        """
        return self.client.search(query, workspace=self.workspace, limit=limit)

    # ------------------------------------------------------------------
    # Internal helpers
    # ------------------------------------------------------------------

    def _fetch_messages(self, thread_id: str, limit: int = 100) -> List[Any]:
        """Fetch raw OpenAI message objects from the Assistants API.

        Args:
            thread_id: OpenAI thread ID.
            limit: Max messages to fetch.

        Returns:
            List of OpenAI message objects (duck-typed).
        """
        response = self.openai_client.beta.threads.messages.list(
            thread_id,
            limit=limit,
        )
        return list(_get_attr(response, "data") or [])

    def _already_synced(self, message_id: str) -> bool:
        """Check whether a message has already been synced to Engram.

        Searches for any memory tagged ``message:<message_id>`` in the
        workspace.  Returns ``True`` if at least one result is found.

        Args:
            message_id: The OpenAI message ID.

        Returns:
            ``True`` if the message is already stored in Engram.
        """
        result = self.client.search(
            f"message:{message_id}",
            workspace=self.workspace,
            limit=1,
        )
        memories = _extract_memories(result)
        # Verify by checking metadata to avoid false positives from full-text
        for mem in memories:
            meta = mem.get("metadata") or {}
            if meta.get("message_id") == message_id:
                return True
        return False

    def _store_messages(
        self,
        thread_id: str,
        messages: List[Any],
        run_id: Optional[str] = None,
    ) -> List[Dict[str, Any]]:
        """Store a list of OpenAI message objects into Engram.

        Skips messages that are already synced (dedup).

        Args:
            thread_id: OpenAI thread ID (used for tagging).
            messages: List of OpenAI message objects.
            run_id: Optional run ID to include as a tag and in metadata.

        Returns:
            List of Engram API response dicts for newly created memories.
        """
        results: List[Dict[str, Any]] = []
        for msg in messages:
            message_id = _get_attr(msg, "id") or ""
            if not message_id:
                continue

            if self._already_synced(message_id):
                continue

            role = _get_attr(msg, "role") or "unknown"
            msg_run_id = run_id or _get_attr(msg, "run_id")
            assistant_id = _get_attr(msg, "assistant_id")
            created_at = _get_attr(msg, "created_at")

            content_text = _extract_message_text(msg)
            if not content_text:
                continue

            tags = [
                "openai",
                f"thread:{thread_id}",
                f"role:{role}",
                f"message:{message_id}",
            ]
            if msg_run_id:
                tags.append(f"run:{msg_run_id}")

            metadata: Dict[str, Any] = {
                "thread_id": thread_id,
                "message_id": message_id,
                "role": role,
                "run_id": msg_run_id,
                "assistant_id": assistant_id,
                "created_at": created_at,
            }

            result = self.client.create(
                content=content_text,
                tags=tags,
                workspace=self.workspace,
                metadata=metadata,
            )
            results.append(result)

        return results


# ---------------------------------------------------------------------------
# Internal helpers
# ---------------------------------------------------------------------------


def _get_attr(obj: Any, attr: str, default: Any = None) -> Any:
    """Safely get an attribute from an object, falling back to dict-key access.

    Supports both attribute access (OpenAI SDK objects) and plain dict access
    (mock objects and test fixtures).

    Args:
        obj: The object to inspect.
        attr: Attribute / key name.
        default: Value to return if not found.

    Returns:
        The attribute value or ``default``.
    """
    if obj is None:
        return default
    value = getattr(obj, attr, _SENTINEL)
    if value is not _SENTINEL:
        return value
    if isinstance(obj, dict):
        return obj.get(attr, default)
    return default


_SENTINEL = object()


def _extract_message_text(message: Any) -> str:
    """Extract plain text from an OpenAI message object.

    OpenAI message objects have a ``content`` field that is a list of content
    blocks.  Each block has a ``type`` (``"text"``, ``"image_file"``, etc.)
    and a ``text`` sub-object with a ``value`` field.

    Falls back to treating ``content`` as a plain string if it is not a list
    (useful for test fixtures).

    Args:
        message: An OpenAI message object or duck-typed equivalent.

    Returns:
        Concatenated text content, stripped.  Empty string if no text found.
    """
    content = _get_attr(message, "content")
    if content is None:
        return ""
    if isinstance(content, str):
        return content.strip()
    if isinstance(content, list):
        parts: List[str] = []
        for block in content:
            if _get_attr(block, "type") == "text":
                text_obj = _get_attr(block, "text")
                value = _get_attr(text_obj, "value") if text_obj else None
                if value:
                    parts.append(str(value))
        return " ".join(parts).strip()
    return ""


def _extract_memories(result: Any) -> List[Dict[str, Any]]:
    """Pull the list of memory dicts from an Engram MCP response.

    Handles the shapes returned by different MCP tools:
    - ``{"memories": [...]}``
    - ``{"results": [...]}``
    - A bare list ``[...]``

    Args:
        result: Raw Engram API response.

    Returns:
        List of memory dicts.
    """
    if isinstance(result, list):
        return result
    if isinstance(result, dict):
        for key in ("memories", "results", "items"):
            if key in result and isinstance(result[key], list):
                return result[key]
    return []
