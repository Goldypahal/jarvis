import json

def test_expenses_extracted():
    with open("expenses.json", "r", encoding="utf-8") as f:
        data = json.load(f)
    assert data["total"] == 260
    assert data["highest_category"].strip().lower() == "hosting"
