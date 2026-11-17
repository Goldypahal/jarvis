"""Tests for /api/chat forwarding to Oblivion through the bridge (B1).

Uses a fake Oblivion HTTP server to test send + event reading until turn_end.
"""
import concurrent.futures
import http.server
import json
import os
import secrets
import sys
import threading
import time

import pytest

HERE = os.path.dirname(os.path.abspath(__file__))
WEB_DIR = os.path.abspath(os.path.join(HERE, "..", "web"))
if WEB_DIR not in sys.path:
    sys.path.insert(0, WEB_DIR)

import oblivion_bridge
from app import app


class FakeOblivionHandler(http.server.BaseHTTPRequestHandler):
    def log_message(self, format, *args):
        pass

    def do_GET(self):
        if self.path.startswith("/events"):
            self.send_response(200)
            self.send_header("Content-Type", "text/event-stream")
            self.send_header("Cache-Control", "no-cache")
            self.end_headers()

            self.server.active_clients.append(self.wfile)
            self.wfile.write(b'data: {"seq":1,"type":"state","running":true}\n\n')
            self.wfile.flush()

            while self.server.running:
                try:
                    time.sleep(0.02)
                except Exception:
                    break

    def do_POST(self):
        if self.path == "/send":
            length = int(self.headers.get("Content-Length", 0))
            body = json.loads(self.rfile.read(length).decode("utf-8"))
            text = body.get("text", "")

            self.send_response(204)
            self.end_headers()

            self.server.emit_turn(text)
        elif self.path == "/cancel":
            self.server.cancelled = True
            self.send_response(204)
            self.end_headers()
        elif self.path == "/approve":
            length = int(self.headers.get("Content-Length", 0))
            body = json.loads(self.rfile.read(length).decode("utf-8"))
            self.server.last_approval = body
            self.send_response(204)
            self.end_headers()
            self.server.emit_event({
                "type": "approval_done",
                "id": body.get("id"),
            })
        else:
            self.send_response(404)
            self.end_headers()


class FakeOblivionServer:
    def __init__(self):
        self.server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), FakeOblivionHandler)
        self.server.active_clients = []
        self.server.running = True
        self.server.seq = 1
        self.server.cancelled = False
        self.server.last_approval = None
        self.server.custom_turn = None
        self.server.emit_turn = self._emit_turn
        self.server.emit_event = self.emit_event
        self.port = self.server.server_address[1]
        self.token = "fake-token-" + secrets.token_hex(16)
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)
        self.thread.start()

    def emit_event(self, event_dict):
        self.server.seq += 1
        event_dict["seq"] = self.server.seq
        data = f"data: {json.dumps(event_dict)}\n\n".encode("utf-8")
        dead = []
        for client in list(self.server.active_clients):
            try:
                client.write(data)
                client.flush()
            except Exception:
                dead.append(client)
        for d in dead:
            if d in self.server.active_clients:
                self.server.active_clients.remove(d)

    def _emit_turn(self, text):
        if self.server.custom_turn:
            self.server.custom_turn(self, text)
            return

        self.emit_event({"type": "user", "text": text})
        time.sleep(0.01)
        if text.startswith("/"):
            self.emit_event({"type": "info", "text": f"Command output for {text}"})
            return
        self.emit_event({"type": "text", "text": "Oblivion response to: "})
        time.sleep(0.01)
        self.emit_event({"type": "text", "text": text})
        time.sleep(0.01)
        self.emit_event({
            "type": "turn_end",
            "outcome": "done",
            "steps": 1,
            "tool_calls": 0,
            "prompt_tokens": 12,
            "completion_tokens": 8,
        })

    def stop(self):
        self.server.running = False
        self.server.shutdown()
        self.server.server_close()


class MockProcess:
    def poll(self):
        return None

    def kill(self):
        pass

    def terminate(self):
        pass

    def wait(self, timeout=None):
        pass


@pytest.fixture
def fake_oblivion():
    srv = FakeOblivionServer()
    yield srv
    srv.stop()


@pytest.fixture
def test_client(fake_oblivion, tmp_path):
    bridge = oblivion_bridge.OblivionBridge(
        binary=str(tmp_path / "mock-oblivion"),
        workdir=str(tmp_path),
    )
    bridge.port = fake_oblivion.port
    bridge.token = fake_oblivion.token
    bridge.proc = MockProcess()

    app.config["OBLIVION_BRIDGE"] = bridge
    client = app.test_client()
    return client, bridge


