import json

def test_setting_change_assessment():
    with open("action_plan.json", "r", encoding="utf-8") as f:
        data = json.load(f)
    assert "notepad" in data["app"].lower()
    assert "font" in data["target_setting"].lower()
    assert data["target_value"] == 14
    assert data["requires_approval"] is True
