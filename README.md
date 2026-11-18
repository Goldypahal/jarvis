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
