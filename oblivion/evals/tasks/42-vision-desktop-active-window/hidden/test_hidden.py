import json

def test_window_parsed():
    with open("window_parsed.json", "r", encoding="utf-8") as f:
        data = json.load(f)
    assert data["title"].strip() == "Document1 - Notepad"
    assert data["process"].strip() == "notepad.exe"
    assert data["area"] == 480000
