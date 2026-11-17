"""
Agent Planner for NEXUS.
Converts user intent and task requirements into an executable ActionPlan.
"""

from typing import Any, Dict, List, Optional
import uuid
import logging
from nexus.models.schemas import NexusIntent, ActionPlan, ActionStep, RiskLevel, LatencyClass
from nexus.models.router import ModelRouter
from .tools.registry import ToolRegistry

logger = logging.getLogger(__name__)


class AgentPlanner:
    """
    Decomposes a user goal into a sequence of atomic ActionSteps.
    Can operate via LLM structured generation or deterministic task templates.
    """

    def __init__(self, tool_registry: ToolRegistry, model_router: Optional[ModelRouter] = None):
        self.tool_registry = tool_registry
        self.model_router = model_router

    async def create_plan(self, intent: NexusIntent, context: Optional[Dict[str, Any]] = None) -> ActionPlan:
        """
        Synthesize an ActionPlan for the given intent.
        """
        plan_id = f"plan_{uuid.uuid4().hex[:8]}"

        # Fast-path deterministic plan generation for common developer commands
        deterministic_plan = self._try_deterministic_plan(plan_id, intent)
        if deterministic_plan:
            return deterministic_plan

        # LLM-guided planning if model router is present
        if self.model_router:
            prompt = (
                f"User Goal: {intent.goal}\n"
                f"Intent Type: {intent.intent.value}\n"
                f"Target Project: {intent.target_project or 'Current Workspace'}\n"
                f"Available Tools: {[t.name for t in self.tool_registry.list_tools()]}\n"
                f"Create a minimal, safe step-by-step ActionPlan to achieve this goal."
            )
            try:
                plan = await self.model_router.route_and_generate_structured(
                    prompt=prompt,
                    schema_cls=ActionPlan,
                    difficulty_score=0.6,
                    requires_tools=True
                )
                return plan
            except Exception as e:
                logger.warning(f"LLM planning failed: {e}. Falling back to default plan.")

        # Fallback minimal plan
        return ActionPlan(
            plan_id=plan_id,
            goal=intent.goal,
            steps=[],
            estimated_latency=LatencyClass.FAST
        )

    def _try_deterministic_plan(self, plan_id: str, intent: NexusIntent) -> Optional[ActionPlan]:
        """Provides instant deterministic plans for standard dev commands without LLM latency."""
        query_lower = intent.raw_query.lower()

        if "git status" in query_lower:
            return ActionPlan(
                plan_id=plan_id,
                goal="Check Git Status",
                steps=[
                    ActionStep(
                        step_id=1,
                        tool_name="git_status",
                        arguments={"repo_path": "."},
                        expected_outcome="Repository status obtained",
                        risk_level=RiskLevel.LOW
                    )
                ]
            )

        if "run tests" in query_lower or "run test" in query_lower:
            return ActionPlan(
                plan_id=plan_id,
                goal="Run project test suite",
                steps=[
                    ActionStep(
                        step_id=1,
                        tool_name="terminal",
                        arguments={"command": "python run_tests.py"},
                        expected_outcome="Tests executed with status report",
                        risk_level=RiskLevel.HIGH
                    )
                ],
                requires_confirmation=False
            )

        return None
