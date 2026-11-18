"""Tests for local test site under oblivion/evals/sites/ (B8).

Verifies the static HTML test site provides multi-page form, quiz,
pagination, and login pages for browser evals and agent testing.
"""
import os
import pathlib
import pytest

HERE = pathlib.Path(__file__).resolve().parent
REPO_ROOT = HERE.parent
SITES_DIR = REPO_ROOT / "oblivion" / "evals" / "sites"


def test_sites_directory_and_pages_exist():
    assert SITES_DIR.is_dir(), f"Sites directory missing: {SITES_DIR}"

    required_pages = [
        "index.html",
        "form.html",
        "quiz.html",
        "pagination.html",
        "login.html",
    ]
    for page in required_pages:
        p = SITES_DIR / page
        assert p.is_file(), f"Missing page: {page}"
        content = p.read_text(encoding="utf-8")
        assert "<!DOCTYPE html>" in content
        assert "<html" in content
        assert "</html>" in content


def test_form_page_structure():
    form_html = (SITES_DIR / "form.html").read_text(encoding="utf-8")
    # Multi-step form elements
    assert "Step 1" in form_html
    assert "Step 2" in form_html
    assert "Step 3" in form_html or "Review" in form_html
    assert 'name="fullname"' in form_html or 'id="fullname"' in form_html
    assert 'name="email"' in form_html or 'id="email"' in form_html
    assert "Submit" in form_html


def test_quiz_page_structure():
    quiz_html = (SITES_DIR / "quiz.html").read_text(encoding="utf-8")
    # Quiz questions and submission
    assert "Question 1" in quiz_html or "Q1" in quiz_html
    assert "Question 2" in quiz_html or "Q2" in quiz_html
    assert "Question 3" in quiz_html or "Q3" in quiz_html
    assert 'type="radio"' in quiz_html
    assert "Submit" in quiz_html
    assert "Score" in quiz_html


def test_pagination_page_structure():
    page_html = (SITES_DIR / "pagination.html").read_text(encoding="utf-8")
    # Paginated table, page controls
    assert "Page 1" in page_html
    assert "Page 2" in page_html
    assert "Next" in page_html
    assert "Previous" in page_html
    assert 'id="search"' in page_html or 'name="search"' in page_html


def test_login_page_structure():
    login_html = (SITES_DIR / "login.html").read_text(encoding="utf-8")
    # Login credentials and dashboard
    assert 'type="password"' in login_html
    assert "username" in login_html.lower()
    assert "Log In" in login_html or "Sign In" in login_html
