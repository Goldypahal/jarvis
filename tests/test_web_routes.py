"""Tests for every route in web/app.py.

Verifies end-to-end functionality, status codes, payload validation,
security guards, and bridge integration for all routes defined in web/app.py:
- GET /
- POST /api/chat
- GET /api/oblivion/status
- POST /api/oblivion/start
- GET /api/oblivion/events
- POST /api/oblivion/send
- POST /api/oblivion/approve
- POST /api/oblivion/cancel
"""
import os
import sys
import pytest

HERE = os.path.dirname(os.path.abspath(__file__))
WEB_DIR = os.path.abspath(os.path.join(HERE, "..", "web"))
if WEB_DIR not in sys.path:
    sys.path.insert(0, WEB_DIR)

from app import app


@pytest.fixture
def client():
    app.config["TESTING"] = True
    token = app.config.get("JARVIS_TOKEN", "test-secret-token")
    app.config["JARVIS_TOKEN"] = token
    with app.test_client() as c:
        yield c


@pytest.fixture
def auth_headers():
    return {
        "X-Jarvis-Token": app.config.get("JARVIS_TOKEN", ""),
        "Host": f"localhost:{app.config.get('JARVIS_PORT', 5000)}",
    }


# ==========================================
# 1. Route: GET /
# ==========================================

def test_index_route(client):
    """GET / renders index.html containing Jarvis desktop and the security token."""
    res = client.get("/")
    assert res.status_code == 200
    assert res.mimetype == "text/html"
    body = res.get_data(as_text=True)
    assert "Jarvis" in body
    # Token must be present in the page for the frontend panel to authenticate
    token = app.config.get("JARVIS_TOKEN", "")
    assert token in body


# ==========================================
# 2. Route: POST /api/chat
# ==========================================

def test_chat_route_validation(client):
    """POST /api/chat validates input payloads."""
    # Missing body
    r = client.post("/api/chat", json=None)
    assert r.status_code == 400
    assert "Please enter a message" in r.get_json().get("response", "")

    # Empty string
    r = client.post("/api/chat", json={"message": ""})
    assert r.status_code == 400

    # Whitespace only
    r = client.post("/api/chat", json={"message": "   \n\t  "})
    assert r.status_code == 400


def test_chat_route_bridge_success(client, monkeypatch):
    """POST /api/chat forwards messages to bridge.send_and_wait and returns response."""
    class MockBridge:
        def send_and_wait(self, msg, timeout=60):
            return {"response": f"Echo: {msg}", "outcome": "done", "error": None}

    monkeypatch.setitem(app.config, "OBLIVION_BRIDGE", MockBridge())

    r = client.post("/api/chat", json={"message": "Hello Jarvis"})
    assert r.status_code == 200
    data = r.get_json()
    assert data["response"] == "Echo: Hello Jarvis"
    assert data["outcome"] == "done"


def test_chat_route_bridge_with_images(client, monkeypatch):
    """POST /api/chat forwards images to bridge.send_and_wait."""
    class MockBridgeWithImages:
        def __init__(self):
            self.last_images = None
        def send_and_wait(self, msg, timeout=60, images=None):
            self.last_images = images
            return {"response": f"Saw {len(images or [])} image(s)", "outcome": "done", "error": None}

    bridge = MockBridgeWithImages()
    monkeypatch.setitem(app.config, "OBLIVION_BRIDGE", bridge)

    r = client.post("/api/chat", json={
        "message": "Analyze screen",
        "images": [{"mime_type": "image/png", "data_base64": "abc", "name": "screen.png"}]
    })
    assert r.status_code == 200
    data = r.get_json()
    assert data["response"] == "Saw 1 image(s)"
    assert bridge.last_images == [{"mime_type": "image/png", "data_base64": "abc", "name": "screen.png"}]


def test_chat_route_timeout(client, monkeypatch):
    """POST /api/chat handles TimeoutError from bridge with 504."""
    class TimeoutBridge:
        def send_and_wait(self, msg, timeout=60):
            raise TimeoutError("Timed out")

    monkeypatch.setitem(app.config, "OBLIVION_BRIDGE", TimeoutBridge())

    r = client.post("/api/chat", json={"message": "Slow command"})
    assert r.status_code == 504
    data = r.get_json()
    assert "timed out" in data.get("response", "").lower()


