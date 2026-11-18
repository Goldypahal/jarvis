"""
Base interface for NEXUS Model Providers.
Every brain (Personal SLM, Local Reasoner, Cloud fallback, Mock) implements this interface.
"""

from abc import ABC, abstractmethod
from typing import Any, AsyncIterator, Dict, Optional, Type, TypeVar
import json
import logging
from pydantic import BaseModel
from .schemas import ModelMetadata

logger = logging.getLogger(__name__)
T = TypeVar("T", bound=BaseModel)


class ModelProvider(ABC):
    """Abstract Base Class for pluggable model brains in NEXUS."""

    def __init__(self, metadata: ModelMetadata):
        self.metadata = metadata
        self._is_loaded: bool = False

    @property
    def name(self) -> str:
        return self.metadata.name

    @property
    def is_loaded(self) -> bool:
        return self._is_loaded

    @abstractmethod
    async def load(self) -> bool:
        """Load model weights into memory/VRAM (or initialize API client)."""
        pass

    @abstractmethod
    async def unload(self) -> bool:
        """Release model weights from VRAM to free memory."""
        pass

    @abstractmethod
    async def generate(
        self,
        prompt: str,
        system_prompt: Optional[str] = None,
        temperature: float = 0.7,
        max_tokens: int = 1024,
        **kwargs: Any
    ) -> str:
        """Generate raw text response."""
        pass

    @abstractmethod
    async def generate_structured(
        self,
        prompt: str,
        schema_cls: Type[T],
        system_prompt: Optional[str] = None,
        temperature: float = 0.1,
        **kwargs: Any
    ) -> T:
        """
        Generate a structured response adhering strictly to the given Pydantic schema.
        Implementations should enforce JSON mode or schema constraints.
        """
        pass

    async def stream(
        self,
        prompt: str,
        system_prompt: Optional[str] = None,
        temperature: float = 0.7,
        max_tokens: int = 1024,
        **kwargs: Any
    ) -> AsyncIterator[str]:
        """Optional streaming text generation. Default yields single block."""
        result = await self.generate(
            prompt, system_prompt=system_prompt, temperature=temperature, max_tokens=max_tokens, **kwargs
        )
        yield result


class MockModelProvider(ModelProvider):
    """Deterministic mock provider for unit testing, CI/CD, and offline verification."""

    def __init__(self, metadata: Optional[ModelMetadata] = None, responses: Optional[Dict[str, Any]] = None):
        if metadata is None:
            from .schemas import ModelCapability, LatencyClass, PrivacyTier
            metadata = ModelMetadata(
                name="mock-provider",
                provider_type="mock",
                capabilities=ModelCapability(
                    latency_class=LatencyClass.FAST,
                    privacy=PrivacyTier.LOCAL,
                    vram_mb=0
                )
            )
        super().__init__(metadata)
        self.responses = responses or {}
        self.call_history = []

    async def load(self) -> bool:
        self._is_loaded = True
        return True

    async def unload(self) -> bool:
        self._is_loaded = False
        return True

    async def generate(
        self,
        prompt: str,
        system_prompt: Optional[str] = None,
        temperature: float = 0.7,
        max_tokens: int = 1024,
        **kwargs: Any
    ) -> str:
        self.call_history.append({"prompt": prompt, "system_prompt": system_prompt})
        for key, resp in self.responses.items():
            if key in prompt:
                return str(resp)
        return "Mock response for: " + prompt[:50]

    async def generate_structured(
        self,
        prompt: str,
        schema_cls: Type[T],
        system_prompt: Optional[str] = None,
        temperature: float = 0.1,
        **kwargs: Any
    ) -> T:
        self.call_history.append({"prompt": prompt, "schema": schema_cls.__name__})
        for key, resp in self.responses.items():
            if key in prompt:
                if isinstance(resp, dict):
                    return schema_cls.model_validate(resp)
                elif isinstance(resp, str):
                    return schema_cls.model_validate_json(resp)
                elif isinstance(resp, schema_cls):
                    return resp
        # Default fallback mock schema instantiation
        try:
            return schema_cls.model_validate({})
        except Exception:
            # Construct using defaults or empty strings if possible
            fields = schema_cls.model_fields
            dummy = {}
            for fname, finfo in fields.items():
                if finfo.default is not None:
                    dummy[fname] = finfo.default
                elif finfo.annotation == str:
                    dummy[fname] = "mock_value"
                elif finfo.annotation == int:
                    dummy[fname] = 1
                elif finfo.annotation == float:
                    dummy[fname] = 1.0
                elif finfo.annotation == bool:
                    dummy[fname] = True
                elif finfo.annotation == list or getattr(finfo.annotation, '__origin__', None) == list:
                    dummy[fname] = []
                elif finfo.annotation == dict or getattr(finfo.annotation, '__origin__', None) == dict:
                    dummy[fname] = {}
            return schema_cls.model_validate(dummy)
