---
name: xlsx
description: Create, read or edit Excel spreadsheets (.xlsx, also .csv), with real formulas. Use when the user asks for a spreadsheet, budget, tracker, Excel file or wants data from a .xlsx.
license: MIT
---
# Excel spreadsheets (.xlsx)

Needs Python with openpyxl: `python -m pip install openpyxl` (once, if missing).
Helper: `scripts/xlsx_tool.py` in this skill's base folder.

Read
- `python <base>/scripts/xlsx_tool.py read book.xlsx` shows every sheet (first 50 rows) with formulas.

Create or edit
1. Write a Python script that uses openpyxl. Rules:
   - Use Excel formulas for anything computed (`ws["D2"] = "=B2*C2"`, `=SUM(D2:D20)`), not values you calculated in Python, so the sheet stays live.
   - Header row bold (`Font(bold=True)`), freeze it (`ws.freeze_panes = "A2"`), set sensible column widths.
   - Number formats: money `'#,##0.00'`, percent `'0.0%'`, dates `'yyyy-mm-dd'`.
   - Keep data in one table per sheet starting at A1; no merged cells in data areas.
2. Run the script, then check it: `python <base>/scripts/xlsx_tool.py check book.xlsx`.
   It reports formulas pointing at missing sheets, error values (#REF!, #DIV/0!…) and, if LibreOffice is installed, recalculates and reports formula errors. Fix everything it reports.
3. Read it back with `read` and confirm the layout matches what the user asked for.

CSV: read and write with Python's csv module or pandas; open in Excel only if the user wants .xlsx.
