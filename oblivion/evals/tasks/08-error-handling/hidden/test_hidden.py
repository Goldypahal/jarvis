from parse import parse_int

def test_parse():
    assert parse_int('5') == 5
    assert parse_int(' 42 ') == 42
    assert parse_int('-3') == -3
    assert parse_int('abc') is None
    assert parse_int('') is None
    assert parse_int(None) is None
    assert parse_int('4.2') is None
