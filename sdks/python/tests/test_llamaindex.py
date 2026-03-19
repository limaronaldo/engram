"""Tests for the LlamaIndex integration.

All tests use MagicMock for EngramClient — no network calls are made.
Covers EngramDocumentStore, EngramLlamaIndexVectorStore, and EngramChatStore.
"""

from __future__ import annotations

from unittest.mock import MagicMock, call

import pytest

from engram_client.integrations.llamaindex import (
    EngramChatStore,
    EngramDocumentStore,
    EngramLlamaIndexVectorStore,
)


# ---------------------------------------------------------------------------
# Shared helpers
# ---------------------------------------------------------------------------


def _make_node(
    node_id: str = "node-1",
    content: str = "hello world",
    metadata: dict | None = None,
    ref_doc_id: str | None = "doc-1",
    hash_val: str | None = "abc123",
    node_type_name: str = "TextNode",
) -> MagicMock:
    """Return a MagicMock that mimics a LlamaIndex TextNode."""
    node = MagicMock()
    node.node_id = node_id
    node.get_content.return_value = content
    node.metadata = metadata or {}
    node.ref_doc_id = ref_doc_id
    node.hash = hash_val
    type(node).__name__ = node_type_name
    return node


def _make_message(role: str = "user", content: str = "hi") -> MagicMock:
    """Return a MagicMock that mimics a LlamaIndex ChatMessage."""
    msg = MagicMock()
    msg.role = role
    msg.content = content
    return msg


# ---------------------------------------------------------------------------
# Fixtures
# ---------------------------------------------------------------------------


@pytest.fixture
def mock_client() -> MagicMock:
    """Return a MagicMock that stands in for EngramClient."""
    client = MagicMock()
    client.create.return_value = {"id": 1, "content": "hello world"}
    client.search.return_value = {"memories": []}
    client.delete.return_value = {"deleted": True}
    client.update.return_value = {"id": 1, "updated": True}
    return client


# ===========================================================================
# EngramDocumentStore Tests (T1)
# ===========================================================================


class TestEngramDocumentStoreInit:
    def test_default_workspace(self, mock_client: MagicMock) -> None:
        """Constructor should default workspace to 'llamaindex-docs'."""
        store = EngramDocumentStore(mock_client)
        assert store.workspace == "llamaindex-docs"

    def test_custom_workspace(self, mock_client: MagicMock) -> None:
        """Constructor should accept a custom workspace."""
        store = EngramDocumentStore(mock_client, workspace="my-docs")
        assert store.workspace == "my-docs"


class TestEngramDocumentStoreAddDocuments:
    def test_add_single_document(self, mock_client: MagicMock) -> None:
        """add_documents() must call client.create once per node."""
        store = EngramDocumentStore(mock_client)
        node = _make_node(node_id="n1", content="Paris is the capital")

        store.add_documents([node])

        mock_client.create.assert_called_once()
        _, kwargs = mock_client.create.call_args
        assert kwargs["content"] == "Paris is the capital"
        assert "node:n1" in kwargs["tags"]
        assert "llamaindex" in kwargs["tags"]
        assert "docstore" in kwargs["tags"]
        assert kwargs["workspace"] == "llamaindex-docs"

    def test_add_multiple_documents(self, mock_client: MagicMock) -> None:
        """add_documents() must create one memory per node."""
        store = EngramDocumentStore(mock_client)
        nodes = [_make_node(node_id=f"n{i}") for i in range(3)]

        store.add_documents(nodes)

        assert mock_client.create.call_count == 3

    def test_add_documents_stores_metadata(self, mock_client: MagicMock) -> None:
        """add_documents() must include node metadata in the memory."""
        store = EngramDocumentStore(mock_client)
        node = _make_node(node_id="n1", hash_val="hash-xyz", ref_doc_id="doc-99")

        store.add_documents([node])

        _, kwargs = mock_client.create.call_args
        meta = kwargs["metadata"]
        assert meta["node_id"] == "n1"
        assert meta["hash"] == "hash-xyz"
        assert meta["ref_doc_id"] == "doc-99"

    def test_add_empty_list(self, mock_client: MagicMock) -> None:
        """add_documents([]) must not call client.create."""
        store = EngramDocumentStore(mock_client)
        store.add_documents([])
        mock_client.create.assert_not_called()


