# Jarvis + Oblivion: project analysis, problems, and a 10-month roadmap

_Written 16 November 2026. Based on the code in `D:\Jarvis` as of that date._

How to read this: section 1 is the short version. Sections 2–3 say what exists and what is
wrong. Section 4 lists the features worth adding. Section 5 is the plan: months 1–3 are the
minimum ("daily driver"), months 4–10 take it to "best local assistant". Section 8 is what to do
in the first two weeks.

Every problem has an ID (S = security, B = bug, Q = quality) so the plan can refer to it.
Each is marked **[read]** (I read the code and saw it), **[tested]** (I ran it), or
**[not run]** (the code exists but has never been run for real).

---

## 1. Short version

**What you have.** Three things live in one repository:

| Part | Size | State |
|---|---|---|
| **Oblivion** (`oblivion/`, Rust) | ~11,700 lines, 7 crates, 90 tests, 19 skills, 10 evals | A working coding agent. This is the real engine. |
| **Jarvis desktop** (`web/`, Flask + HTML/CSS/JS) | ~2,500 lines | A good-looking macOS-style desktop. Most widgets show made-up data. The new Oblivion panel is the only part connected to a real agent. |
| **NEXUS** (`nexus/`, Python) | roughly 8,000–9,000 lines | Two generations of code. The old one (reasoning, modules) returns canned text. The newer one (runtime, agent, models) can call models but is not connected to the web UI and has safety holes. |

**The main problem is not a bug. It is that there are two assistants.** The Jarvis chat window
talks to NEXUS, which does not think. The Oblivion panel talks to Oblivion, which does. They share
no memory, no settings and no model keys. Until there is one brain, every new feature has to be
built twice or lands on the wrong side.

**The three most important things to do**

1. **One brain.** Make Oblivion the only engine. Point the Jarvis chat at it, keep the desktop as
   the face, and retire the NEXUS code that only pretends (month 1).
2. **Close the safety holes** before adding power (month 1). The list is in section 3.1; the worst
   are S1–S4.
3. **Give it memory and local models** (months 2–3). Those two are what make an assistant feel
   like *yours* and keep working when the free limits run out.

**What "best local assistant" means here** (the targets the plan aims at):

- Works with no internet for everyday tasks, using a model on your PC.
- Remembers you across sessions, and you can see and edit what it remembers.
- Can act: files, commands, browser, documents, your apps, on a schedule.
- Never does something risky without asking, and cannot be tricked into it by a web page.
- Starts in under 2 seconds and idles under 100 MB of RAM (without a local model loaded).
- Installs with one file. No Rust, Python or Node needed for the basics.
- You can talk to it and it can see your screen.

---

## 2. What exists today

### 2.1 Oblivion (Rust) — working

| Area | What it does | Evidence |
|---|---|---|
| Model access | Any OpenAI-compatible API; fallback between models; waits out rate limits; per-day usage ledger; Ollama for local models | [tested] against a fake server; Groq used for real by you |
| Tools | read, write, edit (fuzzy match), glob, grep, shell, web search, web fetch, todo; undo via snapshots | [tested] 10/10 on the eval suite with Groq |
| Safety | Ask / auto-edit / never-ask modes; `tool(pattern)` permission rules; diff shown before approval; repeat guard; web and step budgets | [tested] |
| Skills | SKILL.md discovery, precedence, GitHub installer, 19 bundled skills | [tested] loading; most skills never used on a real task |
| MCP | stdio and HTTP servers through one lazy `mcp` tool | [tested] with a fixture server |
| Sub-agents | Parallel read-only helpers; deep-research skill | [tested] with a fake server; one real run hit limits before the fixes |
| Browser agent | Playwright MCP wrapper, confirm-before-submit, page trimming | **[not run]** against a real browser by me |
| Interfaces | Plain terminal, full-screen terminal (`--tui`), web page (`--web`), Jarvis panel | Web [tested]; TUI tested only in a simulated terminal |
| Storage | SQLite sessions and usage | [tested] |

### 2.2 Jarvis desktop (Flask) — a shell

- Lock screen, menu bar, desktop icons, widgets, dock, 30/70 workspace window, four themes.
- The Oblivion panel (added 16 Nov) is real: streaming, diffs, approvals, tasks, helpers.
- Everything else on the desktop is presentation. See B8.

### 2.3 NEXUS (Python) — two generations

