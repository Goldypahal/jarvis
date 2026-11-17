"""
Post-Turn Memory Extractor for NEXUS.
Runs asynchronously in the background following a session turn.
Analyzes conversations to extract episodic interactions, semantic preferences,
and procedural knowledge into persistent memory tiers.
"""

from typing import Any, Dict, List, Optional
import uuid
import logging
from pydantic import BaseModel, Field

from nexus.models.schemas import NexusIntent, IntentType
from nexus.models.router import ModelRouter
from nexus.core.memory import MemorySystem, MemoryEntry, MemoryType

logger = logging.getLogger(__name__)


class ExtractedMemoryItem(BaseModel):
    should_remember: bool = True
    memory_type: str = Field(description="'episodic', 'semantic', 'procedural', or 'relational'")
    summary: str = Field(description="Brief declarative statement of the memory")
    preference_key: Optional[str] = None
    preference_value: Optional[str] = None
    importance_score: float = Field(default=0.7, ge=0.0, le=1.0)
    tags: List[str] = Field(default_factory=list)


class MemoryExtractor:
    """
    Extracts high-value facts, user preferences, and procedural lessons from conversations.
    """

    def __init__(self, memory_system: MemorySystem, model_router: Optional[ModelRouter] = None):
        self.memory_system = memory_system
        self.model_router = model_router

    async def extract_and_store(
        self,
        user_query: str,
        response_text: str,
        intent: Optional[NexusIntent] = None
    ) -> List[MemoryEntry]:
        """
        Analyze the completed interaction and store any newly discovered memories.
        """
        extracted_entries = []

        # 1. Fast deterministic heuristic extraction
        heuristic_items = self._extract_heuristics(user_query, response_text)
        for item in heuristic_items:
            entry = await self._persist_item(item)
            if entry:
                extracted_entries.append(entry)

        # 2. LLM-based cognitive extraction if router is available and query has substance
        if self.model_router and len(user_query.split()) > 4 and not heuristic_items:
            prompt = (
                f"User Message: '{user_query}'\n"
                f"Assistant Response: '{response_text[:300]}'\n\n"
                f"Did the user state a durable personal preference, ongoing project detail, "
                f"or procedural workflow that should be remembered across future sessions?\n"
                f"Extract structured memory item conforming to schema."
            )
            try:
                llm_item = await self.model_router.route_and_generate_structured(
                    prompt=prompt,
                    schema_cls=ExtractedMemoryItem,
                    difficulty_score=0.25,  # Low difficulty suitable for 1-3B personal SLM
                    requires_tools=False
                )
                if llm_item.should_remember:
                    entry = await self._persist_item(llm_item)
                    if entry:
                        extracted_entries.append(entry)
            except Exception as e:
                logger.debug(f"LLM memory extraction skipped or failed: {e}")

        return extracted_entries

    def _extract_heuristics(self, query: str, response: str) -> List[ExtractedMemoryItem]:
        """Instant pattern-based preference extraction."""
        items = []
        q_lower = query.lower()

        # Preference patterns: "i prefer X", "always use X", "my favorite X"
        if "i prefer " in q_lower:
            parts = q_lower.split("i prefer ", 1)[1].strip(" .!?,")
            items.append(
                ExtractedMemoryItem(
                    should_remember=True,
                    memory_type="semantic",
                    summary=f"User prefers {parts}",
                    preference_key="general_preference",
                    preference_value=parts,
                    importance_score=0.85,
                    tags=["preference", "user_stated"]
                )
            )

        if "my project is " in q_lower:
            proj = q_lower.split("my project is ", 1)[1].strip(" .!?,")
            items.append(
                ExtractedMemoryItem(
                    should_remember=True,
                    memory_type="relational",
                    summary=f"Active user project: {proj}",
                    importance_score=0.9,
                    tags=["project", proj]
                )
            )

        return items

    async def _persist_item(self, item: ExtractedMemoryItem) -> Optional[MemoryEntry]:
        """Save extracted memory to persistent storage."""
        if not item.should_remember:
            return None

        # If it's an evolving preference, record with historical tracking
        if item.preference_key and item.preference_value:
            return await self.memory_system.record_preference_evolution(
                preference_key=item.preference_key,
                new_value=item.preference_value,
                reason="Extracted from user conversation"
            )

        # Otherwise store standard memory entry
        mtype_map = {
            "episodic": MemoryType.EPISODIC,
            "semantic": MemoryType.SEMANTIC,
            "procedural": MemoryType.PROCEDURAL,
            "relational": MemoryType.RELATIONAL
        }
        mem_type = mtype_map.get(item.memory_type.lower(), MemoryType.SEMANTIC)
        entry_id = f"mem_{uuid.uuid4().hex[:8]}"

        entry = MemoryEntry(
            id=entry_id,
            memory_type=mem_type,
            content={"summary": item.summary, "details": item.model_dump()},
            importance_score=item.importance_score,
            tags=item.tags
        )
        await self.memory_system.store_memory(entry)
        logger.info(f"Formed new {mem_type.value} memory: '{item.summary}'")
        return entry
