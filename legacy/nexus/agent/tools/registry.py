"""
Tool Registry for NEXUS Agent.
Maintains available tools, validates registrations, and formats OpenAI-compatible schemas.
"""

from typing import Any, Dict, List, Optional
import logging
from .base import BaseTool

logger = logging.getLogger(__name__)


class ToolRegistry:
    """Registry maintaining all active tools accessible to the agent runtime."""

    def __init__(self):
        self._tools: Dict[str, BaseTool] = {}

    def register(self, tool: BaseTool) -> None:
        """Register a tool instance."""
        self._tools[tool.name] = tool
        logger.info(f"Registered tool: {tool.name} (Risk: {tool.risk_level.value})")

    def unregister(self, name: str) -> Optional[BaseTool]:
        """Unregister a tool."""
        return self._tools.pop(name, None)

    def get(self, name: str) -> Optional[BaseTool]:
        """Retrieve tool by name."""
        return self._tools.get(name)

    def list_tools(self) -> List[BaseTool]:
        """List all registered tools."""
        return list(self._tools.values())

    def get_schemas(self) -> List[Dict[str, Any]]:
        """Return list of JSON schemas for all registered tools."""
        return [tool.get_schema() for tool in self._tools.values()]
