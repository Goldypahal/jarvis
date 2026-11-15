---
name: refactor
description: Restructure or clean up code without changing what it does - rename, extract functions, remove duplication, split large files, simplify logic - verified by running tests. Use when the user asks to refactor, clean up, simplify, reorganise or "make this more readable/maintainable".
license: MIT
---
# Refactor safely

The rule: behaviour stays exactly the same. Every step is small and verified.

1. **Agree on the goal** in one line (e.g. "split the 900-line handler into modules", "remove the three
   copies of the date parsing"). Ask if it is vague.
2. **Get a safety net.** Find and run the existing tests; note the result. If the code you will touch
   has no tests, write characterisation tests first (they record what the code does today, even odd
   behaviour) - use the write-tests skill.
3. **Find all usages** before renaming or moving anything: `grep` for the name across the repo,
   including strings, configs, templates and docs.
4. **Change in small steps**, one kind of change at a time (rename, then extract, then move), running
   the tests or at least the build after each step. Common moves:
   - Extract a function for a block with a clear purpose; name it after what it does.
   - Replace duplicated code with one shared function.
   - Flatten deep nesting with early returns; replace flag arguments with two clear functions.
   - Split a large file by responsibility; keep public names working (re-export) unless told otherwise.
   - Delete dead code only after `grep` shows nothing uses it.
5. **Do not mix in fixes or features.** If you find a bug, note it and tell the user; fix it separately.
6. **Finish**: tests and linter pass, the diff is reviewed (`git diff --stat`, then skim), and you report
   what moved where and anything that needs the user's attention (public API changes, found bugs).
