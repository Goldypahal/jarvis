"""Tests verifying documentation truth (Q7).

README.md, WEB_README.md, and RUN_WEB_INTERFACE.md must describe reality:
Jarvis desktop + Oblivion Rust engine, with one Getting Started path.
"""
import os
import pytest

HERE = os.path.dirname(os.path.abspath(__file__))
REPO_ROOT = os.path.abspath(os.path.join(HERE, ".."))


def test_readme_truth():
    readme_path = os.path.join(REPO_ROOT, "README.md")
    with open(readme_path, "r", encoding="utf-8") as f:
        content = f.read()

    assert "Oblivion" in content
    assert "Jarvis" in content
    assert "cargo build" in content
    assert "3.12" in content
    # Should not describe NEXUS as current system
    assert "# NEXUS: Advanced AI Personal Assistant" not in content
    assert "python -m nexus" not in content


def test_web_readme_truth():
    web_readme_path = os.path.join(REPO_ROOT, "WEB_README.md")
    with open(web_readme_path, "r", encoding="utf-8") as f:
        content = f.read()

    assert "Jarvis" in content
    assert "Oblivion" in content
    assert "cargo build" in content
    # Should not describe dead NEXUS routes as active
    assert "GET /api/skills" not in content
    assert "GET /api/status" not in content


def test_run_web_interface_truth():
    run_path = os.path.join(REPO_ROOT, "RUN_WEB_INTERFACE.md")
    with open(run_path, "r", encoding="utf-8") as f:
        content = f.read()

    assert "Jarvis" in content
    assert "Oblivion" in content
    assert "cargo build" in content
    assert "python 3.14" not in content.lower()
    assert "C:\\Users\\Asus" not in content
