---
name: write-tests
description: Write automated tests for existing or new code - unit tests, edge cases, regression tests for a bug - using the project's own test framework. Use when the user asks to add tests, raise coverage, "test this function", or to do test-first (TDD) development.
license: MIT
---
# Write tests

1. **Find the project's test setup.** Look for existing tests (`glob` for `*test*`, `tests/`, `__tests__/`,
   `*_test.go`, `#[cfg(test)]`) and the exact commands to run them (package.json scripts, pytest, Cargo.toml, Makefile).
   Follow the style, helpers, naming conventions, and file layout of existing tests.
   If no test framework is present, choose the standard tool (pytest, cargo test, vitest, go test) and explain why.
2. **Read the code under test** and enumerate observable behaviours:
   - Happy path / standard usage cases.
   - Boundary conditions (empty strings, empty lists, 0, 1, max values, negative numbers, unicode, long inputs).
   - Error and validation paths (invalid formats, missing required fields, permission denials).
   - External side effects (file modifications, network calls, state transitions).
3. **Write small, independent tests**, one behaviour per test, named after what is asserted
   (`test_rejects_expired_token`, not `test_helper_1`). Follow Arrange - Act - Assert.
   Assert on return values and observable state changes, not on private internal implementation details.
4. **Test real code, isolate only external boundaries:**
   - Execute the real functions, classes, and handlers under test. Never mock the system under test.
   - Mock or isolate only external boundaries: external HTTP networks, system clocks, and paid APIs.
   - Use portable path abstractions (`pathlib.Path` in Python, `std::path::PathBuf` in Rust); never hardcode `/tmp` or backslashes.
   - Use standard temporary folder utilities (`tempfile.TemporaryDirectory`, temp test fixtures).
   - Tests must run offline, independently, and pass in any execution order.
5. **Run tests and verify failures first (TDD & Regressions):**
   - For bug fixes, write the reproduction test first and verify that it FAILS for the exact expected reason.
   - Implement the smallest fix to make it pass.
   - For new features: write one failing test, write the minimal code to pass it, then refactor.
6. **Never weaken tests to make them pass.** If a test fails because the production code has a defect,
   fix the production code or report the discrepancy to the user; never alter test expectations to hide a bug.
7. **Summarise**: report covered behaviours, the command used to verify, test pass counts, and any remaining gaps.
