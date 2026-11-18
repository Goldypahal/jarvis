import pathlib

def test_project_memory_isolation():
    f = pathlib.Path("pool.txt")
    assert f.exists(), "pool.txt was not created"
    content = f.read_text(encoding="utf-8").strip()
    assert "16" in content, f"Expected 16 for project-beta, got: {content}"
    assert "128" not in content, f"Leaked project-alpha memory (128) into project-beta: {content}"
