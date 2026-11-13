# Oblivion

A personal AI agent in Rust that reads, writes and edits files and runs commands — like Claude Code or Codex — using **free model APIs** with automatic fallback between providers.

This is **Phase 0** of the Oblivion SRS: the core agent loop, four tools, approvals, undo, sessions and multi-provider fallback. Skills, MCP, the web UI and desktop features come in later phases.

## What works today

- Streaming chat with tool calling against any OpenAI-compatible API (OpenRouter, Groq, NVIDIA, Gemini, Mistral, Cerebras, Ollama…)
- **Fallback**: if a model is rate-limited, down or rejects the request, the next one in `oblivion.toml` takes over, mid-task. Rate-limited models cool down (honouring `Retry-After`).
- Tools: `read_file`, `write_file`, `edit_file` (exact replace, CRLF-safe), `shell` (PowerShell on Windows, bash elsewhere; timeout; output trimmed to head + tail)
- **Approvals**: `ask` (default), `auto-edit` (edits inside the folder are free, commands still ask), `yolo`
- **Undo**: every file change is snapshotted; `/undo` reverts the last request
- Project rules from `OBLIVION.md`, `AGENTS.md` or `CLAUDE.md` in the folder
- Sessions saved to `~/.oblivion/sessions`, resumable with `--resume`
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
4. Get free API keys and set them as environment variables (open a **new** terminal afterwards):
   ```powershell
   setx OPENROUTER_API_KEY "sk-or-..."    # https://openrouter.ai/keys
   setx GROQ_API_KEY "gsk_..."            # https://console.groq.com/keys
   setx NVIDIA_API_KEY "nvapi-..."        # https://build.nvidia.com
   setx GEMINI_API_KEY "..."              # https://aistudio.google.com/apikey
   setx MISTRAL_API_KEY "..."             # https://console.mistral.ai
   ```
   One key is enough to start; more keys mean more free requests per day.
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

Commands inside the chat: `/undo`, `/models`, `/model <text>`, `/approval <mode>`, `/clear`, `/session`, `/help`, `/exit`.

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

## Layout

| Crate | What it does |
| --- | --- |
| `oblivion-providers` | OpenAI-compatible streaming client, tool-call assembly, fallback router |
| `oblivion-tools` | read/write/edit/shell tools, risk levels, snapshots and undo |
| `oblivion-core` | Config, system prompt, sessions, the agent loop |
| `oblivion-cli` | The `oblivion` binary |

Run the tests with `cargo test` (includes an end-to-end test against a fake provider).

## Safety notes

- Default mode asks before every file change and command. Read what it wants to run.
- `--yes` / `yolo` never asks — only use it in a throwaway folder.
- API keys are read from environment variables only and are never written to sessions or logs.
- Gemini's free tier may use prompts for training; disable it in `oblivion.toml` when working on private code.