- **Old (August):** `core/reasoning.py`, `core/context.py`, `modules/*` (~4,400 lines). Keyword
  matching and templates; `asyncio.sleep()` to simulate thinking. No model is called.
- **Newer (September):** `core/runtime.py`, `agent/*`, `models/*`, `memory/*`, `telemetry/*`.
  A state machine (understand → plan → execute → verify → repair) with real model providers
  (Ollama and OpenAI-compatible) and three tools (files, git, terminal). It is not used by the
  web app, and it has the safety problems S1–S3.
- **Worth keeping as ideas:** the memory tiers (episodic / semantic / procedural) with
  consolidation, the action journal for replay, the "modes" concept. Each fits better as a
  feature of Oblivion than as a second engine.

---

## 3. Problems and bugs

### 3.1 Security (fix these first)

| ID | Problem | Where | Status |
|---|---|---|---|
| **S1** | **The model decides how risky its own actions are.** `ActionStep.risk_level` defaults to LOW and the policy allows anything marked LOW without looking at the tool's real risk. A plan from the model that contains a `terminal` step and leaves the risk out runs with no confirmation. | `nexus/models/schemas.py:87`, `nexus/agent/policy.py` | [read] |
| **S2** | **File tools are not limited to the project.** The file says "sandboxed", but `write_file` accepts any path on the disk, and MEDIUM-risk writes are auto-approved. | `nexus/agent/tools/filesystem.py`, `policy.py` | [read] |
| **S3** | **The terminal block-list is five text patterns.** Anything not literally in the list runs (for example `rd /s /q` instead of `rmdir /s /q`). On timeout only the shell is killed, not the programs it started. | `nexus/agent/tools/terminal.py` | [read] |
| **S4** | **Oblivion's `web_fetch` can read addresses on your own PC and network.** I found no check for `localhost`, `127.x`, `192.168.x`, `169.254.x`. A web page could tell the agent to fetch your router page or the Jarvis API. | `oblivion-tools/src/web.rs` | [read] |
| **S5** | **Shell commands are not sandboxed.** Approval is the only barrier, and "Never ask" is one click in the panel. In that mode a prompt-injected command runs with your full user rights. | Oblivion shell tool, panel | [read] |
| **S6** | **The Jarvis API has no login.** I added a same-origin check, which stops other websites. It does not stop another program on your PC, and it does not check the `Host` header (a known trick called DNS rebinding gets past origin checks alone). | `web/oblivion_bridge.py` | [read] |
| **S7** | **Web pages and files can carry instructions.** The skills tell the model to treat page text as data, but that is advice, not enforcement. Nothing marks tool output as untrusted or blocks risky actions right after reading outside content. | Oblivion core | [read] |
| **S8** | **The "local" desktop phones out.** It loads Google Fonts, asks the browser for your location, and sends coordinates to Open-Meteo and OpenStreetMap on every load. | `web/templates/index.html`, `static/js/app.js` | [read] |
| **S9** | **The wrong API key can go to the wrong company.** The NEXUS cloud provider falls back to `OPENAI_API_KEY`, then `GEMINI_API_KEY`, whatever the `base_url` is. | `nexus/models/cloud_provider.py` | [read] |

S1–S3 only matter while the NEXUS runtime exists. If month 1 retires it, they disappear; if you
keep it, they must be fixed before it is ever connected to a model with tools.

### 3.2 Things that do not work or work wrongly

