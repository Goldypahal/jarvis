# Oblivion

A personal AI agent in Rust that reads, writes and edits files and runs commands — like Claude Code or Codex — using **free model APIs** with automatic fallback between providers.

Status: **Phase 2** of the Oblivion SRS. Done: agent loop, 9 built-in tools, approvals and permission rules, undo, SQLite sessions and quota tracking, compaction, Windows Credential Manager keys, multi-provider fallback with rate-limit waits, a 10-task eval suite (10/10 on Groq's free tier), **skills** (Claude/Codex `SKILL.md` format, 10 bundled) and **MCP connectors**. Next: full-screen terminal UI, then the knowledge-work features (Phase 3).

## What works today

- Streaming chat with tool calling against any OpenAI-compatible API (OpenRouter, Groq, NVIDIA, Gemini, Mistral, Cerebras, Ollama…)
- **Fallback**: if a model is rate-limited, down or rejects the request, the next one in `oblivion.toml` takes over, mid-task. Rate-limited models cool down (honouring `Retry-After`).
- Tools: `read_file`, `write_file`, `edit_file`, `glob`, `grep`, `shell`, `web_search`, `web_fetch`, `todo`
  - `edit_file` forgives small copy mistakes (trailing spaces, indentation, CRLF, line-number prefixes) but only edits when exactly one place matches
  - `glob`/`grep` respect `.gitignore`; `web_search` needs no key (DuckDuckGo) or can use Tavily / SearXNG
- **Free-tier aware**: each model can have `requests_per_day` and `max_input_tokens`; requests too big for a model (Groq allows ~8K tokens/min) go to the next one, and daily allowances are tracked in `~/.oblivion/oblivion.db`
- **Compaction**: long conversations are shrunk automatically (old tool output trimmed, then older turns summarized); `/compact` forces it
- **Permission rules** in `oblivion.toml`: `allow = ["shell(cargo test*)"]` skips the prompt, `deny = ["shell(*git push --force*)"]` refuses outright
- **Approvals**: `ask` (default), `auto-edit` (edits inside the folder are free, commands still ask), `yolo`
- **Undo**: every file change is snapshotted; `/undo` reverts the last request
- Project rules from `OBLIVION.md`, `AGENTS.md` or `CLAUDE.md` in the folder
- Sessions saved in SQLite (`~/.oblivion/oblivion.db`), resumable with `--resume`; `--usage` shows today's requests and tokens per model
- Ctrl+C cancels the current request; at the prompt it quits
- ~3 MB release binary, ~5 MB RAM idle

## Install (Windows)

1. Install Rust from <https://rustup.rs> (accept the defaults; it installs the MSVC build tools if needed).
2. Build:
   ```powershell
   cd jarvis\oblivion
   cargo build --release
   # binary: target\release\oblivion.exe — copy it somewhere on your PATH
   ```
3. Create the config:
   ```powershell
   .\target\release\oblivion.exe --init
   ```
   This writes `%USERPROFILE%\.oblivion\oblivion.toml`.
4. Get free API keys and save them in Windows Credential Manager (you paste the key; it is not shown):
   ```powershell
   oblivion --set-key groq         # https://console.groq.com/keys
   oblivion --set-key openrouter   # https://openrouter.ai/keys
   oblivion --set-key nvidia       # https://build.nvidia.com
   oblivion --set-key gemini       # https://aistudio.google.com/apikey
   oblivion --set-key mistral      # https://console.mistral.ai
   ```
   One key is enough to start; more keys mean more free requests per day. Environment variables
   such as `GROQ_API_KEY` still work and take priority over saved keys.
5. Check what will be used:
   ```powershell
   oblivion --models
   ```
   Free model IDs change often. If a model fails with "not found", open the provider's model list and fix the ID in `oblivion.toml`.

## Use

```powershell
cd C:\path\to\some\project
oblivion                                  # interactive chat in this folder
oblivion "fix the failing test in tests/" # one task, then exit
oblivion -m groq "explain src/main.rs"    # only use Groq models
oblivion --approval auto-edit             # don't ask for file edits
oblivion --sessions                       # list saved sessions
oblivion --resume 20261113-201500         # continue one
```

Commands inside the chat: `/undo`, `/models`, `/model <text>`, `/todo`, `/compact`, `/skills`, `/<skill-name> [request]`, `/mcp`, `/approval <mode>`, `/clear`, `/session`, `/help`, `/exit`.

Use Windows Terminal (not the old console window) so colours display correctly.

## Phase 0 acceptance test

From the SRS: *on free models, fix a seeded bug in a small repo and the tests pass.*

```powershell
mkdir $env:TEMP\obl-test; cd $env:TEMP\obl-test
@"
def add(a, b):
    return a - b
"@ | Set-Content calc.py
@"
from calc import add
def test_add():
    assert add(2, 3) == 5
"@ | Set-Content test_calc.py
pip install pytest
oblivion --approval auto-edit "run the tests with pytest, find the bug and fix it, then run the tests again"
```

Pass = the agent edits `calc.py` and pytest reports 1 passed. Try it with two different providers (`-m openrouter`, `-m groq`).

## Skills

Skills are folders with a `SKILL.md` (the open format used by Claude Code and Codex). Only each skill's
name and description go in the prompt; the model loads the full instructions with the `skill` tool when
a request matches, so many skills cost few tokens.

- **Bundled (10):** init-project, plan, code-review, fix-tests, debug, git-workflow, docx, xlsx, pdf, skill-creator.
  The document skills use Python (`pip install python-docx openpyxl pypdf`) and include checker scripts.
- **Your own / installed:** `~/.oblivion/skills/`, plus `~/.agents/skills`, `~/.claude/skills`, `~/.codex/skills`,
  and a project's `.agents/skills` or `.claude/skills`. Later ones win when names clash.
- **Install from GitHub:** `oblivion --install-skill https://github.com/anthropics/skills/tree/main/skills/mcp-builder`
  (needs git; prints the skill's licence).
- **List:** `oblivion --skills` or `/skills`. **Run directly:** `/code-review the last commit`.

## MCP connectors

Add servers to `oblivion.toml`; they connect on first use. The model sees one compact `mcp` tool
(list a server's tools, then call one) instead of every schema, which keeps requests small.

```toml
[[mcp_servers]]
name = "filesystem"
description = "Files in my Documents folder"
command = "npx"
args = ["-y", "@modelcontextprotocol/server-filesystem", "C:\\Users\\me\\Documents"]

[[mcp_servers]]
name = "github"
description = "GitHub issues, pull requests and repos"
url = "https://api.githubcopilot.com/mcp/"
token_env = "GITHUB_TOKEN"        # or: oblivion --set-key mcp-github
```

`oblivion --mcp` (or `/mcp`) connects to each server and lists its tools. MCP calls ask for approval
unless a rule allows them, e.g. `allow = ["mcp(filesystem.read_*)", "mcp(github.list_*)"]`.

## Eval suite (Phase 1 gate)

`evals/tasks/` holds 10 small coding tasks (bug fixes, a stub to implement, a cross-file fix, a CLI flag,
a rename across files, error handling, writing tests, a CSV-to-JSON script). Each is checked by hidden
tests the agent never sees. Phase 1 passes when all 10 pass on free models.

```powershell
cd D:\Jarvis\oblivion
cargo build --release
python evals\run_evals.py                 # all tasks, all configured models
python evals\run_evals.py --model groq    # only Groq
python evals\run_evals.py --only 07       # one task
```

Results and full logs go to `evals\results\<time>\`. The runner uses `--yes` in throwaway temp folders.

## Layout

| Crate | What it does |
| --- | --- |
| `oblivion-providers` | OpenAI-compatible streaming client, tool-call assembly, fallback router |
| `oblivion-tools` | file, search, shell, web and todo tools; risk levels; snapshots and undo |
| `oblivion-core` | Config, system prompt, permission rules, compaction, sessions, the agent loop |
| `oblivion-store` | SQLite: sessions and per-day model usage |
| `oblivion-skills` | SKILL.md discovery, the `skill` tool, GitHub installer, 10 bundled skills |
| `oblivion-mcp` | MCP client (stdio + streamable HTTP) behind one `mcp` tool |
| `oblivion-cli` | The `oblivion` binary |

Run the tests with `cargo test` (includes an end-to-end test against a fake provider).

## Safety notes

- Default mode asks before every file change and command. Read what it wants to run.
- `--yes` / `yolo` never asks — only use it in a throwaway folder.
- API keys live in Windows Credential Manager (or environment variables) and are never written to sessions, logs or the database.
- Gemini's free tier may use prompts for training; disable it in `oblivion.toml` when working on private code.
