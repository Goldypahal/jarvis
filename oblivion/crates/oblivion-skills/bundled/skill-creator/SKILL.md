---
name: skill-creator
description: Create a new skill (a folder with SKILL.md and optional scripts) or improve an existing one. Use when the user wants to save a workflow as a skill, teach the agent a repeatable task, or asks to make or edit a skill.
license: MIT
---
# Create a skill

1. Ask (once, together) only what you cannot infer: what task the skill covers, an example request that should trigger it, and where to save it (default: `~/.oblivion/skills/<name>/`; for one project: `<project>/.agents/skills/<name>/`).
2. Choose a name: lowercase letters, digits and dashes, ≤ 64 chars.
3. Write `SKILL.md`:
   ```
   ---
   name: <name>
   description: <what it does>. Use when <the situations and phrases that should trigger it>.
   ---
   # <Title>
   1. numbered, concrete steps
   ```
   - The description decides when the skill is used: name the task AND the triggers, ≤ 1024 chars.
   - Keep the body under ~150 lines. Put long reference material in separate files and link them by relative path.
   - Write steps a smaller model can follow exactly: one action per step, real commands, expected results.
4. If a step is fiddly (parsing, file formats, calculations), put it in a script under `scripts/` that prints clear errors, and tell the instructions to run it.
5. Test: do the example task by following only the skill. Fix any step that was unclear or wrong.
6. Tell the user the folder path and the trigger phrases. New skills load at the next start of Oblivion.
