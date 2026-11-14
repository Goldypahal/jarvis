---
name: code-review
description: Review code changes (a diff, a branch or files) for bugs, security problems and maintainability, and report findings by severity. Use when the user asks for a review, to check a PR, or "what's wrong with this code".
license: MIT
---
# Code review

1. Get the change: `git diff` (unstaged), `git diff --staged`, or `git diff <base>...HEAD` for a branch. If the user named files, read them.
2. Read enough surrounding code to understand each change. Do not review lines you have not read.
3. Look for, in this order:
   1. Correctness: logic errors, off-by-one, wrong conditions, unhandled None/null/errors, broken edge cases (empty, large, unicode, concurrency).
   2. Security: injection (SQL, shell, path), secrets in code, unsafe deserialization, missing auth checks.
   3. Tests: changed behaviour without tests; tests that cannot fail.
   4. Maintainability: duplication, unclear names, dead code — only if it matters.
4. Only report issues you can point to. For each: `file:line`, severity (high/medium/low), what goes wrong with a concrete input, and the fix.
5. Order findings by severity. If you find nothing serious, say so plainly; do not invent nitpicks.
6. Do not change code unless the user asks you to fix the findings.
