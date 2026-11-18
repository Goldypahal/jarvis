def test_dead_code():
    name = open('dead_code.txt', encoding='utf-8').read().strip()
    assert name == 'legacy_obsolete_hash'
