"""
Bridge between the Jarvis web desktop and the Oblivion agent.

Oblivion (the Rust agent in ../oblivion) has its own local web server
(`oblivion --web`). This module starts it in the background the first time the
Oblivion panel is opened, reads the private address and access key it prints,
and passes the panel's requests through. The browser only ever talks to the
Jarvis server, so the access key never leaves this computer.

Settings (environment variables, all optional):
  OBLIVION_BIN      path to oblivion(.exe); default ../oblivion/target/release/oblivion
  OBLIVION_WORKDIR  folder the agent works in; default the Jarvis folder
"""

import atexit
import http.client
import json
import os
import re
import subprocess
import sys
import threading
import time
from collections import deque

HERE = os.path.dirname(os.path.abspath(__file__))
JARVIS_ROOT = os.path.abspath(os.path.join(HERE, ".."))
READY_RE = re.compile(r"http://127\.0\.0\.1:(\d+)/#t=([0-9a-f]{64})")
START_TIMEOUT = 60  # seconds; the first start can be slow while Windows scans the file


def default_binary():
    name = "oblivion.exe" if os.name == "nt" else "oblivion"
    return os.path.join(JARVIS_ROOT, "oblivion", "target", "release", name)


class OblivionBridge:
    def __init__(self, binary=None, workdir=None, extra_args=None, env=None):
        self.binary = binary or os.environ.get("OBLIVION_BIN") or default_binary()
        self.workdir = workdir or os.environ.get("OBLIVION_WORKDIR") or JARVIS_ROOT
        self.extra_args = list(extra_args or [])
        self.env = env
        self.proc = None
        self.port = None
        self.token = None
        self.error = None
        self.output = deque(maxlen=60)  # last lines Oblivion printed, for error messages
        self._lock = threading.Lock()
        atexit.register(self.stop)

    # ---------- lifecycle ----------

    def running(self):
        return self.proc is not None and self.proc.poll() is None and self.port is not None

    def status(self):
        return {
            "running": self.running(),
            "error": None if self.running() else self.error,
            "binary": self.binary,
            "workdir": self.workdir,
        }

    def start(self):
        """Start Oblivion if it is not running. Returns status()."""
        with self._lock:
            if self.running():
                return self.status()
            self.error = None
            if not os.path.isfile(self.binary):
                self.error = (
                    f"Oblivion is not built yet ({self.binary} not found). Build it with: "
                    "cd D:\\Jarvis\\oblivion  then  cargo build --release"
                )
                return self.status()
            if not os.path.isdir(self.workdir):
                self.error = f"The work folder does not exist: {self.workdir}"
                return self.status()
            cmd = [self.binary, "--web", "--no-open", "--port", "0", *self.extra_args]
            flags = 0
            if os.name == "nt":
                flags = subprocess.CREATE_NO_WINDOW  # no extra console window
            try:
                self.proc = subprocess.Popen(
                    cmd,
                    cwd=self.workdir,
                    stdin=subprocess.DEVNULL,
                    stdout=subprocess.PIPE,
                    stderr=subprocess.STDOUT,
                    env=self.env,
                    creationflags=flags,
                )
            except OSError as e:
                self.error = f"Could not start Oblivion: {e}"
                return self.status()

            ready = threading.Event()
            threading.Thread(target=self._read_output, args=(self.proc, ready), daemon=True).start()
            if not ready.wait(START_TIMEOUT) or self.port is None:
                tail = " | ".join(list(self.output)[-6:])
                if self.proc.poll() is None:
                    self.proc.kill()
                self.error = "Oblivion did not start. " + (
                    f"It printed: {tail}" if tail else "It printed nothing."
                )
                self.proc = None
            return self.status()

    def _read_output(self, proc, ready):
        # Keep reading for the whole run so Oblivion never blocks on a full pipe.
        for raw in iter(proc.stdout.readline, b""):
            line = re.sub(r"\x1b\[[0-9;]*m", "", raw.decode("utf-8", "replace")).rstrip()
            if line:
                self.output.append(line)
            m = READY_RE.search(line)
            if m and not ready.is_set():
                self.port, self.token = int(m.group(1)), m.group(2)
                ready.set()
        if not ready.is_set():
            ready.set()  # exited before it was ready; start() reports the output

    def stop(self):
        proc, self.proc, self.port, self.token = self.proc, None, None, None
        if proc is not None and proc.poll() is None:
            proc.terminate()
            try:
                proc.wait(timeout=5)
            except subprocess.TimeoutExpired:
                proc.kill()

    # ---------- requests ----------

    def post(self, path, body):
        """Forward a POST to Oblivion. Returns (status_code, text)."""
        if not self.running():
            return 503, "Oblivion is not running"
        conn = http.client.HTTPConnection("127.0.0.1", self.port, timeout=15)
        try:
            conn.request(
                "POST",
                path,
                body=json.dumps(body or {}),
                headers={"content-type": "application/json", "x-oblivion-token": self.token},
            )
            res = conn.getresponse()
            return res.status, res.read().decode("utf-8", "replace")
        except OSError as e:
            return 502, f"Could not reach Oblivion: {e}"
        finally:
            conn.close()

    def events(self):
        """Yield Oblivion's event stream (Server-Sent Events) as raw bytes."""
        if not self.running():
            yield b'data: {"seq":0,"type":"notice","text":"Oblivion is not running"}\n\n'
            return
        conn = http.client.HTTPConnection("127.0.0.1", self.port, timeout=None)
        try:
            conn.request("GET", f"/events?t={self.token}", headers={"accept": "text/event-stream"})
            res = conn.getresponse()
            if res.status != 200:
                yield f'data: {{"seq":0,"type":"notice","text":"Oblivion refused the stream ({res.status})"}}\n\n'.encode()
                return
            while True:
                chunk = res.read1(8192) if hasattr(res, "read1") else res.read(1)
                if not chunk:
                    break
                yield chunk
        except OSError:
            return
        finally:
            conn.close()


