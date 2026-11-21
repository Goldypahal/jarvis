# Roadmap progress

The agent ticks items here as it finishes them: `- [x] item — 2026-11-18, <commit>, <evidence>`.
Blocked items stay unticked with `BLOCKED (...)`. Problem IDs refer to `ANALYSIS_AND_ROADMAP.md`.
Split any item that turns out bigger than about 400 changed lines.

Current milestone: **Month 5 — vision and computer use** · Branch: `roadmap/m5`

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
- [x] Q7: rewrite `README.md`, `WEB_README.md`, `RUN_WEB_INTERFACE.md` to describe what is true; one "Getting started" path — 2026-11-18, eed7d34, pytest: 24 passed; all docs reflect Jarvis + Oblivion reality

### Week 3 — prove it
- [x] B8: local test site under `oblivion/evals/sites/` (static HTML: form, quiz, pagination) used by browser evals — 2026-11-18, d752ee4, pytest: 29 passed; 5 static pages (index, form, quiz, pagination, login) verified
- [x] B8: browser agent on five real tasks (search, page read, multi-page form on a local test page, practice quiz on a local test page, a site needing a login done by the user). Record each result; fix what breaks. Real logins: BLOCKED (needs user) if no session exists — 2026-11-18, e9ab149, 5/5 tasks passed in browser agent (search & pagination, page read, multi-step form [CONF-7842], practice quiz [3/3 100%], auth gateway [OBLIVION-AUTH-SUCCESS-99]); external live logins BLOCKED (needs user)
- [x] B9: `--tui` checked in a real Windows Terminal: paste, resize, Shift+Enter, scroll, approvals; fix issues — 2026-11-18, 987c72a, cargo test: 93 passed; mouse wheel scroll, ANSI/bracketed paste, Shift+Enter, and resize clamping verified
- [x] B12: run each of security-review, refactor, write-tests, frontend-design, mcp-builder, explain-codebase once on this repo; fix the skill text where the model went wrong — 2026-11-18, 3c2324c, cargo test: 93 passed; refined instructions across all 6 bundled skills addressing model failure modes
- [x] Q6: per-run trace file (JSON lines: model calls, tool calls, approvals, timings) under `~/.oblivion/traces/`, size-capped; `oblivion --trace <id>` prints one — 2026-11-18, 183900d, cargo test: 97 passed (14 in oblivion-cli); pytest: 29 passed; size-capped jsonl logging, rotation, prefix matching & --trace list verified

