"""Tests for requirements.txt and supported Python version (B6)."""
import os
import re

HERE = os.path.dirname(os.path.abspath(__file__))
REPO_ROOT = os.path.abspath(os.path.join(HERE, ".."))


def test_requirements_no_stdlib_and_has_flask():
    req_path = os.path.join(REPO_ROOT, "requirements.txt")
    with open(req_path, "r", encoding="utf-8") as f:
        lines = [line.strip() for line in f if line.strip() and not line.startswith("#")]

    # sqlite3 and asyncio are in stdlib and break pip install
    assert "sqlite3" not in lines
    assert "asyncio" not in lines

    # Flask 3.x must be pinned
    flask_line = next((l for l in lines if l.lower().startswith("flask")), None)
    assert flask_line is not None, "Flask must be in root requirements.txt"
    assert "3." in flask_line, f"Flask must be pinned to 3.x, found: {flask_line}"


def test_readme_states_python_version():
    readme_path = os.path.join(REPO_ROOT, "README.md")
    with open(readme_path, "r", encoding="utf-8") as f:
        readme = f.read()

    assert "3.12" in readme, "README must state supported Python version 3.12"
