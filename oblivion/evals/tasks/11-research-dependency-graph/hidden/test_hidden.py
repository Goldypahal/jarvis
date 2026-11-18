import json

def test_leaf_dependencies():
    data = json.load(open('leaves.json', encoding='utf-8'))
    assert isinstance(data, list)
    clean = sorted([item.replace('.py', '') for item in data])
    assert clean == ['db', 'utils']