class TestEngramDocumentStoreGetDocument:
    def test_get_document_found(self, mock_client: MagicMock) -> None:
        """get_document() must return the first matching memory."""
        mock_client.search.return_value = {
            "memories": [{"id": 5, "content": "Paris info", "metadata": {"node_id": "n1"}}]
        }
        store = EngramDocumentStore(mock_client)

        doc = store.get_document("n1")

        assert doc is not None
        assert doc["id"] == 5
        mock_client.search.assert_called_once_with(
            "node:n1", workspace="llamaindex-docs", limit=1
        )

    def test_get_document_not_found(self, mock_client: MagicMock) -> None:
        """get_document() must return None when the search result is empty."""
        mock_client.search.return_value = {"memories": []}
        store = EngramDocumentStore(mock_client)

        doc = store.get_document("missing")

        assert doc is None

    def test_get_document_bare_list_response(self, mock_client: MagicMock) -> None:
        """get_document() must handle bare-list search responses."""
        mock_client.search.return_value = [{"id": 7, "content": "bare list"}]
        store = EngramDocumentStore(mock_client)

        doc = store.get_document("n7")

        assert doc is not None
        assert doc["id"] == 7


class TestEngramDocumentStoreDocumentExists:
    def test_document_exists_true(self, mock_client: MagicMock) -> None:
        """document_exists() must return True when the node is found."""
        mock_client.search.return_value = {"memories": [{"id": 1, "content": "exists"}]}
        store = EngramDocumentStore(mock_client)

        assert store.document_exists("n1") is True

    def test_document_exists_false(self, mock_client: MagicMock) -> None:
        """document_exists() must return False when the node is absent."""
        mock_client.search.return_value = {"memories": []}
        store = EngramDocumentStore(mock_client)

        assert store.document_exists("missing") is False


class TestEngramDocumentStoreDeleteDocument:
    def test_delete_found_document(self, mock_client: MagicMock) -> None:
        """delete_document() must delete the memory when the node is found."""
        mock_client.search.return_value = {"memories": [{"id": 42, "content": "node text"}]}
        store = EngramDocumentStore(mock_client)

        store.delete_document("n1")

        mock_client.delete.assert_called_once_with(42)

    def test_delete_multiple_matches(self, mock_client: MagicMock) -> None:
        """delete_document() must delete all memories matching the node tag."""
        mock_client.search.return_value = {
            "memories": [{"id": 10}, {"id": 11}]
        }
        store = EngramDocumentStore(mock_client)

        store.delete_document("n1")

        assert mock_client.delete.call_count == 2

    def test_delete_not_found_is_noop(self, mock_client: MagicMock) -> None:
        """delete_document() must silently succeed when the node is not found."""
        mock_client.search.return_value = {"memories": []}
        store = EngramDocumentStore(mock_client)

        store.delete_document("ghost")  # must not raise

        mock_client.delete.assert_not_called()


class TestEngramDocumentStoreHashOperations:
    def test_get_document_hash_found(self, mock_client: MagicMock) -> None:
        """get_document_hash() must return the hash stored in metadata."""
        mock_client.search.return_value = {
            "memories": [{"id": 1, "content": "text", "metadata": {"hash": "sha256abc"}}]
        }
        store = EngramDocumentStore(mock_client)

        result = store.get_document_hash("n1")

        assert result == "sha256abc"

    def test_get_document_hash_not_found_node(self, mock_client: MagicMock) -> None:
        """get_document_hash() must return None when the node does not exist."""
        mock_client.search.return_value = {"memories": []}
        store = EngramDocumentStore(mock_client)

        result = store.get_document_hash("missing")

        assert result is None

    def test_get_document_hash_no_hash_in_meta(self, mock_client: MagicMock) -> None:
        """get_document_hash() must return None when hash is absent from metadata."""
        mock_client.search.return_value = {
            "memories": [{"id": 1, "content": "text", "metadata": {}}]
        }
        store = EngramDocumentStore(mock_client)

        result = store.get_document_hash("n1")

        assert result is None

    def test_set_document_hash_updates_metadata(self, mock_client: MagicMock) -> None:
        """set_document_hash() must call client.update with the new hash."""
        mock_client.search.return_value = {
            "memories": [{"id": 3, "content": "text", "metadata": {"node_id": "n1"}}]
        }
        store = EngramDocumentStore(mock_client)

        store.set_document_hash("n1", "newhash")

        mock_client.update.assert_called_once_with(
            3, metadata={"node_id": "n1", "hash": "newhash"}
        )

    def test_set_document_hash_noop_when_not_found(self, mock_client: MagicMock) -> None:
        """set_document_hash() must silently do nothing when node is absent."""
        mock_client.search.return_value = {"memories": []}
        store = EngramDocumentStore(mock_client)

        store.set_document_hash("ghost", "hash")  # must not raise

        mock_client.update.assert_not_called()

    def test_get_all_document_hashes(self, mock_client: MagicMock) -> None:
        """get_all_document_hashes() must return node_id -> hash mapping."""
        mock_client.search.return_value = {
            "memories": [
                {"id": 1, "metadata": {"node_id": "n1", "hash": "h1"}},
                {"id": 2, "metadata": {"node_id": "n2", "hash": "h2"}},
                {"id": 3, "metadata": {"node_id": "n3"}},  # no hash
            ]
        }
        store = EngramDocumentStore(mock_client)

        result = store.get_all_document_hashes()

        assert result == {"n1": "h1", "n2": "h2"}

    def test_get_all_document_hashes_empty(self, mock_client: MagicMock) -> None:
        """get_all_document_hashes() must return empty dict when no nodes."""
        mock_client.search.return_value = {"memories": []}
        store = EngramDocumentStore(mock_client)

        result = store.get_all_document_hashes()

        assert result == {}


