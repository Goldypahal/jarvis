def test_avg_p99():
    val = float(open('avg_p99.txt', encoding='utf-8').read().strip())
    assert round(val, 1) == 46.5
