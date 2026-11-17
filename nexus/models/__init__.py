"""
NEXUS Models Package.
Provides model abstractions, capability registry, lifecycle manager,
and capability-based routing for pluggable model brains.
"""

from .schemas import (
    IntentType,
    RiskLevel,
    LatencyClass,
    PrivacyTier,
    ModelCapabilityScores,
    ModelCapability,
    ModelMetadata,
    NexusIntent,
    ActionStep,
    ActionPlan,
    StepObservation,
    VerificationResult,
)
from .base import ModelProvider, MockModelProvider
from .registry import ModelCapabilityRegistry
from .lifecycle import ModelLifecycleManager
from .local_provider import LocalModelProvider
from .cloud_provider import CloudModelProvider
from .router import ModelRouter, RoutingDecision

__all__ = [
    "IntentType",
    "RiskLevel",
    "LatencyClass",
    "PrivacyTier",
    "ModelCapabilityScores",
    "ModelCapability",
    "ModelMetadata",
    "NexusIntent",
    "ActionStep",
    "ActionPlan",
    "StepObservation",
    "VerificationResult",
    "ModelProvider",
    "MockModelProvider",
    "ModelCapabilityRegistry",
    "ModelLifecycleManager",
    "LocalModelProvider",
    "CloudModelProvider",
    "ModelRouter",
    "RoutingDecision",
]
