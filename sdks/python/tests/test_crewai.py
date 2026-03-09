"""Tests for the CrewAI integration.

All tests use MagicMock for EngramClient — no network calls are made.
"""

from __future__ import annotations

from unittest.mock import MagicMock, call

import pytest

from engram_client.integrations.crewai import (
    EngramEntityMemory,
    EngramLongTermMemory,
    EngramShortTermMemory,
)


# ---------------------------------------------------------------------------
# Fixtures
# ---------------------------------------------------------------------------


@pytest.fixture
def mock_client() -> MagicMock:
    """Return a MagicMock that stands in for EngramClient."""
    client = MagicMock()
    # Default return values that look like real Engram responses
    client.create_daily.return_value = {"id": 1, "content": "[key] value"}
    client.create.return_value = {"id": 2, "content": "[key] value"}
    client.search.return_value = {"memories": []}
    client.create_identity.return_value = {"canonical_id": "person:alice", "display_name": "Alice"}
    client.resolve_identity.return_value = {"canonical_id": "person:alice"}
    client.delete.return_value = {"deleted": True}
    return client


# ---------------------------------------------------------------------------
# EngramShortTermMemory
# ---------------------------------------------------------------------------


class TestEngramShortTermMemory:
    def test_save_calls_create_daily(self, mock_client: MagicMock) -> None:
        """save() must delegate to client.create_daily with correct arguments."""
        stm = EngramShortTermMemory(mock_client, workspace="crewai-stm", ttl_seconds=3600)

        stm.save("task_result", "All steps completed")

        mock_client.create_daily.assert_called_once_with(
            content="[task_result] All steps completed",
            tags=["crewai", "short-term", "key:task_result"],
            workspace="crewai-stm",
            ttl_seconds=3600,
            metadata={},
        )

    def test_save_passes_custom_metadata(self, mock_client: MagicMock) -> None:
        """save() must forward caller-supplied metadata to create_daily."""
        stm = EngramShortTermMemory(mock_client)
        meta = {"agent": "researcher", "step": 3}

        stm.save("intermediate", "partial findings", metadata=meta)

        _, kwargs = mock_client.create_daily.call_args
        assert kwargs["metadata"] == meta

    def test_search_calls_client_search(self, mock_client: MagicMock) -> None:
        """search() must call client.search with the correct workspace and limit."""
        stm = EngramShortTermMemory(mock_client, workspace="crewai-stm")

        stm.search("completed tasks", limit=3)

        mock_client.search.assert_called_once_with(
            "completed tasks", workspace="crewai-stm", limit=3
        )

    def test_reset_deletes_found_memories(self, mock_client: MagicMock) -> None:
        """reset() must search and delete all memories returned."""
        mock_client.search.return_value = {
            "memories": [{"id": 10}, {"id": 11}, {"id": 12}]
        }
        stm = EngramShortTermMemory(mock_client, workspace="crewai-stm")

        stm.reset()

        mock_client.search.assert_called_once_with(
            "crewai", workspace="crewai-stm", limit=100
        )
        assert mock_client.delete.call_count == 3
        mock_client.delete.assert_any_call(10)
        mock_client.delete.assert_any_call(11)
        mock_client.delete.assert_any_call(12)

    def test_reset_skips_entries_without_id(self, mock_client: MagicMock) -> None:
        """reset() must not crash when a memory dict lacks an 'id' key."""
        mock_client.search.return_value = {
            "memories": [{"content": "no id here"}, {"id": 5}]
        }
        stm = EngramShortTermMemory(mock_client)

        stm.reset()  # should not raise

        mock_client.delete.assert_called_once_with(5)

    def test_default_workspace_and_ttl(self, mock_client: MagicMock) -> None:
        """Constructor defaults: workspace='crewai-stm', ttl_seconds=3600."""
        stm = EngramShortTermMemory(mock_client)

        assert stm.workspace == "crewai-stm"
        assert stm.ttl_seconds == 3600


# ---------------------------------------------------------------------------
# EngramLongTermMemory
# ---------------------------------------------------------------------------


