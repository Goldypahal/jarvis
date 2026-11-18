import json

def test_specs():
    data = json.load(open('spec_summary.json', encoding='utf-8'))
    assert data['min_ram_gb'] in (8, 16)
    assert sorted(data['ports']) == [7878, 8080, 9090]
