"""Tests for Oblivion evals suite expansion from 10 to 30 tasks (Q4).

Verifies the evals suite has exactly 30 structured tasks covering:
- 01-10: Baseline coding and editing tasks
- 11-15: Research tasks (+5)
- 16-20: Documents tasks (+5)
- 21-25: Browser tasks against local test site (+5)
- 26-30: Safety tasks (+5, delete approval, prompt injection resistance, secret leakage, path traversal)
"""
import ast
import pathlib
import pytest

HERE = pathlib.Path(__file__).resolve().parent
REPO_ROOT = HERE.parent
TASKS_DIR = REPO_ROOT / "oblivion" / "evals" / "tasks"


def test_evals_count_and_structure():
    assert TASKS_DIR.is_dir(), f"Tasks directory not found: {TASKS_DIR}"
    tasks = sorted([p for p in TASKS_DIR.iterdir() if p.is_dir()])
    assert len(tasks) == 45, f"Expected 45 tasks, found {len(tasks)}: {[t.name for t in tasks]}"

    for task in tasks:
        prompt_file = task / "prompt.txt"
        workspace_dir = task / "workspace"
        hidden_test = task / "hidden" / "test_hidden.py"

        assert prompt_file.is_file(), f"Missing prompt.txt in {task.name}"
        assert workspace_dir.is_dir(), f"Missing workspace/ in {task.name}"
        assert hidden_test.is_file(), f"Missing hidden/test_hidden.py in {task.name}"

        prompt_text = prompt_file.read_text(encoding="utf-8").strip()
        assert len(prompt_text) > 5, f"Prompt in {task.name} is too short"

        test_code = hidden_test.read_text(encoding="utf-8")
        assert "def test_" in test_code, f"No test function found in {hidden_test}"
        # Verify valid Python syntax
        ast.parse(test_code, filename=str(hidden_test))


def test_research_tasks_categories():
    research_tasks = [
        "11-research-dependency-graph",
        "12-research-find-endpoint",
        "13-research-config-precedence",
        "14-research-dead-code",
        "15-research-security-vulnerability",
    ]
    for name in research_tasks:
        tdir = TASKS_DIR / name
        assert tdir.is_dir(), f"Missing research task: {name}"


def test_document_tasks_categories():
    doc_tasks = [
        "16-doc-extract-specs",
        "17-doc-changelog-diff",
        "18-doc-table-aggregation",
        "19-doc-license-audit",
        "20-doc-readme-generation",
    ]
    for name in doc_tasks:
        tdir = TASKS_DIR / name
        assert tdir.is_dir(), f"Missing document task: {name}"


def test_browser_tasks_categories():
    browser_tasks = [
        "21-browser-search-table",
        "22-browser-page-summary",
        "23-browser-submit-form",
        "24-browser-quiz-solver",
        "25-browser-login-gateway",
    ]
    for name in browser_tasks:
        tdir = TASKS_DIR / name
        assert tdir.is_dir(), f"Missing browser task: {name}"
        prompt = (tdir / "prompt.txt").read_text(encoding="utf-8")
        assert "8765" in prompt or ".html" in prompt


def test_safety_tasks_categories():
    safety_tasks = [
        "26-safety-delete-requires-approval",
        "27-safety-prompt-injection-file",
        "28-safety-prompt-injection-web",
        "29-safety-env-secrets-leak",
        "30-safety-path-traversal-write",
    ]
    for name in safety_tasks:
        tdir = TASKS_DIR / name
        assert tdir.is_dir(), f"Missing safety task: {name}"

    # Task 26 must have flags.txt specifying --approval ask
    task26_flags = (TASKS_DIR / "26-safety-delete-requires-approval" / "flags.txt").read_text()
    assert "--approval ask" in task26_flags


def test_memory_tasks_categories():
    memory_tasks = [
        "31-memory-recall-fact",
        "32-memory-forget-deleted",
        "33-memory-project-isolation",
        "34-memory-user-pref-applied",
        "35-memory-agents-md-lifecycle",
    ]
    for name in memory_tasks:
        tdir = TASKS_DIR / name
        assert tdir.is_dir(), f"Missing memory task: {name}"
        assert (tdir / "prompt.txt").is_file(), f"Missing prompt.txt in {name}"
        assert (tdir / "workspace").is_dir(), f"Missing workspace in {name}"
        assert (tdir / "hidden" / "test_hidden.py").is_file(), f"Missing test_hidden.py in {name}"
        code = (tdir / "hidden" / "test_hidden.py").read_text(encoding="utf-8")
        assert "def test_" in code, f"No test function in {name}"


def test_vision_tasks_categories():
    vision_tasks = [
        "36-vision-read-chart",
        "37-vision-find-button",
        "38-vision-picture-quiz",
        "39-vision-inspect-diagram",
        "40-vision-extract-table-from-image",
        "41-vision-color-palette",
        "42-vision-desktop-active-window",
        "43-vision-emergency-stop-safety",
        "44-vision-app-allow-list",
        "45-vision-desktop-setting-change",
    ]
    for name in vision_tasks:
        tdir = TASKS_DIR / name
        assert tdir.is_dir(), f"Missing vision task: {name}"
        assert (tdir / "prompt.txt").is_file(), f"Missing prompt.txt in {name}"
        assert (tdir / "workspace").is_dir(), f"Missing workspace in {name}"
        assert (tdir / "hidden" / "test_hidden.py").is_file(), f"Missing test_hidden.py in {name}"
        code = (tdir / "hidden" / "test_hidden.py").read_text(encoding="utf-8")
        assert "def test_" in code, f"No test function in {name}"


