from billing.core import calc_total

def report(items):
    return f'Total: {calc_total(items):.2f}'
