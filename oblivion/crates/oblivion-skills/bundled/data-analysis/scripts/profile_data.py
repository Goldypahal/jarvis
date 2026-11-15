"""Profile a data file without printing all of it: shape, column types, missing values,
distinct counts, numeric stats and example values. Requires pandas (and openpyxl for .xlsx)."""
import argparse
import sys

try:
    import pandas as pd
except ImportError:
    sys.exit("pandas is not installed. Run: python -m pip install pandas openpyxl")


def load(path, sheet):
    p = path.lower()
    if p.endswith((".xlsx", ".xlsm", ".xls")):
        return pd.read_excel(path, sheet_name=sheet or 0)
    if p.endswith(".json"):
        try:
            return pd.read_json(path)
        except ValueError:
            return pd.read_json(path, lines=True)
    if p.endswith(".parquet"):
        return pd.read_parquet(path)
    if p.endswith(".tsv"):
        return pd.read_csv(path, sep="\t", low_memory=False)
    try:
        return pd.read_csv(path, low_memory=False)
    except UnicodeDecodeError:
        return pd.read_csv(path, low_memory=False, encoding="latin-1")


def short(v, n=30):
    s = str(v)
    return s if len(s) <= n else s[: n - 1] + "…"


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("path")
    ap.add_argument("--sheet", help="Excel sheet name (default: first sheet)")
    ap.add_argument("--max-columns", type=int, default=60)
    a = ap.parse_args()
    if hasattr(sys.stdout, "reconfigure"):
        sys.stdout.reconfigure(errors="replace")  # Windows consoles without UTF-8

    if a.path.lower().endswith((".xlsx", ".xlsm", ".xls")) and not a.sheet:
        try:
            sheets = pd.ExcelFile(a.path).sheet_names
            if len(sheets) > 1:
                print(f"Sheets: {', '.join(sheets)} (profiling the first; use --sheet NAME)")
        except Exception:
            pass
    df = load(a.path, a.sheet)
    print(f"{len(df):,} rows x {len(df.columns)} columns")
    dups = int(df.duplicated().sum())
    if dups:
        print(f"{dups:,} fully duplicated rows")
    print()
    print(f"{'column':<28} {'type':<10} {'missing':>9} {'distinct':>9}  summary")
    for col in list(df.columns)[: a.max_columns]:
        s = df[col]
        missing = int(s.isna().sum())
        distinct = int(s.nunique(dropna=True))
        kind = str(s.dtype)
        if pd.api.types.is_numeric_dtype(s) and not pd.api.types.is_bool_dtype(s):
            d = s.describe()
            summary = f"min {d['min']:.4g}  mean {d['mean']:.4g}  max {d['max']:.4g}" if d["count"] else "(empty)"
        else:
            parsed = None
            if (s.dtype == object or pd.api.types.is_string_dtype(s)) and distinct > 0:
                sample = s.dropna().astype(str).head(200)
                try:
                    parsed = pd.to_datetime(sample, errors="coerce", format="mixed")
                except (TypeError, ValueError):
                    parsed = pd.to_datetime(sample, errors="coerce")
                if parsed.notna().mean() < 0.9:
                    parsed = None
            if parsed is not None:
                kind = "date-like"
                summary = f"from {parsed.min().date()} to {parsed.max().date()} (sampled)"
            else:
                top = s.astype(str)[s.notna()].value_counts().head(3)
                summary = "top: " + ", ".join(f"{short(k, 20)} ({v})" for k, v in top.items())
        print(f"{short(col, 28):<28} {kind:<10} {missing:>9,} {distinct:>9,}  {summary}")
    if len(df.columns) > a.max_columns:
        print(f"... {len(df.columns) - a.max_columns} more columns (use --max-columns)")
    print("\nFirst 3 rows:")
    with pd.option_context("display.max_columns", 12, "display.width", 160, "display.max_colwidth", 25):
        print(df.head(3).to_string())


if __name__ == "__main__":
    main()
