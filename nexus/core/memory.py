"""
Memory system for NEXUS
Implements persistent memory with different tiers: episodic, semantic, procedural
"""

from typing import Any, Dict, List, Optional
from datetime import datetime, timedelta
import asyncio
import logging
import json
import sqlite3
import os
from dataclasses import dataclass, asdict, field
from enum import Enum

logger = logging.getLogger(__name__)


class MemoryType(Enum):
    """Types of memory in the NEXUS system"""
    EPISODIC = "episodic"    # Specific events and interactions
    SEMANTIC = "semantic"    # Factual knowledge and learned preferences
    PROCEDURAL = "procedural" # Skills, routines, and workflows
    RELATIONAL = "relational" # Information about people, projects, organizations


@dataclass
class MemoryEntry:
    """Base class for memory entries"""
    id: str
    memory_type: MemoryType
    content: Dict[str, Any]
    timestamp: datetime = field(default_factory=datetime.now)
    last_accessed: datetime = field(default_factory=datetime.now)
    access_count: int = 0
    importance_score: float = 0.5  # 0.0 to 1.0
    tags: List[str] = field(default_factory=list)
    expires_at: Optional[datetime] = None

    def to_dict(self) -> Dict[str, Any]:
        """Convert to dictionary for storage"""
        data = asdict(self)
        data['memory_type'] = self.memory_type.value
        data['timestamp'] = self.timestamp.isoformat()
        data['last_accessed'] = self.last_accessed.isoformat()
        if self.expires_at:
            data['expires_at'] = self.expires_at.isoformat()
        return data

    @classmethod
    def from_dict(cls, data: Dict[str, Any]) -> 'MemoryEntry':
        """Create from dictionary"""
        data['memory_type'] = MemoryType(data['memory_type'])
        data['timestamp'] = datetime.fromisoformat(data['timestamp'])
        data['last_accessed'] = datetime.fromisoformat(data['last_accessed'])
        if data.get('expires_at'):
            data['expires_at'] = datetime.fromisoformat(data['expires_at'])
        return cls(**data)