| ID | Problem | Status |
|---|---|---|
| **B1** | **Jarvis chat is not intelligent.** `/api/chat` and the 30/70 window go through `web/nexus_adapter.py` to the old `ReasoningEngine`: canned replies and a 2-second sleep. The newer `AgentRuntime` is never imported by the web app. | [read] |
| **B2** | **Intent detection matches letters inside words.** In `runtime.py`, "run" matches "b**run**ch", "test" matches "la**test**", "git" matches "di**git**al". Those messages are treated as commands. Replies ignore the conversation so far, and when the model call fails the user sees `[NEXUS] Processed query: ...` with no error. | [read] |
| **B3** | **A paused action can never be approved.** The runtime returns "Execution paused: requires permission" and stops. There is no way to say yes from the web UI. | [read] |
| **B4** | **The Flask app is now multi-threaded but NEXUS is not thread-safe.** I turned on threading so the Oblivion event stream does not block the page. NEXUS shares one SQLite connection (`check_same_thread=False`) and one context object with no lock, and each request calls `asyncio.run()`. Two chat requests at once can corrupt state. Low risk with one user; fix with a lock, or by B1's fix. | [read], introduced 16 Nov |
| **B5** | **Hard-coded paths.** The skills module points at `D:\Desktop\RAG`. Database names are relative, so a new `.db` appears wherever you start the app: there are six stray `.db` files in the root and `web/` folders. | [read] |
| **B6** | **`requirements.txt` cannot be installed.** It lists `sqlite3` and `asyncio` (built into Python; pip fails on them), about 3 GB of unused libraries (transformers, spaCy, OpenCV, sentence-transformers), and not Flask, which is why `python app.py` failed today. `web/requirements.txt` pins Flask 2.3.3; I tested with 3.1. The docs say Python 3.14; the folders show 3.12 and 3.13 in use. | [read] |
| **B7** | **The desktop shows made-up data.** "231 day streak", skill levels, "3 Docs Available", the log lines "RAG Knowledge Vault Loaded", the Now Playing card. The menu-bar date is fixed text ("Aug 12"). Desktop icons open the workspace window and print "Fetching assets..." but fetch nothing. | [read] |
| **B8** | **The browser agent has never driven a real browser.** A sandbox rule stopped me launching one. The tests use a fake server written from Playwright MCP's documentation. Pages that are only pictures cannot be read at all (no vision). | [not run] |
| **B9** | **The full-screen terminal was only tested in a simulated terminal.** Real Windows Terminal behaviour (paste, resize, Shift+Enter, unusual fonts) is unverified. | [not run] |
| **B10** | **The panel has one conversation.** No list of past sessions, no resume, no way to change the work folder without restarting `app.py`. Reloading the page replays the whole history. | [read] |
| **B11** | **Free-tier limits are written into the code** (Groq 1,000 requests/day and 7,000 tokens/request; OpenRouter 50/day). Providers change these; when they do, routing decisions go wrong silently. | [read] |
| **B12** | **Most skills have never done a real job.** pptx and data-analysis scripts were run by me; security-review, refactor, write-tests, frontend-design, mcp-builder and explain-codebase are instructions that no model has followed yet. | [not run] |
| **B13** | **Small models struggle with the whole thing.** One research run used 241K tokens (a full day of one Groq model) and produced nothing before the budgets were added. Budgets now stop the waste; they do not make a 7K-token model good at long tasks. | [tested] by you |

### 3.3 Quality and maintenance

| ID | Problem |
|---|---|
| **Q1** | **Two of everything.** Two model routers (Python keyword scoring, Rust limit-aware), two tool sets, two permission systems, two memory stores, two web servers (Flask and axum). |
| **Q2** | **Leftover files.** Eight "summary / completion" Markdown files and `DONE.txt` in the root describe work as finished that the code does not do. `verify_structure.py`, `run_tests.py`, `diag_test.db`, `test.db`. |
| **Q3** | **Python has almost no tests and no CI.** CI covers `oblivion/` only. The NEXUS tests are three files; the old modules have none. |
| **Q4** | **The eval suite is 10 coding tasks.** Nothing measures research, browser tasks, documents, memory, or how often the agent asks when it should not (or does not when it should). |
| **Q5** | **Installing is a developer job.** It needs Rust, Python, Node (for the browser) and manual key setup. Smart App Control blocked the build once. The `.exe` is unsigned. |
| **Q6** | **Nothing is logged for debugging.** When a run goes wrong, the only record is what scrolled past. NEXUS has a journal idea; Oblivion has no trace file. |
| **Q7** | **Docs disagree with reality.** `README.md` and `WEB_README.md` still describe NEXUS as the system. |

---

## 4. Features worth adding

Ordered by how much each changes daily use, not by how exciting it sounds.

### Tier 1 — makes it a real assistant

| Feature | What it means | Why it matters |
|---|---|---|
| **One brain** | Every chat surface (panel, 30/70 window, terminal, later voice) is the same Oblivion session | Fixes B1, B3, Q1 at once |
| **Long-term memory** | It keeps facts about you, your projects and what worked. Stored in SQLite with text search and embeddings. A "Memory" window on the desktop shows every item; you can edit or delete any of them. Relevant items are added to each request | The difference between a tool and an assistant |
| **Local model mode** | Detects your RAM and GPU, suggests a model that fits, downloads it through Ollama, and routes by task: local for private or simple work, cloud for hard work. A visible "Offline" switch | Works when limits run out or the internet is down; private files never leave the PC |
| **Sessions** | List, search, rename, resume and fork conversations from the desktop; pick the work folder per session | Fixes B10 |
| **Honest desktop** | Every widget shows real data or is removed: usage today, tasks, recent files the agent made, memory count, model status | Fixes B7 |
| **One-file install** | A single signed installer; the desktop is served by Oblivion itself, so the basics need no Python | Fixes Q5 |

