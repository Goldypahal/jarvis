"""
Model Lifecycle Manager for NEXUS.
Manages GPU VRAM budgeting and coordinates model hot-swapping, especially
optimized for hardware environments like the RTX 4050 (6GB VRAM).
"""

from typing import Dict, List, Optional, Set
import asyncio
import logging
from .base import ModelProvider
from .registry import ModelCapabilityRegistry

logger = logging.getLogger(__name__)


class ModelLifecycleManager:
    """
    Coordinates loading, unloading, and VRAM budgeting for local models.
    Prevents CUDA Out-Of-Memory (OOM) errors by swapping models as required.
    """

    def __init__(
        self,
        registry: ModelCapabilityRegistry,
        total_vram_mb: int = 6000,
        reserved_vram_mb: int = 1000
    ):
        self.registry = registry
        self.total_vram_mb = total_vram_mb
        self.reserved_vram_mb = reserved_vram_mb
        self.usable_vram_mb = max(1000, total_vram_mb - reserved_vram_mb)
        self.current_vram_used_mb: int = 0
        self.active_models: List[str] = []  # Ordered by MRU
        self._lock = asyncio.Lock()

    async def ensure_loaded(self, provider_name: str) -> bool:
        """
        Ensure that the specified model is loaded in memory/VRAM.
        If loading exceeds the VRAM budget, unloads the least recently used models.
        """
        async with self._lock:
            provider = self.registry.get_provider(provider_name)
            if not provider:
                logger.error(f"Cannot load unknown provider: {provider_name}")
                return False

            if provider.is_loaded:
                # Update MRU order
                if provider_name in self.active_models:
                    self.active_models.remove(provider_name)
                self.active_models.append(provider_name)
                return True

            needed_vram = provider.metadata.capabilities.vram_mb
            
            # If model is cloud-based or mock (0 VRAM), simply load it
            if needed_vram <= 0:
                await provider.load()
                return True

            # Evict models until there is enough VRAM
            while self.current_vram_used_mb + needed_vram > self.usable_vram_mb and self.active_models:
                lru_name = self.active_models.pop(0)
                lru_provider = self.registry.get_provider(lru_name)
                if lru_provider and lru_provider.is_loaded:
                    logger.info(f"Hot-swapping: Unloading {lru_name} to free VRAM for {provider_name}")
                    await lru_provider.unload()
                    self.current_vram_used_mb = max(
                        0, self.current_vram_used_mb - lru_provider.metadata.capabilities.vram_mb
                    )

            # Now load the target provider
            success = await provider.load()
            if success:
                self.current_vram_used_mb += needed_vram
                self.active_models.append(provider_name)
                logger.info(
                    f"Loaded model {provider_name} ({needed_vram}MB). "
                    f"Total VRAM used: {self.current_vram_used_mb}/{self.usable_vram_mb}MB"
                )
            return success

    async def unload_model(self, provider_name: str) -> bool:
        """Explicitly unload a model from VRAM."""
        async with self._lock:
            provider = self.registry.get_provider(provider_name)
            if not provider or not provider.is_loaded:
                return True

            success = await provider.unload()
            if success:
                needed_vram = provider.metadata.capabilities.vram_mb
                self.current_vram_used_mb = max(0, self.current_vram_used_mb - needed_vram)
                if provider_name in self.active_models:
                    self.active_models.remove(provider_name)
                logger.info(f"Unloaded model {provider_name}. VRAM freed: {needed_vram}MB")
            return success

    async def unload_all(self) -> None:
        """Unload all active models."""
        async with self._lock:
            for name in list(self.active_models):
                provider = self.registry.get_provider(name)
                if provider and provider.is_loaded:
                    await provider.unload()
            self.active_models.clear()
            self.current_vram_used_mb = 0
            logger.info("All models unloaded from VRAM.")
