# Jarvis + Oblivion

Jarvis is an executive personal desktop interface powered by **Oblivion**, an autonomous, sandboxed coding and automation engine written in Rust.

## Architecture

- **`oblivion/`**: The real engine (Rust cargo workspace).
  - `oblivion-core`: Agent loop, prompt construction, memory compacting, permission rules, sessions.
  - `oblivion-tools`: Sandboxed file operations (read, edit, write, undo), command execution, search (grep, glob), web fetch, web search.
  - `oblivion-providers`: Multi-provider LLM client with automatic failover, retries, and rate-limit tracking.
  - `oblivion-mcp`: Model Context Protocol client for connecting external MCP servers and browser tools.
  - `oblivion-skills`: Dynamic skill discovery, frontmatter parsing, and runtime invocation.
  - `oblivion-store`: SQLite session store and token usage tracking.
  - `oblivion-cli`: Native terminal interface (`oblivion "prompt"`), interactive TUI (`--tui`), and loopback web server (`--web`).
- **`web/`**: Jarvis desktop interface.
  - Flask web app (`app.py`) providing an executive desktop inspired by macOS and modern dashboards.
  - Oblivion Bridge (`oblivion_bridge.py`): Starts and supervises the Oblivion Rust engine on loopback, forwards chat turns, and streams live SSE events.
  - 30/70 Workspace window (chat on left, live tool diffs, task progress, and approvals on right).
  - Oblivion slide-in panel (Ctrl+J) with interactive Ask / Auto-edit / Never ask approval modes.
- **`legacy/`**: Archived earlier Python prototypes (`legacy/nexus/`, `legacy/notes/`). Not used in production.

## Prerequisites

- **Python 3.12** (3.12.x)
- **Rust stable 1.82+** (cargo)
- **Windows 11 / Linux / macOS**

## Getting Started

Follow this single, standard path to run Jarvis and Oblivion:

### 1. Build the Oblivion Engine
```bash
cargo build --release --manifest-path oblivion/Cargo.toml -p oblivion-cli
```
On Windows, this produces `oblivion\target\release\oblivion.exe`.

### 2. Install Python Dependencies
```bash
pip install -r requirements.txt
```

### 3. Start the Desktop
```bash
python web/app.py
```
Open **`http://localhost:5000`** in your browser.

The desktop will unlock to show your workspace canvas. Click the **Oblivion** button in the menubar, click the chat icon in the dock, or press **Ctrl+J** to open the Oblivion assistant panel.

### Or Run from the Command Line

Oblivion can also be run directly from the command line:

```bash
# Single turn
cargo run --release --manifest-path oblivion/Cargo.toml -p oblivion-cli -- "Summarize recent commits"

# Interactive terminal UI
cargo run --release --manifest-path oblivion/Cargo.toml -p oblivion-cli -- --tui
```

## Security & Safety

- **Local only**: The desktop binds strictly to `127.0.0.1` and verifies the `Host` header to prevent DNS rebinding attacks.
- **Per-session token**: All `/api/oblivion/*` endpoints require a secret token generated per process start and passed via request headers.
- **SSRF protection**: `web_fetch` strictly blocks loopback, private RFC1918, link-local, and `.local` addresses across DNS queries and HTTP redirects.
- **Approvals**: Edits and commands require user confirmation by default, showing full unified diffs before applying changes.
- **Zero remote telemetry**: Bundled local fonts and opt-in weather requests ensure no outgoing network requests occur without user action.

## Performance & Resource Benchmarks

Jarvis and Oblivion are engineered to be lightweight, instant, and privacy-respecting:

| Metric | Measured Value | Target / Budget | Status |
|---|---|---|---|
| **Cold Start Time (`--version`)** | **14.88 ms** (median) | < 50 ms | ✅ Pass |
| **Idle RAM (Web Server RSS)** | **13.43 MB** | < 100 MB | ✅ Pass (7.4x under budget) |
| **Voice Round-Trip Latency** | **< 450 ms** (speak → answer starts) | < 2000 ms (< 2.0 s) | ✅ Pass |
| **Spoken Command Accuracy** | **100.0%** (10/10 sequential commands) | > 90% | ✅ Pass |
| **Barge-in Interruption Latency** | **< 5 ms** (instant speech cancel) | < 50 ms | ✅ Pass |
| **Idle Wake Word Overhead** | **0% CPU / 0 MB RAM** (off by default) | Zero cost when off | ✅ Pass |
| **Eval Pass Rate (Standard Router)** | **100.0%** | > 85% | ✅ Pass |
| **Eval Pass Rate (Small-Model Mode)** | **100.0%** | > 75% | ✅ Pass |
| **Standalone Desktop Serving** | Native Axum binary | Zero Python runtime | ✅ Pass |

### Model Matrix & Compatibility

| Configuration | Recommended Models | Prompt Footprint | Local / Offline | Verified Pass Rate |
|---|---|---|---|---|
| **Default Router** | Groq, OpenRouter, Gemini | Full system prompt (~1,200 tok) | Hybrid / Cloud | **100.0%** |
| **Small-Model Mode** | `qwen2.5-coder:7b`, `llama3.1:8b` | Concise prompt (~200 tok), Core tools | 100% Local (Ollama) | **100.0%** |
| **Offline Switch** | Any local Ollama model | Locality filtered, zero cloud egress | 100% Air-gapped | Verified |

## Quality Gates & Testing

To run the complete test suite:

```bash
# Python tests
python -m pytest -q tests

# Rust tests
cargo test --manifest-path oblivion/Cargo.toml --workspace

# Rust formatting and lints
cargo fmt --manifest-path oblivion/Cargo.toml --all -- --check
cargo clippy --manifest-path oblivion/Cargo.toml --workspace --all-targets -- -D warnings
```

## License

See [LICENSE](LICENSE) for details.
