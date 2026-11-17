"""
Agent Verifier for NEXUS.
Provides deterministic validation of tool observations and plan success.
"""

from typing import List, Optional
import logging
from nexus.models.schemas import ActionStep, ActionPlan, StepObservation, VerificationResult

logger = logging.getLogger(__name__)


class AgentVerifier:
    """
    Evaluates tool observations against expected conditions.
    Decides whether an action succeeded or whether replanning/repair is needed.
    """

    def verify_step(self, step: ActionStep, observation: StepObservation) -> VerificationResult:
        """Verify the outcome of a single step."""
        if observation.status != "success":
            return VerificationResult(
                passed=False,
                reason=observation.error or f"Step {step.step_id} ({step.tool_name}) failed",
                failed_step_id=step.step_id,
                suggested_repair=self._infer_repair(step, observation),
                confidence=1.0
            )

        # Content inspection
        output = observation.output
        if isinstance(output, dict):
            if output.get("error"):
                return VerificationResult(
                    passed=False,
                    reason=str(output.get("error")),
                    failed_step_id=step.step_id,
                    suggested_repair=self._infer_repair(step, observation),
                    confidence=0.9
                )

        return VerificationResult(
            passed=True,
            reason="Step completed successfully without errors",
            failed_step_id=None,
            suggested_repair=None,
            confidence=1.0
        )

    def verify_plan(self, plan: ActionPlan, observations: List[StepObservation]) -> VerificationResult:
        """Verify the cumulative completion of the full action plan."""
        if not observations and plan.steps:
            return VerificationResult(
                passed=False,
                reason="No observations recorded for active plan.",
                confidence=1.0
            )

        for obs, step in zip(observations, plan.steps):
            step_verif = self.verify_step(step, obs)
            if not step_verif.passed:
                return step_verif

        return VerificationResult(
            passed=True,
            reason=f"All {len(plan.steps)} plan steps verified successfully.",
            confidence=1.0
        )

    def _infer_repair(self, step: ActionStep, observation: StepObservation) -> Optional[str]:
        """Heuristic repair suggestion based on error text."""
        err = (observation.error or "").lower()
        if "filenotfounderror" in err or "no such file" in err:
            return "Verify target file path exists or list directory contents first."
        if "permission denied" in err:
            return "Elevate permission level or request user authorization for this command."
        if "syntaxerror" in err or "importerror" in err:
            return "Inspect recent code edits and resolve syntax or missing import symbols."
        return "Review step arguments and retry with adjusted parameters."
