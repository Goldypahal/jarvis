"""
Deterministic Agent Runtime & State Machine for NEXUS.
Governs agent state transitions, tool execution, safety verification, and telemetry logging.
"""

from typing import Any, Callable, Dict, List, Optional
from enum import Enum
import uuid
import time
import logging
from pydantic import BaseModel

from nexus.models.schemas import (
    NexusIntent,
    IntentType,
    ActionPlan,
    ActionStep,
    StepObservation,
    VerificationResult,
    RiskLevel
)
from nexus.models.router import ModelRouter
from nexus.agent.tools.registry import ToolRegistry
from nexus.agent.planner import AgentPlanner
from nexus.agent.executor import AgentExecutor
from nexus.agent.verifier import AgentVerifier
from nexus.agent.reflection import AgentReflectionEngine
from nexus.agent.policy import PermissionPolicy, PermissionLevel
from nexus.telemetry.journal import ActionJournal, TrajectoryRecord

logger = logging.getLogger(__name__)


class AgentState(str, Enum):
    IDLE = "idle"
    UNDERSTANDING = "understanding"
    PLANNING = "planning"
    WAITING_PERMISSION = "waiting_permission"
    EXECUTING = "executing"
    OBSERVING = "observing"
    VERIFYING = "verifying"
    REPAIR = "repair"
    RESPOND = "respond"


class RuntimeOutput(BaseModel):
    run_id: str
    response_text: str
    state: AgentState
    intent: Optional[NexusIntent] = None
    plan: Optional[ActionPlan] = None
    observations: List[StepObservation] = []
    verification: Optional[VerificationResult] = None
    success: bool = True
    repairs_count: int = 0
    duration_ms: float = 0.0


