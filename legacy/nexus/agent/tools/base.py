"""
Base Tool Contract for NEXUS Agent.
All tools follow a uniform interface with strict schema definitions and risk levels.
"""

from abc import ABC, abstractmethod
from typing import Any, Dict, Optional
import inspect
from pydantic import BaseModel
from nexus.models.schemas import RiskLevel


class BaseTool(ABC):
    """Abstract base class for all NEXUS tools."""

    name: str = "base_tool"
    description: str = "Base tool description"
    risk_level: RiskLevel = RiskLevel.LOW
    args_schema: Optional[type[BaseModel]] = None

    @abstractmethod
    async def execute(self, **kwargs: Any) -> Any:
        """Execute the tool deterministically and return output."""
        pass

    def get_schema(self) -> Dict[str, Any]:
        """Return JSON schema format compatible with LLM tool calling."""
        if self.args_schema:
            parameters = self.args_schema.model_json_schema()
        else:
            parameters = {
                "type": "object",
                "properties": {},
                "required": []
            }

        return {
            "type": "function",
            "function": {
                "name": self.name,
                "description": self.description,
                "parameters": parameters,
                "risk_level": self.risk_level.value
            }
        }
