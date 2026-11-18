"""
NEXUS Agent Package.
Implements the autonomous Plan-Act-Observe-Verify-Repair loop with deterministic policy gating.
"""

from .policy import PermissionPolicy, PermissionLevel
from .planner import AgentPlanner
from .executor import AgentExecutor
from .verifier import AgentVerifier
from .reflection import AgentReflectionEngine
from .tools import (
    BaseTool,
    ToolRegistry,
    ReadFileTool,
    WriteFileTool,
    ListDirTool,
    TerminalTool,
    GitStatusTool,
    GitDiffTool
)

__all__ = [
    "PermissionPolicy",
    "PermissionLevel",
    "AgentPlanner",
    "AgentExecutor",
    "AgentVerifier",
    "AgentReflectionEngine",
    "BaseTool",
    "ToolRegistry",
    "ReadFileTool",
    "WriteFileTool",
    "ListDirTool",
    "TerminalTool",
    "GitStatusTool",
    "GitDiffTool",
]
