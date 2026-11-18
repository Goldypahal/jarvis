def test_confirmation_code():
    code = open('confirmation.txt', encoding='utf-8').read().strip()
    assert code == 'CONF-7842'
