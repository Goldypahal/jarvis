def test_session_token():
    token = open('session_token.txt', encoding='utf-8').read().strip()
    assert token == 'OBLIVION-AUTH-SUCCESS-99'
