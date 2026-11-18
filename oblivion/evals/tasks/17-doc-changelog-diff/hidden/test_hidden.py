import json

def test_breaking_changes():
    items = json.load(open('breaking_changes.json', encoding='utf-8'))
    assert len(items) == 3
    assert any('nexus' in i.lower() for i in items)
    assert any('uuid' in i.lower() for i in items)
    assert any('axum' in i.lower() for i in items)
