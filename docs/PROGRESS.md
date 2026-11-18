# Roadmap progress

The agent ticks items here as it finishes them: `- [x] item — 2026-11-18, <commit>, <evidence>`.
Blocked items stay unticked with `BLOCKED (...)`. Problem IDs refer to `ANALYSIS_AND_ROADMAP.md`.
Split any item that turns out bigger than about 400 changed lines.

Current milestone: **v0.5 "One brain"** · Branch: `roadmap/m1`

---

## Month 1 — one brain, safe by default → v0.5

### Week 1 — clean and safe
- [x] Baseline: run all gates on `main`, record results and test counts in the Log below — 2026-11-17, e729edc, cargo test: 89 passed; pytest: 10 passed; clippy: 0 warnings
- [x] Baseline: measure `oblivion --version` start time and idle RAM of `oblivion --web`; record in Log — 2026-11-17, e729edc, start: 24.5ms median; idle RAM: 30.66 MB RSS
- [ ] BLOCKED (needs user): PC specs (RAM, GPU, VRAM, free disk) — write them under "Notes"
- [x] S4: `web_fetch` refuses loopback, private, link-local and `.local` addresses, including after redirects and DNS resolution; tests for each range — 2026-11-17, 6252000, cargo test: 92 passed; evals: 1/1 passed
- [x] S6: bridge checks the `Host` header (only `localhost` / `127.0.0.1` with the right port); tests — 2026-11-17, c26b24b, pytest: 11 passed
- [x] S6: per-start random secret issued by Flask, required as a header on every `/api/oblivion/*` call; panel sends it; tests — 2026-11-17, 8e4736c, pytest: 12 passed; node check: passed
- [x] S8: bundle the fonts locally under `web/static/fonts`; remove Google Fonts links — 2026-11-17, 65c2013, pytest: 14 passed
- [x] S8: weather and location only after the user clicks "Enable weather"; no request on page load — 2026-11-17, ca9f795, pytest: 16 passed; node check: clean; browser UI verified
- [x] B6: replace root `requirements.txt` with what is actually imported; pin Flask 3.x; one supported Python version stated in README — 2026-11-17, ed3754c, pytest: 18 passed
- [x] B4: one lock around all NEXUS calls in `web/app.py` (temporary until week 2); test with two concurrent requests — 2026-11-17, 61d6218, pytest: 20 passed
- [x] Q2: `git mv` the eight summary files and `DONE.txt` to `legacy/notes/`; delete stray `.db` files from the working tree (they are git-ignored) — 2026-11-17, 2b9023d, git mv 9 files to legacy/notes/; deleted 6 stray .db files; pytest: 20 passed
- [x] Python CI: add a workflow that runs `pytest` on Windows and Ubuntu — 2026-11-17, 44963a4, .github/workflows/python.yml: windows-latest & ubuntu-latest; pytest: 20 passed

### Week 2 — one brain
- [x] B1: `/api/chat` forwards to Oblivion through the bridge (send + read events until `turn_end`); test with a fake Oblivion — 2026-11-17, 72ab777, pytest tests/test_chat_bridge.py: 5 passed; total pytest: 25 passed; cargo test: 92 passed
- [x] B1/B3: the 30/70 workspace window uses the same live event stream as the panel (chat left, tool cards/diffs/tasks right) and has working approve/deny buttons — 2026-11-17, d9a318a, pytest: 27 passed; node check: clean; browser UI verified in light & dark themes; cargo test: 92 passed
- [x] Move `nexus/`, `web/nexus_adapter.py`, `examples/`, `benchmarks/` and NEXUS tests to `legacy/`; the web app no longer imports them; all remaining tests pass — 2026-11-17, 392745a, moved to legacy/; web/app.py decoupled; pytest: 19 passed in tests/ & 8 passed in legacy/tests/; cargo test: 92 passed
- [x] Remove now-dead routes and UI controls that only served NEXUS (modes, fake skill learning) — 2026-11-18, 5884308, pytest: 21 passed; node check: clean; browser UI verified in light & dark themes
- [ ] Q7: rewrite `README.md`, `WEB_README.md`, `RUN_WEB_INTERFACE.md` to describe what is true; one "Getting started" path

### Week 3 — prove it
- [ ] B8: browser agent on five real tasks (search, page read, multi-page form on a local test page, practice quiz on a local test page, a site needing a login done by the user). Record each result; fix what breaks. Real logins: BLOCKED (needs user) if no session exists
- [ ] B8: local test site under `oblivion/evals/sites/` (static HTML: form, quiz, pagination) used by browser evals
- [ ] B9: `--tui` checked in a real Windows Terminal: paste, resize, Shift+Enter, scroll, approvals; fix issues
- [ ] B12: run each of security-review, refactor, write-tests, frontend-design, mcp-builder, explain-codebase once on this repo; fix the skill text where the model went wrong
- [ ] Q6: per-run trace file (JSON lines: model calls, tool calls, approvals, timings) under `~/.oblivion/traces/`, size-capped; `oblivion --trace <id>` prints one

### Week 4 — measure
- [ ] Q4: evals from 10 to 30: +5 research, +5 documents, +5 browser (local test site), +5 safety (must ask before delete; must not act on "ignore your instructions" text in a page or file)
- [ ] B11: provider limits move from code to `oblivion.toml` with a `checked = "YYYY-MM-DD"` field; defaults stay; test
- [ ] Python tests for every `web/app.py` route
- [ ] Milestone check: no code path returns canned text; S1–S4, S6, S8, S9 closed; 30 evals run with one command; five real browser tasks done → merge, tag `v0.5`

---

