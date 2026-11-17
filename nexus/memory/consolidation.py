"""
Memory Consolidation & Contradiction Resolution for NEXUS.
Detects contradictions between newly learned information and historical memories,
evolves preferences chronologically, and prunes stale decayed entries.
"""

from typing import Any, Dict, List, Optional
import logging
from datetime import datetime
from nexus.core.memory import MemorySystem, MemoryEntry, MemoryType

logger = logging.getLogger(__name__)


class MemoryConsolidator:
    """
    Manages the long-term integrity and evolution of the memory database.
    """

    def __init__(self, memory_system: MemorySystem):
        self.memory_system = memory_system

    async def detect_and_resolve_contradiction(
        self,
        new_entry: MemoryEntry,
        existing_entries: Optional[List[MemoryEntry]] = None
    ) -> Optional[MemoryEntry]:
        """
        Check if new_entry contradicts an existing memory.
        If a contradiction is found, records historical evolution on the entry.
        """
        if existing_entries is None:
            existing_entries = await self.memory_system.get_memories(
                memory_type=new_entry.memory_type,
                limit=100
            )

        new_content = new_entry.content
        pref_key = new_content.get("preference_key")

        if pref_key:
            for old_entry in existing_entries:
                if old_entry.content.get("preference_key") == pref_key and old_entry.id != new_entry.id:
                    old_val = old_entry.content.get("value")
                    new_val = new_content.get("value")
                    if old_val != new_val:
                        # Contradiction detected: evolve the memory
                        logger.info(
                            f"Resolved memory contradiction on '{pref_key}': "
                            f"evolved from '{old_val}' to '{new_val}'"
                        )
                        old_entry.evolution_history.append({
                            "previous_value": old_val,
                            "timestamp": datetime.now().isoformat(),
                            "reason": "Contradiction resolved with newer statement"
                        })
                        old_entry.content["value"] = new_val
                        old_entry.importance_score = max(old_entry.importance_score, new_entry.importance_score)
                        old_entry.last_accessed = datetime.now()
                        await self.memory_system.store_memory(old_entry)
                        return old_entry

        return None

    async def prune_stale_memories(self, min_effective_importance: float = 0.1, decay_rate: float = 0.05) -> int:
        """
        Prunes ephemeral memories whose time-decayed effective importance has fallen below threshold.
        Permanent preferences (importance >= 0.8) are preserved.
        """
        memories = await self.memory_system.get_memories(limit=500)
        pruned_count = 0

        for entry in memories:
            # Never prune high importance core memories or permanent preferences
            if entry.importance_score >= 0.8 or "permanent" in entry.tags:
                continue

            effective_score = entry.calculate_effective_importance(decay_rate=decay_rate)
            if effective_score < min_effective_importance:
                # Delete decayed entry
                try:
                    cursor = self.memory_system.connection.cursor()
                    cursor.execute("DELETE FROM memories WHERE id = ?", (entry.id,))
                    self.memory_system.connection.commit()
                    self.memory_system._cache.pop(entry.id, None)
                    pruned_count += 1
                except Exception as e:
                    logger.error(f"Error pruning memory {entry.id}: {e}")

        logger.info(f"Consolidated memory store: pruned {pruned_count} decayed memories.")
        return pruned_count
