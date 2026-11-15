---
name: security-review
description: Check code for security problems - leaked secrets, injection, unsafe auth, risky dependencies - and report them by severity with fixes. Use when the user asks for a security review or audit, "is this safe", before a release, or after adding login, payments, file upload or user input handling.
license: MIT
---
# Security review

Scope first: the files or diff the user named; otherwise the whole repo, starting from entry points
(HTTP routes, CLI arguments, file/network input, config loading).

1. **Secrets.** `grep` for keys and passwords: `(api[_-]?key|secret|token|password|passwd|private[_-]?key)\s*[:=]`,
   `AKIA[0-9A-Z]{16}`, `sk-[A-Za-z0-9]{20,}`, `ghp_[A-Za-z0-9]{36}`, `-----BEGIN .*PRIVATE KEY-----`.
   Check `.env*`, config files and test fixtures. Check `.gitignore` covers `.env` and key files.
   If a real secret is committed: say it must be revoked/rotated, not just deleted (git history keeps it).
2. **Injection.** Follow user-controlled input to dangerous sinks:
   - SQL built with string formatting or concatenation instead of parameters.
   - Shell commands built from input (`os.system`, `subprocess(..., shell=True)`, `exec`, backticks, `Command::new("sh").arg("-c")`).
   - `eval`/`exec`/`Function()`, unsafe deserialization (`pickle.loads`, `yaml.load` without SafeLoader, Java/PHP unserialize).
   - File paths from input without normalising and checking they stay inside the allowed folder (path traversal `../`).
   - HTML built from input without escaping (XSS); `innerHTML`, `dangerouslySetInnerHTML`, `|safe`, `v-html`.
   - Server-side requests to URLs from input (SSRF) without an allow-list.
3. **Auth and access.** Every route that reads or changes user data checks who is asking and that they own it.
   Passwords hashed with bcrypt/scrypt/argon2 (never MD5/SHA1/plain). Sessions/JWTs: expiry set, signature
   verified, secret not hard-coded. CSRF protection on cookie-authenticated forms. Rate limits on login.
4. **Data exposure.** Error pages and logs do not print stack traces, tokens or personal data to users.
   CORS is not `*` together with credentials. Debug mode is off in production config.
5. **Dependencies.** Run the ecosystem's audit if available: `npm audit --omit=dev`, `pip-audit`,
   `cargo audit`, `govulncheck ./...`. Report only issues that apply to how the code uses the package.
6. **Crypto and randomness.** No home-made crypto; tokens from a secure RNG (`secrets`, `crypto.randomBytes`,
   `rand::rngs::OsRng`), not `random`/`Math.random`. TLS verification never disabled.

Report (most severe first), each finding with: **severity** (Critical / High / Medium / Low), **file:line**,
**what an attacker could do** in one sentence, and **the fix** (code snippet when short).
Only report what you verified in the code; mark guesses as "needs checking". Say what you did not review.
Offer to apply fixes; do not change code during the review unless asked.