### Tier 2 — makes it better than a chat window

| Feature | What it means |
|---|---|
| **Voice** | Push-to-talk first, wake word later. Speech-to-text and text-to-speech run on the PC (whisper.cpp and Piper are the usual choices). Barge-in: speaking stops the reply |
| **Vision** | Paste or drop an image; "look at my screen"; the browser agent can read picture-only pages. Needs a model with image input (cloud, or a small local vision model) |
| **Your documents as knowledge** | Point it at folders; it indexes PDFs, Word files, notes and code, and answers with the file and page it used. Replaces the hard-coded `D:\Desktop\RAG` |
| **Scheduled and background tasks** | "Every morning at 8, summarise my inbox". Runs while the desktop is closed, sends a Windows notification, and still stops for approval on risky steps |
| **Email, calendar, GitHub** | Through MCP connectors with a setup screen instead of editing a config file |
| **Real sandbox** | Commands run with restricted rights limited to the work folder (Windows job objects and restricted tokens, or WSL). Makes "Never ask" reasonable |
| **Injection defence** | Tool output is tagged as untrusted; after reading outside content, risky actions always ask, whatever the mode |

### Tier 3 — makes it the best

| Feature | What it means |
|---|---|
| **Computer use** | It can see and operate desktop apps (screenshots, mouse, keyboard) with a per-app allow list |
| **Parallel coding agents** | Several write-capable agents, each in its own git worktree, with a review screen to merge the results |
| **Self-improvement loop** | Failed runs are saved as new eval cases; a weekly report shows pass rate per model and per skill |
| **Skill and connector gallery** | Browse, install and update skills and MCP servers from the desktop |
| **Phone access** | Use it from your phone over your home network or a private tunnel, with pairing and approval prompts on the phone |
| **Hooks** | Your own scripts run before or after tool calls (format on save, block certain paths, log everything) |
| **Trace viewer** | Every run recorded; replay it step by step to see why it did something |

### Deliberately not on the list

- **Fine-tuning your own model.** High cost, small gain next to better memory and routing.
- **Rewriting the desktop in a new framework.** The current HTML/CSS is fine; it needs real data.
- **More canned "modes" and "modules".** Skills already do this job and are easier to write.

---

## 5. The plan

Assumptions: one developer, part-time, free model tiers, Windows as the main platform. Months are
four weeks. Dates assume a start on 16 November 2026, so month 2 onwards falls in 2027. Each month ends with something you can use
and a tag in git.

| Milestone | End of | What you can say |
|---|---|---|
| **v0.5 "One brain"** | Month 1 | "There is one assistant, and it is safe to use." |
| **v1.0 "Daily driver"** | Month 3 | "It remembers me, works offline, and installs with one file." |
| **v1.5 "Hands and eyes"** | Month 6 | "I can talk to it, it can see, and it works while I am away." |
| **v2.0 "Best local assistant"** | Month 10 | "Sandboxed, measurable, extensible, and usable from my phone." |

If you only have three months, do months 1–3. They are ordered so that stopping after any month
still leaves a coherent product.

---

### Month 1 (16 Nov – 13 Dec): one brain, safe by default → v0.5

**Goal:** remove the fake parts, close the security holes, prove what has never been run.

Week 1 — clean and safe
- Fix **S4**: block private and local addresses in `web_fetch` (and in redirects).
- Fix **S6**: check the `Host` header; add a per-start secret the page must send.
- Fix **S8**: bundle the fonts; weather only after you click "enable", no automatic location.
- Fix **B6**: one `requirements.txt` with only what is used (Flask, and pydantic/aiohttp only if
  NEXUS runtime survives); state one Python version.
- Fix **B4** with a lock around NEXUS calls (temporary, until B1 is done).
- Delete the eight summary files, `DONE.txt`, stray databases and helper scripts (**Q2**).

Week 2 — one brain
- Point `/api/chat` and the 30/70 window at Oblivion through the bridge (**B1**, **B3**).
  The left side stays the chat; the right 70% shows the same live tool cards, diffs and tasks the
  panel shows. One conversation, two views.
