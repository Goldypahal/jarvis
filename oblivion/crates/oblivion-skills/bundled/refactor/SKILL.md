---
name: refactor
description: Restructure or clean up code without changing what it does - rename, extract functions, remove duplication, split large files, simplify logic - verified by running tests. Use when the user asks to refactor, clean up, simplify, reorganise or "make this more readable/maintainable".
license: MIT
---
# Refactor safely

The rule: behaviour stays exactly the same. Every step is small, surgical, and verified.

1. **Agree on the goal** in one line (e.g. "split the 900-line handler into modules", "remove the three
   copies of the date parsing"). Ask if the scope is vague.
2. **Get a safety net.** Find and run the existing tests; record the baseline result. If the code you will touch
   has no tests, write characterisation tests first (recording current behaviour, even quirky edge cases) using
   the write-tests skill. Never refactor without working tests.
3. **Find all usages** before renaming or moving anything: search for the identifier across the repo,
   including call sites, type definitions, configs, templates, docstrings, and tests.
4. **Change in small, surgical steps**, one kind of change at a time (rename, then extract, then move):
   - Never rewrite entire multi-hundred line files to adjust a function; use precise, targeted contiguous edits.
   - Extract a function for a block with a clear purpose; name it after what it does.
   - Replace duplicated code with a single shared helper.
   - Flatten deep nesting with early returns; replace ambiguous flag arguments with two distinct functions.
   - Split large files by single responsibility; preserve public backwards compatibility (via re-exports) unless explicitly asked to break it.
   - Delete dead code only after searching confirms nothing references it.
5. **Run tests after every single step.** Build and run tests immediately after extracting a helper or renaming a symbol.
   If a test breaks, revert the immediate change or fix it before proceeding.
6. **Do not mix in fixes or features.** If you find a bug during refactoring, note it and report it separately;
   do not alter behaviour or patch logic during the refactoring pass.
7. **Finish cleanly**: verify that all tests and linters pass (`cargo clippy`, `pytest`, `cargo fmt`).
   Review the diff (`git diff --stat`, then inspect changes). Report what moved where, public API notes,
   and any unrelated issues discovered during the process.