class TestEngramDocumentStoreGetNodes:
    def test_get_nodes_batch(self, mock_client: MagicMock) -> None:
        """get_nodes() must retrieve each node individually."""
        mock_client.search.side_effect = [
            {"memories": [{"id": 1, "content": "node 1"}]},
            {"memories": [{"id": 2, "content": "node 2"}]},
        ]
        store = EngramDocumentStore(mock_client)

        results = store.get_nodes(["n1", "n2"])

        assert len(results) == 2
        assert results[0]["id"] == 1
        assert results[1]["id"] == 2

    def test_get_nodes_skips_missing(self, mock_client: MagicMock) -> None:
        """get_nodes() must omit nodes that are not found."""
        mock_client.search.side_effect = [
            {"memories": [{"id": 1, "content": "found"}]},
            {"memories": []},
        ]
        store = EngramDocumentStore(mock_client)

        results = store.get_nodes(["n1", "missing"])

        assert len(results) == 1
        assert results[0]["id"] == 1

    def test_get_nodes_empty_list(self, mock_client: MagicMock) -> None:
        """get_nodes([]) must return an empty list without calling search."""
        store = EngramDocumentStore(mock_client)

        results = store.get_nodes([])

        assert results == []
        mock_client.search.assert_not_called()


# ===========================================================================
# EngramLlamaIndexVectorStore Tests (T2)
# ===========================================================================


class TestEngramLlamaIndexVectorStoreInit:
    def test_default_workspace(self, mock_client: MagicMock) -> None:
        """Constructor should default workspace to 'llamaindex-vectors'."""
        store = EngramLlamaIndexVectorStore(mock_client)
        assert store.workspace == "llamaindex-vectors"

    def test_custom_workspace(self, mock_client: MagicMock) -> None:
        """Constructor should accept a custom workspace."""
        store = EngramLlamaIndexVectorStore(mock_client, workspace="my-vecs")
        assert store.workspace == "my-vecs"


class TestEngramLlamaIndexVectorStoreAdd:
    def test_add_nodes_returns_ids(self, mock_client: MagicMock) -> None:
        """add() must return a list of memory IDs as strings."""
        mock_client.create.side_effect = [{"id": 10}, {"id": 11}]
        store = EngramLlamaIndexVectorStore(mock_client)
        nodes = [_make_node("n1"), _make_node("n2")]

        ids = store.add(nodes)

        assert ids == ["10", "11"]

    def test_add_tags_nodes_correctly(self, mock_client: MagicMock) -> None:
        """add() must tag each node with 'llamaindex', 'vector-store', 'node:<id>'."""
        mock_client.create.return_value = {"id": 1}
        store = EngramLlamaIndexVectorStore(mock_client)
        node = _make_node("n-abc")

        store.add([node])

        _, kwargs = mock_client.create.call_args
        assert "llamaindex" in kwargs["tags"]
        assert "vector-store" in kwargs["tags"]
        assert "node:n-abc" in kwargs["tags"]

    def test_add_empty_list(self, mock_client: MagicMock) -> None:
        """add([]) must return an empty list and not call create."""
        store = EngramLlamaIndexVectorStore(mock_client)

        ids = store.add([])

        assert ids == []
        mock_client.create.assert_not_called()

    def test_add_node_with_none_id_fallback(self, mock_client: MagicMock) -> None:
        """add() must still work when create returns no id."""
        mock_client.create.return_value = {}
        store = EngramLlamaIndexVectorStore(mock_client)
        node = _make_node("n1")

        ids = store.add([node])

        assert ids == [""]


