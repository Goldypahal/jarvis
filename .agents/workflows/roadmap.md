---
description: Continue the Jarvis/Oblivion roadmap autonomously from docs/PROGRESS.md
---

1. Read `docs/PROGRESS.md`, `docs/DECISIONS.md` and the "Notes for next session" list.
   Read the matching month section of `docs/ANALYSIS_AND_ROADMAP.md`.
2. Run `git status` and `git branch --show-current`. If there are uncommitted changes from an
   earlier session, run the quality gates: commit them if they pass, otherwise restore them and
   note it in `PROGRESS.md`. Switch to (or create) the branch for the current month.
3. Run the quality gates once on the clean tree to know the starting state. If anything already
   fails, fixing that is the first item.
4. Create a task list with every unchecked item of the current week section.
5. Work through the loop in `.agents/rules/autopilot.md` for each item, without asking for
   approval, until the week section is done.
6. At the end of the week section: push the branch, add a dated summary to the
   "Log" section of `PROGRESS.md` (items done, items blocked, gate results, eval pass rate),
   and update "Notes for next session".
7. If the milestone's "Done when" conditions are all met, merge, tag and push as the rules say.
8. Continue with the next week section in the same conversation if context allows; otherwise
   end with a one-paragraph summary and the exact next item, so a new conversation running
   `/roadmap` picks it up.
