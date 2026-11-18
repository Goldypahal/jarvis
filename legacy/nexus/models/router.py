"""
Multi-Signal Model Router for NEXUS.
Dynamically routes user tasks to the optimal model brain based on task difficulty,
declared model capabilities, VRAM availability, and objective runtime signals.
"""

from typing import Any, Dict, List, Optional, Type, TypeVar
import logging
from pydantic import BaseModel

from .base import ModelProvider
from .registry import ModelCapabilityRegistry
from .lifecycle import ModelLifecycleManager
from .schemas import (
    NexusIntent,
    IntentType,
    RiskLevel,
    LatencyClass,
    PrivacyTier,
    ModelCapability
)

logger = logging.getLogger(__name__)
T = TypeVar("T", bound=BaseModel)


class RoutingDecision(BaseModel):
    selected_model: str
    difficulty_score: float  # 0.0 to 1.0
    reasoning_mode: str      # "system_1" or "system_2"
    estimated_latency: LatencyClass
    fallback_model: Optional[str] = None


class ModelRouter:
    """
    Decoupled router evaluating task complexity and matching against
    registered model capabilities.
    """

    def __init__(
        self,
        registry: ModelCapabilityRegistry,
        lifecycle_manager: Optional[ModelLifecycleManager] = None
    ):
        self.registry = registry
        self.lifecycle_manager = lifecycle_manager or ModelLifecycleManager(registry)

    def estimate_difficulty(self, query: str, context: Optional[Dict[str, Any]] = None) -> float:
        """
        Calculates an objective task difficulty score between 0.0 and 1.0.
        Factors: query length, complex reasoning indicators, tool keywords, context size.
        """
        text = query.lower()
        score = 0.1  # base difficulty

        # Deep reasoning indicators (+0.3)
        complex_indicators = [
            "analyze", "compare", "evaluate", "diagnose", "fix failing",
            "refactor", "debug", "benchmark", "optimize", "architecture",
            "explain why", "trade-offs", "plan", "root cause"
        ]
        if any(ind in text for ind in complex_indicators):
            score += 0.35

        # Code and terminal indicators (+0.25)
        coding_indicators = [
            "function", "class", "test", "git", "bash", "powershell",
            "pytest", "import", "syntax", "error", "traceback", "commit"
        ]
        if any(ind in text for ind in coding_indicators):
            score += 0.25

        # Query length indicator
        words = len(query.split())
        if words > 25:
            score += 0.2
        elif words > 12:
            score += 0.1

        # Multi-turn or rich context
        if context and len(context.get("recent_interactions", [])) > 4:
            score += 0.1

        return min(1.0, score)

    def select_model(
        self,
        difficulty_score: float,
        requires_tools: bool = False,
        privacy_preference: Optional[PrivacyTier] = PrivacyTier.LOCAL
    ) -> RoutingDecision:
        """
        Selects the best registered model based on difficulty and capabilities.
        """
        if difficulty_score < 0.45:
            # System 1: Fast Personal SLM (1-3B)
            min_reasoning = 1
            max_latency = LatencyClass.FAST
            mode = "system_1"
        else:
            # System 2: Local Reasoner (7-8B) or Cloud
            min_reasoning = 3
            max_latency = LatencyClass.MEDIUM
            mode = "system_2"

        capable_models = self.registry.find_capable_models(
            requires_tool_calling=requires_tools,
            min_reasoning=min_reasoning,
            privacy_preference=privacy_preference,
            max_latency=max_latency
        )

        if not capable_models:
            # Fallback: Relax latency or reasoning constraints
            capable_models = self.registry.find_capable_models(
                requires_tool_calling=requires_tools,
                min_reasoning=1,
                privacy_preference=privacy_preference
            )

        if not capable_models:
            # Absolute fallback: any registered provider
            all_providers = [
                self.registry.get_provider(m.name) for m in self.registry.list_providers()
            ]
            capable_models = [p for p in all_providers if p is not None]

        if not capable_models:
            raise RuntimeError("No model providers registered in ModelCapabilityRegistry!")

        selected = capable_models[0]
        fallback = capable_models[1].name if len(capable_models) > 1 else None

        return RoutingDecision(
            selected_model=selected.name,
            difficulty_score=difficulty_score,
            reasoning_mode=mode,
            estimated_latency=selected.metadata.capabilities.latency_class,
            fallback_model=fallback
        )

    async def route_and_generate_structured(
        self,
        prompt: str,
        schema_cls: Type[T],
        difficulty_score: Optional[float] = None,
        system_prompt: Optional[str] = None,
        requires_tools: bool = False,
        max_retries: int = 2
    ) -> T:
        """
        Executes structured generation with automatic multi-signal fallback.
        If the primary model fails schema validation, falls back to the secondary model.
        """
        if difficulty_score is None:
            difficulty_score = self.estimate_difficulty(prompt)

        decision = self.select_model(difficulty_score, requires_tools=requires_tools)
        primary_provider = self.registry.get_provider(decision.selected_model)
        
        # Ensure model is resident in VRAM
        await self.lifecycle_manager.ensure_loaded(decision.selected_model)

        try:
            return await primary_provider.generate_structured(
                prompt,
                schema_cls,
                system_prompt=system_prompt
            )
        except Exception as primary_error:
            logger.warning(
                f"Model {decision.selected_model} failed structured generation: {primary_error}."
                f" Triggering fallback."
            )
            if decision.fallback_model:
                fallback_provider = self.registry.get_provider(decision.fallback_model)
                if fallback_provider:
                    await self.lifecycle_manager.ensure_loaded(decision.fallback_model)
                    return await fallback_provider.generate_structured(
                        prompt,
                        schema_cls,
                        system_prompt=system_prompt
                    )
            raise primary_error