class AgentRuntime:
    """
    Deterministic Agent Platform Runtime.
    The language model proposes actions; the runtime enforces state, safety, and verification.
    """

    def __init__(
        self,
        router: ModelRouter,
        tool_registry: ToolRegistry,
        policy: Optional[PermissionPolicy] = None,
        journal: Optional[ActionJournal] = None,
        max_repairs: int = 2
    ):
        self.router = router
        self.tool_registry = tool_registry
        self.policy = policy or PermissionPolicy(default_level=PermissionLevel.LEVEL_1)
        self.journal = journal or ActionJournal()
        self.planner = AgentPlanner(tool_registry, model_router=router)
        self.executor = AgentExecutor(tool_registry, self.policy)
        self.verifier = AgentVerifier()
        self.reflection = AgentReflectionEngine(model_router=router, max_repairs=max_repairs)
        self.state: AgentState = AgentState.IDLE
        self._state_listener: Optional[Callable[[AgentState], None]] = None

    def set_state_listener(self, listener: Callable[[AgentState], None]) -> None:
        """Register listener for real-time UI/CLI state animations."""
        self._state_listener = listener

    def _set_state(self, new_state: AgentState) -> None:
        self.state = new_state
        logger.debug(f"Agent state changed to: {new_state.value}")
        if self._state_listener:
            self._state_listener(new_state)

    async def process_request(
        self,
        query: str,
        context: Optional[Dict[str, Any]] = None,
        auto_mode: bool = False
    ) -> RuntimeOutput:
        """
        Process a user request through the complete deterministic state machine.
        """
        start_time = time.perf_counter()
        run_id = f"run_{uuid.uuid4().hex[:8]}"
        self._set_state(AgentState.UNDERSTANDING)

        # 1. Understanding & Intent Extraction
        difficulty = self.router.estimate_difficulty(query, context)
        decision = self.router.select_model(difficulty)

        # Fast deterministic classification for common commands
        intent = self._parse_intent_fast(query, difficulty)
        self._set_state(AgentState.PLANNING)

        # 2. Planning
        plan = await self.planner.create_plan(intent, context)

        # If no tools are required, generate direct response
        if not plan.steps:
            self._set_state(AgentState.RESPOND)
            response_text = await self._generate_conversational_response(query, decision.selected_model)
            duration = (time.perf_counter() - start_time) * 1000
            self._set_state(AgentState.IDLE)
            return RuntimeOutput(
                run_id=run_id,
                response_text=response_text,
                state=AgentState.IDLE,
                intent=intent,
                plan=plan,
                success=True,
                duration_ms=round(duration, 2)
            )

        # 3. Check Permissions
        self._set_state(AgentState.WAITING_PERMISSION)
        for step in plan.steps:
            if not self.policy.is_action_allowed(step, auto_mode=auto_mode):
                self._set_state(AgentState.RESPOND)
                duration = (time.perf_counter() - start_time) * 1000
                msg = f"Execution paused: Action '{step.tool_name}' requires permission."
                self._set_state(AgentState.IDLE)
                return RuntimeOutput(
                    run_id=run_id,
                    response_text=msg,
                    state=AgentState.IDLE,
                    intent=intent,
                    plan=plan,
                    success=False,
                    duration_ms=round(duration, 2)
                )

        # 4. Executing, Observing, Verifying & Repairing Loop
        observations: List[StepObservation] = []
        repairs_count = 0
        overall_success = True
        final_verif: Optional[VerificationResult] = None

        idx = 0
        while idx < len(plan.steps):
            step = plan.steps[idx]
            self._set_state(AgentState.EXECUTING)

            obs = await self.executor.execute_step(step, auto_mode=auto_mode)
            self._set_state(AgentState.OBSERVING)
            observations.append(obs)

            self._set_state(AgentState.VERIFYING)
            verif = self.verifier.verify_step(step, obs)
            final_verif = verif

            if not verif.passed:
                # Enter Repair Loop
                self._set_state(AgentState.REPAIR)
                repairs_count += 1
                repaired_step = await self.reflection.propose_repair(
                    failed_step=step,
                    observation=obs,
                    verification=verif,
                    repair_attempt=repairs_count
                )
                if repaired_step:
                    logger.info(f"Retrying with repaired step: {repaired_step.tool_name}")
                    plan.steps[idx] = repaired_step
                    continue  # Retry with repaired step
                else:
                    overall_success = False
                    break

            idx += 1

        # 5. Synthesize Final Response
        self._set_state(AgentState.RESPOND)
        response_text = self._format_final_response(query, plan, observations, overall_success, final_verif)
        duration = (time.perf_counter() - start_time) * 1000

        # 6. Immutable Action Journal Recording
        self.journal.record_run(
            TrajectoryRecord(
                run_id=run_id,
                user_request=query,
                model_used=decision.selected_model,
                intent=intent.model_dump(),
                plan=plan.model_dump(),
                tools_executed=[s.tool_name for s in plan.steps],
                observations=[o.model_dump() for o in observations],
                verification=final_verif.model_dump() if final_verif else {},
                repairs=repairs_count,
                success=overall_success,
                total_latency_ms=round(duration, 2)
            )
        )

        self._set_state(AgentState.IDLE)
        return RuntimeOutput(
            run_id=run_id,
            response_text=response_text,
            state=AgentState.IDLE,
            intent=intent,
            plan=plan,
            observations=observations,
            verification=final_verif,
            success=overall_success,
            repairs_count=repairs_count,
            duration_ms=round(duration, 2)
        )

    def _parse_intent_fast(self, query: str, difficulty: float) -> NexusIntent:
        """Quick deterministic classification for common developer actions."""
        q = query.lower()
        if "git" in q or "branch" in q or "diff" in q:
            itype = IntentType.SYSTEM_COMMAND
            req_tools = True
        elif "test" in q or "pytest" in q or "run" in q:
            itype = IntentType.CODE_REPAIR if "fail" in q or "fix" in q else IntentType.TASK_ORCHESTRATION
            req_tools = True
        elif "remember" in q or "prefer" in q or "what was" in q:
            itype = IntentType.MEMORY_RETRIEVAL
            req_tools = False
        else:
            itype = IntentType.KNOWLEDGE_QUERY
            req_tools = False

        return NexusIntent(
            raw_query=query,
            intent=itype,
            goal=query,
            requires_tools=req_tools,
            requires_deep_reasoning=(difficulty >= 0.45),
            risk=RiskLevel.HIGH if "run" in q or "terminal" in q else RiskLevel.LOW
        )

    async def _generate_conversational_response(self, query: str, model_name: str) -> str:
        provider = self.router.registry.get_provider(model_name)
        if provider:
            try:
                return await provider.generate(query)
            except Exception:
                pass
        return f"[NEXUS] Processed query: '{query}'"

    def _format_final_response(
        self,
        query: str,
        plan: ActionPlan,
        observations: List[StepObservation],
        success: bool,
        verification: Optional[VerificationResult]
    ) -> str:
        status_icon = "PASS" if success else "FAIL"
        lines = [f"[NEXUS {status_icon}] Completed execution for: '{query}'"]
        for obs in observations:
            lines.append(f"  - Step {obs.step_id} [{obs.tool_name}]: {obs.status} ({obs.duration_ms}ms)")
            if obs.output and isinstance(obs.output, dict):
                if "stdout" in obs.output and obs.output["stdout"]:
                    lines.append(f"    Output: {obs.output['stdout'][:200]}...")
                elif "clean" in obs.output:
                    lines.append(f"    Clean: {obs.output['clean']}, Branch: {obs.output.get('branch', 'unknown')}")
            if obs.error:
                lines.append(f"    Error: {obs.error}")

        if verification and not verification.passed:
            lines.append(f"  Diagnosis: {verification.reason}")
            if verification.suggested_repair:
                lines.append(f"  Suggested Action: {verification.suggested_repair}")

        return "\n".join(lines)
