import json

def test_chart_reading():
    with open("result.json", "r", encoding="utf-8") as f:
        data = json.load(f)
    assert data["total"] in [87000, 87], f"Expected total 87000, got {data.get('total')}"
    assert data["top_quarter"].upper() == "Q4"
