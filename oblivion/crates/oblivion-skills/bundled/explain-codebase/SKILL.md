---
name: explain-codebase
description: Explore an unfamiliar repository and explain how it works - purpose, structure, entry points, main data flow, how to run and test it - or answer "where/how does X happen". Use when the user asks to explain, understand, onboard to, or map a codebase, or asks where something is implemented.
license: MIT
---
# Explain a codebase

Work from the outside in and keep reading cheap: list and search before opening files, and read
only the parts that matter. Never dump whole files into context when a header or targeted slice suffices.

1. **Orientation** (3-5 targeted lookups):
   - Read root docs (`README.md`, `AGENTS.md`, `CONTRIBUTING.md`).
   - Inspect build/package manifests (`Cargo.toml`, `pyproject.toml`, `package.json`, `go.mod`) to identify language, dependencies, toolchain, and scripts.
   - List top-level directories (`glob` with `*` and `*/*`) to grasp repository layout.
2. **Entry points**:
   - Locate startup files, CLI argument parsers, HTTP route initializations, and main binaries (`src/main.rs`, `app.py`, `index.ts`, `cmd/`).
   - Identify how configuration and environment variables are loaded.
3. **Trace one real flow end to end**:
   - Trace a concrete user action (e.g. an incoming HTTP request, CLI invocation, or agent turn):
     Entrypoint &rarr; Route Handler &rarr; Business Logic &rarr; External / Database Storage &rarr; Response / Event.
   - Search for function and type definitions across files to map connections without guessing.
4. **Identify core architectural foundations**:
   - Primary data models, state machines, and schemas.
   - Concurrency, locking, or background worker models.
   - Security boundaries: input sanitization, authentication, permission checks, and sandboxing.
   - Test suite layout and execution commands.
5. **Answer at the right level**:
   - For repository overviews: provide a one-paragraph summary of purpose, a directory map (directory &rarr; responsibility), key control flows with exact `file:line` citations, and standard build/test instructions.
   - For "where does X happen": provide the exact `file:line` references and a concise walkthrough of how control reaches that point.
6. **Diagramming & persistence**:
   - Provide a Mermaid flowchart (`flowchart TD` or `sequenceDiagram`) for flows involving multiple components.
   - Offer to update or persist architectural notes in `AGENTS.md` for future sessions.
Never guess: verify claims by reading the actual source code; if unverified, explicitly label it as "needs confirmation".
