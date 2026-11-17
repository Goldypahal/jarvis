"""
Agent Reflection & Repair Engine for NEXUS.
Synthesizes repair actions when verification detects a failure.
"""

from typing import Any, Dict, Optional
import logging
from nexus.models.schemas import ActionStep, ActionPlan, StepObservation, VerificationResult
from nexus.models.router import ModelRouter

logger = logging.getLogger(__name__)


class AgentReflectionEngine:
    """
    Formulates corrective actions and replanning when verification fails.
    Escalates to stronger reasoning models when repairs repeat.
    """

    def __init__(self, model_router: Optional[ModelRouter] = None, max_repairs: int = 2):
        self.model_router = model_router
        self.max_repairs = max_repairs

    async def propose_repair(
        self,
        failed_step: ActionStep,
        observation: StepObservation,
        verification: VerificationResult,
        repair_attempt: int
    ) -> Optional[ActionStep]:
        """
        Propose a repaired ActionStep to resolve the failure.
        """
        if repair_attempt > self.max_repairs:
            logger.warning(f"Exceeded max repair attempts ({self.max_repairs}). Escalating.")
            return None

        # If model router is available, ask the reasoner model to synthesize a repair
        if self.model_router:
            prompt = (
                f"Action failed on step {failed_step.step_id} using tool '{failed_step.tool_name}'.\n"
                f"Arguments: {failed_step.arguments}\n"
                f"Error: {observation.error}\n"
                f"Verifier Diagnosis: {verification.reason}\n"
                f"Repair Attempt: {repair_attempt}/{self.max_repairs}\n"
                f"Provide a corrected ActionStep with adjusted arguments to fix this failure."
            )
            try:
                repaired_step = await self.model_router.route_and_generate_structured(
                    prompt=prompt,
                    schema_cls=ActionStep,
                    difficulty_score=0.75,  # Escalate difficulty to reach reasoner model
                    requires_tools=True
                )
                repaired_step.step_id = failed_step.step_id
                return repaired_step
            except Exception as e:
                logger.warning(f"Model repair synthesis failed: {e}")

        # Fallback heuristic repair
        if failed_step.tool_name == "read_file" and "file not found" in (observation.error or "").lower():
            # Try listing directory to locate the file
            return ActionStep(
                step_id=failed_step.step_id,
                tool_name="list_directory",
                arguments={"directory_path": "."},
                expected_outcome="Locate target file"
            )

        return None
