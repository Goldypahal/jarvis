"""
Model Capability Registry for NEXUS.
Allows models to declare their capabilities and provides query interfaces
for finding the most suitable model for a given task.
"""

from typing import Dict, List, Optional
import logging
from .base import ModelProvider
from .schemas import ModelMetadata, PrivacyTier, LatencyClass

logger = logging.getLogger(__name__)


class ModelCapabilityRegistry:
    """Central registry of all available and registered model brains."""

    def __init__(self):
        self._providers: Dict[str, ModelProvider] = {}

    def register_provider(self, provider: ModelProvider) -> None:
        """Register a new model provider."""
        self._providers[provider.name] = provider
        logger.info(f"Registered model provider: {provider.name} (Type: {provider.metadata.provider_type})")

    def unregister_provider(self, name: str) -> Optional[ModelProvider]:
        """Unregister a model provider."""
        return self._providers.pop(name, None)

    def get_provider(self, name: str) -> Optional[ModelProvider]:
        """Retrieve a provider by name."""
        return self._providers.get(name)

    def list_providers(self) -> List[ModelMetadata]:
        """List metadata for all registered providers."""
        return [p.metadata for p in self._providers.values()]

    def find_capable_models(
        self,
        requires_tool_calling: bool = False,
        min_reasoning: int = 1,
        min_coding: int = 1,
        privacy_preference: Optional[PrivacyTier] = None,
        max_latency: Optional[LatencyClass] = None,
    ) -> List[ModelProvider]:
        """
        Query for registered models that satisfy the given operational requirements.
        Returns matching providers sorted by capability fit.
        """
        candidates = []
        for provider in self._providers.values():
            cap = provider.metadata.capabilities
            
            if requires_tool_calling and not cap.tool_calling:
                continue
            if cap.scores.reasoning < min_reasoning:
                continue
            if cap.scores.coding < min_coding:
                continue
            if privacy_preference == PrivacyTier.LOCAL and cap.privacy != PrivacyTier.LOCAL:
                continue

            # Latency filter
            if max_latency == LatencyClass.FAST and cap.latency_class != LatencyClass.FAST:
                continue
            elif max_latency == LatencyClass.MEDIUM and cap.latency_class == LatencyClass.SLOW:
                continue

            candidates.append(provider)

        # Sort: local first, then by reasoning score descending
        candidates.sort(
            key=lambda p: (
                1 if p.metadata.capabilities.privacy == PrivacyTier.LOCAL else 0,
                p.metadata.capabilities.scores.reasoning
            ),
            reverse=True
        )
        return candidates
