def test_deprecated_component():
    comp_id = open('deprecated.txt', encoding='utf-8').read().strip()
    assert comp_id == 'CMP-06'
