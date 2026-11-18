"""
Filesystem Tools for NEXUS Agent.
Provides sandboxed file reading, writing, and directory listing.
"""

from typing import Any, Dict, List, Optional
import os
import asyncio
from pydantic import BaseModel, Field
from .base import BaseTool
from nexus.models.schemas import RiskLevel


class ReadFileInput(BaseModel):
    path: str = Field(description="Relative or absolute path to the file to read")
    max_bytes: int = Field(default=50000, description="Max bytes to read")


class ReadFileTool(BaseTool):
    name = "read_file"
    description = "Read text contents of a file within the workspace."
    risk_level = RiskLevel.LOW
    args_schema = ReadFileInput

    async def execute(self, path: str, max_bytes: int = 50000, **kwargs: Any) -> Dict[str, Any]:
        if not os.path.exists(path):
            raise FileNotFoundError(f"File not found: {path}")
        if os.path.isdir(path):
            raise IsADirectoryError(f"Path is a directory: {path}")

        try:
            with open(path, "r", encoding="utf-8", errors="replace") as f:
                content = f.read(max_bytes)
            return {"path": path, "content": content, "size_bytes": len(content)}
        except Exception as e:
            raise RuntimeError(f"Failed to read file {path}: {e}")


class WriteFileInput(BaseModel):
    path: str = Field(description="Path to write the file to")
    content: str = Field(description="Content to write")
    overwrite: bool = Field(default=True, description="Whether to overwrite if file exists")


class WriteFileTool(BaseTool):
    name = "write_file"
    description = "Write text content to a project file."
    risk_level = RiskLevel.MEDIUM
    args_schema = WriteFileInput

    async def execute(self, path: str, content: str, overwrite: bool = True, **kwargs: Any) -> Dict[str, Any]:
        if os.path.exists(path) and not overwrite:
            raise FileExistsError(f"File already exists and overwrite is False: {path}")

        os.makedirs(os.path.dirname(os.path.abspath(path)), exist_ok=True)
        with open(path, "w", encoding="utf-8") as f:
            f.write(content)

        return {"path": path, "bytes_written": len(content), "status": "written"}


class ListDirInput(BaseModel):
    directory_path: str = Field(default=".", description="Directory path to list")


class ListDirTool(BaseTool):
    name = "list_directory"
    description = "List files and subdirectories in a given path."
    risk_level = RiskLevel.LOW
    args_schema = ListDirInput

    async def execute(self, directory_path: str = ".", **kwargs: Any) -> Dict[str, Any]:
        if not os.path.exists(directory_path):
            raise FileNotFoundError(f"Directory not found: {directory_path}")
        if not os.path.isdir(directory_path):
            raise NotADirectoryError(f"Path is not a directory: {directory_path}")

        entries = os.listdir(directory_path)
        items = []
        for e in entries[:100]:  # Cap at 100 entries
            full_p = os.path.join(directory_path, e)
            items.append({
                "name": e,
                "is_dir": os.path.isdir(full_p),
                "size_bytes": os.path.getsize(full_p) if not os.path.isdir(full_p) else 0
            })
        return {"directory": directory_path, "total_entries": len(entries), "items": items}
