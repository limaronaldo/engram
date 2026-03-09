"""Tests for the LangChain integration adapters.

All tests use unittest.mock so no live Engram server or langchain-core
installation is required.
"""

import unittest
from unittest.mock import MagicMock, call, patch

from engram_client.integrations.langchain import (
    EngramChatMessageHistory,
    EngramVectorStore,
    _extract_id,
    _extract_memories,
    _parse_message_content,
)


# ---------------------------------------------------------------------------
# Helpers / fixtures
# ---------------------------------------------------------------------------


def _make_client() -> MagicMock:
    """Return a MagicMock shaped like an EngramClient."""
    return MagicMock()


def _make_message(role: str, content: str) -> MagicMock:
    """Return a MagicMock shaped like a LangChain BaseMessage."""
    msg = MagicMock()
    msg.type = role
    msg.content = content
    return msg


# ---------------------------------------------------------------------------
# EngramChatMessageHistory tests
# ---------------------------------------------------------------------------


class TestEngramChatMessageHistoryAddMessage(unittest.TestCase):
    def test_add_message_calls_create_with_correct_content(self):
        client = _make_client()
        history = EngramChatMessageHistory(client, session_id="sess-1")
        msg = _make_message("human", "Hello!")

        history.add_message(msg)

        client.create.assert_called_once_with(
            content="[human] Hello!",
            tags=["langchain", "chat-history", "session:sess-1", "role:human"],
            workspace="langchain",
            metadata={"session_id": "sess-1", "role": "human"},
        )

    def test_add_message_uses_custom_workspace(self):
        client = _make_client()
        history = EngramChatMessageHistory(client, session_id="s2", workspace="my-ws")
        msg = _make_message("ai", "Hi there!")

        history.add_message(msg)

        _, kwargs = client.create.call_args
        self.assertEqual(kwargs["workspace"], "my-ws")

    def test_add_messages_calls_create_for_each_message(self):
        client = _make_client()
        history = EngramChatMessageHistory(client, session_id="sess-2")
        msgs = [
            _make_message("human", "First"),
            _make_message("ai", "Second"),
            _make_message("human", "Third"),
        ]

        history.add_messages(msgs)

        self.assertEqual(client.create.call_count, 3)

    def test_add_message_ai_role(self):
        client = _make_client()
        history = EngramChatMessageHistory(client, session_id="s")
        msg = _make_message("ai", "I am an AI.")

        history.add_message(msg)

        args, kwargs = client.create.call_args
        self.assertIn("role:ai", kwargs["tags"])
        self.assertEqual(kwargs["content"], "[ai] I am an AI.")


# ---------------------------------------------------------------------------
# EngramChatMessageHistory.messages property
# ---------------------------------------------------------------------------


class TestEngramChatMessageHistoryMessages(unittest.TestCase):
    def test_messages_calls_search_with_session_tag(self):
        client = _make_client()
        client.search.return_value = {"memories": []}
        history = EngramChatMessageHistory(client, session_id="sess-3")

        _ = history.messages

        client.search.assert_called_once_with(
            "session:sess-3",
            workspace="langchain",
            limit=100,
        )

    def test_messages_parses_returned_memories(self):
        client = _make_client()
        client.search.return_value = {
            "memories": [
                {"id": 1, "content": "[human] Hello"},
                {"id": 2, "content": "[ai] World"},
            ]
        }
        history = EngramChatMessageHistory(client, session_id="sess-4")

        msgs = history.messages

        self.assertEqual(len(msgs), 2)
        self.assertEqual(msgs[0], {"type": "human", "content": "Hello"})
        self.assertEqual(msgs[1], {"type": "ai", "content": "World"})

    def test_messages_returns_empty_list_when_no_results(self):
        client = _make_client()
        client.search.return_value = {"memories": []}
        history = EngramChatMessageHistory(client, session_id="empty-session")

        msgs = history.messages

        self.assertEqual(msgs, [])

    def test_messages_handles_results_key(self):
        """Engram may return 'results' instead of 'memories'."""
        client = _make_client()
        client.search.return_value = {
            "results": [
                {"id": 5, "content": "[human] test"},
            ]
        }
        history = EngramChatMessageHistory(client, session_id="s")

        msgs = history.messages

        self.assertEqual(len(msgs), 1)
        self.assertEqual(msgs[0]["type"], "human")


