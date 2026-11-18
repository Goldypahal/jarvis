"""Tests for Desktop Memory window endpoints and bridge proxy."""
import json
import os
import sys

import pytest

sys.path.insert(0, os.path.join(os.path.dirname(__file__), "..", "web"))
flask = pytest.importorskip("flask")
import oblivion_bridge  # noqa: E402


class MockBridge:
    """Mock bridge simulating Oblivion memory endpoints."""

    def __init__(self):
        self.memories = [
            {
                "id": 1,
                "memory_type": "decision",
                "text": "Always write tests first",
                "project_path": "D:/Jarvis",
                "importance": 0.9,
            }
        ]
        self.next_id = 2

    def status(self):
        return {"running": True, "port": 53211}

    def request(self, method, path, body=None, params=None):
        method = method.upper()
        if path == "/api/memory" and method == "GET":
            return 200, json.dumps(self.memories), "application/json"
        elif path == "/api/memory" and method == "POST":
            new_item = {
                "id": self.next_id,
                "memory_type": body.get("memory_type", "observation"),
                "text": body.get("text", ""),
                "project_path": body.get("project_path"),
                "importance": float(body.get("importance", 0.5)),
            }
            self.next_id += 1
            self.memories.append(new_item)
            return 200, json.dumps(new_item), "application/json"
        elif path.startswith("/api/memory/") and method == "PUT":
            mem_id = int(path.split("/")[-1])
            for m in self.memories:
                if m["id"] == mem_id:
                    m["text"] = body.get("text", m["text"])
                    m["importance"] = float(body.get("importance", m["importance"]))
                    return 200, json.dumps(m), "application/json"
            return 404, json.dumps({"error": "not found"}), "application/json"
        elif path.startswith("/api/memory/") and method == "DELETE":
            mem_id = int(path.split("/")[-1])
            orig_len = len(self.memories)
            self.memories = [m for m in self.memories if m["id"] != mem_id]
            if len(self.memories) < orig_len:
                return 200, json.dumps({"deleted": True}), "application/json"
            return 404, json.dumps({"error": "not found"}), "application/json"
        elif path == "/api/memory/forget" and method == "POST":
            topic = body.get("topic", "")
            return 200, json.dumps({"forgotten": 1, "topic": topic}), "application/json"
        elif path == "/api/memory/export" and method == "GET":
            fmt = (params or {}).get("format", "json")
            if fmt == "markdown":
                return 200, "# Memories\n- Always write tests first", "text/markdown"
            return 200, json.dumps(self.memories), "application/json"
        return 404, json.dumps({"error": "unknown path"}), "application/json"

    def get(self, path, params=None):
        return self.request("GET", path, params=params)

    def post(self, path, body):
        code, text, _ = self.request("POST", path, body=body)
        return code, text

    def put(self, path, body):
        return self.request("PUT", path, body=body)

    def delete(self, path):
        return self.request("DELETE", path)


def make_test_app(bridge=None):
    app = flask.Flask(__name__)
    b = bridge or MockBridge()
    oblivion_bridge.register(app, b)
    client = app.test_client()
    client.environ_base["HTTP_X_JARVIS_TOKEN"] = app.config["JARVIS_TOKEN"]
    return client, b, app.config["JARVIS_TOKEN"]


def test_memory_routes_require_token():
    app = flask.Flask(__name__)
    oblivion_bridge.register(app, MockBridge())
    client = app.test_client()

    # Route and valid method pairs
    routes = [
        ("GET", "/api/oblivion/memory"),
        ("POST", "/api/oblivion/memory"),
        ("GET", "/api/memory"),
        ("PUT", "/api/oblivion/memory/1"),
        ("DELETE", "/api/oblivion/memory/1"),
        ("PUT", "/api/memory/1"),
        ("POST", "/api/oblivion/memory/forget"),
        ("GET", "/api/oblivion/memory/export"),
        ("GET", "/api/memory/export"),
    ]

    for method, path in routes:
        if method == "GET":
            res = client.get(path)
        elif method == "POST":
            res = client.post(path, json={"text": "x"})
        elif method == "PUT":
            res = client.put(path, json={"text": "x"})
        elif method == "DELETE":
            res = client.delete(path)
        assert res.status_code == 403, f"{method} {path} without token must be 403"


def test_memory_routes_crud_lifecycle():
    client, bridge, _ = make_test_app()

    # 1. GET memories
    res = client.get("/api/oblivion/memory")
    assert res.status_code == 200
    data = res.get_json()
    assert len(data) == 1
    assert data[0]["text"] == "Always write tests first"

    # 2. POST create memory
    res = client.post(
        "/api/oblivion/memory",
        json={"text": "Rust engine is primary", "memory_type": "fact", "importance": 0.8},
    )
    assert res.status_code == 200
    created = res.get_json()
    assert created["id"] == 2
    assert created["text"] == "Rust engine is primary"

    # Verify list updated
    res = client.get("/api/memory")
    assert len(res.get_json()) == 2

    # 3. PUT edit memory
    res = client.put(
        "/api/oblivion/memory/2",
        json={"text": "Rust engine is the primary source of truth", "importance": 0.95},
    )
    assert res.status_code == 200
    updated = res.get_json()
    assert updated["text"] == "Rust engine is the primary source of truth"

    # 4. POST forget-by-topic
    res = client.post("/api/oblivion/memory/forget", json={"topic": "legacy python"})
    assert res.status_code == 200
    assert res.get_json()["forgotten"] == 1

    # 5. GET export
    res_json = client.get("/api/oblivion/memory/export?format=json")
    assert res_json.status_code == 200
    assert "attachment; filename=\"memories.json\"" in res_json.headers.get("Content-Disposition", "")

    res_md = client.get("/api/memory/export?format=markdown")
    assert res_md.status_code == 200
    assert "attachment; filename=\"memories.md\"" in res_md.headers.get("Content-Disposition", "")
    assert b"# Memories" in res_md.data

    # 6. DELETE memory
    res = client.delete("/api/oblivion/memory/2")
    assert res.status_code == 200

    # Verify deleted
    res = client.get("/api/oblivion/memory")
    assert len(res.get_json()) == 1


def test_memory_routes_offline_returns_503(tmp_path):
    # Unstarted real bridge should return 503
    app = flask.Flask(__name__)
    b = oblivion_bridge.OblivionBridge(
        binary=str(tmp_path / "missing-oblivion"), workdir=str(tmp_path)
    )
    oblivion_bridge.register(app, b)
    client = app.test_client()
    client.environ_base["HTTP_X_JARVIS_TOKEN"] = app.config["JARVIS_TOKEN"]

    res = client.get("/api/oblivion/memory")
    assert res.status_code == 503
    assert b"Oblivion is not running" in res.data
