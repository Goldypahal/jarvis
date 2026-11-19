"""Tests for Model Manager endpoints (Month 3).

Verifies:
1. S6 security checks (x-jarvis-token required) on all model manager routes.
2. GET /api/models returns Ollama status, model list, and hardware recommendation.
3. POST /api/models/delete forwards to Oblivion engine.
4. POST /api/models/speed-test forwards to Oblivion engine.
"""
import json
import flask
import pytest
from web import oblivion_bridge


class MockModelBridge:
    def __init__(self):
        self.port = 54321
        self.token = "mock-oblivion-token"
        self.models_payload = {
            "status": {
                "running": True,
                "endpoint": "http://localhost:11434",
                "version": "0.5.4",
                "error": None,
            },
            "models": [
                {
                    "name": "qwen2.5-coder:7b",
                    "modified_at": "2026-11-19T10:00:00Z",
                    "size": 4683074360,
                    "size_formatted": "4.36 GB",
                    "digest": "sha256:00e13365561b",
                    "details": {
                        "format": "gguf",
                        "family": "qwen2",
                        "parameter_size": "7.6B",
                        "quantization_level": "Q4_K_M",
                    },
                }
            ],
            "recommendation": {
                "recommended_size": "7B-8B",
                "recommended_models": ["qwen2.5-coder:7b", "llama3.1:8b"],
                "tradeoff_explanation": "Optimal model size for 16 GB RAM.",
                "fits_in_vram": False,
                "estimated_ram_required_gb": 5.5,
                "recommended_quantization": "Q4_K_M",
                "max_recommended_context_tokens": 16384,
            },
        }

    def status(self):
        return {"running": True, "port": self.port}

    def request(self, method, path, body=None, params=None):
        method = method.upper()
        if path == "/api/models" and method == "GET":
            return 200, json.dumps(self.models_payload), "application/json"
        elif path == "/api/models/delete" and method == "POST":
            return 200, json.dumps({"status": "ok"}), "application/json"
        elif path == "/api/models/speed-test" and method == "POST":
            return 200, json.dumps({
                "status": "ok",
                "result": {
                    "model": body.get("model", ""),
                    "eval_count": 50,
                    "eval_duration_ms": 1000.0,
                    "tokens_per_second": 50.0,
                    "prompt_eval_count": 20,
                    "prompt_eval_duration_ms": 200.0,
                    "prompt_eval_tokens_per_second": 100.0,
                    "load_duration_ms": 300.0,
                    "total_duration_ms": 1500.0,
                    "sample_text": "Sample generation output.",
                },
            }), "application/json"
        return 404, json.dumps({"error": "not found"}), "application/json"


def make_test_app():
    app = flask.Flask(__name__)
    bridge = MockModelBridge()
    oblivion_bridge.register(app, bridge)
    client = app.test_client()
    token = app.config["JARVIS_TOKEN"]
    return client, token


def test_models_security_requires_token():
    client, token = make_test_app()
    # Missing token -> 403
    res = client.get("/api/models")
    assert res.status_code == 403
    assert "invalid or missing token" in res.get_json()["error"]

    # Wrong token -> 403
    res = client.get("/api/models", headers={"X-Jarvis-Token": "bad-token"})
    assert res.status_code == 403

    # Authorized
    res = client.get("/api/models", headers={"X-Jarvis-Token": token})
    assert res.status_code == 200


def test_models_list_proxy():
    client, token = make_test_app()
    res = client.get("/api/models", headers={"X-Jarvis-Token": token})
    assert res.status_code == 200
    data = res.get_json()
    assert data["status"]["running"] is True
    assert data["status"]["version"] == "0.5.4"
    assert len(data["models"]) == 1
    assert data["models"][0]["name"] == "qwen2.5-coder:7b"
    assert data["models"][0]["size_formatted"] == "4.36 GB"
    assert data["recommendation"]["recommended_size"] == "7B-8B"


def test_models_delete_proxy():
    client, token = make_test_app()
    res = client.post(
        "/api/models/delete",
        headers={"X-Jarvis-Token": token},
        json={"name": "qwen2.5-coder:7b"},
    )
    assert res.status_code == 200
    data = res.get_json()
    assert data["status"] == "ok"


def test_models_speed_test_proxy():
    client, token = make_test_app()
    res = client.post(
        "/api/models/speed-test",
        headers={"X-Jarvis-Token": token},
        json={"model": "qwen2.5-coder:7b"},
    )
    assert res.status_code == 200
    data = res.get_json()
    assert data["status"] == "ok"
    assert data["result"]["model"] == "qwen2.5-coder:7b"
    assert data["result"]["tokens_per_second"] == 50.0
    assert "Sample generation output." in data["result"]["sample_text"]
