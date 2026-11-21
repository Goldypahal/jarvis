import json

def test_picture_quiz_answers():
    with open("answers.json", "r", encoding="utf-8") as f:
        data = json.load(f)
    assert str(data.get("q1")).strip() in ["90", "90 degrees", "right angle"]
    assert str(data.get("q2")).strip().lower() == "and"