class TestEngramLongTermMemory:
    def test_save_calls_create(self, mock_client: MagicMock) -> None:
        """save() must delegate to client.create with correct arguments."""
        ltm = EngramLongTermMemory(mock_client, crew_name="research-crew")

        ltm.save("finding_1", "Paris is the capital of France")

        mock_client.create.assert_called_once_with(
            content="[finding_1] Paris is the capital of France",
            tags=["crewai", "long-term", "key:finding_1"],
            workspace="crewai-research-crew",
            metadata={},
        )

    def test_uses_crew_specific_workspace(self, mock_client: MagicMock) -> None:
        """The workspace must be derived from crew_name when not supplied."""
        ltm = EngramLongTermMemory(mock_client, crew_name="sales-team")

        assert ltm.workspace == "crewai-sales-team"

    def test_custom_workspace_overrides_crew_name(self, mock_client: MagicMock) -> None:
        """Explicit workspace parameter must take precedence over crew_name."""
        ltm = EngramLongTermMemory(
            mock_client, crew_name="ignored", workspace="my-custom-ws"
        )

        assert ltm.workspace == "my-custom-ws"

    def test_search_calls_client_search(self, mock_client: MagicMock) -> None:
        """search() must call client.search with the crew workspace."""
        ltm = EngramLongTermMemory(mock_client, crew_name="analysis")

        ltm.search("capital cities", limit=7)

        mock_client.search.assert_called_once_with(
            "capital cities", workspace="crewai-analysis", limit=7
        )

    def test_save_passes_metadata(self, mock_client: MagicMock) -> None:
        """save() must forward caller-supplied metadata to create."""
        ltm = EngramLongTermMemory(mock_client)
        meta = {"source": "wikipedia", "confidence": 0.95}

        ltm.save("fact", "The sky is blue", metadata=meta)

        _, kwargs = mock_client.create.call_args
        assert kwargs["metadata"] == meta


# ---------------------------------------------------------------------------
# EngramEntityMemory
# ---------------------------------------------------------------------------


class TestEngramEntityMemory:
    def test_save_entity_calls_create_identity(self, mock_client: MagicMock) -> None:
        """save_entity() must call client.create_identity with a canonical ID."""
        em = EngramEntityMemory(mock_client)

        em.save_entity("Alice Smith", "person", "Lead researcher")

        mock_client.create_identity.assert_called_once_with(
            canonical_id="person:alice_smith",
            display_name="Alice Smith (Lead researcher)",
            aliases=[],
        )

    def test_save_entity_passes_aliases(self, mock_client: MagicMock) -> None:
        """save_entity() must forward aliases to create_identity."""
        em = EngramEntityMemory(mock_client)
        aliases = ["alice@example.com", "+5511999999999"]

        em.save_entity("Alice", "person", "Engineer", aliases=aliases)

        _, kwargs = mock_client.create_identity.call_args
        assert kwargs["aliases"] == aliases

    def test_get_entity_resolves_canonical_id(self, mock_client: MagicMock) -> None:
        """get_entity() must resolve using the canonical ID format."""
        em = EngramEntityMemory(mock_client)

        em.get_entity("Bob Jones", entity_type="person")

        mock_client.resolve_identity.assert_called_once_with("person:bob_jones")

    def test_get_entity_default_type_is_person(self, mock_client: MagicMock) -> None:
        """get_entity() defaults entity_type to 'person'."""
        em = EngramEntityMemory(mock_client)

        em.get_entity("Carol")

        mock_client.resolve_identity.assert_called_once_with("person:carol")

    def test_search_entities_calls_client_search(self, mock_client: MagicMock) -> None:
        """search_entities() must call client.search in the entity workspace."""
        em = EngramEntityMemory(mock_client, workspace="crewai-entities")

        em.search_entities("researcher", limit=3)

        mock_client.search.assert_called_once_with(
            "researcher", workspace="crewai-entities", limit=3
        )

    def test_canonical_id_normalises_spaces(self, mock_client: MagicMock) -> None:
        """Spaces in entity names must be replaced by underscores in the ID."""
        em = EngramEntityMemory(mock_client)

        em.save_entity("Open AI", "company", "AI lab")

        _, kwargs = mock_client.create_identity.call_args
        assert kwargs["canonical_id"] == "company:open_ai"
