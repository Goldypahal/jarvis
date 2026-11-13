from text import reverse_words

def test_more():
    assert reverse_words('a') == 'a'
    assert reverse_words('one two') == 'two one'
    assert reverse_words('  spaced   out ') == 'out spaced'