def test_chat_forwards_to_oblivion_and_waits_for_turn_end(test_client):
    client, _ = test_client
    r = client.post("/api/chat", json={"message": "What is Oblivion?"})
    assert r.status_code == 200
    data = r.get_json()
    assert data["response"] == "Oblivion response to: What is Oblivion?"
    assert data["outcome"] == "done"


def test_chat_handles_slash_command_info_event(test_client):
    client, _ = test_client
    r = client.post("/api/chat", json={"message": "/help"})
    assert r.status_code == 200
    data = r.get_json()
    assert data["response"] == "Command output for /help"


def test_chat_rejects_empty_message(test_client):
    client, _ = test_client
    r = client.post("/api/chat", json={"message": "   "})
    assert r.status_code == 400
    data = r.get_json()
    assert "Please enter a message" in data["response"]


def test_chat_handles_turn_error_outcome(test_client, fake_oblivion):
    def error_turn(srv, text):
        srv.emit_event({"type": "user", "text": text})
        time.sleep(0.01)
        srv.emit_event({
            "type": "turn_end",
            "outcome": "error",
            "error": "Rate limit reached on provider",
            "steps": 0,
            "tool_calls": 0,
            "prompt_tokens": 0,
            "completion_tokens": 0,
        })

    fake_oblivion.server.custom_turn = error_turn
    client, _ = test_client

    r = client.post("/api/chat", json={"message": "Do work"})
    assert r.status_code == 200
    data = r.get_json()
    assert data["outcome"] == "error"
    assert "Rate limit reached on provider" in data["response"]


def test_chat_concurrent_requests(test_client):
    client, _ = test_client

    def send_chat(msg):
        return client.post("/api/chat", json={"message": msg})

    with concurrent.futures.ThreadPoolExecutor(max_workers=2) as executor:
        f1 = executor.submit(send_chat, "Alpha request")
        f2 = executor.submit(send_chat, "Beta request")
        r1 = f1.result(timeout=10)
        r2 = f2.result(timeout=10)

    assert r1.status_code == 200
    assert r2.status_code == 200
    d1 = r1.get_json()
    d2 = r2.get_json()
    assert d1["response"] == "Oblivion response to: Alpha request"
    assert d2["response"] == "Oblivion response to: Beta request"


def test_approve_and_deny_endpoints_forward_to_oblivion(test_client, fake_oblivion):
    client, _ = test_client
    token = app.config.get("JARVIS_TOKEN")
    headers = {"X-Jarvis-Token": token}

    # Test Allow (yes)
    r = client.post("/api/oblivion/approve", json={"id": 101, "answer": "yes"}, headers=headers)
    assert r.status_code == 204
    assert fake_oblivion.server.last_approval == {"id": 101, "answer": "yes"}

    # Test Always
    r = client.post("/api/oblivion/approve", json={"id": 102, "answer": "always"}, headers=headers)
    assert r.status_code == 204
    assert fake_oblivion.server.last_approval == {"id": 102, "answer": "always"}

    # Test Deny (no)
    r = client.post("/api/oblivion/approve", json={"id": 103, "answer": "no"}, headers=headers)
    assert r.status_code == 204
    assert fake_oblivion.server.last_approval == {"id": 103, "answer": "no"}


def test_split_workspace_dom_elements_present(test_client):
    client, _ = test_client
    r = client.get("/")
    assert r.status_code == 200
    html = r.get_data(as_text=True)

    # Core 30/70 window structure
    assert 'id="splitModal"' in html
    assert 'id="split-chat-messages"' in html
    assert 'id="split-chat-form"' in html
    assert 'id="split-user-input"' in html
    assert 'id="split-approvals"' in html
    assert 'id="split-tasks-card"' in html
    assert 'id="split-task-name"' in html
    assert 'id="split-tasks-bar"' in html
    assert 'id="split-tasks-list"' in html
    assert 'id="split-tools-card"' in html
    assert 'id="split-tools-stream"' in html

    # Verify no fake stub data remains inside the workspace modal
    modal_start = html.find('id="splitModal"')
    assert modal_start != -1
    modal_end = html.find('<!--', modal_start)
    modal_html = html[modal_start:modal_end if modal_end != -1 else modal_start + 3000]
    assert "nexus_web_memory.db" not in modal_html
    assert "RAG Knowledge Vault Loaded" not in modal_html
    assert "System 1/2 Reasoning" not in modal_html
