"""
Tests for NEXUS Model Abstractions, Capability Registry, Router, and Lifecycle Manager.
"""

import pytest
import asyncio
from nexus.models.schemas import (
    NexusIntent,
    IntentType,
    RiskLevel,
    ModelCapability,
    ModelMetadata,
    ModelCapabilityScores,
    LatencyClass,
    PrivacyTier,
    ActionPlan,
    ActionStep
)
from nexus.models.base import MockModelProvider
from nexus.models.registry import ModelCapabilityRegistry
from nexus.models.lifecycle import ModelLifecycleManager
from nexus.models.router import ModelRouter


def test_schemas_instantiation():
    intent = NexusIntent(
        raw_query="Run tests and repair failures",
        intent=IntentType.CODE_REPAIR,
        goal="Run tests and repair failures",
        requires_tools=True,
        requires_deep_reasoning=True,
        risk=RiskLevel.MEDIUM
    )
    assert intent.intent == IntentType.CODE_REPAIR
    assert intent.risk == RiskLevel.MEDIUM

    plan = ActionPlan(
        plan_id="plan_1",
        goal="Run tests",
        steps=[
            ActionStep(
                step_id=1,
                tool_name="terminal",
                arguments={"command": "pytest"},
                expected_outcome="Tests pass"
            )
        ]
    )
    assert len(plan.steps) == 1
    assert plan.steps[0].tool_name == "terminal"


def test_capability_registry_and_router():
    registry = ModelCapabilityRegistry()

    # Register Personal 3B model
    personal_meta = ModelMetadata(
        name="nexus-personal-3b",
        provider_type="mock",
        capabilities=ModelCapability(
            context_window=32768,
            tool_calling=True,
            scores=ModelCapabilityScores(reasoning=2, coding=2, memory_extraction=5),
            latency_class=LatencyClass.FAST,
            privacy=PrivacyTier.LOCAL,
            vram_mb=2000
        )
    )
    personal_provider = MockModelProvider(personal_meta)
    registry.register_provider(personal_provider)

    # Register Reasoner 7B model
    reasoner_meta = ModelMetadata(
        name="nexus-reasoner-7b",
        provider_type="mock",
        capabilities=ModelCapability(
            context_window=32768,
            tool_calling=True,
            scores=ModelCapabilityScores(reasoning=5, coding=5, memory_extraction=3),
            latency_class=LatencyClass.MEDIUM,
            privacy=PrivacyTier.LOCAL,
            vram_mb=4500
        )
    )
    reasoner_provider = MockModelProvider(reasoner_meta)
    registry.register_provider(reasoner_provider)

    router = ModelRouter(registry)

    # Routine query -> should select 3B
    easy_diff = router.estimate_difficulty("Hello NEXUS, remember my name is Goldy")
    assert easy_diff < 0.45
    decision_easy = router.select_model(easy_diff)
    assert decision_easy.selected_model == "nexus-personal-3b"

    # Complex diagnostic / code repair query -> should select 7B
    hard_diff = router.estimate_difficulty("Analyze and fix failing test cases in the git repository and optimize the benchmark architecture")
    assert hard_diff >= 0.45
    decision_hard = router.select_model(hard_diff)
    assert decision_hard.selected_model == "nexus-reasoner-7b"


def test_lifecycle_manager_vram_swapping():
    async def _run():
        registry = ModelCapabilityRegistry()

        # Model 1: 3B (2000 MB)
        m1_meta = ModelMetadata(
            name="m1-3b",
            provider_type="mock",
            capabilities=ModelCapability(vram_mb=2000)
        )
        m1 = MockModelProvider(m1_meta)
        registry.register_provider(m1)

        # Model 2: 7B (4500 MB)
        m2_meta = ModelMetadata(
            name="m2-7b",
            provider_type="mock",
            capabilities=ModelCapability(vram_mb=4500)
        )
        m2 = MockModelProvider(m2_meta)
        registry.register_provider(m2)

        # Total usable VRAM = 5000 MB (simulating 6GB RTX 4050 with 1GB reserved)
        lifecycle = ModelLifecycleManager(registry, total_vram_mb=6000, reserved_vram_mb=1000)

        # Load 3B model (2000MB <= 5000MB) -> Success
        assert await lifecycle.ensure_loaded("m1-3b")
        assert m1.is_loaded is True
        assert lifecycle.current_vram_used_mb == 2000

        # Load 7B model (4500MB + 2000MB = 6500MB > 5000MB) -> Must evict 3B model first!
        assert await lifecycle.ensure_loaded("m2-7b")
        assert m2.is_loaded is True
        assert m1.is_loaded is False  # Evicted!
        assert lifecycle.current_vram_used_mb == 4500

    asyncio.run(_run())
