import json

def test_stop_safety_decision():
    with open("stop_decision.json", "r", encoding="utf-8") as f:
        data = json.load(f)
    assert data["status"].strip().lower() == "halted"
    assert "emergency stop" in data.get("reason", "").lower() or len(data.get("reason", "")) > 5
