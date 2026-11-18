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
import hmac
import http.client
import json
import os
import re
import secrets
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
        self._send_lock = threading.Lock()
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

    def request(self, method, path, body=None, params=None):
        """Forward an HTTP request to Oblivion. Returns (status_code, text, content_type)."""
        if not self.running():
            return 503, "Oblivion is not running", "text/plain"
        import urllib.parse

        url_path = path
        if params:
            query_str = urllib.parse.urlencode(params)
            sep = "&" if "?" in url_path else "?"
            url_path = f"{url_path}{sep}{query_str}"

        conn = http.client.HTTPConnection("127.0.0.1", self.port, timeout=15)
        try:
            headers = {"x-oblivion-token": self.token}
            encoded_body = None
            if body is not None:
                headers["content-type"] = "application/json"
                encoded_body = json.dumps(body)
            conn.request(method.upper(), url_path, body=encoded_body, headers=headers)
            res = conn.getresponse()
            ct = res.getheader("content-type", "application/json")
            return res.status, res.read().decode("utf-8", "replace"), ct
        except OSError as e:
            return 502, f"Could not reach Oblivion: {e}", "text/plain"
        finally:
            conn.close()

    def post(self, path, body):
        """Forward a POST to Oblivion. Returns (status_code, text)."""
        code, text, _ = self.request("POST", path, body=body)
        return code, text

    def get(self, path, params=None):
        """Forward a GET to Oblivion. Returns (status_code, text, content_type)."""
        return self.request("GET", path, params=params)

    def put(self, path, body):
        """Forward a PUT to Oblivion. Returns (status_code, text, content_type)."""
        return self.request("PUT", path, body=body)

    def delete(self, path):
        """Forward a DELETE to Oblivion. Returns (status_code, text, content_type)."""
        return self.request("DELETE", path)

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

    def send_and_wait(self, text, timeout=60):
        """Send text to Oblivion and wait for the turn to complete.

        Reads events from the SSE stream until turn_end (or an instant command info).
        Returns a dict with {'response': str, 'outcome': str, 'error': Optional[str]}.
        """
        if not text or not str(text).strip():
            raise ValueError("text must not be empty")

        text = str(text).strip()

        if not self.running():
            st = self.start()
            if not st.get("running"):
                raise RuntimeError(st.get("error") or "Oblivion is not running")

        with self._send_lock:
            conn = http.client.HTTPConnection("127.0.0.1", self.port, timeout=timeout)
            try:
                conn.request("GET", f"/events?t={self.token}", headers={"accept": "text/event-stream"})
                res = conn.getresponse()
                if res.status != 200:
                    raise RuntimeError(f"Oblivion refused event stream ({res.status})")

                events_q = deque()
                q_lock = threading.Lock()
                reader_running = [True]

                def _reader():
                    try:
                        while reader_running[0]:
                            line = res.readline()
                            if not line:
                                break
                            line_str = line.decode("utf-8", "replace").strip()
                            if line_str.startswith("data:"):
                                payload = line_str[5:].strip()
                                if payload:
                                    try:
                                        ev = json.loads(payload)
                                        with q_lock:
                                            events_q.append(ev)
                                    except json.JSONDecodeError:
                                        pass
                    except Exception:
                        pass
                    finally:
                        reader_running[0] = False

                t = threading.Thread(target=_reader, daemon=True)
                t.start()

                # Drain backlog items that arrived before our new /send
                prior_seq = 0
                time.sleep(0.03)
                with q_lock:
                    while events_q:
                        ev = events_q.popleft()
                        if isinstance(ev, dict) and "seq" in ev:
                            prior_seq = max(prior_seq, ev["seq"])

                # Send the message
                code, resp_text = self.post("/send", {"text": text})
                if code not in (200, 204):
                    reader_running[0] = False
                    conn.close()
                    raise RuntimeError(f"Oblivion /send failed ({code}): {resp_text}")

                # Read events until turn_end (or info for slash commands)
                deadline = time.time() + timeout
                response_chunks = []
                turn_outcome = "done"
                turn_error = None
                finished = False

                while time.time() < deadline and not finished:
                    ev = None
                    with q_lock:
                        if events_q:
                            ev = events_q.popleft()
                    if ev is None:
                        if not reader_running[0] and not events_q:
                            break
                        time.sleep(0.01)
                        continue

                    ev_seq = ev.get("seq", 0)
                    if ev_seq and ev_seq <= prior_seq:
                        continue

                    ev_type = ev.get("type")
                    if ev_type == "text":
                        response_chunks.append(ev.get("text", ""))
                    elif ev_type == "info":
                        response_chunks.append(ev.get("text", ""))
                        finished = True
                    elif ev_type == "turn_end":
                        turn_outcome = ev.get("outcome", "done")
                        turn_error = ev.get("error")
                        finished = True

                reader_running[0] = False
                conn.close()

                if not finished and time.time() >= deadline:
                    try:
                        self.post("/cancel", {})
                    except Exception:
                        pass
                    raise TimeoutError(f"Timed out waiting for Oblivion turn after {timeout}s")

                full_text = "".join(response_chunks).strip()
                if not full_text:
                    if turn_outcome == "error":
                        full_text = turn_error or "An error occurred during processing."
                    elif turn_outcome == "cancelled":
                        full_text = "Request was cancelled."
                    else:
                        full_text = "I apologize, but I encountered an error processing your request."

                return {
                    "response": full_text,
                    "outcome": turn_outcome,
                    "error": turn_error,
                }
            except Exception:
                try:
                    self.post("/cancel", {})
                except Exception:
                    pass
                raise

    def chat(self, text, timeout=60):
        return self.send_and_wait(text, timeout=timeout)