- Move NEXUS into `legacy/` and stop importing it from the web app. This removes **S1–S3**, **S9**,
  **B2**, **B5** from the running system. Keep `memory/` and `telemetry/` as reference for month 2.
- Update `README.md` to describe what is true (**Q7**).

Week 3 — prove it
- Run the browser agent on real sites: a search, a login you do by hand, a multi-page form, a
  practice quiz. Fix what breaks (**B8**).
- Run `--tui` in Windows Terminal for a full day of normal work (**B9**).
- Use each untested skill once on a real task and fix the instructions (**B12**).
- Add a trace file per run (JSON lines: every model call, tool call, approval) (**Q6**).

Week 4 — measure
- Grow evals from 10 to 30: add research (5), documents (5), browser on a local test site (5),
  safety (5: must ask before delete, must refuse a page that says "ignore your instructions").
- Python tests for the bridge and web routes in CI.
- Move rate-limit numbers into the config file with a "last checked" date (**B11**).

**Done when:** no code path returns canned text; all S-items except S5 and S7 are closed; 30 evals
run with one command; the browser agent has completed five real tasks.

---

### Month 2 (14 Dec – 10 Jan): memory and an honest desktop

**Goal:** it knows you, and everything on screen is true.

- **Memory store** in Oblivion's SQLite: items with type (fact about you, project fact, how-to,
  past outcome), source session, time, and importance. Full-text search first; embeddings second
  (a small local embedding model through Ollama, vectors in `sqlite-vec`).
- **Memory tools:** `remember`, `recall`, `forget`. After each session a cheap model pass proposes
  items to keep (this is the NEXUS `extractor` idea done for real).
- **Recall on every request:** the top few relevant items go into the prompt, inside a budget
  (about 500 tokens) so small models still fit.
- **Memory window** on the desktop: list, search, edit, delete, "forget everything about X",
  export. Nothing is remembered that you cannot see.
- **Sessions window:** list, search, resume, rename, delete; choose the work folder per session
  (**B10**).
- **Real widgets** (**B7**): today's requests and tokens per model, current tasks, files created
  this week (the desktop icons open these for real), memory count, model status. Remove the
  streak, the skill bars and the music card, or wire them to something true.
- **Project memory:** `AGENTS.md` is read at start and offered an update at the end of a session.

