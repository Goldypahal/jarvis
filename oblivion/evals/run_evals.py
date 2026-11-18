"""Oblivion eval runner (SRS section 9).

Each task in evals/tasks/<id>/ has:
  prompt.txt   what the agent is asked to do
  workspace/   the starting project, copied to a temp folder
  hidden/      tests copied in AFTER the agent finishes (it never sees them)

A task passes when `pytest hidden/test_hidden.py` passes in the agent's folder.

Usage (from the oblivion folder):
  python evals/run_evals.py                      # all tasks, release binary
  python evals/run_evals.py --model groq         # only Groq models
  python evals/run_evals.py --only 01 07         # tasks whose id starts with 01 or 07
"""

import argparse
import datetime as dt
import http.server
import json
import os
import pathlib
import re
import shutil
import subprocess
import sys
import tempfile
import threading
import time

HERE = pathlib.Path(__file__).resolve().parent
EXE = "oblivion.exe" if os.name == "nt" else "oblivion"
ANSI = re.compile(r"\x1b\[[0-9;]*m")


def start_local_sites_server(port: int = 8765):
    sites_dir = HERE / "sites"
    if not sites_dir.is_dir():
        return None
    class QuietHandler(http.server.SimpleHTTPRequestHandler):
        def __init__(self, *args, **kwargs):
            super().__init__(*args, directory=str(sites_dir), **kwargs)
        def log_message(self, format, *args):
            pass
    try:
        server = http.server.ThreadingHTTPServer(("127.0.0.1", port), QuietHandler)
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        return server
    except Exception as e:
        print(f"Note: local test site server not started on port {port}: {e}")
        return None


def run_task(task: pathlib.Path, binary: str, model: str | None, timeout: int, logs: pathlib.Path) -> dict:
    work = pathlib.Path(tempfile.mkdtemp(prefix=f"obl-eval-{task.name}-"))
    shutil.copytree(task / "workspace", work, dirs_exist_ok=True)
    prompt = (task / "prompt.txt").read_text(encoding="utf-8").strip()
    flags_file = task / "flags.txt"
    if flags_file.exists():
        flags = flags_file.read_text(encoding="utf-8").strip().split()
    else:
        flags = ["--yes"]
    cmd = [binary, "-C", str(work)] + flags
    if model:
        cmd += ["--model", model]
    cmd.append(prompt)

    start = time.time()
    try:
        agent = subprocess.run(cmd, capture_output=True, text=True, encoding="utf-8", errors="replace", timeout=timeout)
        agent_out, agent_code, timed_out = agent.stdout + agent.stderr, agent.returncode, False
    except subprocess.TimeoutExpired as e:
        agent_out = (e.stdout or "") if isinstance(e.stdout, str) else ""
        agent_code, timed_out = None, True
    seconds = round(time.time() - start, 1)
    agent_out = ANSI.sub("", agent_out)

    # Hidden tests go in only now, overwriting anything the agent wrote with the same name.
    shutil.copytree(task / "hidden", work, dirs_exist_ok=True)
    check = subprocess.run(
        [sys.executable, "-m", "pytest", "-q", "-p", "no:cacheprovider", "test_hidden.py"],
        cwd=work, capture_output=True, text=True, encoding="utf-8", errors="replace",
    )
    passed = check.returncode == 0 and not timed_out

    log = logs / f"{task.name}.log"
    log.write_text(
        f"$ {' '.join(cmd)}\n\n--- agent (exit {agent_code}, {seconds}s{', TIMED OUT' if timed_out else ''}) ---\n"
        f"{agent_out}\n--- hidden tests ---\n{check.stdout}{check.stderr}",
        encoding="utf-8",
    )
    shutil.rmtree(work, ignore_errors=True)
    steps = next((ln for ln in agent_out.splitlines() if "step(s)" in ln), "").strip()
    return {"task": task.name, "passed": passed, "seconds": seconds, "timed_out": timed_out, "summary": steps}


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--oblivion", default=str(HERE.parent / "target" / "release" / EXE))
    ap.add_argument("--model", help="only use models whose label contains this text")
    ap.add_argument("--only", nargs="*", default=[], help="task id prefixes to run")
    ap.add_argument("--timeout", type=int, default=300, help="seconds per task")
    a = ap.parse_args()

    if not pathlib.Path(a.oblivion).exists():
        print(f"Oblivion binary not found at {a.oblivion}. Run `cargo build --release` first.")
        return 2
    tasks = sorted(p for p in (HERE / "tasks").iterdir() if p.is_dir())
    if a.only:
        tasks = [t for t in tasks if any(t.name.startswith(o) for o in a.only)]

    stamp = dt.datetime.now().strftime("%Y%m%d-%H%M%S")
    out_dir = HERE / "results" / stamp
    out_dir.mkdir(parents=True, exist_ok=True)

    server = start_local_sites_server()
    results = []
    try:
        for t in tasks:
            print(f"  {t.name:22} ", end="", flush=True)
            r = run_task(t, a.oblivion, a.model, a.timeout, out_dir)
            results.append(r)
            status = "PASS" if r["passed"] else ("TIMEOUT" if r["timed_out"] else "FAIL")
            print(f"{status:8} {r['seconds']:>6}s  {r['summary']}")
    finally:
        if server:
            server.shutdown()

    passed = sum(r["passed"] for r in results)
    print(f"\n{passed}/{len(results)} passed  ·  logs: {out_dir}")
    (out_dir / "results.json").write_text(
        json.dumps({"when": stamp, "model": a.model, "passed": passed, "total": len(results), "tasks": results}, indent=2),
        encoding="utf-8",
    )
    return 0 if passed == len(results) else 1


if __name__ == "__main__":
    sys.exit(main())
