import json
import pathlib

def test_agents_md_rules_followed():
    manifest = pathlib.Path("dist/manifest.json")
    assert manifest.exists(), "dist/manifest.json was not created per AGENTS.md rules"
    assert not pathlib.Path("build").exists(), "build/ folder must not exist per AGENTS.md rules"
    data = json.loads(manifest.read_text(encoding="utf-8"))
    assert data.get("version") == "1.2.0"
