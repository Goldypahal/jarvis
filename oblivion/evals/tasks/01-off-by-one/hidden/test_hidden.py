from mathutil import sum_to

def test_values():
    assert sum_to(1) == 1
    assert sum_to(10) == 55
    assert sum_to(0) == 0
