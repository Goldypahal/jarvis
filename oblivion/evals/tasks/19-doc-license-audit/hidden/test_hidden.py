import json

def test_licenses():
    data = json.load(open('audited_licenses.json', encoding='utf-8'))
    assert sorted(data) == ['Apache-2.0', 'BSD-3-Clause', 'MIT']