class MemorySystem:
    """
    Persistent memory system for NEXUS with multiple memory tiers.
    Uses SQLite for storage with JSON serialization for flexible content.
    """

    def __init__(self, db_path: str = "nexus_memory.db"):
        self.db_path = db_path
        self.connection: Optional[sqlite3.Connection] = None
        self._cache: Dict[str, MemoryEntry] = {}
        self._initialize_database()

    def _initialize_database(self) -> None:
        """Initialize the SQLite database and create tables if needed."""
        try:
            self.connection = sqlite3.connect(self.db_path, check_same_thread=False)
            self.connection.execute("""
                CREATE TABLE IF NOT EXISTS memories (
                    id TEXT PRIMARY KEY,
                    memory_type TEXT NOT NULL,
                    content TEXT NOT NULL,  -- JSON serialized
                    timestamp TEXT NOT NULL,
                    last_accessed TEXT NOT NULL,
                    access_count INTEGER DEFAULT 0,
                    importance_score REAL DEFAULT 0.5,
                    tags TEXT DEFAULT '[]',  -- JSON array
                    expires_at TEXT
                )
            """)
            self.connection.execute("""
                CREATE INDEX IF NOT EXISTS idx_memory_type ON memories(memory_type)
            """)
            self.connection.execute("""
                CREATE INDEX IF NOT EXISTS idx_timestamp ON memories(timestamp)
            """)
            self.connection.execute("""
                CREATE INDEX IF NOT EXISTS idx_importance ON memories(importance_score)
            """)
            self.connection.commit()
            logger.info(f"Initialized memory database at {self.db_path}")
        except Exception as e:
            logger.error(f"Failed to initialize memory database: {e}")
            raise

    def _close_connection(self) -> None:
        """Close the database connection."""
        if self.connection:
            self.connection.close()
            self.connection = None

    def close(self) -> None:
        """Public method to close the database connection."""
        self._close_connection()

    def __del__(self):
        """Cleanup on deletion."""
        self._close_connection()

    def _row_to_memory_entry(self, row: tuple) -> MemoryEntry:
        """Convert a database row to a MemoryEntry."""
        (
            id_val, memory_type_val, content_val, timestamp_val,
            last_accessed_val, access_count_val, importance_score_val,
            tags_val, expires_at_val
        ) = row

        entry = MemoryEntry(
            id=id_val,
            memory_type=MemoryType(memory_type_val),
            content=json.loads(content_val),
            timestamp=datetime.fromisoformat(timestamp_val),
            last_accessed=datetime.fromisoformat(last_accessed_val),
            access_count=access_count_val,
            importance_score=importance_score_val,
            tags=json.loads(tags_val) if tags_val else [],
            expires_at=datetime.fromisoformat(expires_at_val) if expires_at_val else None
        )
        return entry

    async def store_memory(
        self,
        memory_entry: MemoryEntry
    ) -> str:
        """
        Store a memory entry.

        Args:
            memory_entry: MemoryEntry to store

        Returns:
            ID of the stored memory
        """
        try:
            cursor = self.connection.cursor()
            cursor.execute("""
                INSERT OR REPLACE INTO memories
                (id, memory_type, content, timestamp, last_accessed,
                 access_count, importance_score, tags, expires_at)
                VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
            """, (
                memory_entry.id,
                memory_entry.memory_type.value,
                json.dumps(memory_entry.content),
                memory_entry.timestamp.isoformat(),
                memory_entry.last_accessed.isoformat(),
                memory_entry.access_count,
                memory_entry.importance_score,
                json.dumps(memory_entry.tags),
                memory_entry.expires_at.isoformat() if memory_entry.expires_at else None
            ))
            self.connection.commit()

            # Update cache
            self._cache[memory_entry.id] = memory_entry

            logger.debug(f"Stored memory {memory_entry.id} of type {memory_entry.memory_type.value}")
            return memory_entry.id
        except Exception as e:
            logger.error(f"Failed to store memory: {e}")
            raise

    async def retrieve_memory(self, memory_id: str) -> Optional[MemoryEntry]:
        """
        Retrieve a memory entry by ID.

        Args:
            memory_id: ID of memory to retrieve

        Returns:
            MemoryEntry if found, None otherwise
        """
        # Check cache first
        if memory_id in self._cache:
            entry = self._cache[memory_id]
            entry.last_accessed = datetime.now()
            entry.access_count += 1
            await self._update_access_info(memory_id)
            return entry

        try:
            cursor = self.connection.cursor()
            cursor.execute("""
                SELECT id, memory_type, content, timestamp, last_accessed,
                       access_count, importance_score, tags, expires_at
                FROM memories
                WHERE id = ?
            """, (memory_id,))

            row = cursor.fetchone()
            if row:
                entry = self._row_to_memory_entry(row)
                # Update access info
                entry.last_accessed = datetime.now()
                entry.access_count += 1
                await self._update_access_info(memory_id)

                # Update cache
                self._cache[memory_id] = entry

                logger.debug(f"Retrieved memory {memory_id}")
                return entry
            else:
                logger.debug(f"Memory {memory_id} not found")
                return None
        except Exception as e:
            logger.error(f"Failed to retrieve memory {memory_id}: {e}")
            return None

    async def _update_access_info(self, memory_id: str) -> None:
        """Update access count and last accessed time in database."""
        try:
            cursor = self.connection.cursor()
            cursor.execute("""
                UPDATE memories
                SET last_accessed = ?, access_count = access_count + 1
                WHERE id = ?
            """, (datetime.now().isoformat(), memory_id))
            self.connection.commit()
        except Exception as e:
            logger.error(f"Failed to update access info for memory {memory_id}: {e}")

    async def search_memories(
        self,
        memory_type: Optional[MemoryType] = None,
        tags: Optional[List[str]] = None,
        limit: int = 10,
        min_importance: float = 0.0
    ) -> List[MemoryEntry]:
        """
        Search for memories based on criteria.

        Args:
            memory_type: Filter by memory type
            tags: Filter by tags (must contain all specified tags)
            limit: Maximum number of results
            min_importance: Minimum importance score

        Returns:
            List of matching MemoryEntry objects
        """
        try:
            query = """
                SELECT id, memory_type, content, timestamp, last_accessed,
                       access_count, importance_score, tags, expires_at
                FROM memories
                WHERE 1=1
            """
            params = []

            if memory_type:
                query += " AND memory_type = ?"
                params.append(memory_type.value)

            if min_importance > 0.0:
                query += " AND importance_score >= ?"
                params.append(min_importance)

            query += " ORDER BY importance_score DESC, last_accessed DESC LIMIT ?"
            params.append(limit)

            cursor = self.connection.cursor()
            cursor.execute(query, params)

            results = []
            for row in cursor.fetchall():
                entry = self._row_to_memory_entry(row)

                # Filter by tags if specified (post-filter since JSON contains is complex in SQLite)
                if tags:
                    entry_tags = set(entry.tags)
                    if not set(tags).issubset(entry_tags):
                        continue

                results.append(entry)
                # Update cache
                self._cache[entry.id] = entry

            logger.debug(f"Found {len(results)} memories matching criteria")
            return results
        except Exception as e:
            logger.error(f"Failed to search memories: {e}")
            return []

    async def forget_memory(self, memory_id: str) -> bool:
        """
        Remove a memory entry (forgetting).

        Args:
            memory_id: ID of memory to forget

        Returns:
            True if removed, False if not found
        """
        try:
            cursor = self.connection.cursor()
            cursor.execute("DELETE FROM memories WHERE id = ?", (memory_id,))
            self.connection.commit()

            removed = cursor.rowcount > 0
            if removed:
                # Remove from cache
                self._cache.pop(memory_id, None)
                logger.debug(f"Forgot memory {memory_id}")
            else:
                logger.debug(f"Memory {memory_id} not found for forgetting")

            return removed
        except Exception as e:
            logger.error(f"Failed to forget memory {memory_id}: {e}")
            return False

    async def consolidate_memories(self) -> Dict[str, int]:
        """
        Perform memory consolidation: strengthen important memories,
        weaken unused ones, and remove expired memories.

        Returns:
            Dictionary with consolidation statistics
        """
        stats = {
            "expired_removed": 0,
            "importance_updated": 0,
            "accessed_recently": 0
        }

        try:
            cursor = self.connection.cursor()
            now = datetime.now()

            # Remove expired memories
            cursor.execute("""
                DELETE FROM memories
                WHERE expires_at IS NOT NULL AND expires_at < ?
            """, (now.isoformat(),))
            stats["expired_removed"] = cursor.rowcount

            # Update importance based on recent access and recency
            cursor.execute("""
                UPDATE memories
                SET importance_score = CASE
                    WHEN last_accessed > ? THEN MIN(1.0, importance_score + 0.1)
                    WHEN last_accessed < ? THEN MAX(0.1, importance_score - 0.05)
                    ELSE importance_score
                END
                WHERE last_accessed IS NOT NULL
            """, (
                (now - timedelta(days=1)).isoformat(),  # Accessed in last day
                (now - timedelta(days=30)).isoformat()   # Not accessed in last 30 days
            ))
            stats["importance_updated"] = cursor.rowcount

            # Count recently accessed memories
            cursor.execute("""
                SELECT COUNT(*) FROM memories
                WHERE last_accessed > ?
            """, ((now - timedelta(hours=24)).isoformat(),))
            stats["accessed_recently"] = cursor.fetchone()[0]

            self.connection.commit()
            logger.info(f"Memory consolidation completed: {stats}")

            # Clear cache to force reload from DB
            self._cache.clear()

        except Exception as e:
            logger.error(f"Failed to consolidate memories: {e}")

        return stats

    def get_memory_stats(self) -> Dict[str, Any]:
        """
        Get statistics about the memory system.

        Returns:
            Dictionary with memory statistics
        """
        try:
            cursor = self.connection.cursor()

            # Total count
            cursor.execute("SELECT COUNT(*) FROM memories")
            total_count = cursor.fetchone()[0]

            # Count by type
            cursor.execute("""
                SELECT memory_type, COUNT(*)
                FROM memories
                GROUP BY memory_type
            """)
            type_counts = {row[0]: row[1] for row in cursor.fetchall()}

            # Average importance
            cursor.execute("SELECT AVG(importance_score) FROM memories")
            avg_importance = cursor.fetchone()[0] or 0.0

            # Oldest and newest
            cursor.execute("""
                SELECT MIN(timestamp), MAX(timestamp)
                FROM memories
            """)
            oldest, newest = cursor.fetchone()

            return {
                "total_memories": total_count,
                "type_counts": type_counts,
                "average_importance": round(avg_importance, 3),
                "oldest_memory": oldest,
                "newest_memory": newest,
                "cache_size": len(self._cache)
            }
        except Exception as e:
            logger.error(f"Failed to get memory stats: {e}")
            return {"error": str(e)}


