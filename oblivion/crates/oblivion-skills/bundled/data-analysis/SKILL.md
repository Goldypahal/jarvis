---
name: data-analysis
description: Analyse data files (CSV, Excel, JSON) with Python - profile the data, clean it, compute statistics and trends, compare groups, and make charts - then explain the findings. Use when the user gives a data file and asks questions about it, wants a summary, a chart, trends, or "analyse this data".
license: MIT
---
# Data analysis

Needs Python with pandas and matplotlib (`python -m pip install pandas matplotlib openpyxl`).
Never paste whole data files into the conversation: compute in Python and print small results.

1. **Profile first**: `python <base>/scripts/profile_data.py data.csv` (also .xlsx, .json, .parquet;
   `--sheet NAME` for Excel). It prints row/column counts, each column's type, missing values, distinct
   counts, min/max/mean and example values, so you understand the data without reading it all.
2. **Restate the question** in measurable terms (which column, which grouping, which period).
   If the data cannot answer it, say what is missing.
3. **Clean, in a script** (`analysis.py`, so it can be rerun): parse dates, fix types, trim text,
   handle missing values and duplicates explicitly, and print how many rows each step changed.
   Never overwrite the original file; write cleaned data to a new file if needed.
4. **Analyse** with pandas: groupby/aggregate, pivot tables, growth rates, shares, correlations.
   Print only the result tables (`.head(20)`, `.round(2)`). Sanity-check totals against the raw data.
5. **Charts** with matplotlib, saved as PNG (`plt.savefig("chart.png", dpi=150, bbox_inches="tight")`):
   line for time, bar for categories (sorted), scatter for two numbers, histogram for distributions.
   Title says the finding, axes have labels and units, no 3-D or pie charts with many slices.
6. **Report**: the answer first (2-4 sentences with the key numbers), then supporting tables/charts,
   caveats (missing data, small samples, correlation is not causation), and the script and output file names.
   For a spreadsheet deliverable use the xlsx skill; for slides, the pptx skill.
