"""Tests for weather opt-in (S8)."""
import os
import re

HERE = os.path.dirname(os.path.abspath(__file__))
REPO_ROOT = os.path.abspath(os.path.join(HERE, ".."))
TEMPLATES_DIR = os.path.join(REPO_ROOT, "web", "templates")
STATIC_JS_DIR = os.path.join(REPO_ROOT, "web", "static", "js")


def test_weather_opt_in_in_html():
    index_path = os.path.join(TEMPLATES_DIR, "index.html")
    with open(index_path, "r", encoding="utf-8") as f:
        html = f.read()

    assert "enable-weather-btn" in html
    assert "Enable weather" in html


def test_weather_not_fetched_on_page_load():
    app_js_path = os.path.join(STATIC_JS_DIR, "app.js")
    with open(app_js_path, "r", encoding="utf-8") as f:
        js = f.read()

    # initWeatherWidget must not directly invoke fetchWeather without a user click
    m = re.search(r"function\s+initWeatherWidget\s*\(\)\s*\{([^}]+)\}", js)
    assert m, "initWeatherWidget function not found in app.js"
    body = m.group(1)
    assert "fetchWeather(" not in body, "initWeatherWidget must not call fetchWeather directly on load"
    assert "navigator.geolocation" not in body, "initWeatherWidget must not request geolocation directly on load"