# ---------------------------------------------------------------------------
# EngramChatMessageHistory.clear
# ---------------------------------------------------------------------------


class TestEngramChatMessageHistoryClear(unittest.TestCase):
    def test_clear_deletes_all_session_memories(self):
        client = _make_client()
        client.search.return_value = {
            "memories": [
                {"id": 10, "content": "[human] msg1"},
                {"id": 11, "content": "[ai] msg2"},
            ]
        }
        history = EngramChatMessageHistory(client, session_id="sess-5")

        history.clear()

        client.delete.assert_any_call(10)
        client.delete.assert_any_call(11)
        self.assertEqual(client.delete.call_count, 2)

    def test_clear_does_nothing_when_no_messages(self):
        client = _make_client()
        client.search.return_value = {"memories": []}
        history = EngramChatMessageHistory(client, session_id="empty")

        history.clear()

        client.delete.assert_not_called()

    def test_clear_searches_with_correct_session_id(self):
        client = _make_client()
        client.search.return_value = {"memories": []}
        history = EngramChatMessageHistory(client, session_id="my-session", workspace="ws")

        history.clear()

        client.search.assert_called_once_with(
            "session:my-session",
            workspace="ws",
            limit=100,
        )


# ---------------------------------------------------------------------------
# EngramVectorStore.add_texts
# ---------------------------------------------------------------------------


class TestEngramVectorStoreAddTexts(unittest.TestCase):
    def test_add_texts_creates_one_memory_per_text(self):
        client = _make_client()
        client.create.return_value = {"id": 42}
        store = EngramVectorStore(client)

        ids = store.add_texts(["text one", "text two"])

        self.assertEqual(client.create.call_count, 2)
        self.assertEqual(ids, ["42", "42"])

    def test_add_texts_passes_metadata(self):
        client = _make_client()
        client.create.return_value = {"id": 1}
        store = EngramVectorStore(client)
        metas = [{"source": "doc1"}, {"source": "doc2"}]

        store.add_texts(["a", "b"], metadatas=metas)

        first_call_kwargs = client.create.call_args_list[0][1]
        self.assertEqual(first_call_kwargs["metadata"], {"source": "doc1"})
        second_call_kwargs = client.create.call_args_list[1][1]
        self.assertEqual(second_call_kwargs["metadata"], {"source": "doc2"})

    def test_add_texts_uses_custom_workspace(self):
        client = _make_client()
        client.create.return_value = {"id": 7}
        store = EngramVectorStore(client, workspace="my-vectors")

        store.add_texts(["hello"])

        _, kwargs = client.create.call_args
        self.assertEqual(kwargs["workspace"], "my-vectors")

    def test_add_texts_tags_memories_correctly(self):
        client = _make_client()
        client.create.return_value = {"id": 3}
        store = EngramVectorStore(client)

        store.add_texts(["sample text"])

        _, kwargs = client.create.call_args
        self.assertIn("langchain", kwargs["tags"])
        self.assertIn("vector-store", kwargs["tags"])

    def test_add_texts_handles_empty_list(self):
        client = _make_client()
        store = EngramVectorStore(client)

        ids = store.add_texts([])

        client.create.assert_not_called()
        self.assertEqual(ids, [])

    def test_add_texts_returns_empty_string_for_missing_id(self):
        client = _make_client()
        client.create.return_value = {}
        store = EngramVectorStore(client)

        ids = store.add_texts(["text"])

        self.assertEqual(ids, [""])


# ---------------------------------------------------------------------------
# EngramVectorStore.similarity_search
# ---------------------------------------------------------------------------


