"""Small helper for Excel files: read, check. Requires openpyxl."""
import os
import re
import shutil
import subprocess
import sys
import tempfile

try:
    import openpyxl
except ImportError:
    sys.exit("openpyxl is not installed. Run: python -m pip install openpyxl")

ERRORS = ('#REF!', '#DIV/0!', '#VALUE!', '#NAME?', '#N/A', '#NUM!', '#NULL!')


def read(path, max_rows=50):
    wb = openpyxl.load_workbook(path)
    for ws in wb.worksheets:
        print(f'== Sheet "{ws.title}" ({ws.max_row} rows x {ws.max_column} cols)')
        for r, row in enumerate(ws.iter_rows(values_only=True), start=1):
            if r > max_rows:
                print(f'... {ws.max_row - max_rows} more rows')
                break
            print(f'{r:>4}: ' + ' | '.join('' if v is None else str(v) for v in row))
        print()


def check(path):
    problems = []
    wb = openpyxl.load_workbook(path)
    names = {ws.title for ws in wb.worksheets}
    formulas = 0
    for ws in wb.worksheets:
        for row in ws.iter_rows():
            for c in row:
                v = c.value
                if isinstance(v, str) and v.startswith('='):
                    formulas += 1
                    for ref in re.findall(r"(?:'([^']+)'|([A-Za-z_][\w ]*))!", v):
                        sheet = ref[0] or ref[1]
                        if sheet and sheet not in names:
                            problems.append(f'{ws.title}!{c.coordinate}: formula refers to missing sheet "{sheet}": {v}')
                    if '#REF!' in v:
                        problems.append(f'{ws.title}!{c.coordinate}: broken reference in formula: {v}')
                elif isinstance(v, str) and v.strip() in ERRORS:
                    problems.append(f'{ws.title}!{c.coordinate}: error value {v}')
    soffice = shutil.which('soffice') or shutil.which('libreoffice')
    if not soffice and os.name == 'nt':
        for p in (r'C:\Program Files\LibreOffice\program\soffice.exe', r'C:\Program Files (x86)\LibreOffice\program\soffice.exe'):
            if os.path.exists(p):
                soffice = p
    if soffice and formulas:
        out = tempfile.mkdtemp()
        subprocess.run([soffice, '--headless', '--convert-to', 'xlsx', '--outdir', out, path], capture_output=True, timeout=120)
        calc = os.path.join(out, os.path.basename(path))
        if os.path.exists(calc):
            wbv = openpyxl.load_workbook(calc, data_only=True)
            for ws in wbv.worksheets:
                for row in ws.iter_rows():
                    for c in row:
                        if isinstance(c.value, str) and c.value.strip() in ERRORS:
                            problems.append(f'{ws.title}!{c.coordinate}: formula evaluates to {c.value}')
        shutil.rmtree(out, ignore_errors=True)
    elif formulas:
        print('(LibreOffice not found: formulas were checked for structure only, not calculated)')
    print(f'{formulas} formula(s) checked.')
    if problems:
        print('PROBLEMS:')
        for p in problems:
            print('  ' + p)
        sys.exit(1)
    print('OK: no problems found.')


if __name__ == '__main__':
    a = sys.argv[1:]
    if len(a) == 2 and a[0] == 'read':
        read(a[1])
    elif len(a) == 2 and a[0] == 'check':
        check(a[1])
    else:
        sys.exit('usage: xlsx_tool.py read FILE | check FILE')