class TestEngramLlamaIndexVectorStoreQuery:
    def test_query_returns_structured_result(self, mock_client: MagicMock) -> None:
        """query() must return dict with 'nodes', 'similarities', 'ids'."""
        mock_client.search.return_value = {
            "memories": [
                {"id": 5, "content": "Paris", "score": 0.9},
                {"id": 6, "content": "London", "score": 0.7},
            ]
        }
        store = EngramLlamaIndexVectorStore(mock_client)
        q = MagicMock()
        q.query_str = "capital cities"
        q.similarity_top_k = 2
        q.mode = "DEFAULT"

        result = store.query(q)

        assert len(result["nodes"]) == 2
        assert result["similarities"] == [0.9, 0.7]
        assert result["ids"] == ["5", "6"]

    def test_query_calls_search_with_correct_params(self, mock_client: MagicMock) -> None:
        """query() must translate VectorStoreQuery to client.search args."""
        mock_client.search.return_value = {"memories": []}
        store = EngramLlamaIndexVectorStore(mock_client)
        q = MagicMock()
        q.query_str = "machine learning"
        q.similarity_top_k = 5
        q.mode = "DEFAULT"

        store.query(q)

        mock_client.search.assert_called_once_with(
            "machine learning", workspace="llamaindex-vectors", limit=5
        )

    def test_query_empty_results(self, mock_client: MagicMock) -> None:
        """query() on an empty store must return empty lists."""
        mock_client.search.return_value = {"memories": []}
        store = EngramLlamaIndexVectorStore(mock_client)
        q = MagicMock()
        q.query_str = "nothing"
        q.similarity_top_k = 4
        q.mode = "DEFAULT"

        result = store.query(q)

        assert result["nodes"] == []
        assert result["similarities"] == []
        assert result["ids"] == []

    def test_query_sparse_mode_passes_mode_hint(self, mock_client: MagicMock) -> None:
        """query() in SPARSE mode must pass mode='sparse' to search."""
        mock_client.search.return_value = {"memories": []}
        store = EngramLlamaIndexVectorStore(mock_client)
        q = MagicMock()
        q.query_str = "sparse query"
        q.similarity_top_k = 3
        q.mode = "SPARSE"

        store.query(q)

        _, kwargs = mock_client.search.call_args
        assert kwargs.get("mode") == "sparse"

    def test_query_missing_score_defaults_to_zero(self, mock_client: MagicMock) -> None:
        """query() must default similarity score to 0.0 when absent."""
        mock_client.search.return_value = {
            "memories": [{"id": 1, "content": "no score"}]
        }
        store = EngramLlamaIndexVectorStore(mock_client)
        q = MagicMock()
        q.query_str = "test"
        q.similarity_top_k = 1
        q.mode = "DEFAULT"

        result = store.query(q)

        assert result["similarities"] == [0.0]


class TestEngramLlamaIndexVectorStoreDelete:
    def test_delete_node(self, mock_client: MagicMock) -> None:
        """delete() must find and delete the node by its tag."""
        mock_client.search.return_value = {"memories": [{"id": 99, "content": "x"}]}
        store = EngramLlamaIndexVectorStore(mock_client)

        store.delete("n-99")

        mock_client.delete.assert_called_once_with(99)

    def test_delete_not_found_is_noop(self, mock_client: MagicMock) -> None:
        """delete() must not raise when the node is not found."""
        mock_client.search.return_value = {"memories": []}
        store = EngramLlamaIndexVectorStore(mock_client)

        store.delete("ghost")  # must not raise

        mock_client.delete.assert_not_called()

    def test_delete_nodes_batch(self, mock_client: MagicMock) -> None:
        """delete_nodes() must delete each node in the list."""
        mock_client.search.side_effect = [
            {"memories": [{"id": 10}]},
            {"memories": [{"id": 11}]},
        ]
        store = EngramLlamaIndexVectorStore(mock_client)

        store.delete_nodes(["n1", "n2"])

        assert mock_client.delete.call_count == 2
        mock_client.delete.assert_any_call(10)
        mock_client.delete.assert_any_call(11)


