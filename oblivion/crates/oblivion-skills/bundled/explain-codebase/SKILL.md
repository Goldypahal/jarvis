---
name: explain-codebase
description: Explore an unfamiliar repository and explain how it works - purpose, structure, entry points, main data flow, how to run and test it - or answer "where/how does X happen". Use when the user asks to explain, understand, onboard to, or map a codebase, or asks where something is implemented.
license: MIT
---
# Explain a codebase

Work from the outside in and keep reading cheap: list and search before opening files, and read
only the parts that matter.

1. **Orientation** (3-5 calls): README, AGENTS.md/CONTRIBUTING, the manifest (package.json, Cargo.toml,
   pyproject.toml, go.mod, pom.xml) for language, dependencies and scripts, and the top-level folders
   (`glob` with `*` and `*/*`).
2. **Entry points**: main files, CLI definitions, HTTP route registration, `bin/`, Dockerfile CMD,
   framework conventions (e.g. `app/`, `pages/`, `src/main.rs`, `manage.py`).
3. **Follow one real flow end to end** (e.g. "an HTTP request to /orders", "the `build` command"):
   entry -> handler -> business logic -> storage/external calls -> response. Use `grep` for function names
   to jump between files. For large repos, use the `task` tool to explore separate areas in parallel.
4. **Note the important pieces**: core data types/models, configuration and environment variables,
   database schema/migrations, background jobs, external services, test layout and commands.
5. **Answer at the right level.** For "explain the repo": a one-paragraph summary, a folder map
   (folder -> responsibility), the main flow as numbered steps with file paths, how to run/test, and
   gotchas you noticed. For "where does X happen": the file:line(s) and a short explanation of the path to it.
6. Offer a diagram as Mermaid text if the flow has many parts, and offer to save the overview as
   AGENTS.md (init-project skill) so future sessions start faster.
Never guess: if you did not read it, say "probably" or check first.
