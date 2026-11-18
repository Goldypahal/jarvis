import json
import pathlib

def test_user_preference_applied():
    f = pathlib.Path("config.json")
    assert f.exists(), "config.json was not created"
    raw = f.read_text(encoding="utf-8")
    assert raw.endswith("\n"), "config.json must end with newline"
    assert "    \"name\"" in raw or "    \"active\"" in raw, "Must use 4-space indentation per user preference"
    parsed = json.loads(raw)
    assert parsed.get("name") == "Jarvis"
    assert parsed.get("active") is True
