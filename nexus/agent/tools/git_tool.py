"""
Git Tools for NEXUS Agent.
Provides repository status inspection, branch querying, and diff analysis.
"""

from typing import Any, Dict, Optional
import os
import subprocess
from pydantic import BaseModel, Field
from .base import BaseTool
from nexus.models.schemas import RiskLevel


class GitStatusInput(BaseModel):
    repo_path: str = Field(default=".", description="Path to git repository")


class GitStatusTool(BaseTool):
    name = "git_status"
    description = "Inspect current working directory git status and branch."
    risk_level = RiskLevel.LOW
    args_schema = GitStatusInput

    async def execute(self, repo_path: str = ".", **kwargs: Any) -> Dict[str, Any]:
        try:
            res = subprocess.run(
                ["git", "status", "--porcelain", "-b"],
                cwd=repo_path,
                capture_output=True,
                text=True,
                check=False
            )
            branch_info = ""
            changed_files = []
            lines = res.stdout.strip().split("\n")
            if lines and lines[0].startswith("##"):
                branch_info = lines[0][3:].strip()
                lines = lines[1:]

            for line in lines:
                if line.strip():
                    changed_files.append(line.strip())

            return {
                "branch": branch_info,
                "clean": len(changed_files) == 0,
                "changed_files_count": len(changed_files),
                "changed_files": changed_files[:30]
            }
        except Exception as e:
            return {"error": f"Git command failed: {e}", "clean": False}


class GitDiffInput(BaseModel):
    repo_path: str = Field(default=".", description="Path to git repository")
    staged: bool = Field(default=False, description="Whether to check staged diff")


class GitDiffTool(BaseTool):
    name = "git_diff"
    description = "Get working tree git diff for code changes."
    risk_level = RiskLevel.LOW
    args_schema = GitDiffInput

    async def execute(self, repo_path: str = ".", staged: bool = False, **kwargs: Any) -> Dict[str, Any]:
        try:
            cmd = ["git", "diff"]
            if staged:
                cmd.append("--cached")
            res = subprocess.run(
                cmd,
                cwd=repo_path,
                capture_output=True,
                text=True,
                check=False
            )
            diff_text = res.stdout.strip()
            max_chars = 15000
            if len(diff_text) > max_chars:
                diff_text = diff_text[:max_chars] + f"\n... [Diff truncated: {len(diff_text)-max_chars} chars]"
            return {"diff": diff_text, "has_diff": len(diff_text) > 0}
        except Exception as e:
            return {"error": f"Git diff failed: {e}", "has_diff": False}
