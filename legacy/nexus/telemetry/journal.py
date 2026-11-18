"""
Immutable Action Journal for NEXUS.
Appends full execution trajectories to structured JSONL logs to power
reproducible benchmarking, telemetry, and model fine-tuning datasets.
"""

from typing import Any, Dict, List, Optional
from datetime import datetime
import json
import os
import uuid
import logging
from pydantic import BaseModel, Field

logger = logging.getLogger(__name__)


class TrajectoryRecord(BaseModel):
    run_id: str
    timestamp: str = Field(default_factory=lambda: datetime.now().isoformat())
    user_request: str
    model_used: str
    intent: Dict[str, Any]
    plan: Dict[str, Any]
    tools_executed: List[str] = Field(default_factory=list)
    observations: List[Dict[str, Any]] = Field(default_factory=list)
    verification: Dict[str, Any] = Field(default_factory=dict)
    repairs: int = 0
    success: bool = True
    total_latency_ms: float = 0.0
    user_correction: Optional[str] = None


class ActionJournal:
    """
    Append-only repository of agent runs.
    Ensures complete, immutable trajectory persistence.
    """

    def __init__(self, log_dir: str = ".nexus/trajectories"):
        self.log_dir = log_dir
        os.makedirs(self.log_dir, exist_ok=True)
        self.log_file = os.path.join(self.log_dir, "journal.jsonl")

    def record_run(self, record: TrajectoryRecord) -> None:
        """Write trajectory record to append-only JSONL."""
        line = record.model_dump_json() + "\n"
        with open(self.log_file, "a", encoding="utf-8") as f:
            f.write(line)
        logger.info(f"Journaled trajectory run {record.run_id} (Success: {record.success})")

    def get_run(self, run_id: str) -> Optional[TrajectoryRecord]:
        """Fetch a specific trajectory by run_id."""
        if not os.path.exists(self.log_file):
            return None

        with open(self.log_file, "r", encoding="utf-8") as f:
            for line in f:
                if line.strip():
                    data = json.loads(line)
                    if data.get("run_id") == run_id:
                        return TrajectoryRecord.model_validate(data)
        return None

    def list_runs(self, limit: int = 50) -> List[TrajectoryRecord]:
        """List the most recent trajectories."""
        if not os.path.exists(self.log_file):
            return []

        runs = []
        with open(self.log_file, "r", encoding="utf-8") as f:
            for line in f:
                if line.strip():
                    runs.append(TrajectoryRecord.model_validate_json(line))
        return runs[-limit:]
