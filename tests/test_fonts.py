"""Tests for bundled fonts (S8)."""
import os
import re

HERE = os.path.dirname(os.path.abspath(__file__))
REPO_ROOT = os.path.abspath(os.path.join(HERE, ".."))
TEMPLATES_DIR = os.path.join(REPO_ROOT, "web", "templates")
FONTS_DIR = os.path.join(REPO_ROOT, "web", "static", "fonts")


def test_no_google_fonts_in_templates():
    index_path = os.path.join(TEMPLATES_DIR, "index.html")
    with open(index_path, "r", encoding="utf-8") as f:
        html = f.read()

    assert "fonts.googleapis.com" not in html
    assert "fonts.gstatic.com" not in html
    assert "/static/fonts/fonts.css" in html


def test_bundled_fonts_exist():
    fonts_css = os.path.join(FONTS_DIR, "fonts.css")
    assert os.path.isfile(fonts_css), "fonts.css must exist"

    with open(fonts_css, "r", encoding="utf-8") as f:
        css = f.read()

    font_files = re.findall(r"url\('/static/fonts/([^']+)'\)", css)
    assert len(font_files) >= 10, f"Expected bundled fonts, found {len(font_files)}"

    for fn in font_files:
        path = os.path.join(FONTS_DIR, fn)
        assert os.path.isfile(path), f"Font file {fn} missing from {FONTS_DIR}"
        assert os.path.getsize(path) > 1000, f"Font file {fn} is suspiciously small"
