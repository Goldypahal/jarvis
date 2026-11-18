"""Tests for Desktop Sessions window endpoints and bridge proxy (B10)."""
import json
import os
import sys

import pytest

sys.path.insert(0, os.path.join(os.path.dirname(__file__), "..", "web"))
flask = pytest.importorskip("flask")
import oblivion_bridge  # noqa: E402


class MockSessionsBridge:
    """Mock bridge simulating Oblivion sessions endpoints."""

    def __init__(self):
        self.sessions = [
            {
                "id": "20261118-100000",
                "created": "2026-11-18T10:00:00Z",
                "updated": "2026-11-18T10:30:00Z",
                "workspace": "D:/Jarvis",
                "first_prompt": "Explain this project architecture",
                "message_count": 4,
            }
        ]
        self.active_session_id = "20261118-100000"

    def status(self):
        return {"running": True, "port": 53211}

    def request(self, method, path, body=None, params=None):
        method = method.upper()
        if path == "/api/sessions" and method == "GET":
            q = (params or {}).get("q", "").lower()
            if q:
                filtered = [
                    s
                    for s in self.sessions
                    if q in s["first_prompt"].lower()
                    or q in s["workspace"].lower()
                    or q in s["id"].lower()
                ]
                return 200, json.dumps(filtered), "application/json"
            return 200, json.dumps(self.sessions), "application/json"

        elif path == "/api/sessions" and method == "POST":
            ws = (body or {}).get("workspace") or "D:/Jarvis"
            new_id = f"20261119-{len(self.sessions) + 1:06d}"
            new_session = {
                "id": new_id,
                "created": "2026-11-19T00:00:00Z",
                "updated": "2026-11-19T00:00:00Z",
                "workspace": ws,
                "first_prompt": "",
                "message_count": 0,
            }
            self.sessions.insert(0, new_session)
            self.active_session_id = new_id
            return 201, json.dumps({"status": "created", "workspace": ws, "id": new_id}), "application/json"

        elif path.startswith("/api/sessions/") and method == "GET":
            sid = path.split("/")[3]
            for s in self.sessions:
                if s["id"] == sid:
                    return (
                        200,
                        json.dumps(
                            {
                                "id": sid,
                                "created": s["created"],
                                "workspace": s["workspace"],
                                "messages": [
                                    {"role": "user", "content": s["first_prompt"]},
                                    {"role": "assistant", "content": "Acknowledged."},
                                ],
                            }
                        ),
                        "application/json",
                    )
            return 404, json.dumps({"error": "not found"}), "application/json"

        elif "/rename" in path and method == "POST":
            sid = path.split("/")[3]
            for s in self.sessions:
                if s["id"] == sid:
                    s["first_prompt"] = (body or {}).get("title", s["first_prompt"])
                    return 200, json.dumps(s), "application/json"
            return 404, json.dumps({"error": "not found"}), "application/json"

        elif "/resume" in path and method == "POST":
            sid = path.split("/")[3]
            for s in self.sessions:
                if s["id"] == sid:
                    self.active_session_id = sid
                    return 200, json.dumps(s), "application/json"
            return 404, json.dumps({"error": "not found"}), "application/json"

        elif "/workspace" in path and method == "POST":
            sid = path.split("/")[3]
            ws = (body or {}).get("workspace", "")
            for s in self.sessions:
                if s["id"] == sid:
                    s["workspace"] = ws
                    return 200, json.dumps(s), "application/json"
            return 404, json.dumps({"error": "not found"}), "application/json"

        elif path.startswith("/api/sessions/") and method == "DELETE":
            sid = path.split("/")[3]
            orig_len = len(self.sessions)
            self.sessions = [s for s in self.sessions if s["id"] != sid]
            if len(self.sessions) < orig_len:
                return 200, json.dumps({"deleted": True}), "application/json"
            return 404, json.dumps({"error": "not found"}), "application/json"

        return 404, json.dumps({"error": "unknown path"}), "application/json"

    def get(self, path, params=None):
        return self.request("GET", path, params=params)

    def post(self, path, body):
        code, text, _ = self.request("POST", path, body=body)
        return code, text

    def delete(self, path):
        return self.request("DELETE", path)


