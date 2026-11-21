import json

def test_find_button():
    with open("button_info.json", "r", encoding="utf-8") as f:
        data = json.load(f)
    assert data["id"] == "btn-confirm-deploy"
    assert "deploy to production" in data["text"].lower()
