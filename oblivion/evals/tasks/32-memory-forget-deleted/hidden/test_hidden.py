import pathlib

def test_deleted_memory_not_recalled():
    f = pathlib.Path("auth.txt")
    assert f.exists(), "auth.txt was not created"
    content = f.read_text(encoding="utf-8").strip()
    assert "UNKNOWN" in content or "unknown" in content.lower() or "not found" in content.lower(), f"Expected UNKNOWN for deleted password memory, got: {content}"
    assert "SecretRootP@ss" not in content, "Deleted password was recalled!"
