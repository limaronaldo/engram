"""Tests for the OpenAI Assistants threads adapter.

All tests use unittest.mock — no live Engram server or openai package
installation is required.
"""

from __future__ import annotations

import unittest
from typing import Any, Dict, List, Optional
from unittest.mock import MagicMock, call, patch

from engram_client.integrations.openai_threads import (
    EngramThreadStore,
    _extract_memories,
    _extract_message_text,
    _get_attr,
)


# ---------------------------------------------------------------------------
# Test doubles / fixtures
# ---------------------------------------------------------------------------


def _make_engram_client() -> MagicMock:
    """Return a MagicMock shaped like an EngramClient."""
    client = MagicMock()
    client.create.return_value = {"id": 1, "content": "synced message"}
    client.search.return_value = {"memories": []}
    return client


def _make_openai_client() -> MagicMock:
    """Return a MagicMock shaped like an openai.OpenAI client."""
    openai_client = MagicMock()
    return openai_client


def _make_message(
    message_id: str = "msg_001",
    role: str = "user",
    content_text: str = "Hello, assistant!",
    run_id: Optional[str] = None,
    assistant_id: Optional[str] = None,
    created_at: int = 1700000000,
) -> MagicMock:
    """Build a MagicMock that mimics an OpenAI message object.

    The ``content`` field is structured as a list of text blocks, mirroring
    the real OpenAI Assistants API response shape.
    """
    msg = MagicMock()
    msg.id = message_id
    msg.role = role
    msg.run_id = run_id
    msg.assistant_id = assistant_id
    msg.created_at = created_at

    # Content is a list of blocks; each block has type="text" and text.value
    block = MagicMock()
    block.type = "text"
    text_obj = MagicMock()
    text_obj.value = content_text
    block.text = text_obj
    msg.content = [block]

    return msg


def _make_list_response(messages: List[Any]) -> MagicMock:
    """Build a MagicMock simulating the OpenAI list response with .data."""
    response = MagicMock()
    response.data = messages
    return response


# ---------------------------------------------------------------------------
# EngramThreadStore.sync_thread
# ---------------------------------------------------------------------------


class TestSyncThread(unittest.TestCase):
    def test_sync_thread_fetches_messages_from_openai(self) -> None:
        """sync_thread must call the OpenAI messages.list API."""
        engram = _make_engram_client()
        openai = _make_openai_client()
        openai.beta.threads.messages.list.return_value = _make_list_response([])

        store = EngramThreadStore(engram, openai_client=openai)
        store.sync_thread("thread_abc")

        openai.beta.threads.messages.list.assert_called_once_with("thread_abc", limit=100)

    def test_sync_thread_stores_messages_in_engram(self) -> None:
        """sync_thread must create one Engram memory per new message."""
        engram = _make_engram_client()
        openai = _make_openai_client()
        msgs = [
            _make_message("msg_001", "user", "Hello"),
            _make_message("msg_002", "assistant", "Hi there", run_id="run_r1", assistant_id="asst_a1"),
        ]
        openai.beta.threads.messages.list.return_value = _make_list_response(msgs)

        store = EngramThreadStore(engram, openai_client=openai)
        results = store.sync_thread("thread_t1")

        self.assertEqual(engram.create.call_count, 2)
        self.assertEqual(len(results), 2)

    def test_sync_thread_applies_correct_tags(self) -> None:
        """Synced memories must include thread, role, and message tags."""
        engram = _make_engram_client()
        openai = _make_openai_client()
        msgs = [_make_message("msg_001", "user", "Test message")]
        openai.beta.threads.messages.list.return_value = _make_list_response(msgs)

        store = EngramThreadStore(engram, openai_client=openai)
        store.sync_thread("thread_t2")

        _, kwargs = engram.create.call_args
        self.assertIn("openai", kwargs["tags"])
        self.assertIn("thread:thread_t2", kwargs["tags"])
        self.assertIn("role:user", kwargs["tags"])
        self.assertIn("message:msg_001", kwargs["tags"])

    def test_sync_thread_includes_run_tag_when_run_id_present(self) -> None:
        """Messages with a run_id must be tagged with run:<run_id>."""
        engram = _make_engram_client()
        openai = _make_openai_client()
        msgs = [_make_message("msg_003", "assistant", "Response", run_id="run_xyz")]
        openai.beta.threads.messages.list.return_value = _make_list_response(msgs)

        store = EngramThreadStore(engram, openai_client=openai)
        store.sync_thread("thread_t3")

        _, kwargs = engram.create.call_args
        self.assertIn("run:run_xyz", kwargs["tags"])

    def test_sync_thread_stores_metadata(self) -> None:
        """Each synced memory must carry structured metadata."""
        engram = _make_engram_client()
        openai = _make_openai_client()
        msgs = [
            _make_message(
                "msg_004",
                "assistant",
                "Answer",
                run_id="run_r2",
                assistant_id="asst_a2",
                created_at=1700001000,
            )
        ]
        openai.beta.threads.messages.list.return_value = _make_list_response(msgs)

        store = EngramThreadStore(engram, openai_client=openai)
        store.sync_thread("thread_t4")

        _, kwargs = engram.create.call_args
        meta = kwargs["metadata"]
        self.assertEqual(meta["thread_id"], "thread_t4")
        self.assertEqual(meta["message_id"], "msg_004")
        self.assertEqual(meta["role"], "assistant")
        self.assertEqual(meta["run_id"], "run_r2")
        self.assertEqual(meta["assistant_id"], "asst_a2")
        self.assertEqual(meta["created_at"], 1700001000)

    def test_sync_thread_raises_without_openai_client(self) -> None:
        """sync_thread must raise RuntimeError if no OpenAI client is set."""
        engram = _make_engram_client()
        store = EngramThreadStore(engram)

        with self.assertRaises(RuntimeError):
            store.sync_thread("thread_noopenai")

    def test_sync_thread_uses_custom_workspace(self) -> None:
        """Messages must be stored in the configured workspace."""
        engram = _make_engram_client()
        openai = _make_openai_client()
        msgs = [_make_message("msg_005", "user", "Test")]
        openai.beta.threads.messages.list.return_value = _make_list_response(msgs)

        store = EngramThreadStore(engram, openai_client=openai, workspace="my-threads")
        store.sync_thread("thread_t5")

        _, kwargs = engram.create.call_args
        self.assertEqual(kwargs["workspace"], "my-threads")

    def test_sync_thread_skips_empty_messages(self) -> None:
        """Messages with no extractable text content must be skipped."""
        engram = _make_engram_client()
        openai = _make_openai_client()

        empty_msg = MagicMock()
        empty_msg.id = "msg_empty"
        empty_msg.role = "user"
        empty_msg.run_id = None
        empty_msg.assistant_id = None
        empty_msg.created_at = 1700000000
        empty_msg.content = []

        openai.beta.threads.messages.list.return_value = _make_list_response([empty_msg])

        store = EngramThreadStore(engram, openai_client=openai)
        results = store.sync_thread("thread_empty")

        engram.create.assert_not_called()
        self.assertEqual(results, [])


