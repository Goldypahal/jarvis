import json

def test_app_allow_list():
    with open("allow_check.json", "r", encoding="utf-8") as f:
        data = json.load(f)
    assert data["calc_allowed"] is True
    assert data["cmd_allowed"] is False
