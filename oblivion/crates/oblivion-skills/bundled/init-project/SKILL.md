---
name: init-project
description: Create or update AGENTS.md for a code project so future sessions know how to build, test and work in it. Use when the user asks to initialise, set up or document a repo for the agent, or when a project has no AGENTS.md.
license: MIT
---
# Write AGENTS.md for this project

1. Look around with `glob` (e.g. `*`, `**/*.toml`, `**/package.json`, `**/pyproject.toml`, `**/*.csproj`) and read the README, the build files and any CI config (`.github/workflows/*`).
2. Work out, and verify by running when cheap:
   - how to install dependencies, build, run tests (one test and all tests), lint and format;
   - the main folders and what lives in each;
   - conventions visible in the code (naming, error handling, test style).
3. If AGENTS.md exists, read it and update it; keep anything the user wrote.
4. Write AGENTS.md with these sections, short and specific (real commands, real paths):
   `# <Project name>` · `## Commands` · `## Layout` · `## Conventions` · `## Gotchas`
5. Keep it under about 60 lines. No marketing text, no guesses: leave out what you could not confirm.
6. Tell the user what you found and anything you could not verify.
