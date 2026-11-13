from strings import is_palindrome

def test_pal():
    assert is_palindrome('A man, a plan, a canal: Panama')
    assert is_palindrome('racecar')
    assert is_palindrome('')
    assert not is_palindrome('hello')
    assert not is_palindrome('ab')
