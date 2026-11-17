# Decisions

Each entry: date · decision · why · how to reverse. The agent adds to this file; it does not
reopen an entry without a new reason.

- **2026-11-17 · Oblivion is the only engine; NEXUS moves to `legacy/`.** NEXUS returns canned
  text and has security holes S1–S3; keeping two engines doubles all work. Reverse: `git mv`
  `legacy/nexus` back and restore the adapter import in `web/app.py`.
- **2026-11-17 · The desktop moves from Flask into Oblivion (axum) in month 3.** Needed for a
  one-file install without Python. Flask stays until then. Reverse: keep `web/` and the bridge.
- **2026-11-17 · Names: "Jarvis" is the desktop, "Oblivion" is the engine.**
- **2026-11-17 · Privacy default.** File contents may go to cloud models only when a cloud key
  is configured; the "Offline" switch forces local models only.
- **2026-11-17 · Autopilot.** The coding agent works through `PROGRESS.md` without asking,
  within the limits in `.agents/rules/autopilot.md`. Merges to `main` only at milestones.