# ===========================================================================
# EngramChatStore Tests (T3)
# ===========================================================================


class TestEngramChatStoreInit:
    def test_default_workspace(self, mock_client: MagicMock) -> None:
        """Constructor should default workspace to 'llamaindex-chat'."""
        store = EngramChatStore(mock_client)
        assert store.workspace == "llamaindex-chat"

    def test_custom_workspace(self, mock_client: MagicMock) -> None:
        """Constructor should accept a custom workspace."""
        store = EngramChatStore(mock_client, workspace="my-chat")
        assert store.workspace == "my-chat"


class TestEngramChatStoreSetMessages:
    def test_set_messages_replaces_existing(self, mock_client: MagicMock) -> None:
        """set_messages() must delete existing messages before storing new ones."""
        # First call (delete_messages search) returns 1 old message
        mock_client.search.side_effect = [
            {"memories": [{"id": 1, "content": "[user] old"}]},  # delete_messages
            {"memories": []},  # get_messages in delete (inner search if any)
        ]
        store = EngramChatStore(mock_client)
        msgs = [_make_message("user", "hello")]

        store.set_messages("sess-1", msgs)

        # Old message deleted
        mock_client.delete.assert_called_once_with(1)
        # New message created
        mock_client.create.assert_called_once()
        _, kwargs = mock_client.create.call_args
        assert "[user] hello" == kwargs["content"]

    def test_set_messages_empty_list(self, mock_client: MagicMock) -> None:
        """set_messages() with empty list must only delete existing messages."""
        mock_client.search.return_value = {"memories": []}
        store = EngramChatStore(mock_client)

        store.set_messages("sess-1", [])

        mock_client.create.assert_not_called()


class TestEngramChatStoreGetMessages:
    def test_get_messages_returns_parsed_list(self, mock_client: MagicMock) -> None:
        """get_messages() must return list of {role, content} dicts."""
        mock_client.search.return_value = {
            "memories": [
                {"id": 1, "content": "[user] hello"},
                {"id": 2, "content": "[assistant] hi there"},
            ]
        }
        store = EngramChatStore(mock_client)

        msgs = store.get_messages("sess-1")

        assert len(msgs) == 2
        assert msgs[0] == {"role": "user", "content": "hello"}
        assert msgs[1] == {"role": "assistant", "content": "hi there"}

    def test_get_messages_empty_session(self, mock_client: MagicMock) -> None:
        """get_messages() must return empty list for unknown session."""
        mock_client.search.return_value = {"memories": []}
        store = EngramChatStore(mock_client)

        msgs = store.get_messages("unknown")

        assert msgs == []

    def test_get_messages_calls_correct_workspace(self, mock_client: MagicMock) -> None:
        """get_messages() must search in the correct workspace."""
        mock_client.search.return_value = {"memories": []}
        store = EngramChatStore(mock_client, workspace="custom-chat")

        store.get_messages("sess-x")

        mock_client.search.assert_called_once_with(
            "session:sess-x", workspace="custom-chat", limit=500
        )


class TestEngramChatStoreAddMessage:
    def test_add_message_calls_create(self, mock_client: MagicMock) -> None:
        """add_message() must call client.create with correct args."""
        store = EngramChatStore(mock_client)
        msg = _make_message("user", "how are you?")

        store.add_message("sess-1", msg)

        mock_client.create.assert_called_once_with(
            content="[user] how are you?",
            tags=["llamaindex", "chat-store", "session:sess-1", "role:user"],
            workspace="llamaindex-chat",
            metadata={"session_key": "sess-1", "role": "user"},
        )

    def test_add_message_assistant_role(self, mock_client: MagicMock) -> None:
        """add_message() must handle assistant role correctly."""
        store = EngramChatStore(mock_client)
        msg = _make_message("assistant", "I am fine!")

        store.add_message("sess-2", msg)

        _, kwargs = mock_client.create.call_args
        assert kwargs["content"] == "[assistant] I am fine!"
        assert "role:assistant" in kwargs["tags"]