# ---------------------------------------------------------------------------
# EngramThreadStore.sync_run
# ---------------------------------------------------------------------------


class TestSyncRun(unittest.TestCase):
    def test_sync_run_filters_by_run_id(self) -> None:
        """sync_run must only store messages matching the supplied run_id."""
        engram = _make_engram_client()
        openai = _make_openai_client()
        msgs = [
            _make_message("msg_a", "user", "Question", run_id=None),
            _make_message("msg_b", "assistant", "Answer", run_id="run_target"),
            _make_message("msg_c", "assistant", "Other", run_id="run_other"),
        ]
        openai.beta.threads.messages.list.return_value = _make_list_response(msgs)

        store = EngramThreadStore(engram, openai_client=openai)
        results = store.sync_run("thread_r1", run_id="run_target")

        # Only msg_b matches
        self.assertEqual(engram.create.call_count, 1)
        self.assertEqual(len(results), 1)
        _, kwargs = engram.create.call_args
        self.assertIn("run:run_target", kwargs["tags"])

    def test_sync_run_raises_without_openai_client(self) -> None:
        """sync_run must raise RuntimeError if no OpenAI client is set."""
        engram = _make_engram_client()
        store = EngramThreadStore(engram)

        with self.assertRaises(RuntimeError):
            store.sync_run("thread_x", run_id="run_x")

    def test_sync_run_returns_empty_when_no_match(self) -> None:
        """sync_run returns empty list when no messages match the run_id."""
        engram = _make_engram_client()
        openai = _make_openai_client()
        msgs = [_make_message("msg_d", "user", "Hello", run_id="run_other")]
        openai.beta.threads.messages.list.return_value = _make_list_response(msgs)

        store = EngramThreadStore(engram, openai_client=openai)
        results = store.sync_run("thread_r2", run_id="run_missing")

        engram.create.assert_not_called()
        self.assertEqual(results, [])


# ---------------------------------------------------------------------------
# EngramThreadStore.search_threads
# ---------------------------------------------------------------------------


class TestSearchThreads(unittest.TestCase):
    def test_search_threads_delegates_to_client_search(self) -> None:
        """search_threads must call client.search with the given query."""
        engram = _make_engram_client()
        engram.search.return_value = {"memories": [{"id": 1, "content": "Hello"}]}
        store = EngramThreadStore(engram, openai_client=None)

        result = store.search_threads("billing question", limit=5)

        engram.search.assert_called_once_with(
            "billing question", workspace="openai-threads", limit=5
        )
        self.assertEqual(result, {"memories": [{"id": 1, "content": "Hello"}]})

    def test_search_threads_uses_custom_workspace(self) -> None:
        """search_threads must search in the configured workspace."""
        engram = _make_engram_client()
        store = EngramThreadStore(engram, openai_client=None, workspace="custom-ws")

        store.search_threads("query")

        _, kwargs = engram.search.call_args
        self.assertEqual(kwargs["workspace"], "custom-ws")


