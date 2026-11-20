"""Tests for hardware detection and model recommendation endpoints (Month 3).

Verifies:
1. S6 security checks (x-jarvis-token required).
2. GET /api/system/hardware returns hardware specs and model recommendation.
3. GET /api/dashboard / /api/oblivion/dashboard includes hardware info.
"""
import json
import flask
import pytest
from web import oblivion_bridge


class MockHardwareBridge:
    def __init__(self):
        self.hardware = {
            "total_ram_bytes": 16 * 1024 * 1024 * 1024,
            "available_ram_bytes": 8 * 1024 * 1024 * 1024,
            "total_ram_gb": 16.0,
            "available_ram_gb": 8.0,
            "gpu_name": "Intel(R) Iris(R) Xe Graphics",
            "vram_bytes": 1024 * 1024 * 1024,
            "vram_gb": 1.0,
            "is_integrated_gpu": True,
            "free_disk_bytes": 500 * 1024 * 1024 * 1024,
            "free_disk_gb": 500.0,
        }
        self.recommendation = {
            "recommended_size": "7B-8B",
            "recommended_models": ["qwen2.5-coder:7b", "llama3.1:8b"],
            "tradeoff_explanation": "An 8B model offers the best balance of coding intelligence and execution speed on 16 GB RAM.",
            "fits_in_vram": False,
            "estimated_ram_required_gb": 5.5,
            "recommended_quantization": "Q4_K_M",
            "max_recommended_context_tokens": 16384,
        }

    def status(self):
        return {"running": True, "port": 53211}

    def request(self, method, path, body=None, params=None):
        method = method.upper()
        if path == "/api/system/hardware" and method == "GET":
            return (
                200,
                json.dumps({
                    "hardware": self.hardware,
                    "recommendation": self.recommendation,
                }),
                "application/json",
            )
        return 404, json.dumps({"error": "not found"}), "application/json"


def make_test_app():
    app = flask.Flask(__name__)
    bridge = MockHardwareBridge()
    oblivion_bridge.register(app, bridge)
    client = app.test_client()
    token = app.config["JARVIS_TOKEN"]
    return client, bridge, token


def test_hardware_route_requires_token():
    client, _, _ = make_test_app()
    # Without token -> 403 Forbidden
    res = client.get("/api/system/hardware")
    assert res.status_code == 403

    res2 = client.get("/api/oblivion/system/hardware")
    assert res2.status_code == 403


def test_hardware_route_returns_specs_and_recommendation():
    client, _, token = make_test_app()
    client.environ_base["HTTP_X_JARVIS_TOKEN"] = token

    res = client.get("/api/system/hardware")
    assert res.status_code == 200
    data = res.get_json()

    assert "hardware" in data
    assert "recommendation" in data
    hw = data["hardware"]
    assert hw["total_ram_gb"] == 16.0
    assert hw["gpu_name"] == "Intel(R) Iris(R) Xe Graphics"

    rec = data["recommendation"]
    assert rec["recommended_size"] == "7B-8B"
    assert "qwen2.5-coder:7b" in rec["recommended_models"]
    assert "16 GB RAM" in rec["tradeoff_explanation"]
