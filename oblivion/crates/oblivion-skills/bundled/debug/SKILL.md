---
name: debug
description: Systematically find the cause of a bug, crash or wrong output and fix it. Use when the user reports an error message, a stack trace, unexpected behaviour, or "it doesn't work".
license: MIT
---
# Debug

1. Reproduce: find or write the smallest command that shows the problem. Run it and keep the exact output.
2. Read the stack trace from the bottom frame in YOUR code. Open those files.
3. Form 1–3 hypotheses. For each, name the observation that would confirm or rule it out.
4. Test hypotheses cheaply: read code, add a temporary print/log, or run a one-line check. Search the web for unfamiliar error messages.
5. Fix the root cause, not the symptom. Remove temporary prints.
6. Re-run the reproduction, then the project's tests.
7. Explain the cause in two sentences and list the change.
If you cannot reproduce it, say what you tried and ask the user for the exact steps or input.