# ---------------------------------------------------------------------------
# Dedup: _already_synced
# ---------------------------------------------------------------------------


class TestDedup(unittest.TestCase):
    def test_already_synced_message_is_skipped(self) -> None:
        """Messages that are already in Engram must not be re-synced."""
        engram = _make_engram_client()
        # Simulate search returning a memory with matching message_id in metadata
        engram.search.return_value = {
            "memories": [
                {
                    "id": 99,
                    "content": "existing",
                    "metadata": {"message_id": "msg_dup"},
                }
            ]
        }
        openai = _make_openai_client()
        msgs = [_make_message("msg_dup", "user", "Duplicate message")]
        openai.beta.threads.messages.list.return_value = _make_list_response(msgs)

        store = EngramThreadStore(engram, openai_client=openai)
        results = store.sync_thread("thread_dedup")

        # create must NOT be called because the message is already synced
        engram.create.assert_not_called()
        self.assertEqual(results, [])

    def test_new_message_is_stored_when_search_returns_empty(self) -> None:
        """Messages not yet in Engram must be stored on sync."""
        engram = _make_engram_client()
        engram.search.return_value = {"memories": []}
        openai = _make_openai_client()
        msgs = [_make_message("msg_new", "user", "Fresh message")]
        openai.beta.threads.messages.list.return_value = _make_list_response(msgs)

        store = EngramThreadStore(engram, openai_client=openai)
        results = store.sync_thread("thread_fresh")

        engram.create.assert_called_once()
        self.assertEqual(len(results), 1)


# ---------------------------------------------------------------------------
# Internal helpers
# ---------------------------------------------------------------------------


class TestExtractMessageText(unittest.TestCase):
    def test_extracts_text_from_content_blocks(self) -> None:
        """Must extract text from a list of OpenAI content blocks."""
        msg = _make_message(content_text="Hello world")
        self.assertEqual(_extract_message_text(msg), "Hello world")

    def test_returns_plain_string_content(self) -> None:
        """Must handle messages where content is already a plain string."""
        msg = MagicMock()
        msg.content = "plain text"
        self.assertEqual(_extract_message_text(msg), "plain text")

    def test_returns_empty_for_none_content(self) -> None:
        """Must return empty string when content is None."""
        msg = MagicMock()
        msg.content = None
        self.assertEqual(_extract_message_text(msg), "")

    def test_skips_non_text_blocks(self) -> None:
        """Non-text blocks (e.g. image_file) must be ignored."""
        msg = MagicMock()
        img_block = MagicMock()
        img_block.type = "image_file"
        img_block.text = None
        msg.content = [img_block]
        self.assertEqual(_extract_message_text(msg), "")

    def test_concatenates_multiple_text_blocks(self) -> None:
        """Multiple text blocks must be joined with a space."""
        msg = MagicMock()
        block1 = MagicMock()
        block1.type = "text"
        block1.text = MagicMock()
        block1.text.value = "Part one."
        block2 = MagicMock()
        block2.type = "text"
        block2.text = MagicMock()
        block2.text.value = "Part two."
        msg.content = [block1, block2]
        self.assertEqual(_extract_message_text(msg), "Part one. Part two.")


class TestGetAttr(unittest.TestCase):
    def test_gets_attribute_from_object(self) -> None:
        obj = MagicMock()
        obj.foo = "bar"
        self.assertEqual(_get_attr(obj, "foo"), "bar")

    def test_falls_back_to_dict_key(self) -> None:
        self.assertEqual(_get_attr({"key": "value"}, "key"), "value")

    def test_returns_default_for_missing(self) -> None:
        self.assertIsNone(_get_attr({}, "missing"))

    def test_returns_default_for_none_object(self) -> None:
        self.assertIsNone(_get_attr(None, "anything"))


class TestExtractMemories(unittest.TestCase):
    def test_extracts_from_memories_key(self) -> None:
        self.assertEqual(_extract_memories({"memories": [{"id": 1}]}), [{"id": 1}])

    def test_extracts_from_results_key(self) -> None:
        self.assertEqual(_extract_memories({"results": [{"id": 2}]}), [{"id": 2}])

    def test_returns_list_directly(self) -> None:
        self.assertEqual(_extract_memories([{"id": 3}]), [{"id": 3}])

    def test_returns_empty_list_for_empty_dict(self) -> None:
        self.assertEqual(_extract_memories({}), [])

    def test_returns_empty_list_for_none(self) -> None:
        self.assertEqual(_extract_memories(None), [])


if __name__ == "__main__":
    unittest.main()
