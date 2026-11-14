# Working on Oblivion

Rust cargo workspace. Read the SRS (Oblivion project doc) before adding features; build phases in order.

## Commands

- Build: `cargo build` (release: `cargo build --release`)
- Test: `cargo test` — must pass before finishing any change
- Lint: `cargo clippy --all-targets` — keep at zero warnings
- Format: `cargo fmt`

## Layout

- `crates/oblivion-providers` — model client + router. No knowledge of tools or the CLI.
- `crates/oblivion-tools` — tools. Each tool: a spec, a risk level, an async implementation, unit tests.
- `crates/oblivion-core` — agent loop, config, prompt, sessions, sub-agents (`subagent.rs`: read-only tool list, parallel `task` calls). `tests/e2e.rs` drives the loop against a fake server.
- `crates/oblivion-skills` — SKILL.md support. Bundled skills live in `bundled/` and are embedded by `src/bundled.rs` (regenerate that list when adding files).
- `crates/oblivion-mcp` — MCP client; `tests/fixtures/echo_server.py` is a stdlib-only MCP server for tests.
- `crates/oblivion-store` — SQLite (sessions, daily usage ledger). Implements core's `SessionStore` and providers' `UsageLedger`.
- `crates/oblivion-cli` — terminal UI, key storage (`keys.rs`); no business logic.
- `evals/` — 10 end-to-end tasks with hidden tests; `python evals/run_evals.py`. Run before merging prompt or tool changes.

## Rules

- No blocking calls inside async code except via `tokio::task::block_in_place` (approval prompts).
- Every file-changing tool must call `journal().snapshot()` before writing.
- Never log or persist API keys.
- Keep dependencies small; the idle RAM target is under 50 MB.
- New provider behaviour gets a unit test in `client.rs` using recorded SSE lines.
- Extra tool sets plug in through `oblivion_tools::ToolProvider`; keep their prompt cost small (free tiers allow ~7K tokens/request on Groq).
