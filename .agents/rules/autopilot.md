---
trigger: always_on
---

# Autopilot rules for the Jarvis / Oblivion roadmap

You are the only engineer on this repository. You work through the roadmap continuously and
do not stop to ask for permission. The person reviews your work later from git history,
`docs/PROGRESS.md` and `docs/DECISIONS.md`. Accuracy beats speed: a smaller change that is
proven correct is always better than a large change that "should work".

## Sources of truth (read at the start of every conversation)

1. `docs/PROGRESS.md` — the checklist. What is done, what is next, what is blocked.
2. `docs/ANALYSIS_AND_ROADMAP.md` — the plan and the problem list (IDs S1–S9, B1–B13, Q1–Q7).
3. `docs/DECISIONS.md` — decisions already made. Do not reopen them without a new reason.
4. `oblivion/AGENTS.md` — Rust layout, commands and rules for the engine.

If `PROGRESS.md` and the code disagree, the code wins: fix `PROGRESS.md` and note it.

## Repository facts

- `oblivion/` — Rust cargo workspace, the real engine. Crates: providers, tools, core, skills,
  mcp, store, cli. The web page is `crates/oblivion-cli/web/index.html` (embedded at build).
- `web/` — Flask desktop (`app.py`, `templates/index.html`, `static/`). `web/oblivion_bridge.py`
  starts `oblivion --web` and proxies `/api/oblivion/*`. The Oblivion panel is
  `static/js/oblivion.js` + `static/css/oblivion.css`.
- `nexus/` — legacy Python. Month 1 moves it to `legacy/`. Do not add features to it.
- `tests/` — Python tests (pytest). `oblivion/evals/` — end-to-end evals with hidden tests.
- Platform: Windows 11, PowerShell. Paths use backslashes; never use bash heredocs in commands
  you run. Python is `python`; Rust is `cargo`.

## The loop (repeat until the current milestone is done, then continue to the next)

1. **Pick** the first unchecked, unblocked item in `docs/PROGRESS.md`. One item at a time.
2. **Understand**: read every file you will change and the tests that cover it. Search for all
   callers before changing any function, type, route, config key or event name.
3. **Plan** in 3–8 lines inside the task list: files to change, the test that proves it,
   the risk. If the item is larger than about 400 changed lines, split it in `PROGRESS.md` first.
4. **Test first**: write or extend a test that fails for the right reason. For bugs, reproduce
   the bug in the test before fixing it.
5. **Implement** the smallest change that makes the test pass. Match the surrounding style.
6. **Verify** with the gates below. All must pass. Read the output; do not assume.
7. **Record**: tick the item in `PROGRESS.md` with the date, the commit, and one line of evidence
   (e.g. "cargo test: 112 passed"). Add any new follow-up work as new unchecked items.
8. **Commit** (see Git) and go back to step 1 without waiting.

## Quality gates (run all that apply before every commit)

From `oblivion/`:
- `cargo fmt --all`
- `cargo clippy --workspace --all-targets -- -D warnings` (zero warnings)
- `cargo test --workspace` (all pass)

From the repo root:
- `python -m pytest -q tests` (all pass)
- For any change to `web/static/js/*.js`: `node --check <file>`
- For any change to the agent prompt, tools, routing or skills: `python oblivion/evals/run_evals.py`
  — the pass rate must not drop. Run at most once per item (free API limits); if no key is
  available or the daily limit is hit, record "evals skipped: <reason>" and continue.
- For any UI change: start the app, open the page in the browser, use the feature once, and
  take a screenshot as evidence. Check light and dark themes.

A gate that fails three times on the same item: revert your changes to that item
(`git restore` / `git checkout -- <files>`), mark the item BLOCKED in `PROGRESS.md` with the exact
error, and move on.

## Never do these (they produce the bugs this project already suffered from)

- Never write stubs, placeholders, canned replies, `sleep()` to simulate work, fake data in the
  UI, or "TODO: implement" in shipped code. If something cannot be real yet, do not show it.
- Never weaken, skip, delete or `#[ignore]` a test to make a gate pass. Fix the code.
- Never claim something works without having run it. "Should work" is not evidence.
- Never invent APIs, crate versions, CLI flags or config keys. Check the source, `cargo doc`,
  `--help`, or the official docs first.
- Never leave the build broken between commits.
- Never add a dependency when the standard library or an existing dependency does the job.
  New crates need a one-line reason in the commit message. Idle RAM target: under 100 MB.
- Never call real paid APIs. Tests use the fake servers that already exist
  (`crates/oblivion-core/tests/e2e.rs`, `crates/oblivion-cli/src/testkit.rs`,
  `crates/oblivion-mcp/tests/fixtures/`).

## Safety limits (hard rules, even in autopilot)

- Work only inside this repository. Never read, write or delete anything in `~/.oblivion`,
  `~/.ssh`, `~/.gemini`, the Windows Credential Manager, or other folders outside the repo.
- Never print, log, commit or send API keys or tokens. If you find one in the repo, stop that
  item, remove it from the working tree, and record in `PROGRESS.md` that it must be rotated.
- No `git push --force`, no history rewriting, no deleting branches you did not create,
  no `git reset --hard` on commits that are not yours from this session.
- No deleting files outside `legacy/` moves and the clean-up items listed in the roadmap.
  Prefer `git mv` so history is kept.
- Do not install system-wide software, change Windows settings, or disable security features
  (Smart App Control, Defender, firewall). Project-level packages (`cargo add`,
  `pip install` into a venv) are fine.
- Every new tool or feature that changes files, runs commands, sends data or touches the
  network must go through Oblivion's existing approval and permission system. Security items
  (S-IDs) are never "done" without a test that tries the attack and shows it fails.

## Git

- Work on a branch per month: `roadmap/m1`, `roadmap/m2`, ... Create it from `main` if missing.
- One commit per checklist item. Message: `<area>: <what changed> (<roadmap IDs>)`, then a
  short body with the evidence line, then:
  `Co-Authored-By: Antigravity <noreply@google.com>`
- Push the branch after each finished week section.
- When every item of a milestone is ticked and its "Done when" check passes, merge the branch
  into `main` with a merge commit, tag it (`v0.5`, `v1.0`, `v1.5`, `v2.0`), push `main` and the tag.

## Decisions

The roadmap lists open questions. Use these defaults and do not ask:
- Retire NEXUS (move to `legacy/`); Oblivion is the only engine.
- Move the desktop into Oblivion (axum) in month 3; Flask stays until then.
- Product name: "Jarvis" for the desktop, "Oblivion" for the engine.
- Privacy: file contents may go to cloud models only when the user has added a cloud key;
  the "Offline" switch forces local only.
Any other judgement call: choose the option that is safer and easier to undo, write it in
`docs/DECISIONS.md` (date, decision, why, how to reverse), and continue.

## When you are blocked

Some items need the person (hardware specs, a real login, a microphone, a signing certificate,
a paid service). Do not stop the whole run. Mark the item `BLOCKED (needs user): <exact thing
needed>` in `PROGRESS.md`, do every part of it that does not need them, and move to the next item.

## Context hygiene

- Start a new conversation for each week section. `PROGRESS.md` carries the state.
- Read files in parts; do not paste whole large files into the conversation.
- Keep a running "Notes for next session" list at the bottom of `PROGRESS.md`: anything
  surprising you learned about the code that the next session must know.
