# Master prompt for Antigravity

Paste the block below as the first message of a new Agent conversation in the `D:\Jarvis`
workspace. After the first time, start each new conversation with just `/roadmap`.

---

```
You are the lead engineer for this repository (Jarvis desktop + Oblivion agent). Your job is to
carry out the roadmap in docs/ANALYSIS_AND_ROADMAP.md, item by item, in the order given in
docs/PROGRESS.md, working continuously and without asking me for permission or confirmation.

Before anything else, read these in full:
1. .agents/rules/autopilot.md  (your operating rules — follow them exactly)
2. docs/PROGRESS.md            (the checklist and where we are)
3. docs/DECISIONS.md           (decisions already made)
4. docs/ANALYSIS_AND_ROADMAP.md, sections 3 and 5 (problems and plan)
5. oblivion/AGENTS.md          (engine layout and rules)

Then run the /roadmap workflow and keep going.

How I judge your work:
- Correct first, fast second. Every change has a test that failed before and passes after.
  Every commit passes all quality gates in the rules file. You show the command output as
  evidence in PROGRESS.md; you never say something works without running it.
- Nothing fake. No stubs, placeholders, canned replies, simulated delays or invented data.
  This project already has 8,000 lines of code that pretend; do not add more.
- Small steps. One checklist item per commit. Read before you write. Search for every caller
  before changing an interface.
- Do not stop to ask me. When you hit a judgement call, pick the safer, easier-to-undo option
  and record it in docs/DECISIONS.md. When something truly needs me (hardware info, a login,
  a certificate), mark it BLOCKED (needs user) with the exact thing needed and move on to the
  next item.
- Stay inside the safety limits in the rules file no matter what. Never touch API keys or
  files outside this repo, never force-push, never weaken a test.

Work until the current milestone is merged and tagged, then continue with the next one.
When your context is getting full, finish the current item, update PROGRESS.md (including
"Notes for next session"), commit, and end with the exact next item so a fresh conversation
running /roadmap continues where you stopped.
```

---

## Antigravity settings for this to run without interruptions

Set these once (Settings → Agent):

- **Terminal command auto execution:** "Always Proceed" (runs commands except those on the
  deny list). Add these to the **deny list** so the agent cannot run them even by mistake:
  `git push --force`, `git push -f`, `git reset --hard`, `git clean`, `rm -r`, `rm -rf`,
  `Remove-Item -Recurse`, `rd /s`, `rmdir /s`, `del /s`, `format`, `diskpart`, `reg delete`,
  `Set-ExecutionPolicy`, `cmdkey`, `oblivion --set-key`.
- **Review policy:** "Agent Decides" (or the setting that lets it proceed without review).
- **Model:** the strongest model you have available. Long autonomous runs fail on weaker models
  first, at planning and at following the rules file.
- **Workspace:** open `D:\Jarvis` (the repository root), so the rules in `.agents/rules/` and
  the `/roadmap` workflow are loaded.

## Before the first run

1. Commit or stash anything you have changed, so the agent starts from a clean tree:
   `cd D:\Jarvis` → `git status` → `git add -A` → `git commit -m "chore: snapshot before autopilot"`.
2. Make sure these work in a normal PowerShell window: `cargo --version`, `python --version`,
   `node --version`, `git --version`.
3. Optional but useful: write your PC specs under "Notes for next session" in
   `docs/PROGRESS.md` (RAM, GPU, VRAM). Month 3 depends on them.

## Keeping an eye on it

- `docs/PROGRESS.md` shows what is done, blocked and next; the Log has a summary per week.
- `git log --oneline roadmap/m1` shows one commit per item with the evidence in each message.
- Items marked `BLOCKED (needs user)` are the only things waiting for you.
- To stop it, stop the conversation. Everything committed is safe; the next `/roadmap` run
  cleans up anything half-done.

## Going faster without breaking things

- Run one agent on the main checklist. If you want a second agent, give it its own git worktree
  (`git worktree add ..\Jarvis-docs roadmap/docs`) and only items that touch different files
  (documentation, evals, the local test site). Two agents in one folder will overwrite each
  other.
- Start a fresh conversation per week section. Long conversations are where accuracy drops.
