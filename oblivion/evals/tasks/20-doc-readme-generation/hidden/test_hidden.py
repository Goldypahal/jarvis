def test_api_md():
    content = open('API.md', encoding='utf-8').read()
    assert 'clamp' in content
    assert 'lerp' in content
    assert 'safe_div' in content
    assert 'float' in content