**Done when:** you tell it your preferences on Monday and it applies them on Friday in a new
session; 5 memory evals pass (recalls a fact, does not recall a deleted one, does not leak one
project's facts into another); every number on the desktop comes from the database.

---

### Month 3 (11 Jan – 7 Feb): local models and one-file install → v1.0

**Goal:** works offline; a non-developer could install it.

- **Hardware check:** read RAM, GPU and VRAM; recommend a model size that fits; explain the
  trade-off in one sentence. (First task: write down your PC's specs; the choice of default local
  model depends on them.)
- **Model manager** on the desktop: installed models, download with progress, delete, test speed.
- **Smart routing:** each request gets a class (private, simple, hard, needs vision, needs long
  context) and goes to the cheapest model that can do it. Rules are visible and editable.
  "Offline" switch forces local only.
- **Small-model mode:** shorter system prompt, fewer tools offered, stricter output checks, so a
  7–8B local model can still finish basic tasks (**B13**).
- **Serve the desktop from Oblivion** (axum) instead of Flask: move `web/templates` and
  `web/static` into the binary. One process, one port, no Python for the basics. Python stays
  optional for document skills.
- **Installer:** signed `.msi` or a Tauri shell with a tray icon, auto-start option and updates.
  First-run wizard: pick the folder, add keys or choose local-only, test (**Q5**).
- **Measure:** cold start time, idle RAM, eval pass rate per model. Publish the table in the
  README.

**Done when:** with the network cable out it can still edit a file, answer from memory and
summarise a document; a fresh Windows PC goes from download to first answer in under 10 minutes;
idle RAM under 100 MB without a local model loaded.

---

### Month 4 (8 Feb – 7 Mar): voice

**Goal:** you can talk to it and it answers out loud, all on the PC.

- Push-to-talk (a global hotkey and a mic button in the panel) with local speech-to-text.
- Local text-to-speech with sentence-by-sentence streaming so it starts speaking early.
- Barge-in: when you start talking, it stops.
- A "spoken reply" style: short, no code read aloud, long output goes to the panel.
- Approvals by voice ("yes" / "no") only for low-risk actions; risky ones still need a click.
- Wake word last, and optional (always-listening has a privacy and CPU cost).

**Done when:** a hands-free round trip (speak → answer starts) takes under 2 seconds on a cloud
model; ten spoken commands in a row are understood in a quiet room.

---

### Month 5 (8 Mar – 4 Apr): vision and computer use

**Goal:** it can see.

- Image input in the provider layer; paste and drag-drop images in every interface.
- "Look at my screen": one screenshot on request, shown to you before it is sent.
- Browser agent falls back to a screenshot when the page text is not enough (**B8**).
- Computer use, behind a setting: screenshot, click, type; per-app allow list; every action
  visible in the panel; a hard stop hotkey.
- Evals: 10 vision tasks (read a chart, find a button, picture-based quiz on a test page).

**Done when:** it completes a picture-based practice quiz and changes a setting in one desktop
app, asking before each final action.

---

### Month 6 (5 Apr – 2 May): automations and connectors → v1.5

**Goal:** it works when you are not looking.

- Scheduler in Oblivion: one-off and repeating tasks, stored in SQLite, run by the tray process.
- Each run is a fresh session with its own trace; results arrive as a Windows notification and a
  card on the desktop.
- Unattended rules: read-only actions run; anything that sends, pays, deletes or posts waits for
  you (approval queue on the desktop and in the notification).
- Connector setup screen for MCP servers: Gmail, calendar, GitHub, Notion, files. Keys go in the
  Windows Credential Manager.
- Starter automations: morning brief, "watch this repo's issues", weekly cleanup report.

**Done when:** a morning brief has run seven days in a row without help and never took a
risky action without approval.

---

### Month 7 (3 May – 30 May): your documents as knowledge

**Goal:** answers from your own files, with sources.

- Indexer for chosen folders: PDF, Word, PowerPoint, Markdown, code. Incremental (only changed
  files), runs in the background at low priority.
- Search that combines keywords and embeddings; answers cite file and page; click opens the file.
- "Collections" on the desktop (the Research Papers and Documents icons become real).
- Research skill upgrade: uses your documents first, the web second, and says which is which.
- Evals: 15 question-answer pairs over a fixed set of documents; measure correct citations.

**Done when:** on your own papers folder it answers 12 of 15 test questions with the right
source page.

---

### Month 8 (31 May – 27 Jun): sandbox and defence

**Goal:** safe enough that "Never ask" is not reckless (**S5**, **S7**).

- Commands run in a restricted process: limited to the work folder, no network unless allowed,
  whole process tree killed on timeout. Windows first (job objects + restricted token; WSL as an
  alternative), then Linux and macOS.
- Untrusted-content tracking: once web or document text enters the conversation, write, shell,
  send and browser-submit actions ask, in every mode, until you clear it.
- Secrets scanner on outgoing requests: warn before a key or `.env` content is sent to a model.
- Network allow list for tools.
- Red-team evals: 20 injection cases (web page, PDF, file name, tool output) that must all fail
  to cause an action.

**Done when:** 20 of 20 injection cases are blocked; a command told to delete a file outside
the work folder fails at the operating-system level, not just at the prompt.

---

### Month 9 (28 Jun – 25 Jul): parallel agents, hooks, gallery

**Goal:** scale up what it can do at once and let others extend it.

- Write-capable sub-agents, each in its own git worktree; a review screen shows each diff with
  accept / reject / merge.
- Hooks: scripts before and after tool calls, defined in config.
- Skill and connector gallery on the desktop: browse, install, update, remove; show what each
  one is allowed to do before install.
- Self-improvement loop: a failed or undone run can be saved as an eval case with one click;
  weekly report of pass rate per model and skill.

**Done when:** three agents fix three different issues in one repo at the same time and you
merge all three from the review screen.

---

### Month 10 (26 Jul – 22 Aug): phone, polish, release → v2.0

**Goal:** finish.

- Phone access: a small web app served by Oblivion; pairing by QR code; approvals and voice from
  the phone; home network or private tunnel only.
- Trace viewer: replay any run step by step.
- Performance pass: start time, memory, token use per task; fix the worst three of each.
- Evals to 100 tasks across all areas; run nightly; badge in the README.
- Docs: install, first hour, safety model, writing a skill, adding a connector.
- Security review of the whole thing using the checklist from month 8.

**Done when:** a new user installs it, sets it up from the docs alone and completes five
different kinds of task in the first hour.

---

## 6. How to measure progress

| Measure | Now | v1.0 (month 3) | v2.0 (month 10) |
|---|---|---|---|
| Eval tasks | 10 (coding only) | 45 | 100 |
| Eval pass rate, best free model | 10/10 on coding | ≥ 80% overall | ≥ 85% overall |
| Eval pass rate, local model | not measured | ≥ 50% | ≥ 65% |
| Open S-items | 9 | ≤ 2 (S5, S7) | 0 |
| Places returning canned text | Jarvis chat, most widgets | 0 | 0 |
| Install steps | Rust + Python + Node + config | 1 installer | 1 installer |
| Idle RAM (no local model) | not measured | < 100 MB | < 100 MB |
| Time to first token | not measured | < 2 s | < 1.5 s |
| Injection cases blocked | not measured | 5/5 | 20/20 |

Measure the "now" column in week 1 so later numbers mean something.

---

## 7. Risks and open questions

| Risk | What to do about it |
|---|---|
| **Free tiers shrink or vanish.** | Month 3 (local models) is the insurance. Keep provider limits in config, not code. |
| **Small local models cannot use tools reliably.** | Small-model mode, strict output checks, and routing hard tasks to cloud. Measure per model; do not guess. |
| **Your PC may not run a useful local model.** | Unknown until you check RAM and GPU. If it cannot, "offline" means memory, search and documents only, and that should be stated plainly. |
| **Scope.** Ten months of features for one part-time developer is a lot. | Every month ships something usable. Cut from the end, never from month 1. |
| **Windows sandboxing is hard.** | Month 8 has WSL as a fallback. Do not promise it earlier. |
| **Voice and vision quality depend on models that change quickly.** | Keep them behind interfaces; pick models when you get there, not now. |
| **Playwright MCP changes its tool names.** | The wrapper already detects one such change; add a version check and an eval. |

Decisions that are yours to make:

1. **Retire NEXUS or repair it?** This plan retires it. Repairing it means fixing S1–S3, B2, B3
   and keeping two engines.
2. **Keep Flask or move the desktop into Oblivion?** The plan moves it in month 3 for the
   one-file install. Keeping Flask is less work now and more work at install time.
3. **Name.** The repo is "jarvis", the desktop says "JARVIS / NEXUS", the engine is "Oblivion".
   Pick one for the product.
4. **How private?** Decide whether cloud models may ever see file contents by default. It changes
   the routing rules in month 3.

---

## 8. The first two weeks, as a checklist

Week 1
- [ ] Write down PC specs (RAM, GPU, VRAM, free disk).
- [ ] Measure: start time, idle RAM, eval pass rate with your current keys.
- [ ] `web_fetch`: block local and private addresses (S4).
- [ ] Bridge: `Host` check and per-start secret (S6).
- [ ] Desktop: bundle fonts; weather opt-in (S8).
- [ ] Replace `requirements.txt`; state the Python version (B6).
- [ ] Lock around NEXUS calls (B4).
- [ ] Delete summary files, stray databases, helper scripts (Q2).

Week 2
- [ ] `/api/chat` and the 30/70 window use Oblivion (B1, B3).
- [ ] Move `nexus/` to `legacy/`; remove imports from `web/`.
- [ ] Rewrite `README.md` and `RUN_WEB_INTERFACE.md` to match (Q7).
- [ ] Run the browser agent on five real tasks; note every failure (B8).
- [ ] One day of real work in `--tui`; note every problem (B9).
- [ ] Tag `v0.5-rc1`.

---

## Appendix: where each problem gets fixed

| ID | Month | ID | Month | ID | Month |
|---|---|---|---|---|---|
| S1–S3 | 1 (by retiring NEXUS) | B1 | 1 | B10 | 2 |
| S4 | 1 | B2 | 1 (retired) | B11 | 1 |
| S5 | 8 | B3 | 1 | B12 | 1 |
| S6 | 1 | B4 | 1 | B13 | 3 |
| S7 | 8 (first step in 1: safety evals) | B5 | 1 (retired), 7 (real RAG) | Q1 | 1, 3 |
| S8 | 1 | B6 | 1 | Q2, Q7 | 1 |
| S9 | 1 (retired) | B7 | 2 | Q3, Q4 | 1, then every month |
| | | B8 | 1, 5 | Q5 | 3 |
| | | B9 | 1 | Q6 | 1, 10 |
