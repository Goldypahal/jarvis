---
name: security-review
description: Check code for security problems - leaked secrets, injection, unsafe auth, risky dependencies - and report them by severity with fixes. Use when the user asks for a security review or audit, "is this safe", before a release, or after adding login, payments, file upload or user input handling.
license: MIT
---
# Security review

Scope first: the files or diff the user named; otherwise the whole repo, starting from entry points
(HTTP routes, CLI arguments, file/network input, config loading).

1. **Secrets.** Check for keys and passwords using pattern searches across code, config files, and tests:
   - Search for assignment patterns: `api_key`, `secret`, `token`, `password`, `passwd`, `private_key`.
   - Specific formats: AWS (`AKIA[0-9A-Z]{16}`), GitHub (`ghp_[A-Za-z0-9]{36}`), PEM blocks (`-----BEGIN .*PRIVATE KEY-----`).
   - Check `.env*`, config files, test fixtures, and verify `.gitignore` covers `.env` and local credentials.
   - Run `git status` to ensure untracked key files are not staged for commit.
   - If a real secret is committed: note that it must be revoked/rotated, not just deleted (git history preserves it).
2. **Injection.** Follow user-controlled input to dangerous sinks:
   - SQL built with string formatting or concatenation instead of parameterized queries.
   - Shell commands built from input (`os.system`, `subprocess(..., shell=True)`, `exec`, backticks, `Command::new("sh").arg("-c")`).
   - Code execution: `eval`/`exec`/`Function()`, unsafe deserialization (`pickle.loads`, `yaml.load` without SafeLoader, Java/PHP unserialize).
   - Path traversal: file paths from input without normalising (`canonicalize`) and checking they stay within the allowed root.
   - XSS: HTML constructed from input without contextual escaping; `innerHTML`, `dangerouslySetInnerHTML`, `|safe`, `v-html`.
   - SSRF: network requests to input URLs. String checks are insufficient; verify DNS resolution and IP ranges to block loopback, link-local, and RFC1918 private ranges both initially and across redirects.
3. **Auth, tokens and access.**
   - Every route that reads or changes user data must verify caller identity and ownership.
   - Passwords hashed with bcrypt, scrypt, or argon2 (never plain, MD5, or SHA-1).
   - Sessions/tokens: per-start random generation with secure entropy (`secrets`, `OsRng`), signature verification, and expiration.
   - Local web/loopback services: verify `Host` header to prevent DNS rebinding attacks; require per-session authentication tokens for administrative endpoints.
   - CSRF protection on cookie-authenticated state-changing forms. Rate limits on authentication attempts.
4. **Data exposure.** Error pages and logs must not print stack traces, tokens, or personal data to users.
   CORS must not combine `*` with credentials. Debug mode must be disabled in production configurations.
5. **Dependencies.** Run the ecosystem's audit if available: `npm audit --omit=dev`, `pip-audit`,
   `cargo audit`, `govulncheck ./...`. Report only vulnerabilities that apply to how the codebase uses the package.
6. **Crypto and randomness.** Use established standard libraries; never roll custom cryptography.
   Tokens from a cryptographically secure RNG (`secrets`, `crypto.randomBytes`, `rand::rngs::OsRng`), not `random`/`Math.random`.
   TLS verification must never be disabled in production code.

Report (most severe first), each finding with: **severity** (Critical / High / Medium / Low), **file:line**,
**what an attacker could do** in one sentence, and **the fix** (code snippet when short).
Only report what you verified in the code; mark guesses as "needs checking". Say what you did not review.
Offer to apply fixes; do not change code during the review unless asked.