def make_test_app(bridge=None):
    app = flask.Flask(__name__)
    b = bridge or MockSessionsBridge()
    oblivion_bridge.register(app, b)
    client = app.test_client()
    client.environ_base["HTTP_X_JARVIS_TOKEN"] = app.config["JARVIS_TOKEN"]
    return client, b, app.config["JARVIS_TOKEN"]


def test_session_routes_require_token():
    app = flask.Flask(__name__)
    oblivion_bridge.register(app, MockSessionsBridge())
    client = app.test_client()

    routes = [
        ("GET", "/api/oblivion/sessions"),
        ("POST", "/api/oblivion/sessions"),
        ("GET", "/api/sessions"),
        ("GET", "/api/oblivion/sessions/20261118-100000"),
        ("DELETE", "/api/oblivion/sessions/20261118-100000"),
        ("POST", "/api/oblivion/sessions/20261118-100000/rename"),
        ("POST", "/api/oblivion/sessions/20261118-100000/resume"),
        ("POST", "/api/oblivion/sessions/20261118-100000/workspace"),
    ]

    for method, path in routes:
        if method == "GET":
            res = client.get(path)
        elif method == "POST":
            res = client.post(path, json={"title": "x", "workspace": "."})
        elif method == "DELETE":
            res = client.delete(path)
        assert res.status_code == 403, f"{method} {path} without token must be 403"


def test_session_routes_crud_and_resume():
    client, bridge, _ = make_test_app()

    # 1. GET sessions
    res = client.get("/api/oblivion/sessions")
    assert res.status_code == 200
    sessions = res.get_json()
    assert len(sessions) == 1
    assert sessions[0]["id"] == "20261118-100000"

    # 2. POST create session with chosen work folder (B10)
    res = client.post(
        "/api/oblivion/sessions",
        json={"workspace": "D:/Jarvis/oblivion"},
    )
    assert res.status_code == 201
    created = res.get_json()
    assert created["workspace"] == "D:/Jarvis/oblivion"
    new_id = created["id"]

    # Verify session listed
    res = client.get("/api/sessions")
    assert len(res.get_json()) == 2

    # 3. Rename session
    res = client.post(
        f"/api/oblivion/sessions/{new_id}/rename",
        json={"title": "Oblivion Rust Subsystem Analysis"},
    )
    assert res.status_code == 200
    assert res.get_json()["first_prompt"] == "Oblivion Rust Subsystem Analysis"

    # 4. Search sessions
    res_search = client.get("/api/sessions?q=subsystem")
    assert res_search.status_code == 200
    results = res_search.get_json()
    assert len(results) == 1
    assert results[0]["id"] == new_id

    # 5. Change workspace per session (B10)
    res_ws = client.post(
        f"/api/sessions/{new_id}/workspace",
        json={"workspace": "D:/Jarvis/crates"},
    )
    assert res_ws.status_code == 200
    assert res_ws.get_json()["workspace"] == "D:/Jarvis/crates"

    # 6. Resume session
    res_resume = client.post(f"/api/oblivion/sessions/{new_id}/resume")
    assert res_resume.status_code == 200
    assert bridge.active_session_id == new_id

    # 7. GET full session details
    res_get = client.get(f"/api/sessions/{new_id}")
    assert res_get.status_code == 200
    assert res_get.get_json()["id"] == new_id

    # 8. DELETE session
    res_del = client.delete(f"/api/oblivion/sessions/{new_id}")
    assert res_del.status_code == 200

    # Verify deleted
    res_after = client.get("/api/sessions")
    assert len(res_after.get_json()) == 1


def test_session_routes_offline_returns_503(tmp_path):
    app = flask.Flask(__name__)
    b = oblivion_bridge.OblivionBridge(
        binary=str(tmp_path / "missing-oblivion"), workdir=str(tmp_path)
    )
    oblivion_bridge.register(app, b)
    client = app.test_client()
    client.environ_base["HTTP_X_JARVIS_TOKEN"] = app.config["JARVIS_TOKEN"]

    res = client.get("/api/oblivion/sessions")
    assert res.status_code == 503
    assert b"Oblivion is not running" in res.data
