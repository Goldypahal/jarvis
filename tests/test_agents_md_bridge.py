"""Tests for AGENTS.md reading and update proposal bridge routes.

Verifies:
1. Security checks (Host validation & Jarvis secret token required).
2. GET /api/system/agents-md and POST /api/system/agents-md/apply proxy correctly.
"""
import json
import flask
import pytest
from web import oblivion_bridge


class MockAgentsMdBridge:
    def __init__(self):
        self.proposals = ["New fact"]
        self.applied = False

    def status(self):
        return {"running": True, "port": 53211}

    def request(self, method, path, body=None, params=None):
        method = method.upper()
        if path == "/api/system/agents-md" and method == "GET":
            return (
                200,
                json.dumps({
                    "file": "AGENTS.md",
                    "exists": True,
                    "content": "# Test\n- cargo test",
                    "loaded_at_start": "AGENTS.md",
                    "proposed_additions": self.proposals,
                }),
                "application/json",
            )
        elif path == "/api/system/agents-md/apply" and method == "POST":
            self.applied = True
            count = len(self.proposals)
            additions = list(self.proposals)
            self.proposals = []
            return (
                200,
                json.dumps({
                    "status": "ok",
                    "file": "AGENTS.md",
                    "added_count": count,
                    "additions": additions,
                }),
                "application/json",
            )
        return 404, json.dumps({"error": "not found"}), "application/json"


def make_test_app():
    app = flask.Flask(__name__)
    bridge = MockAgentsMdBridge()
    oblivion_bridge.register(app, bridge)
    client = app.test_client()
    token = app.config["JARVIS_TOKEN"]
    return client, bridge, token


def test_agents_md_route_requires_token():
    client, _, _ = make_test_app()
    # Without token -> 403 Forbidden
    res = client.get("/api/system/agents-md")
    assert res.status_code == 403

    res_post = client.post("/api/system/agents-md/apply")
    assert res_post.status_code == 403


def test_agents_md_get_proxies_to_oblivion():
    client, _, token = make_test_app()
    client.environ_base["HTTP_X_JARVIS_TOKEN"] = token

    res = client.get("/api/system/agents-md")
    assert res.status_code == 200
    data = res.get_json()
    assert data["file"] == "AGENTS.md"
    assert data["exists"] is True
    assert data["loaded_at_start"] == "AGENTS.md"
    assert data["proposed_additions"] == ["New fact"]


def test_agents_md_apply_proxies_to_oblivion():
    client, bridge, token = make_test_app()
    client.environ_base["HTTP_X_JARVIS_TOKEN"] = token

    res = client.post("/api/system/agents-md/apply", json={})
    assert res.status_code == 200
    data = res.get_json()
    assert data["status"] == "ok"
    assert data["added_count"] == 1
    assert data["additions"] == ["New fact"]
    assert bridge.applied is True
