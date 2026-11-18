"""
Agent Executor for NEXUS.
Executes individual ActionSteps deterministically through registered tools with policy enforcement.
"""

from typing import Any, Dict, Optional
import time
import logging
from nexus.models.schemas import ActionStep, StepObservation
from .tools.registry import ToolRegistry
from .policy import PermissionPolicy

logger = logging.getLogger(__name__)


class AgentExecutor:
    """
    Executes ActionSteps against registered tools, enforces policy permission,
    and returns deterministic StepObservation records.
    """

    def __init__(self, tool_registry: ToolRegistry, policy: PermissionPolicy):
        self.tool_registry = tool_registry
        self.policy = policy

    async def execute_step(self, step: ActionStep, auto_mode: bool = False) -> StepObservation:
        """Execute a single ActionStep."""
        start_time = time.perf_counter()

        # Permission verification
        if not self.policy.is_action_allowed(step, auto_mode=auto_mode):
            duration = (time.perf_counter() - start_time) * 1000
            return StepObservation(
                step_id=step.step_id,
                tool_name=step.tool_name,
                status="failure",
                output=None,
                error=f"Permission denied: Action '{step.tool_name}' blocked by security policy.",
                duration_ms=round(duration, 2)
            )

        tool = self.tool_registry.get(step.tool_name)
        if not tool:
            duration = (time.perf_counter() - start_time) * 1000
            return StepObservation(
                step_id=step.step_id,
                tool_name=step.tool_name,
                status="failure",
                output=None,
                error=f"Tool not found in registry: {step.tool_name}",
                duration_ms=round(duration, 2)
            )

        try:
            output = await tool.execute(**step.arguments)
            duration = (time.perf_counter() - start_time) * 1000

            # Check if output contains explicit error signals (e.g. terminal non-zero return code)
            is_error = False
            error_msg = None
            if isinstance(output, dict):
                if output.get("status") == "error" or output.get("returncode", 0) != 0:
                    is_error = True
                    error_msg = output.get("stderr") or f"Tool returned error state: {output}"

            return StepObservation(
                step_id=step.step_id,
                tool_name=step.tool_name,
                status="failure" if is_error else "success",
                output=output,
                error=error_msg,
                duration_ms=round(duration, 2)
            )
        except Exception as e:
            duration = (time.perf_counter() - start_time) * 1000
            logger.error(f"Execution error on step {step.step_id} ({step.tool_name}): {e}")
            return StepObservation(
                step_id=step.step_id,
                tool_name=step.tool_name,
                status="failure",
                output=None,
                error=str(e),
                duration_ms=round(duration, 2)
            )