def test_chat_route_error(client, monkeypatch):
    """POST /api/chat handles generic exceptions with 500."""
    class FailingBridge:
        def send_and_wait(self, msg, timeout=60):
            raise RuntimeError("Engine crashed")

    monkeypatch.setitem(app.config, "OBLIVION_BRIDGE", FailingBridge())

    r = client.post("/api/chat", json={"message": "Crash"})
    assert r.status_code == 500
    data = r.get_json()
    assert "error" in data.get("response", "").lower()


def test_chat_route_no_bridge_configured(client, monkeypatch):
    """POST /api/chat returns 500 when bridge is not configured."""
    monkeypatch.setitem(app.config, "OBLIVION_BRIDGE", None)
    r = client.post("/api/chat", json={"message": "Test"})
    assert r.status_code == 500
    assert "not configured" in r.get_json().get("response", "")


# ==========================================
# 3. Route: GET /api/oblivion/status
# ==========================================

def test_oblivion_status_security(client):
    """GET /api/oblivion/status requires valid token and Host header."""
    # No auth
    assert client.get("/api/oblivion/status").status_code == 403

    # Invalid token
    assert client.get(
        "/api/oblivion/status",
        headers={"X-Jarvis-Token": "bad-token"}
    ).status_code == 403

    # Invalid Host
    valid_token = app.config["JARVIS_TOKEN"]
    assert client.get(
        "/api/oblivion/status",
        headers={"X-Jarvis-Token": valid_token, "Host": "evil.com"}
    ).status_code == 403


def test_oblivion_status_success(client, auth_headers):
    """GET /api/oblivion/status returns bridge status object."""
    r = client.get("/api/oblivion/status", headers=auth_headers)
    assert r.status_code == 200
    data = r.get_json()
    assert "running" in data
    assert "binary" in data
    assert "workdir" in data


# ==========================================
# 4. Route: POST /api/oblivion/start
# ==========================================

def test_oblivion_start_security(client):
    """POST /api/oblivion/start requires authentication."""
    assert client.post("/api/oblivion/start", json={}).status_code == 403


def test_oblivion_start_success(client, auth_headers):
    """POST /api/oblivion/start returns startup status."""
    r = client.post("/api/oblivion/start", json={}, headers=auth_headers)
    assert r.status_code == 200
    data = r.get_json()
    assert "running" in data


# ==========================================
# 5. Route: GET /api/oblivion/events
# ==========================================

def test_oblivion_events_security(client):
    """GET /api/oblivion/events requires authentication."""
    assert client.get("/api/oblivion/events").status_code == 403


def test_oblivion_events_stream_headers(client, auth_headers, monkeypatch):
    """GET /api/oblivion/events returns SSE stream with proper cache headers."""
    class FakeEventsBridge:
        def events(self):
            yield 'data: {"type":"state"}\n\n'

    monkeypatch.setitem(app.config, "OBLIVION_BRIDGE", FakeEventsBridge())

    r = client.get("/api/oblivion/events", headers=auth_headers)
    assert r.status_code == 200
    assert "text/event-stream" in r.mimetype
    assert r.headers.get("Cache-Control") == "no-store"
    # Read first event chunk
    chunk = next(r.response)
    assert b'data: {"type":"state"}' in chunk


# ==========================================
# 6. Route: POST /api/oblivion/send
# ==========================================

def test_oblivion_send_route(client, auth_headers, monkeypatch):
    """POST /api/oblivion/send forwards JSON payload to bridge /send."""
    called = []
    class FakePostBridge:
        def post(self, path, body):
            called.append((path, body))
            return 204, ""

    monkeypatch.setitem(app.config, "OBLIVION_BRIDGE", FakePostBridge())

    # Missing body
    r = client.post("/api/oblivion/send", headers=auth_headers)
    assert r.status_code == 400

    # Valid payload
    r = client.post("/api/oblivion/send", json={"text": "list files"}, headers=auth_headers)
    assert r.status_code == 204
    assert called == [("/send", {"text": "list files"})]


# ==========================================
# 7. Route: POST /api/oblivion/approve
# ==========================================

def test_oblivion_approve_route(client, auth_headers, monkeypatch):
    """POST /api/oblivion/approve forwards decision to bridge /approve."""
    called = []
    class FakeApproveBridge:
        def post(self, path, body):
            called.append((path, body))
            return 200, "OK"

    monkeypatch.setitem(app.config, "OBLIVION_BRIDGE", FakeApproveBridge())

    # Unauthorized
    assert client.post("/api/oblivion/approve", json={"id": 1, "decision": "yes"}).status_code == 403

    # Authorized forward
    r = client.post(
        "/api/oblivion/approve",
        json={"id": 1, "decision": "yes"},
        headers=auth_headers,
    )
    assert r.status_code == 200
    assert called == [("/approve", {"id": 1, "decision": "yes"})]


