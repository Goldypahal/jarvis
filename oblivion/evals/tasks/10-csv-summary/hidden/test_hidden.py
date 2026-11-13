import json

def test_summary():
    data = json.load(open('summary.json'))
    assert {k.lower(): round(float(v), 2) for k, v in data.items()} == {'north': 12.5, 'south': 7.0, 'east': 7.25}
