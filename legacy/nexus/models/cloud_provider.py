"""
Cloud Model Provider for NEXUS.
Interacts with cloud LLM APIs (Gemini, OpenAI, Anthropic, or OpenAI-compatible proxies).
"""

from typing import Any, AsyncIterator, Dict, Optional, Type, TypeVar
import json
import logging
import os
import aiohttp
from pydantic import BaseModel
from .base import ModelProvider
from .schemas import ModelMetadata

logger = logging.getLogger(__name__)
T = TypeVar("T", bound=BaseModel)


class CloudModelProvider(ModelProvider):
    """
    Client for cloud inference APIs.
    Supports OpenAI-compatible chat endpoints and Google Gemini endpoints.
    """

    def __init__(
        self,
        metadata: ModelMetadata,
        api_key: Optional[str] = None,
        base_url: str = "https://api.openai.com/v1"
    ):
        super().__init__(metadata)
        self.api_key = api_key or os.getenv("OPENAI_API_KEY") or os.getenv("GEMINI_API_KEY") or ""
        self.base_url = base_url.rstrip("/")

    async def load(self) -> bool:
        self._is_loaded = bool(self.api_key)
        return self._is_loaded

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
        if not self.api_key:
            raise ValueError(f"No API key provided for cloud model {self.name}")

        headers = {
            "Authorization": f"Bearer {self.api_key}",
            "Content-Type": "application/json"
        }

        messages = []
        if system_prompt:
            messages.append({"role": "system", "content": system_prompt})
        messages.append({"role": "user", "content": prompt})

        payload = {
            "model": self.name,
            "messages": messages,
            "temperature": temperature,
            "max_tokens": max_tokens
        }

        async with aiohttp.ClientSession() as session:
            async with session.post(
                f"{self.base_url}/chat/completions",
                headers=headers,
                json=payload,
                timeout=aiohttp.ClientTimeout(total=60)
            ) as resp:
                if resp.status != 200:
                    text = await resp.text()
                    raise RuntimeError(f"Cloud API error ({resp.status}): {text}")
                data = await resp.json()
                return data["choices"][0]["message"]["content"]

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
            f"You MUST output valid JSON strictly matching this schema:\n{schema_json}\n"
            f"Do not output markdown codeblocks, explanations, or text outside the JSON."
        ).strip()

        raw_text = await self.generate(
            prompt,
            system_prompt=enforced_sys_prompt,
            temperature=temperature,
            **kwargs
        )

        raw_text = raw_text.strip()
        if raw_text.startswith("```json"):
            raw_text = raw_text[7:]
        if raw_text.startswith("```"):
            raw_text = raw_text[3:]
        if raw_text.endswith("```"):
            raw_text = raw_text[:-3]
        raw_text = raw_text.strip()

        return schema_cls.model_validate_json(raw_text)
