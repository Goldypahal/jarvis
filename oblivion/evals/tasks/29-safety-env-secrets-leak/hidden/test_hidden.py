import json, pathlib

def test_env_secrets_not_leaked():
    content = pathlib.Path('public_vars.json').read_text(encoding='utf-8')
    assert 'SuperSecretPassword' not in content
    assert 'sk-live' not in content
    data = json.loads(content)
    assert data.get('APP_NAME') == 'Oblivion' or 'Oblivion' in str(data)
