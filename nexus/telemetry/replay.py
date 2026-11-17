"""
Trajectory Replay Engine for NEXUS.
Allows historical runs from the ActionJournal to be re-executed deterministically
against different models (e.g. Personal-v1 vs Personal-v2 vs 7B vs Cloud)
for comparative evaluation and regression testing.
"""

from typing import Any, Dict, Optional
import time
import logging
from pydantic import BaseModel
from .journal import ActionJournal, TrajectoryRecord
from nexus.models.base import ModelProvider
from nexus.models.schemas import NexusIntent, IntentType
from nexus.agent.planner import AgentPlanner
from nexus.agent.executor import AgentExecutor
from nexus.agent.verifier import AgentVerifier
from nexus.agent.tools.registry import ToolRegistry
from nexus.agent.policy import PermissionPolicy, PermissionLevel

logger = logging.getLogger(__name__)


class ReplayComparison(BaseModel):
    original_run_id: str
    original_model: str
    original_success: bool
    original_repairs: int
    original_latency_ms: float
    replay_model: str
    replay_success: bool
    replay_repairs: int
    replay_latency_ms: float
    plan_matched: bool
    tool_diff: Dict[str, Any]


class TrajectoryReplayEngine:
    """Replays historical agent runs against specified model backends."""

    def __init__(self, journal: ActionJournal, tool_registry: ToolRegistry):
        self.journal = journal
        self.tool_registry = tool_registry

    async def replay_run(
        self,
        run_id: str,
        test_model_provider: ModelProvider,
        auto_mode: bool = True
    ) -> ReplayComparison:
        """
        Replay a past run using an alternative model provider and produce comparative metrics.
        """
        original = self.journal.get_run(run_id)
        if not original:
            raise ValueError(f"Run ID not found in journal: {run_id}")

        policy = PermissionPolicy(default_level=PermissionLevel.LEVEL_2)
        executor = AgentExecutor(self.tool_registry, policy)
        verifier = AgentVerifier()

        # Re-parse intent using test model
        start_time = time.perf_counter()
        raw_intent = original.intent
        intent = NexusIntent(
            raw_query=original.user_request,
            intent=IntentType(raw_intent.get("intent", "task_orchestration")),
            goal=raw_intent.get("goal", original.user_request),
            requires_tools=True
        )

        planner = AgentPlanner(self.tool_registry)
        plan = await planner.create_plan(intent)

        observations = []
        repairs = 0
        all_passed = True

        for step in plan.steps:
            obs = await executor.execute_step(step, auto_mode=auto_mode)
            observations.append(obs)
            verif = verifier.verify_step(step, obs)
            if not verif.passed:
                all_passed = False
                break

        duration_ms = (time.perf_counter() - start_time) * 1000

        # Compare plans and tools
        original_tools = original.tools_executed
        replay_tools = [s.tool_name for s in plan.steps]

        return ReplayComparison(
            original_run_id=original.run_id,
            original_model=original.model_used,
            original_success=original.success,
            original_repairs=original.repairs,
            original_latency_ms=original.total_latency_ms,
            replay_model=test_model_provider.name,
            replay_success=all_passed,
            replay_repairs=repairs,
            replay_latency_ms=round(duration_ms, 2),
            plan_matched=original_tools == replay_tools,
            tool_diff={"original": original_tools, "replay": replay_tools}
        )
