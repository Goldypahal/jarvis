"""Tests for voice status and transcription proxy routes in web/oblivion_bridge.py."""

import json
import os
import sys
import pytest

HERE = os.path.dirname(os.path.abspath(__file__))
WEB_DIR = os.path.abspath(os.path.join(HERE, "..", "web"))
if WEB_DIR not in sys.path:
    sys.path.insert(0, WEB_DIR)

from app import app


class FakeVoiceBridge:
    def __init__(self):
        self.running_state = True
        self.last_method = None
        self.last_path = None
        self.last_body = None

    def running(self):
        return self.running_state

    def status(self):
        return {"running": True, "port": 5000, "error": None}

    def start(self):
        return {"running": True, "port": 5000, "error": None}

    def post(self, path, body):
        code, text, _ = self.request("POST", path, body)
        return code, text

    def request(self, method, path, body=None, params=None, headers=None):
        self.last_method = method
        self.last_path = path
        self.last_body = body
        if path == "/api/voice/status":
            return (
                200,
                json.dumps({
                    "enabled": True,
                    "stt_engine": "auto",
                    "hotkey": "Ctrl+Space",
                    "language": "en",
                    "local_whisper_available": True,
                    "cloud_whisper_available": False,
                    "web_speech_available": True,
                }),
                "application/json",
            )
        elif path == "/api/voice/transcribe":
            return (
                200,
                json.dumps({
                    "text": "open terminal and run cargo check",
                    "language": "en",
                    "duration_ms": 450,
                    "engine": "local-whisper",
                }),
                "application/json",
            )
        elif path == "/api/voice/tts/speak":
            return (
                200,
                json.dumps({
                    "text": "Hello world from Jarvis",
                    "duration_ms": 120,
                    "audio_base64": "UklGRgAAAAA=",
                    "format": "audio/wav",
                }),
                "application/json",
            )
        elif path == "/approve":
            if (
                self.last_body
                and self.last_body.get("source") == "voice"
                and self.last_body.get("answer") == "yes"
                and self.last_body.get("is_high_risk")
            ):
                return (
                    403,
                    json.dumps({
                        "error": "Voice approval disallowed for high-risk action",
                        "code": "VOICE_APPROVAL_DISALLOWED",
                    }),
                    "application/json",
                )
            return 204, "", "text/plain"
        return 404, "Not found", "text/plain"


@pytest.fixture
def client_with_fake_bridge():
    app.config["TESTING"] = True
    old_token = app.config.get("JARVIS_TOKEN")
    old_bridge = app.config.get("OBLIVION_BRIDGE")

    token = "test-voice-token-xyz"
    app.config["JARVIS_TOKEN"] = token
    fake = FakeVoiceBridge()
    app.config["OBLIVION_BRIDGE"] = fake
    try:
        with app.test_client() as c:
            yield c, fake
    finally:
        if old_token is not None:
            app.config["JARVIS_TOKEN"] = old_token
        if old_bridge is not None:
            app.config["OBLIVION_BRIDGE"] = old_bridge


@pytest.fixture
def auth_headers():
    return {
        "X-Jarvis-Token": "test-voice-token-xyz",
        "Host": f"localhost:{app.config.get('JARVIS_PORT', 5000)}",
    }


def test_voice_status_requires_auth(client_with_fake_bridge):
    client, _ = client_with_fake_bridge
    res = client.get("/api/voice/status")
    assert res.status_code == 403


def test_voice_status_authorized(client_with_fake_bridge, auth_headers):
    client, fake = client_with_fake_bridge
    res = client.get("/api/voice/status", headers=auth_headers)
    assert res.status_code == 200
    data = res.get_json()
    assert data["enabled"] is True
    assert data["hotkey"] == "Ctrl+Space"
    assert data["local_whisper_available"] is True
    assert fake.last_method == "GET"
    assert fake.last_path == "/api/voice/status"


def test_voice_transcribe_requires_auth(client_with_fake_bridge):
    client, _ = client_with_fake_bridge
    res = client.post("/api/voice/transcribe", json={"audio_base64": "abc"})
    assert res.status_code == 403


def test_voice_transcribe_authorized(client_with_fake_bridge, auth_headers):
    client, fake = client_with_fake_bridge
    payload = {"audio_base64": "dGVzdC1hdWRpby1ieXRlcw==", "language": "en"}
    res = client.post(
        "/api/oblivion/voice/transcribe",
        headers=auth_headers,
        json=payload,
    )
    assert res.status_code == 200
    data = res.get_json()
    assert data["text"] == "open terminal and run cargo check"
    assert data["engine"] == "local-whisper"
    assert fake.last_method == "POST"
    assert fake.last_path == "/api/voice/transcribe"


def test_voice_tts_speak_requires_auth(client_with_fake_bridge):
    client, _ = client_with_fake_bridge
    res = client.post("/api/voice/tts/speak", json={"text": "hello"})
    assert res.status_code == 403


def test_voice_tts_speak_authorized(client_with_fake_bridge, auth_headers):
    client, fake = client_with_fake_bridge
    payload = {"text": "Hello world from Jarvis"}
    res = client.post(
        "/api/oblivion/voice/tts/speak",
        headers=auth_headers,
        json=payload,
    )
    assert res.status_code == 200
    data = res.get_json()
    assert data["text"] == "Hello world from Jarvis"
    assert data["format"] == "audio/wav"
    assert data["audio_base64"] == "UklGRgAAAAA="
    assert fake.last_method == "POST"
    assert fake.last_path == "/api/voice/tts/speak"


def test_voice_approval_low_risk_forwarded(client_with_fake_bridge, auth_headers):
    client, fake = client_with_fake_bridge
    payload = {"id": 1, "answer": "yes", "source": "voice"}
    res = client.post(
        "/api/oblivion/approve",
        headers=auth_headers,
        json=payload,
    )
    assert res.status_code == 204
    assert fake.last_method == "POST"
    assert fake.last_path == "/approve"
    assert fake.last_body["source"] == "voice"
    assert fake.last_body["answer"] == "yes"


def test_voice_approval_high_risk_forbidden(client_with_fake_bridge, auth_headers):
    client, fake = client_with_fake_bridge
    payload = {
        "id": 2,
        "answer": "yes",
        "source": "voice",
        "is_high_risk": True,
    }
    res = client.post(
        "/api/oblivion/approve",
        headers=auth_headers,
        json=payload,
    )
    assert res.status_code == 403
    data = res.get_json()
    assert data["code"] == "VOICE_APPROVAL_DISALLOWED"
    assert fake.last_method == "POST"
    assert fake.last_path == "/approve"
