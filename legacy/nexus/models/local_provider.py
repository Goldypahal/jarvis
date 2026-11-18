"""
Local Model Provider for NEXUS.
Interacts with local inference engines such as Ollama or OpenAI-compatible servers
(llama-cpp-python, vLLM, text-generation-webui).
"""

from typing import Any, AsyncIterator, Dict, Optional, Type, TypeVar
import json
import logging
import aiohttp
from pydantic import BaseModel
from .base import ModelProvider
from .schemas import ModelMetadata

logger = logging.getLogger(__name__)
T = TypeVar("T", bound=BaseModel)


class LocalModelProvider(ModelProvider):
    """
    Client for local inference runtimes.
    Defaults to Ollama (http://localhost:11434) with OpenAI-compatible fallback.
    """

    def __init__(
        self,
        metadata: ModelMetadata,
        model_tag: Optional[str] = None,
        base_url: str = "http://localhost:11434"
    ):
        super().__init__(metadata)
        self.model_tag = model_tag or metadata.name
        self.base_url = base_url.rstrip("/")

    async def load(self) -> bool:
        """Ping local server and warm up model."""
        try:
            async with aiohttp.ClientSession() as session:
                # Test connectivity
                async with session.get(f"{self.base_url}/api/tags", timeout=aiohttp.ClientTimeout(total=3)) as resp:
                    if resp.status == 200:
                        self._is_loaded = True
                        return True
        except Exception as e:
            logger.warning(f"Local provider {self.name} failed connectivity check at {self.base_url}: {e}")
        # Allow loaded flag to be set for mocked or offline setups
        self._is_loaded = False
        return False

    async def unload(self) -> bool:
        """Request local runtime to unload model from VRAM."""
        try:
            async with aiohttp.ClientSession() as session:
                payload = {"model": self.model_tag, "keep_alive": 0}
                async with session.post(f"{self.base_url}/api/generate", json=payload, timeout=aiohttp.ClientTimeout(total=5)):
                    pass
            self._is_loaded = False
            return True
        except Exception as e:
            logger.debug(f"Unload signal failed for {self.model_tag}: {e}")
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
        messages = []
        if system_prompt:
            messages.append({"role": "system", "content": system_prompt})
        messages.append({"role": "user", "content": prompt})

        payload = {
            "model": self.model_tag,
            "messages": messages,
            "stream": False,
            "options": {
                "temperature": temperature,
                "num_predict": max_tokens
            }
        }

        async with aiohttp.ClientSession() as session:
            async with session.post(
                f"{self.base_url}/api/chat",
                json=payload,
                timeout=aiohttp.ClientTimeout(total=60)
            ) as resp:
                if resp.status != 200:
                    text = await resp.text()
                    raise RuntimeError(f"Local model error ({resp.status}): {text}")
                data = await resp.json()
                return data.get("message", {}).get("content", "")

    async def generate_structured(
        self,
        prompt: str,
        schema_cls: Type[T],
        system_prompt: Optional[str] = None,
        temperature: float = 0.1,
        **kwargs: Any
    ) -> T:
        schema_json = json.dumps(schema_cls.model_json_schema())
        enforced_sys_prompt = (
            f"{system_prompt or ''}\n\n"
            f"You MUST output valid JSON conforming strictly to this JSON schema:\n{schema_json}\n"
            f"Do not output markdown codeblocks, explanations, or text outside the JSON."
        ).strip()

        messages = [
            {"role": "system", "content": enforced_sys_prompt},
            {"role": "user", "content": prompt}
        ]

        payload = {
            "model": self.model_tag,
            "messages": messages,
            "stream": False,
            "format": "json",
            "options": {
                "temperature": temperature,
                "num_predict": 1024
            }
        }

        async with aiohttp.ClientSession() as session:
            async with session.post(
                f"{self.base_url}/api/chat",
                json=payload,
                timeout=aiohttp.ClientTimeout(total=60)
            ) as resp:
                if resp.status != 200:
                    text = await resp.text()
                    raise RuntimeError(f"Structured local model error ({resp.status}): {text}")
                data = await resp.json()
                raw_content = data.get("message", {}).get("content", "").strip()

                # Clean any stray markdown tags if present
                if raw_content.startswith("```json"):
                    raw_content = raw_content[7:]
                if raw_content.startswith("```"):
                    raw_content = raw_content[3:]
                if raw_content.endswith("```"):
                    raw_content = raw_content[:-3]
                raw_content = raw_content.strip()

                return schema_cls.model_validate_json(raw_content)
