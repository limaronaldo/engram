"""Engram integrations with popular AI frameworks."""

from engram_client.integrations.crewai import (
    EngramEntityMemory,
    EngramLongTermMemory,
    EngramShortTermMemory,
)
from engram_client.integrations.langchain import EngramChatMessageHistory, EngramVectorStore
from engram_client.integrations.openai_threads import EngramThreadStore

try:
    from engram_client.integrations.llamaindex import (
        EngramChatStore,
        EngramDocumentStore,
        EngramLlamaIndexVectorStore,
    )
    _llamaindex_available = True
except ImportError:
    _llamaindex_available = False

__all__ = [
    "EngramChatMessageHistory",
    "EngramVectorStore",
    "EngramShortTermMemory",
    "EngramLongTermMemory",
    "EngramEntityMemory",
    "EngramThreadStore",
    "EngramDocumentStore",
    "EngramLlamaIndexVectorStore",
    "EngramChatStore",
]