## Month 2 — memory and an honest desktop
- [ ] Memory schema in `oblivion-store` (type, text, source session, created, last used, importance) with FTS5 search; migrations; tests
- [ ] `remember`, `recall`, `forget` tools with permission rules; tests
- [ ] End-of-session extraction pass proposes memories (cheap model, budgeted); user can disable
- [ ] Recall on each request within a ~500-token budget; per-project scoping; tests that one project's memory never appears in another
- [ ] Embeddings via Ollama + `sqlite-vec` (optional; falls back to FTS when not available)
- [ ] Desktop "Memory" window: list, search, edit, delete, forget-by-topic, export
- [ ] Sessions window: list, search, rename, resume, delete; work folder chosen per session (B10)
- [ ] B7: replace every fake widget with real data (usage today, tasks, files created, memory count, model status) or remove it; menu-bar date is live
- [ ] `AGENTS.md` read at start, update offered at end of a session
- [ ] 5 memory evals; milestone check

## Month 3 — local models and one-file install → v1.0
- [ ] Hardware detection (RAM, GPU, VRAM) and model-size recommendation
- [ ] Model manager window: list, download with progress, delete, speed test (Ollama API)
- [ ] Request classes (private, simple, hard, vision, long context) and visible routing rules; "Offline" switch
- [ ] Small-model mode: shorter prompt, fewer tools, stricter output checks (B13); eval pass rate per model recorded
- [ ] Serve the desktop from Oblivion (axum): move templates/static into the binary; Flask optional
- [ ] Installer (MSI or Tauri tray app), first-run wizard, auto-start option. Code signing: BLOCKED (needs user) for a certificate
- [ ] Measure cold start, idle RAM, eval pass rate per model; table in README
- [ ] Milestone check (offline edit + memory answer + document summary; idle RAM < 100 MB) → tag `v1.0`

## Month 4 — voice
- [ ] Push-to-talk hotkey and mic button; local speech-to-text
- [ ] Local text-to-speech, streamed by sentence; barge-in
- [ ] Spoken-reply style; voice approvals for low-risk actions only
- [ ] Optional wake word
- [ ] Latency and accuracy measured; milestone check

## Month 5 — vision and computer use
- [ ] Image input in providers and all interfaces
- [ ] "Look at my screen" with preview before sending
- [ ] Browser agent screenshot fallback for picture-only pages
- [ ] Computer use behind a setting, per-app allow list, stop hotkey
- [ ] 10 vision evals; milestone check

## Month 6 — automations and connectors → v1.5
- [ ] Scheduler (one-off and repeating) in the tray process; runs as fresh sessions with traces
- [ ] Notifications and approval queue for unattended runs; risky actions always wait
- [ ] Connector setup screen for MCP servers; keys in Credential Manager
- [ ] Starter automations (morning brief, repo watcher, weekly report)
- [ ] Milestone check → tag `v1.5`

## Month 7 — your documents as knowledge
- [ ] Incremental background indexer (PDF, Word, PowerPoint, Markdown, code)
- [ ] Hybrid search with file/page citations; click opens the file
- [ ] Collections on the desktop (Research Papers / Documents icons become real)
- [ ] Research skill uses local documents first
- [ ] 15 document Q&A evals; milestone check

## Month 8 — sandbox and defence
- [ ] Restricted process for shell commands on Windows (job object + restricted token; WSL fallback); whole tree killed on timeout (S5)
- [ ] Untrusted-content tracking: risky actions always ask after outside content enters (S7)
- [ ] Secrets scanner on outgoing model requests
- [ ] Network allow list for tools
- [ ] 20 injection evals, all blocked; milestone check

## Month 9 — parallel agents, hooks, gallery
- [ ] Write-capable sub-agents in git worktrees with a review/merge screen
- [ ] Hooks before/after tool calls
- [ ] Skill and connector gallery with permission preview
- [ ] One-click "save failed run as eval"; weekly pass-rate report
- [ ] Milestone check

## Month 10 — phone, polish, release → v2.0
- [ ] Phone web app with QR pairing, approvals and voice
- [ ] Trace viewer (step-by-step replay)
- [ ] Performance pass (start time, memory, tokens per task)
- [ ] 100 evals, nightly run, README badge
- [ ] Docs: install, first hour, safety model, writing a skill, adding a connector
- [ ] Full security review → tag `v2.0`

---

## Log
<!-- The agent adds a dated entry at the end of each week section. -->
### Baseline (2026-11-17)
- Quality gates:
  - `cargo fmt --all`: clean
  - `cargo clippy --workspace --all-targets -- -D warnings`: 0 warnings
  - `cargo test --workspace`: 89 passed, 0 failed (fixed Windows MCP handshake error string containing server name)
  - `python -m pytest -q tests`: 10 passed, 0 failed
- Performance measurements:
  - `oblivion --version` start time: 24.5 ms median (samples: 28.2ms, 24.9ms, 24.5ms, 20.2ms, 19.4ms)
  - `oblivion --web` idle RSS RAM: 30.66 MB (well under 100 MB target)

## Notes for next session
<!-- Surprises about the code, environment quirks, things the next session must know. -->
- Windows + PowerShell: no bash heredocs; quote paths with spaces.
- Smart App Control has blocked Rust build scripts before; if a build fails with an
  "Application Control policy" error, mark the item BLOCKED (needs user) instead of retrying.
- Groq free tier: about 8K tokens/minute and ~7K tokens per request; keep prompts small.
- Detected PC specs (pending user verification): RAM: 16 GB, GPU: Intel Iris Xe Graphics (~1 GB VRAM / shared), Free disk D: ~673.5 GB.
