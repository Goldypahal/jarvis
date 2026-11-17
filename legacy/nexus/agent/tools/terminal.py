"""
Terminal Execution Tool for NEXUS Agent.
Executes shell commands with timeout, output truncation, and safety filters.
"""

from typing import Any, Dict, Optional
import asyncio
import os
import subprocess
from pydantic import BaseModel, Field
from .base import BaseTool
from nexus.models.schemas import RiskLevel


class TerminalInput(BaseModel):
    command: str = Field(description="The shell command line to execute")
    cwd: Optional[str] = Field(default=None, description="Working directory for command execution")
    timeout_seconds: int = Field(default=30, description="Max execution timeout in seconds")


class TerminalTool(BaseTool):
    name = "terminal"
    description = "Execute a shell command with safety controls, timeout, and output capture."
    risk_level = RiskLevel.HIGH
    args_schema = TerminalInput

    # Disallowed commands that must never run automatically
    DISALLOWED_PATTERNS = [
        "format ", "del /f /s /q c:\\", "rmdir /s /q c:\\",
        ":(){ :|:& };:", "dd if="
    ]

    async def execute(
        self,
        command: str,
        cwd: Optional[str] = None,
        timeout_seconds: int = 30,
        **kwargs: Any
    ) -> Dict[str, Any]:
        cmd_lower = command.lower()
        for disallowed in self.DISALLOWED_PATTERNS:
            if disallowed in cmd_lower:
                raise PermissionError(f"Command rejected by safety policy: {command}")

        target_cwd = cwd or os.getcwd()

        try:
            process = await asyncio.create_subprocess_shell(
                command,
                cwd=target_cwd,
                stdout=asyncio.subprocess.PIPE,
                stderr=asyncio.subprocess.PIPE
            )

            try:
                stdout_data, stderr_data = await asyncio.wait_for(
                    process.communicate(),
                    timeout=timeout_seconds
                )
            except asyncio.TimeoutError:
                try:
                    process.kill()
                except Exception:
                    pass
                return {
                    "command": command,
                    "returncode": -1,
                    "stdout": "",
                    "stderr": f"Command timed out after {timeout_seconds} seconds",
                    "status": "timeout"
                }

            stdout_str = stdout_data.decode("utf-8", errors="replace").strip()
            stderr_str = stderr_data.decode("utf-8", errors="replace").strip()

            # Truncate very large outputs to prevent context overflow
            max_chars = 10000
            if len(stdout_str) > max_chars:
                stdout_str = stdout_str[:max_chars] + f"\n... [Truncated: {len(stdout_str) - max_chars} characters]"

            return {
                "command": command,
                "returncode": process.returncode,
                "stdout": stdout_str,
                "stderr": stderr_str,
                "status": "success" if process.returncode == 0 else "error"
            }

        except Exception as e:
            return {
                "command": command,
                "returncode": -1,
                "stdout": "",
                "stderr": str(e),
                "status": "error"
            }