def register(app, bridge=None):
    """Add the /api/oblivion/* routes to a Flask app."""
    from flask import Response, jsonify, request, stream_with_context

    bridge = bridge or OblivionBridge()
    app.config["OBLIVION_BRIDGE"] = bridge

    def same_origin():
        # Block other websites open in the browser from driving the agent:
        # a cross-site request carries an Origin that is not this server.
        origin = request.headers.get("Origin")
        if origin is None:
            return True
        return origin.rstrip("/") == request.host_url.rstrip("/")

    def forward(path):
        if not same_origin():
            return jsonify({"error": "cross-site request refused"}), 403
        body = request.get_json(silent=True)
        if body is None:
            return jsonify({"error": "expected a JSON body"}), 400
        code, text = bridge.post(path, body)
        return Response(text, status=code, mimetype="text/plain")

    @app.get("/api/oblivion/status")
    def oblivion_status():
        return jsonify(bridge.status())

    @app.post("/api/oblivion/start")
    def oblivion_start():
        if not same_origin():
            return jsonify({"error": "cross-site request refused"}), 403
        return jsonify(bridge.start())

    @app.get("/api/oblivion/events")
    def oblivion_events():
        if not same_origin():
            return jsonify({"error": "cross-site request refused"}), 403
        return Response(
            stream_with_context(bridge.events()),
            mimetype="text/event-stream",
            headers={"Cache-Control": "no-store", "X-Accel-Buffering": "no"},
        )

    @app.post("/api/oblivion/send")
    def oblivion_send():
        return forward("/send")

    @app.post("/api/oblivion/approve")
    def oblivion_approve():
        return forward("/approve")

    @app.post("/api/oblivion/cancel")
    def oblivion_cancel():
        return forward("/cancel")

    return bridge


if __name__ == "__main__":
    b = OblivionBridge()
    print(json.dumps(b.start(), indent=2))
    if b.running():
        print(f"Oblivion is listening on 127.0.0.1:{b.port}. Press Ctrl+C to stop.")
        try:
            while b.running():
                time.sleep(1)
        except KeyboardInterrupt:
            pass
        b.stop()
    sys.exit(0 if b.status()["running"] or b.error is None else 1)
