"""Tests for Request Classes, Visible Routing Rules, and Offline Switch (Month 3).

Verifies:
1. S6 security checks (x-jarvis-token required) on routing routes.
2. GET /api/routing returns active rules, request classes, and offline status.
3. POST /api/routing toggles offline mode and updates rules.
"""
import json
import flask
import pytest
from web import oblivion_bridge


class MockRoutingBridge:
    def __init__(self):
        self.port = 54321
        self.token = "mock-oblivion-token"
        self.offline = False
        self.rules = {
            "private": ["ollama/*", "local/*"],
            "simple": ["groq/*", "ollama/*", "gemini-flash*"],
            "hard": ["deepseek-r1*", "claude-3-5-sonnet*", "gemini-pro*"],
            "vision": ["gemini-flash*", "claude-3-5-sonnet*"],
            "long_context": ["gemini-1.5-pro*", "gemini-flash*"],
        }
        self.classes = [
            {
                "id": "private",
                "label": "Private / Sensitive",
                "description": "Credentials, keys, or sensitive patterns. Forced to local models only.",
                "patterns": self.rules["private"],
                "local_only": True,
            },
            {
                "id": "simple",
                "label": "Simple",
                "description": "Greetings, syntax lookups, and short queries. Fast models prioritized.",
                "patterns": self.rules["simple"],
                "local_only": False,
            },
            {
                "id": "hard",
                "label": "Hard Reasoning",
                "description": "Complex logic, math, debugging, and multi-step coding.",
                "patterns": self.rules["hard"],
                "local_only": False,
            },
            {
                "id": "vision",
                "label": "Vision",
                "description": "Image inspection and visual document inputs.",
                "patterns": self.rules["vision"],
                "local_only": False,
            },
            {
                "id": "long_context",
                "label": "Long Context",
                "description": "Requests with context exceeding 16,000 tokens.",
                "patterns": self.rules["long_context"],
                "local_only": False,
            },
        ]

    def status(self):
        return {"running": True, "port": self.port}

    def request(self, method, path, body=None, params=None):
        method = method.upper()
        if path == "/api/routing":
            if method == "GET":
                return 200, json.dumps({
                    "offline": self.offline,
                    "rules": self.rules,
                    "classes": self.classes,
                }), "application/json"
            elif method == "POST":
                body = body or {}
                if "offline" in body:
                    self.offline = bool(body["offline"])
                if "rules" in body:
                    self.rules.update(body["rules"])
                return 200, json.dumps({
                    "offline": self.offline,
                    "rules": self.rules,
                    "classes": self.classes,
                }), "application/json"
        return 404, json.dumps({"error": "not found"}), "application/json"


def make_test_app():
    app = flask.Flask(__name__)
    bridge = MockRoutingBridge()
    oblivion_bridge.register(app, bridge)
    client = app.test_client()
    token = app.config["JARVIS_TOKEN"]
    return client, token, bridge


def test_routing_security_token_required():
    client, token, _ = make_test_app()
    # Without token
    res = client.get("/api/routing")
    assert res.status_code == 403
    assert b"unauthorized" in res.data or "invalid or missing token" in res.get_json()["error"]

    res_post = client.post("/api/routing", json={"offline": True})
    assert res_post.status_code == 403


def test_get_routing():
    client, token, _ = make_test_app()
    res = client.get("/api/routing", headers={"X-Jarvis-Token": token})
    assert res.status_code == 200
    data = res.get_json()
    assert not data["offline"]
    assert len(data["classes"]) == 5
    assert "private" in data["rules"]
    assert "simple" in data["rules"]
    assert "hard" in data["rules"]
    assert "vision" in data["rules"]
    assert "long_context" in data["rules"]


def test_post_routing_toggle_offline():
    client, token, bridge = make_test_app()
    # Toggle offline to True
    res = client.post(
        "/api/routing",
        headers={"X-Jarvis-Token": token},
        json={"offline": True},
    )
    assert res.status_code == 200
    data = res.get_json()
    assert data["offline"] is True

    # Check via GET
    res2 = client.get("/api/routing", headers={"X-Jarvis-Token": token})
    assert res2.status_code == 200
    assert res2.get_json()["offline"] is True

    # Toggle offline back to False
    res3 = client.post(
        "/api/routing",
        headers={"X-Jarvis-Token": token},
        json={"offline": False},
    )
    assert res3.status_code == 200
    assert res3.get_json()["offline"] is False
