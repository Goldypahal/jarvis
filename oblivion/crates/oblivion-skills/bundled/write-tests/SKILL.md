---
name: write-tests
description: Write automated tests for existing or new code - unit tests, edge cases, regression tests for a bug - using the project's own test framework. Use when the user asks to add tests, raise coverage, "test this function", or to do test-first (TDD) development.
license: MIT
---
# Write tests

1. **Find the project's test setup.** Look for existing tests (`glob` for `*test*`, `tests/`, `__tests__/`,
   `*_test.go`, `#[cfg(test)]`) and the command to run them (package.json scripts, pyproject/pytest.ini,
   Cargo.toml, Makefile, AGENTS.md). Copy the style, helpers and file layout of the existing tests.
   No tests yet: use the standard tool (pytest, vitest/jest, cargo test, go test, JUnit) and say so.
2. **Read the code under test** and list its behaviours: normal cases, boundaries (empty, zero, one,
   max, negative, unicode, very long), invalid input and error paths, and side effects (files, network, time).
3. **Write small, independent tests**, one behaviour each, named after the behaviour
   (`test_rejects_expired_token`, not `test1`). Arrange - act - assert. Assert on outputs and visible
   effects, not on private internals.
4. **Isolate the outside world**: temp folders for files, fakes or mocks for network and clock,
   fixed random seeds. Tests must pass offline and in any order.
5. **Run them.** New tests should pass against correct code. For a bug fix, first write the test that
   reproduces the bug and watch it FAIL, then fix the code and watch it pass (that is the regression test).
   For TDD: write one failing test, write the least code to pass it, refactor, repeat.
6. If a test fails because the code is wrong (not the test), stop and report it to the user with the
   failing input and the expected result; do not weaken the test to make it pass.
7. Summarise: which behaviours are now covered, the command to run the tests, and notable gaps.
