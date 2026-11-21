import json

def test_palette_colors():
    with open("palette.json", "r", encoding="utf-8") as f:
        data = json.load(f)
    assert data["primary"].strip().lower() == "#10b981"
    assert data["secondary"].strip().lower() == "#6366f1"
    assert data["accent"].strip().lower() == "#f59e0b"