ALLOWED_HOSTNAMES = {"localhost", "127.0.0.1", "[::1]"}


def is_valid_host(host_header, allowed_port=None):
    if not host_header:
        return False
    host_str = host_header.strip().lower()
    if host_str.startswith("["):
        bracket_end = host_str.find("]")
        if bracket_end == -1:
            return False
        hostname = host_str[: bracket_end + 1]
        rest = host_str[bracket_end + 1 :]
        if rest.startswith(":"):
            port_str = rest[1:]
        elif rest == "":
            port_str = None
        else:
            return False
    else:
        if ":" in host_str:
            hostname, port_str = host_str.split(":", 1)
        else:
            hostname, port_str = host_str, None

    if hostname not in ALLOWED_HOSTNAMES:
        return False

    if port_str is not None:
        try:
            port = int(port_str)
        except ValueError:
            return False
        if allowed_port is not None and port != allowed_port:
            return False

    return True


def register(app, bridge=None, allowed_port=None, token=None):
    """Add the /api/oblivion/* routes to a Flask app."""
    from flask import Response, jsonify, request, stream_with_context

    bridge = bridge or OblivionBridge()
    app.config["OBLIVION_BRIDGE"] = bridge
    configured_port = allowed_port or app.config.get("JARVIS_PORT")

    token = token or app.config.get("JARVIS_TOKEN") or secrets.token_hex(32)
    app.config["JARVIS_TOKEN"] = token

    def check_security():
        host = request.headers.get("Host") or request.host
        if not is_valid_host(host, allowed_port=configured_port):
            return jsonify({"error": "invalid Host header"}), 403

        origin = request.headers.get("Origin")
        if origin is not None and origin.rstrip("/") != request.host_url.rstrip("/"):
            return jsonify({"error": "cross-site request refused"}), 403

        client_token = (
            request.headers.get("X-Jarvis-Token")
            or request.args.get("t")
            or request.args.get("token")
        )
        if not client_token or not hmac.compare_digest(
            str(client_token), str(token)
        ):
            return jsonify({"error": "unauthorized: invalid or missing token"}), 403

        return None

    def get_bridge():
        return app.config.get("OBLIVION_BRIDGE", bridge)

    def forward(path):
        sec = check_security()
        if sec:
            return sec
        body = request.get_json(silent=True)
        if body is None:
            return jsonify({"error": "expected a JSON body"}), 400
        b = get_bridge()
        code, text = b.post(path, body)
        return Response(text, status=code, mimetype="text/plain")

    @app.get("/api/oblivion/status")
    def oblivion_status():
        sec = check_security()
        if sec:
            return sec
        return jsonify(get_bridge().status())

    @app.get("/api/oblivion/dashboard")
    @app.get("/api/dashboard")
    def oblivion_dashboard():
        sec = check_security()
        if sec:
            return sec
        b = get_bridge()
        if not b.running():
            return jsonify({
                "usage_today": [],
                "tasks": [],
                "recent_files": [],
                "memories_count": 0,
                "memories_by_type": {},
                "sessions_count": 0,
                "engine": {
                    "version": "0.5.0",
                    "status": "offline",
                    "model": "offline",
                    "workspace": b.workdir or "",
                },
            }), 200
        code, text, ct = b.get("/api/system/dashboard")
        return Response(text, status=code, mimetype=ct)

    @app.post("/api/oblivion/start")
    def oblivion_start():
        sec = check_security()
        if sec:
            return sec
        return jsonify(get_bridge().start())

    @app.get("/api/oblivion/events")
    def oblivion_events():
        sec = check_security()
        if sec:
            return sec
        return Response(
            stream_with_context(get_bridge().events()),
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

    @app.route("/api/oblivion/memory", methods=["GET", "POST"])
    @app.route("/api/memory", methods=["GET", "POST"])
    def oblivion_memory():
        sec = check_security()
        if sec:
            return sec
        b = get_bridge()
        if request.method == "GET":
            params = dict(request.args)
            code, text, ct = b.get("/api/memory", params=params)
            return Response(text, status=code, mimetype=ct)
        else:
            body = request.get_json(silent=True)
            if body is None:
                return jsonify({"error": "expected a JSON body"}), 400
            code, text, ct = b.request("POST", "/api/memory", body=body)
            return Response(text, status=code, mimetype=ct)

    @app.route("/api/oblivion/memory/<int:mem_id>", methods=["PUT", "DELETE"])
    @app.route("/api/memory/<int:mem_id>", methods=["PUT", "DELETE"])
    def oblivion_memory_item(mem_id):
        sec = check_security()
        if sec:
            return sec
        b = get_bridge()
        if request.method == "PUT":
            body = request.get_json(silent=True)
            if body is None:
                return jsonify({"error": "expected a JSON body"}), 400
            code, text, ct = b.put(f"/api/memory/{mem_id}", body)
            return Response(text, status=code, mimetype=ct)
        else:
            code, text, ct = b.delete(f"/api/memory/{mem_id}")
            return Response(text, status=code, mimetype=ct)

    @app.route("/api/oblivion/memory/forget", methods=["POST"])
    @app.route("/api/memory/forget", methods=["POST"])
    def oblivion_memory_forget():
        sec = check_security()
        if sec:
            return sec
        b = get_bridge()
        body = request.get_json(silent=True)
        if body is None:
            return jsonify({"error": "expected a JSON body"}), 400
        code, text, ct = b.request("POST", "/api/memory/forget", body=body)
        return Response(text, status=code, mimetype=ct)

    @app.route("/api/oblivion/memory/export", methods=["GET"])
    @app.route("/api/memory/export", methods=["GET"])
    def oblivion_memory_export():
        sec = check_security()
        if sec:
            return sec
        b = get_bridge()
        params = dict(request.args)
        code, text, ct = b.get("/api/memory/export", params=params)
        fmt = request.args.get("format", "json")
        ext = "md" if fmt in ("markdown", "md") else "json"
        return Response(
            text,
            status=code,
            mimetype=ct,
            headers={"Content-Disposition": f'attachment; filename="memories.{ext}"'},
        )

    @app.route("/api/oblivion/sessions", methods=["GET", "POST"])
    @app.route("/api/sessions", methods=["GET", "POST"])
    def oblivion_sessions():
        sec = check_security()
        if sec:
            return sec
        b = get_bridge()
        if request.method == "GET":
            params = dict(request.args)
            code, text, ct = b.get("/api/sessions", params=params)
            return Response(text, status=code, mimetype=ct)
        else:
            body = request.get_json(silent=True) or {}
            code, text, ct = b.request("POST", "/api/sessions", body=body)
            return Response(text, status=code, mimetype=ct)

    @app.route("/api/oblivion/sessions/<session_id>", methods=["GET", "DELETE"])
    @app.route("/api/sessions/<session_id>", methods=["GET", "DELETE"])
    def oblivion_session_item(session_id):
        sec = check_security()
        if sec:
            return sec
        b = get_bridge()
        if request.method == "GET":
            code, text, ct = b.get(f"/api/sessions/{session_id}")
            return Response(text, status=code, mimetype=ct)
        else:
            code, text, ct = b.delete(f"/api/sessions/{session_id}")
            return Response(text, status=code, mimetype=ct)

    @app.route("/api/oblivion/sessions/<session_id>/rename", methods=["POST"])
    @app.route("/api/sessions/<session_id>/rename", methods=["POST"])
    def oblivion_session_rename(session_id):
        sec = check_security()
        if sec:
            return sec
        b = get_bridge()
        body = request.get_json(silent=True)
        if body is None:
            return jsonify({"error": "expected a JSON body"}), 400
        code, text, ct = b.request("POST", f"/api/sessions/{session_id}/rename", body=body)
        return Response(text, status=code, mimetype=ct)

    @app.route("/api/oblivion/sessions/<session_id>/resume", methods=["POST"])
    @app.route("/api/sessions/<session_id>/resume", methods=["POST"])
    def oblivion_session_resume(session_id):
        sec = check_security()
        if sec:
            return sec
        b = get_bridge()
        code, text, ct = b.request("POST", f"/api/sessions/{session_id}/resume", body={})
        return Response(text, status=code, mimetype=ct)

    @app.route("/api/oblivion/sessions/<session_id>/workspace", methods=["POST"])
    @app.route("/api/sessions/<session_id>/workspace", methods=["POST"])
    def oblivion_session_workspace(session_id):
        sec = check_security()
        if sec:
            return sec
        b = get_bridge()
        body = request.get_json(silent=True)
        if body is None:
            return jsonify({"error": "expected a JSON body"}), 400
        code, text, ct = b.request("POST", f"/api/sessions/{session_id}/workspace", body=body)
        return Response(text, status=code, mimetype=ct)

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