class TestEngramVectorStoreSimilaritySearch(unittest.TestCase):
    def test_similarity_search_calls_client_search(self):
        client = _make_client()
        client.search.return_value = {"memories": []}
        store = EngramVectorStore(client)

        store.similarity_search("what is AI?", k=3)

        client.search.assert_called_once_with(
            query="what is AI?",
            workspace="langchain-vectors",
            limit=3,
        )

    def test_similarity_search_returns_document_dicts(self):
        client = _make_client()
        client.search.return_value = {
            "memories": [
                {"id": 1, "content": "Paris is the capital of France.", "metadata": {"source": "geo"}},
                {"id": 2, "content": "Berlin is the capital of Germany.", "metadata": None},
            ]
        }
        store = EngramVectorStore(client)

        docs = store.similarity_search("capital", k=5)

        self.assertEqual(len(docs), 2)
        self.assertEqual(docs[0]["page_content"], "Paris is the capital of France.")
        self.assertEqual(docs[0]["metadata"], {"source": "geo"})
        self.assertEqual(docs[1]["page_content"], "Berlin is the capital of Germany.")
        self.assertEqual(docs[1]["metadata"], {})

    def test_similarity_search_returns_empty_list_when_no_results(self):
        client = _make_client()
        client.search.return_value = {"results": []}
        store = EngramVectorStore(client)

        docs = store.similarity_search("nothing")

        self.assertEqual(docs, [])

    def test_similarity_search_uses_k_as_limit(self):
        client = _make_client()
        client.search.return_value = {"memories": []}
        store = EngramVectorStore(client)

        store.similarity_search("query", k=10)

        _, kwargs = client.search.call_args
        self.assertEqual(kwargs["limit"], 10)


# ---------------------------------------------------------------------------
# Internal helper unit tests
# ---------------------------------------------------------------------------


class TestExtractMemories(unittest.TestCase):
    def test_extracts_from_memories_key(self):
        self.assertEqual(_extract_memories({"memories": [{"id": 1}]}), [{"id": 1}])

    def test_extracts_from_results_key(self):
        self.assertEqual(_extract_memories({"results": [{"id": 2}]}), [{"id": 2}])

    def test_returns_list_directly(self):
        self.assertEqual(_extract_memories([{"id": 3}]), [{"id": 3}])

    def test_returns_empty_list_for_empty_dict(self):
        self.assertEqual(_extract_memories({}), [])

    def test_returns_empty_list_for_none(self):
        self.assertEqual(_extract_memories(None), [])


class TestExtractId(unittest.TestCase):
    def test_extracts_id_key(self):
        self.assertEqual(_extract_id({"id": 5}), 5)

    def test_extracts_memory_id_key(self):
        self.assertEqual(_extract_id({"memory_id": 9}), 9)

    def test_extracts_nested_memory_id(self):
        self.assertEqual(_extract_id({"memory": {"id": 7}}), 7)

    def test_returns_none_for_missing_id(self):
        self.assertIsNone(_extract_id({}))

    def test_returns_none_for_non_dict(self):
        self.assertIsNone(_extract_id(None))


class TestParseMessageContent(unittest.TestCase):
    def test_parses_human_message(self):
        role, text = _parse_message_content("[human] Hello world")
        self.assertEqual(role, "human")
        self.assertEqual(text, "Hello world")

    def test_parses_ai_message(self):
        role, text = _parse_message_content("[ai] I am here to help.")
        self.assertEqual(role, "ai")
        self.assertEqual(text, "I am here to help.")

    def test_returns_unknown_for_unrecognised_format(self):
        role, text = _parse_message_content("no bracket here")
        self.assertEqual(role, "unknown")
        self.assertEqual(text, "no bracket here")

    def test_parses_system_message(self):
        role, text = _parse_message_content("[system] You are a helpful assistant.")
        self.assertEqual(role, "system")
        self.assertEqual(text, "You are a helpful assistant.")


if __name__ == "__main__":
    unittest.main()
