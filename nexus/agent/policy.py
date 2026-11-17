"""
Permission & Risk Policy Engine for NEXUS Agent.
Enforces deterministic boundaries between model proposals and real system execution.
"""

from typing import Any, Callable, Dict, Optional, Set
from enum import IntEnum
import logging
from nexus.models.schemas import ActionStep, RiskLevel

logger = logging.getLogger(__name__)


class PermissionLevel(IntEnum):
    LEVEL_0 = 0  # Read-only queries (Auto-approved)
    LEVEL_1 = 1  # File writes / modifications (Workspace restricted)
    LEVEL_2 = 2  # Shell commands & execution (Explicit confirmation required by default)


class PermissionPolicy:
    """
    Deterministic safety policy governing tool execution.
    The LLM never directly executes; the policy grants or denies execution.
    """

    def __init__(self, default_level: PermissionLevel = PermissionLevel.LEVEL_1):
        self.default_level = default_level
        self._auto_approved_tools: Set[str] = {"read_file", "list_directory", "git_status", "git_diff"}
        self._confirmation_handler: Optional[Callable[[ActionStep], bool]] = None

    def set_confirmation_handler(self, handler: Callable[[ActionStep], bool]) -> None:
        """Register interactive UI/CLI confirmation handler."""
        self._confirmation_handler = handler

    def is_action_allowed(self, action: ActionStep, auto_mode: bool = False) -> bool:
        """
        Check if an action can be executed based on risk level and policy.
        """
        if action.tool_name in self._auto_approved_tools:
            return True

        if action.risk_level == RiskLevel.LOW:
            return True

        if action.risk_level == RiskLevel.MEDIUM:
            # If default level is at least LEVEL_1, allow writes
            if self.default_level >= PermissionLevel.LEVEL_1:
                return True

        # HIGH risk (e.g. terminal execution)
        if auto_mode and self.default_level >= PermissionLevel.LEVEL_2:
            return True

        # Delegate to interactive confirmation handler if registered
        if self._confirmation_handler:
            return self._confirmation_handler(action)

        # Default safe behavior: deny unconfirmed high risk actions
        logger.warning(f"Action '{action.tool_name}' blocked: requires explicit user confirmation.")
        return False
