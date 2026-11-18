import json

def test_config_resolution():
    data = json.load(open('resolved.json', encoding='utf-8'))
    assert data == {'host': '0.0.0.0', 'port': 9000, 'debug': True, 'workers': 4}
