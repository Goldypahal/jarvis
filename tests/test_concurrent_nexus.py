"""Tests for concurrent NEXUS calls in web/app.py (B4)."""
import concurrent.futures
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
WEB_DIR = os.path.abspath(os.path.join(HERE, "..", "web"))
if WEB_DIR not in sys.path:
    sys.path.insert(0, WEB_DIR)

import app as web_app


def test_nexus_lock_exists():
    assert hasattr(web_app, "nexus_lock"), "web/app.py must define nexus_lock"


def test_two_concurrent_chat_requests():
    client = web_app.app.test_client()

    def send_chat(msg):
        return client.post("/api/chat", json={"message": msg})

    with concurrent.futures.ThreadPoolExecutor(max_workers=2) as executor:
        f1 = executor.submit(send_chat, "Hello from thread 1")
        f2 = executor.submit(send_chat, "Hello from thread 2")
        r1 = f1.result(timeout=10)
        r2 = f2.result(timeout=10)

    assert r1.status_code == 200
    assert r2.status_code == 200
    assert "response" in r1.get_json()
    assert "response" in r2.get_json()
