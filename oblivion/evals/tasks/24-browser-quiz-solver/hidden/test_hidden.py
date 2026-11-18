import json

def test_quiz_answers():
    ans = json.load(open('answers.json', encoding='utf-8'))
    assert [a.strip().lower() for a in ans] == ['rust', 'find', 'ask']
