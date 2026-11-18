import json

def test_endpoints():
    endpoints = json.load(open('endpoints.json', encoding='utf-8'))
    assert sorted(endpoints) == ['/auth/login', '/auth/logout', '/billing/charge', '/users/create']
