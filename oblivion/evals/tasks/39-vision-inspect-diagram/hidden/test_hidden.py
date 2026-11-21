import json

def test_pipeline_order():
    with open("pipeline.json", "r", encoding="utf-8") as f:
        data = json.load(f)
    expected = ["Input Parser", "Classifier", "Router", "Tool Executor"]
    assert [x.strip() for x in data] == expected