class TestEngramChatStoreDeleteMessages:
    def test_delete_messages_returns_deleted(self, mock_client: MagicMock) -> None:
        """delete_messages() must return list of deleted message dicts."""
        mock_client.search.return_value = {
            "memories": [
                {"id": 1, "content": "[user] bye"},
                {"id": 2, "content": "[assistant] see ya"},
            ]
        }
        store = EngramChatStore(mock_client)

        deleted = store.delete_messages("sess-1")

        assert deleted is not None
        assert len(deleted) == 2
        assert deleted[0] == {"role": "user", "content": "bye"}
        assert deleted[1] == {"role": "assistant", "content": "see ya"}
        assert mock_client.delete.call_count == 2

    def test_delete_messages_returns_none_when_empty(self, mock_client: MagicMock) -> None:
        """delete_messages() must return None when the session has no messages."""
        mock_client.search.return_value = {"memories": []}
        store = EngramChatStore(mock_client)

        result = store.delete_messages("empty-sess")

        assert result is None
        mock_client.delete.assert_not_called()


class TestEngramChatStoreDeleteMessage:
    def test_delete_message_by_index(self, mock_client: MagicMock) -> None:
        """delete_message() must delete the message at the given index."""
        mock_client.search.return_value = {
            "memories": [
                {"id": 10, "content": "[user] first"},
                {"id": 11, "content": "[user] second"},
                {"id": 12, "content": "[user] third"},
            ]
        }
        store = EngramChatStore(mock_client)

        deleted = store.delete_message("sess-1", idx=1)

        assert deleted == {"role": "user", "content": "second"}
        mock_client.delete.assert_called_once_with(11)

    def test_delete_message_out_of_range(self, mock_client: MagicMock) -> None:
        """delete_message() must return None when index is out of range."""
        mock_client.search.return_value = {
            "memories": [{"id": 10, "content": "[user] only"}]
        }
        store = EngramChatStore(mock_client)

        result = store.delete_message("sess-1", idx=5)

        assert result is None
        mock_client.delete.assert_not_called()

    def test_delete_message_negative_index(self, mock_client: MagicMock) -> None:
        """delete_message() with negative index must return None."""
        mock_client.search.return_value = {
            "memories": [{"id": 10, "content": "[user] msg"}]
        }
        store = EngramChatStore(mock_client)

        result = store.delete_message("sess-1", idx=-1)

        assert result is None


class TestEngramChatStoreDeleteLastMessage:
    def test_delete_last_message(self, mock_client: MagicMock) -> None:
        """delete_last_message() must delete and return the last message."""
        mock_client.search.return_value = {
            "memories": [
                {"id": 20, "content": "[user] first"},
                {"id": 21, "content": "[assistant] last"},
            ]
        }
        store = EngramChatStore(mock_client)

        deleted = store.delete_last_message("sess-1")

        assert deleted == {"role": "assistant", "content": "last"}
        mock_client.delete.assert_called_once_with(21)

    def test_delete_last_message_empty_session(self, mock_client: MagicMock) -> None:
        """delete_last_message() must return None for empty sessions."""
        mock_client.search.return_value = {"memories": []}
        store = EngramChatStore(mock_client)

        result = store.delete_last_message("empty")

        assert result is None
        mock_client.delete.assert_not_called()


class TestEngramChatStoreGetKeys:
    def test_get_keys_extracts_unique_sessions(self, mock_client: MagicMock) -> None:
        """get_keys() must return unique session keys sorted."""
        mock_client.search.return_value = {
            "memories": [
                {"id": 1, "tags": ["llamaindex", "session:sess-b", "role:user"]},
                {"id": 2, "tags": ["llamaindex", "session:sess-a", "role:user"]},
                {"id": 3, "tags": ["llamaindex", "session:sess-b", "role:assistant"]},
            ]
        }
        store = EngramChatStore(mock_client)

        keys = store.get_keys()

        assert keys == ["sess-a", "sess-b"]

    def test_get_keys_empty_workspace(self, mock_client: MagicMock) -> None:
        """get_keys() must return empty list when no messages exist."""
        mock_client.search.return_value = {"memories": []}
        store = EngramChatStore(mock_client)

        keys = store.get_keys()

        assert keys == []

    def test_get_keys_no_duplicate_sessions(self, mock_client: MagicMock) -> None:
        """get_keys() must deduplicate session keys."""
        mock_client.search.return_value = {
            "memories": [
                {"id": i, "tags": ["session:same-session"]} for i in range(5)
            ]
        }
        store = EngramChatStore(mock_client)

        keys = store.get_keys()

        assert keys == ["same-session"]
