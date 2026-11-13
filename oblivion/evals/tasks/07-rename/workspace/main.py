from billing.core import calc_total
from billing.report import report

if __name__ == '__main__':
    print(calc_total([(2.5, 2)]))
    print(report([(2.5, 2)]))