# Example usage
if __name__ == "__main__":
    import asyncio

    async def main():
        # Initialize memory system
        memory = MemorySystem("test_nexus_memory.db")

        # Store an episodic memory (specific interaction)
        episodic_memory = MemoryEntry(
            id="ep_001",
            memory_type=MemoryType.EPISODIC,
            content={
                "event_type": "user_query",
                "query": "How do I improve my productivity?",
                "response": "Suggested time-blocking technique and Pomodoro method",
                "user_satisfaction": 0.8
            },
            importance_score=0.7,
            tags=["productivity", "query", "interaction"],
            expires_at=datetime.now() + timedelta(days=30)  # Expire in 30 days
        )

        await memory.store_memory(episodic_memory)
        print("Stored episodic memory")

        # Store a semantic memory (learned preference)
        semantic_memory = MemoryEntry(
            id="sem_001",
            memory_type=MemoryType.SEMANTIC,
            content={
                "preference_type": "communication_style",
                "value": "concise_with_examples",
                "context": "technical_explanations"
            },
            importance_score=0.9,
            tags=["preference", "communication"],
            expires_at=None  # Persistent preference
        )

        await memory.store_memory(semantic_memory)
        print("Stored semantic memory")

        # Retrieve a memory
        retrieved = await memory.retrieve_memory("ep_001")
        if retrieved:
            print(f"\nRetrieved memory: {retrieved.content['query']}")
            print(f"Importance: {retrieved.importance_score}")
            print(f"Access count: {retrieved.access_count}")

        # Search memories
        results = await memory.search_memories(
            memory_type=MemoryType.EPISODIC,
            tags=["productivity"],
            limit=5
        )
        print(f"\nFound {len(results)} episodic memories with productivity tag")

        # Get stats
        stats = memory.get_memory_stats()
        print(f"\nMemory Statistics: {stats}")

        # Consolidate memories
        consolidation_stats = await memory.consolidate_memories()
        print(f"\nConsolidation Stats: {consolidation_stats}")

    asyncio.run(main())