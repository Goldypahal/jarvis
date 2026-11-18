import json

def test_features_list():
    data = json.load(open('features.json', encoding='utf-8'))
    assert isinstance(data, list)
    lower = [item.lower() for item in data]
    assert any('form' in item for item in lower)
    assert any('quiz' in item for item in lower)
    assert any('pagination' in item for item in lower)
