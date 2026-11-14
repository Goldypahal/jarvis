---
name: fix-tests
description: Make a failing test suite or CI build pass by finding and fixing the real cause. Use when tests fail, CI is red, or the user says "fix the tests" or "make it build".
license: MIT
---
# Fix failing tests

1. Find the test command (AGENTS.md, README, CI config, package files). Run the full suite once and save the failure list.
2. Take ONE failure at a time. Run just that test (e.g. `pytest path::test -x`, `cargo test name`, `npm test -- -t name`).
3. Read the error and the code under test. Decide whether the bug is in the code or in the test:
   - Default assumption: the test is right and the code is wrong.
   - Only change a test when it clearly checks the wrong thing; say so explicitly.
4. Make the smallest fix. Re-run that test, then the full suite to check nothing else broke.
5. Never delete, skip or weaken tests, or hard-code expected values, just to get green.
6. Finish with: what was wrong, what you changed (files), and the final test result.
