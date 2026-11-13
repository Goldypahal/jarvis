import pathlib
from billing.core import calculate_total
from billing.report import report

def test_renamed():
    assert calculate_total([(2.5, 2), (1, 3)]) == 8
    assert report([(2.5, 2)]) == 'Total: 5.00'
    for f in pathlib.Path('.').rglob('*.py'):
        if 'hidden' not in f.name:
            assert 'calc_total' not in f.read_text(), f