# ==========================================
# 8. Route: POST /api/oblivion/cancel
# ==========================================

def test_oblivion_cancel_route(client, auth_headers, monkeypatch):
    """POST /api/oblivion/cancel forwards cancel signal to bridge /cancel."""
    called = []
    class FakeCancelBridge:
        def post(self, path, body):
            called.append((path, body))
            return 204, ""

    monkeypatch.setitem(app.config, "OBLIVION_BRIDGE", FakeCancelBridge())

    # Unauthorized
    assert client.post("/api/oblivion/cancel", json={}).status_code == 403

    # Authorized forward
    r = client.post("/api/oblivion/cancel", json={}, headers=auth_headers)
    assert r.status_code == 204
    assert called == [("/cancel", {})]


# ==========================================
# 9. Query param authentication
# ==========================================

def test_query_param_auth_alternative(client):
    """Routes accept token as ?t= or ?token= in query string."""
    token = app.config["JARVIS_TOKEN"]
    port = app.config.get("JARVIS_PORT", 5000)
    host = f"127.0.0.1:{port}"

    # Using ?t=
    r = client.get(f"/api/oblivion/status?t={token}", headers={"Host": host})
    assert r.status_code == 200

    # Using ?token=
    r = client.get(f"/api/oblivion/status?token={token}", headers={"Host": host})
    assert r.status_code == 200


# ==========================================
# 10. Route: POST /api/oblivion/screen/capture
# ==========================================

def test_oblivion_screen_capture_route(client, auth_headers, monkeypatch):
    """POST /api/oblivion/screen/capture forwards to bridge screen capture endpoint."""
    called = []
    class FakeScreenBridge:
        def post(self, path, body):
            called.append((path, body))
            return 200, '{"ok": true, "image": {"mime_type": "image/png", "data_base64": "fakeb64", "name": "screen.png"}}'

    monkeypatch.setitem(app.config, "OBLIVION_BRIDGE", FakeScreenBridge())

    # Unauthorized
    assert client.post("/api/oblivion/screen/capture").status_code == 403

    # Authorized forward
    r = client.post("/api/oblivion/screen/capture", headers=auth_headers)
    assert r.status_code == 200
    data = r.get_json()
    assert data["ok"] is True
    assert data["image"]["mime_type"] == "image/png"
    assert called == [("/api/oblivion/screen/capture", {})]


# ==========================================
# 11. Computer Use Routes: Status, Stop, Reset
# ==========================================

def test_oblivion_computer_routes(client, auth_headers, monkeypatch):
    """Computer use routes require auth and forward correctly."""
    calls = []

    class FakeComputerBridge:
        def request(self, method, path, body=None, headers=None):
            calls.append((method, path, body))
            if path == "/api/computer/status":
                return 200, '{"enabled": true, "stopped": false, "allowed_apps": ["notepad.exe"], "stop_hotkey": "Escape"}', "application/json"
            elif path == "/api/computer/stop":
                return 200, '{"ok": true, "stopped": true, "message": "Emergency stop activated"}', "application/json"
            elif path == "/api/computer/reset":
                return 200, '{"ok": true, "stopped": false, "message": "Emergency stop reset"}', "application/json"
            return 404, "Not found", "text/plain"

    monkeypatch.setitem(app.config, "OBLIVION_BRIDGE", FakeComputerBridge())

    # Unauthorized requests fail with 403
    assert client.get("/api/oblivion/computer/status").status_code == 403
    assert client.post("/api/oblivion/computer/stop").status_code == 403
    assert client.post("/api/oblivion/computer/reset").status_code == 403

    # Authorized GET status
    r_status = client.get("/api/oblivion/computer/status", headers=auth_headers)
    assert r_status.status_code == 200
    st_data = r_status.get_json()
    assert st_data["enabled"] is True
    assert st_data["stopped"] is false_or_true(False)
    assert st_data["stop_hotkey"] == "Escape"

    # Authorized POST stop
    r_stop = client.post("/api/oblivion/computer/stop", headers=auth_headers)
    assert r_stop.status_code == 200
    assert r_stop.get_json()["stopped"] is True

    # Authorized POST reset
    r_reset = client.post("/api/oblivion/computer/reset", headers=auth_headers)
    assert r_reset.status_code == 200
    assert r_reset.get_json()["stopped"] is False


def false_or_true(val):
    return val
