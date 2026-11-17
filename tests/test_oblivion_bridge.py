"""Tests for web/oblivion_bridge.py (no Oblivion build needed)."""
import os
import sys

import pytest

sys.path.insert(0, os.path.join(os.path.dirname(__file__), "..", "web"))
flask = pytest.importorskip("flask")
import oblivion_bridge  # noqa: E402


def make_app(tmp_path, binary=None):
    app = flask.Flask(__name__)
    bridge = oblivion_bridge.OblivionBridge(
        binary=binary or str(tmp_path / "missing-oblivion"), workdir=str(tmp_path)
    )
    oblivion_bridge.register(app, bridge)
    return app.test_client(), bridge


def test_missing_binary_explains_how_to_build(tmp_path):
    client, _ = make_app(tmp_path)
    st = client.post("/api/oblivion/start", json={}).get_json()
    assert st["running"] is False
    assert "cargo build --release" in st["error"]


def test_requests_from_other_sites_are_refused(tmp_path):
    client, _ = make_app(tmp_path)
    for path in ("/api/oblivion/send", "/api/oblivion/approve", "/api/oblivion/cancel", "/api/oblivion/start"):
        r = client.post(path, json={"text": "x"}, headers={"Origin": "http://evil.example"})
        assert r.status_code == 403, path
    r = client.get("/api/oblivion/events", headers={"Origin": "http://evil.example"})
    assert r.status_code == 403


def test_send_needs_a_running_agent_and_json(tmp_path):
    client, _ = make_app(tmp_path)
    assert client.post("/api/oblivion/send", data="text", content_type="text/plain").status_code == 400
    assert client.post("/api/oblivion/send", json={"text": "hi"}).status_code == 503


def test_ready_line_is_recognised_with_colours():
    line = "\x1b[36m  http://127.0.0.1:53211/#t=" + "ab" * 32 + "\x1b[0m"
    m = oblivion_bridge.READY_RE.search(line)
    assert m and m.group(1) == "53211" and len(m.group(2)) == 64