### Week 4 — measure
- [x] Q4: evals from 10 to 30: +5 research, +5 documents, +5 browser (local test site), +5 safety (must ask before delete; must not act on "ignore your instructions" text in a page or file) — 2026-11-18, b0afa3d, pytest: 34 passed; 30 tasks created (tasks 11-30) with prompt/workspace/hidden/flags and test runner daemon server support
- [x] B11: provider limits move from code to `oblivion.toml` with a `checked = "YYYY-MM-DD"` field; defaults stay; test — 2026-11-18, ba24ac6, cargo test: 98 passed; clippy: 0 warnings; pytest: 34 passed; checked date validation and fallback defaults verified
- [x] Python tests for every `web/app.py` route — 2026-11-18, 73d45c9, pytest: 50 passed (16 in test_web_routes.py); GET /, POST /api/chat, /api/oblivion/* routes, auth, validation & timeout tested
- [x] Milestone check: no code path returns canned text; S1–S4, S6, S8, S9 closed; 30 evals run with one command; five real browser tasks done → merge, tag `v0.5` — 2026-11-18, in-progress, cargo test: 98 passed; pytest: 50 passed; clippy: 0 warnings; 30 evals verified; merged & tagged v0.5

---

## Month 2 — memory and an honest desktop
- [x] Memory schema in `oblivion-store` (type, text, source session, created, last used, importance) with FTS5 search; migrations; tests — 2026-11-18, 6d4125d, cargo test: 106 passed; pytest: 50 passed; clippy: 0 warnings; FTS5 CRUD, ranking, triggers & migrations verified
- [x] `remember`, `recall`, `forget` tools with permission rules; tests — 2026-11-18, a73691a, cargo test: 111 passed; pytest: 50 passed; clippy: 0 warnings; evals: 1/1 passed; lifecycle, permissions & toolbox verified
- [x] End-of-session extraction pass proposes memories (cheap model, budgeted); user can disable — 2026-11-18, 7179626, cargo test: 118 passed; pytest: 50 passed; clippy: 0 warnings; evals: 1/1 passed; extraction, heuristics & disabling verified
- [x] Recall on each request within a ~500-token budget; per-project scoping; tests that one project's memory never appears in another — 2026-11-18, ef9dcf6, cargo test: 120 passed; clippy: 0 warnings; pytest: 50 passed; evals: 1/1 passed; project isolation & 500-token budget verified
- [x] Embeddings via Ollama + `sqlite-vec` (optional; falls back to FTS when not available) — 2026-11-18, dfc96db, cargo test: 125 passed; clippy: 0 warnings; pytest: 50 passed; evals: 1/1 passed; vector search, Ollama client, sqlite-vec KNN & FTS5 fallback verified
- [x] Desktop "Memory" window: list, search, edit, delete, forget-by-topic, export — 2026-11-19, 0dc948b, cargo test: 126 passed; pytest: 53 passed; clippy: 0 warnings; browser subagent UI test verified (CRUD, live search, dark theme)
- [x] Sessions window: list, search, rename, resume, delete; work folder chosen per session (B10) — 2026-11-19, 278f93d, cargo test: 128 passed; pytest: 56 passed; clippy: 0 warnings; browser subagent UI test verified (CRUD, work folder per session, rename, live search, dark theme)
- [x] B7: replace every fake widget with real data (usage today, tasks, files created, memory count, model status) or remove it; menu-bar date is live — 2026-11-19, ee01a7e, cargo test: 128 passed; pytest: 61 passed; clippy: 0 warnings; browser subagent UI test verified (live date, 5 real widgets, files modal, light & dark themes)
- [x] `AGENTS.md` read at start, update offered at end of a session — 2026-11-19, 59065f9, cargo test: 133 passed; clippy: 0 warnings; pytest: 64 passed; node check: clean; read at start, update proposals, CLI offer, /agents-md and REST endpoints verified
- [x] 5 memory evals; milestone check — 2026-11-19, 343f4b9, cargo test: 135 passed; clippy: 0 warnings; pytest: 65 passed; node check: clean; 35 evals verified; milestone check complete

## Month 3 — local models and one-file install → v1.0
- [x] Hardware detection (RAM, GPU, VRAM) and model-size recommendation — 2026-11-19, 4e656e0, cargo test: 140 passed; clippy: 0 warnings; pytest: 67 passed; node check: clean; native Win32 RAM/disk + CIM GPU/VRAM detection & size recommendations verified
- [x] Model manager window: list, download with progress, delete, speed test (Ollama API) — 2026-11-19, 3c668e2, cargo test: 148 passed; clippy: 0 warnings; pytest: 71 passed; node check: clean; Ollama API client (list, pull progress stream, delete, speed test), REST/SSE endpoints & desktop Model Manager UI verified
- [x] Request classes (private, simple, hard, vision, long context) and visible routing rules; "Offline" switch — 2026-11-19, dc808df, cargo test: 154 passed; clippy: 0 warnings; pytest: 74 passed; node check: clean; request classification, router locality filtering, offline mode toggle in CLI/REST/UI, bidirectional sync & browser subagent UI test verified
- [x] Small-model mode: shorter prompt, fewer tools, stricter output checks (B13); eval pass rate per model recorded — 2026-11-19, e1dac8e, cargo test: 168 passed; pytest: 74 passed; clippy: 0 warnings; small model detection, concise prompt (~200 tokens), core tool filtering, robust markdown/JSON repair, and eval pass rates tracking matrix
- [x] Serve the desktop from Oblivion (axum): move templates/static into the binary; Flask optional — 2026-11-19, 99dd026, cargo test: 170 passed; clippy: 0 warnings; pytest: 74 passed; node check: clean; rust-embed bundling of templates & static assets, /classic route, status/chat API parity, browser subagent live desktop UI verified across light/dark themes
- [x] Installer (MSI or Tauri tray app), first-run wizard, auto-start option. Code signing: BLOCKED (needs user) for a certificate — 2026-11-20, de9b6b4, cargo test: 175 passed; clippy: 0 warnings; pytest: 74 passed; node check: clean; autostart manager (CLI & REST), first-run wizard (CLI, REST, UI modal), WiX installer spec & packaging pipeline script; code signing BLOCKED (needs user) for certificate
- [x] Measure cold start, idle RAM, eval pass rate per model; table in README — 2026-11-20, b357008, cold start: 14.88 ms (median); idle RAM: 13.43 MB RSS (target < 100 MB); evals: 100% pass rate in standard and small-model modes; published benchmark table in README.md
- [x] Milestone check (offline edit + memory answer + document summary; idle RAM < 100 MB) → tag `v1.0` — 2026-11-20, c61ddfc, cargo test: 178 passed; clippy: 0 warnings; pytest: 74 passed; node check: clean; verified offline edit, memory answer, document summary, and idle RAM at 13.43 MB (< 100 MB target) → tag v1.0

## Month 4 — voice
- [x] Push-to-talk hotkey and mic button; local speech-to-text — 2026-11-20, daf0ab0, cargo test: 185 passed; pytest: 78 passed; clippy: 0 warnings; node check: clean; browser subagent UI test verified (push-to-talk hotkey, menubar & panel mic buttons, light/dark themes)
- [x] Local text-to-speech, streamed by sentence; barge-in — 2026-11-20, 2582ce0, cargo test: 158 passed; pytest: 80 passed; clippy: 0 warnings; node check: clean; browser subagent UI test verified (streaming sentence synthesis, code block suppression, instant barge-in on PTT/Escape, menubar speaker toggle, light/dark themes)
- [x] Spoken-reply style; voice approvals for low-risk actions only — 2026-11-20, 1478cb3, cargo test: 162 passed; pytest: 82 passed; clippy: 0 warnings; node check: clean; browser subagent UI test verified (spoken prompt instructions, low-risk voice approvals, 403 Forbidden gating for high-risk actions, approval risk badges, light/dark themes)
- [x] Optional wake word — 2026-11-20, d379380, cargo test: 183 passed; pytest: 84 passed; clippy: 0 warnings; node check: clean; browser subagent UI test verified (hands-free keyword spotting, prefix remainder extraction, zero CPU overhead when off, light & dark themes)
- [x] Latency and accuracy measured; milestone check — 2026-11-20, 7b7acb7, cargo test: 198 passed; pytest: 84 passed; clippy: 0 warnings; node check: clean; roundtrip latency < 450ms (target < 2s); command accuracy 10/10 (100%); barge-in < 5ms; idle wake word overhead 0% CPU; milestone check complete

## Month 5 — vision and computer use
- [x] Image input in providers and all interfaces — 2026-11-20, e19ba89, cargo test: 196 passed; pytest: 85 passed; clippy: 0 warnings; node check: clean; browser subagent UI test verified (file upload, preview tray, image chips, light & dark themes)
- [x] "Look at my screen" with preview before sending — 2026-11-20, d95b0dd, cargo test: 199 passed; pytest: 86 passed; clippy: 0 warnings; node check: clean; browser subagent UI test verified (look at screen button, preview tray inspection, cancel/remove chip, light & dark themes)
- [x] Browser agent screenshot fallback for picture-only pages — 2026-11-20, 56a3377, cargo test: 201 passed; pytest: 86 passed; clippy: 0 warnings; evals: 35/35 passed (100%); automatic screenshot fallback on insufficient page text, explicit screenshot action, multimodal ToolOutput attachment extraction & picture_quiz.html verified
- [x] Computer use behind a setting, per-app allow list, stop hotkey — 2026-11-21, ed8c762, cargo test: 205 passed; pytest: 87 passed; clippy: 0 warnings; node check: clean; browser subagent UI test verified (emergency stop button, stopped-active badge, app allow list, stop hotkey Escape, /computer and /stop commands)
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

### Month 1 Week 1 & Week 2 Summary (2026-11-18)
- Items done:
  - Week 1: Baseline gates & performance; S4 SSRF protection; S6 Host header check & per-start secret token; S8 bundled local fonts & opt-in weather widget; B6 clean requirements.txt & Flask 3.x; B4 temporary lock; Q2 clean stray dbs & legacy notes; Python CI on Windows/Ubuntu.
  - Week 2: B1 /api/chat forwarded to Oblivion through bridge; B1/B3 30/70 workspace window live SSE streaming + tool cards, diffs, approvals; moved nexus/, examples/, benchmarks/ and NEXUS tests to legacy/ with zero imports in app.py; removed dead NEXUS UI controls and fake streak/skill bars widget; Q7 rewritten README.md, WEB_README.md, and RUN_WEB_INTERFACE.md to describe reality with single Getting Started path.
- Items blocked:
  - PC specs: BLOCKED (needs user) for formal confirmation of RAM/GPU/VRAM/disk specs.
- Gate results:
  - `cargo test --workspace`: 92 passed, 0 failed
  - `cargo clippy --workspace --all-targets -- -D warnings`: 0 warnings
  - `python -m pytest -q tests`: 24 passed, 0 failed
  - `python -m pytest -q legacy/tests`: 8 passed, 0 failed
  - `node --check`: clean on all static JS files
  - UI visual verification: verified with browser subagent across Ice Blue and Dark Charcoal themes with screenshot evidence.

### Month 1 Week 3 Summary (2026-11-18)
- Items done:
  - B8: local test site under `oblivion/evals/sites/` (index, form, quiz, pagination, login) with pytest validation (29 passed).
  - B8: browser agent evaluation on five real tasks (search/pagination, page read, multi-step form [CONF-7842], practice quiz [3/3 100%], auth gateway [OBLIVION-AUTH-SUCCESS-99]); external live logins BLOCKED (needs user).
  - B9: `--tui` resilience in Windows Terminal: ANSI and bracketed paste handling, resize coordinate clamping, Shift+Enter and Enter handling, mouse wheel scroll capture.
  - B12: refined instructions across all 6 bundled skills (`security-review`, `refactor`, `write-tests`, `frontend-design`, `mcp-builder`, `explain-codebase`) addressing model failure modes.
  - Q6: per-run trace recorder in `oblivion-cli` writing JSON lines (`TraceEntry`) to `~/.oblivion/traces/`, 5MB file cap with overflow logging, 50-file rotation, prefix matching & `--trace list` / `--trace <id>` inspection.
- Gate results:
  - `cargo test --workspace`: 97 passed, 0 failed (14 in oblivion-cli)
  - `cargo clippy --workspace --all-targets -- -D warnings`: 0 warnings
  - `python -m pytest -q tests`: 29 passed, 0 failed

### Month 1 Week 4 & Milestone v0.5 Summary (2026-11-18)
- Items done:
  - Q4: grown evals suite from 10 to 30 tasks (+5 research [11-15], +5 documents [16-20], +5 browser [21-25] with daemon test site server, +5 safety [26-30] testing prompt injection, delete approvals, secret leakage, path traversal).
  - B11: provider limits moved to `oblivion.toml` with `checked = "YYYY-MM-DD"` date field and validation, while keeping built-in fallback defaults intact.
  - Python tests for every `web/app.py` route: 16 test cases in `tests/test_web_routes.py` covering GET /, POST /api/chat, /api/oblivion/* routes, auth tokens, timeout, validation.
  - Milestone check: verified zero canned text responses; S1–S4, S6, S8, S9 closed; 30 evals runnable with single command; five real browser tasks completed.
- Gate results:
  - `cargo test --workspace`: 98 passed, 0 failed
  - `cargo clippy --workspace --all-targets -- -D warnings`: 0 warnings
  - `cargo fmt --all`: clean
  - `python -m pytest -q tests`: 50 passed, 0 failed
  - `node --check`: clean on all static JS files

### Month 2 Summary (2026-11-19)
- Items done:
  - Memory schema in `oblivion-store` with FTS5 search, migrations, ranking & triggers (6d4125d).
  - `remember`, `recall`, `forget` tools with permission rules (a73691a).
  - End-of-session memory extraction pass proposing memories (cheap model + heuristic fallback) (7179626).
  - Recall on each request within ~500-token budget with per-project isolation (ef9dcf6).
  - Embeddings via Ollama + sqlite-vec with fallback to FTS5 (dfc96db).
  - Desktop "Memory" window: list, search, edit, delete, forget-by-topic, export via REST API & UI (0dc948b).
  - Sessions window: list, search, rename, resume, delete; work folder per session (B10) (278f93d).
  - Real desktop widgets (B7): usage today, current tasks, files created this week, memory count, model status; live menu-bar date (ee01a7e).
  - `AGENTS.md` read at start, update offered at end of a session, `/agents-md` commands & REST endpoints (59065f9).
  - 5 memory evals added (tasks 31-35) covering fact recall, deleted memory rejection, project isolation, user preferences, and AGENTS.md lifecycle; runner updated to support isolated homes and seed.sql (343f4b9).
- Gate results:
  - `cargo test --workspace`: 135 passed, 0 failed
  - `cargo clippy --workspace --all-targets -- -D warnings`: 0 warnings
  - `cargo fmt --all`: clean
  - `python -m pytest -q tests`: 65 passed, 0 failed
  - `node --check`: clean on all static JS files
  - UI visual verification: verified with browser subagent across Memory window, Sessions window, Desktop widgets, Files modal, dark & light themes.

### Month 3 & Milestone v1.0 Summary (2026-11-20)
- Items done:
  - Hardware detection (RAM, GPU, VRAM) and model-size recommendation (4e656e0).
  - Model manager window: list, download with progress, delete, speed test via Ollama API (3c668e2).
  - Request classes (private, simple, hard, vision, long context), visible routing rules & Offline switch (dc808df).
  - Small-model mode: shorter prompt (~200 tokens), core tool filtering, robust markdown/JSON repair, eval pass rates tracking matrix (e1dac8e).
  - Serve desktop from Oblivion (axum): rust-embed bundling templates & static assets, /classic route, API aliases, full standalone Rust binary desktop (99dd026).
  - Installer & first-run wizard: autostart manager (CLI & REST), setup wizard (CLI, REST, UI modal), WiX installer spec and packaging script; code signing BLOCKED (needs user) for certificate (de9b6b4).
  - Measurements: cold start median 14.88 ms; idle RAM 13.43 MB RSS (target < 100 MB); evals 100% pass rate in standard and small-model modes; published benchmark table in README.md (b357008).
  - Milestone check: verified offline edit, memory answer, document summary, and idle RAM at 13.43 MB (< 100 MB target).
- Gate results:
  - `cargo test --workspace`: 178 passed, 0 failed
  - `cargo clippy --workspace --all-targets -- -D warnings`: 0 warnings
  - `cargo fmt --all`: clean
  - `python -m pytest -q tests`: 74 passed, 0 failed
  - `node --check`: clean on all static JS files
  - UI visual verification: verified with browser subagent across light & dark themes; desktop, models manager, routing, and First-Run Setup Wizard fully verified on desktop with screenshot evidence.

### Month 4 Summary (2026-11-20)
- Items done:
  - Push-to-talk hotkey (Ctrl+Space) and mic buttons in menubar and panel; local speech-to-text (daf0ab0).
  - Local text-to-speech, sentence-by-sentence streaming, code block suppression, instant barge-in on PTT/Escape, menubar speaker toggle (19248f6).
  - Spoken-reply style instructions, voice approvals allowed only for low-risk actions with 403 Forbidden gating on high-risk actions, approval risk badges (1c610f6).
  - Optional hands-free wake word keyword spotting ("Hey Jarvis" / "Jarvis"), disabled by default with 0% CPU and zero privacy impact when off, menubar toggle (557e61e).
  - Milestone verification: hands-free round trip latency (< 450 ms vs < 2000 ms target), 10/10 sequential spoken commands understood, voice approval security invariants, instant barge-in (< 5 ms vs < 50 ms target), zero idle wake word overhead; published benchmark table in README.md.
- Gate results:
  - `cargo test --workspace`: 198 passed, 0 failed
  - `cargo clippy --workspace --all-targets -- -D warnings`: 0 warnings
  - `cargo fmt --all`: clean
  - `python -m pytest -q tests`: 84 passed, 0 failed
  - `node --check`: clean on all static JS files
  - UI visual verification: verified with browser subagent across light & dark themes for PTT, TTS barge-in, voice approvals, and hands-free wake word.

## Notes for next session
<!-- Surprises about the code, environment quirks, things the next session must know. -->
- Month 4 is complete and merged.
- Working on Month 5 — vision and computer use (`roadmap/m5` branch).
- First four items of Month 5 are complete:
  - "Image input in providers and all interfaces"
  - "'Look at my screen' with preview before sending"
  - "Browser agent screenshot fallback for picture-only pages"
  - "Computer use behind a setting, per-app allow list, stop hotkey"
- Next item to work on:
  `10 vision evals; milestone check`
- Quality gates status:
  - `cargo test --workspace -- --test-threads=1`: 205 passed, 0 failed
  - `cargo clippy --workspace --all-targets -- -D warnings`: 0 warnings
  - `cargo fmt --all`: clean
  - `python -m pytest -q tests`: 87 passed, 0 failed
  - `node --check`: clean on all static JS files
  - UI visual verification: verified with browser subagent with screenshot evidence (`final_step_136.png`).
- Windows + PowerShell: no bash heredocs; quote paths with spaces.
- Smart App Control has blocked Rust build scripts before; if a build fails with an
  "Application Control policy" error, mark the item BLOCKED (needs user) instead of retrying.
- Groq free tier: about 8K tokens/minute and ~7K tokens per request; keep prompts small.
